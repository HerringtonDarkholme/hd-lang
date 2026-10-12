//! A REPL session on wasmtime (repl-plan.md §2.3 and §2.4, job S1).
//!
//! One `Store` and one `Linker` hold the whole session. Each input is one
//! module that `hd_wasm::link` linked in `LinkMode::Session`: it imports
//! host functions, which the linker defines once for every input, and the
//! session keys of earlier inputs (module `hd_wasm::SESSION_MODULE`),
//! which the linker gained from them. An input runs as a program's entry
//! does: `hd.init`, then `hd.poll` and `hd.wake` until it ends. Its
//! instance stays in the store, so its bindings and functions live on.
//!
//! An input that ends with status 0 adds its session keys to the linker,
//! for later inputs to import; the caller then adds the driver's
//! `Output::session_added` to its `hd_driver::Session`. An input that
//! panics adds nothing (`cli.repl.panic.binding`), but what it changed in
//! earlier inputs' values stays (`cli.repl.panic.mutations`), and its host
//! calls are never made again (`cli.repl.host.once`). The session resets
//! the runtime state a panic can leave set, so the next input starts as
//! the first did (`cli.repl.panic`).

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use hd_run::{HostSetup, Instance, Outcome, StartError};
use hd_wasm::{MEMORY_KEY, SESSION_MODULE, global_key, is_session_key};
use wasmtime::{Extern, Global, Linker, Val};

use crate::engine::{EngineConfig, WasmtimeEngine, new_store};
use crate::host::{Host, HostIo, Stop, define_session, supported, unsupported_message};
use crate::instance::WasmtimeInstance;
use crate::profiles::RuntimeProfile;
use crate::{cache, limits};

/// A REPL session: the instances of every input that ran, in one store.
/// Like the `Engine` methods, it runs on a thread with at least
/// `thread_stack` of stack.
pub struct WasmtimeSession {
    /// The store, and the input that runs or ran last.
    run: WasmtimeInstance,
    /// The host imports and the session keys of the inputs that ended
    /// with status 0.
    linker: Linker<Host>,
    /// The host imports `linker` defines, as (module, name).
    hosts: HashSet<(String, String)>,
    /// Whether `linker` holds the session's memory.
    memory: bool,
    /// The runtime globals every input shares (`hd_wasm::rt`), once the
    /// input that defines them has ended with status 0.
    runtime: Vec<Global>,
    config: EngineConfig,
    profile: RuntimeProfile,
    /// Set by a `Stopper`; the store's epoch callback reads it.
    flag: Arc<AtomicBool>,
}

/// Stops the input a `WasmtimeSession` is running (`cli.repl.stop`). It is
/// `Send + Sync` and cheap to clone, for a Ctrl-C handler on another
/// thread. The running input traps at the next epoch tick (about ten
/// milliseconds) or host call and ends as a panicking input does, reported
/// as `stopped`. A stop while no input runs does nothing: the flag clears
/// when the next input starts.
#[derive(Clone)]
pub struct Stopper(Arc<AtomicBool>);

impl Stopper {
    pub fn stop(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

impl WasmtimeEngine {
    /// A session with no input yet, its standard streams on `io`.
    pub fn session(&self, setup: &HostSetup, io: HostIo) -> Result<WasmtimeSession, StartError> {
        let mut host = Host::new(
            io,
            setup.args.clone(),
            setup.program.clone(),
            setup.grants.clone(),
            setup.limits.wall_ms,
            setup.limits.memory_bytes,
        );
        let flag = Arc::new(AtomicBool::new(false));
        host.interrupt = Some(Arc::clone(&flag));
        let mut run =
            WasmtimeInstance::stopped(new_store(&self.engine, host)?, Arc::clone(&self.engine));
        // Sockets and HTTP clients outlive an input: a later input may use
        // a handle an earlier one opened.
        run.close_on_end = false;
        Ok(WasmtimeSession {
            run,
            linker: Linker::new(&self.engine),
            hosts: HashSet::new(),
            memory: false,
            runtime: Vec::new(),
            config: self.config,
            profile: self.profile,
            flag,
        })
    }
}

impl WasmtimeSession {
    /// Runs the input module `wasm` to its end and answers how it ended,
    /// as `hd run` would: status 0, a program's own status, or 3 after a
    /// panic, which is reported on standard error. Its standard output is
    /// written before this returns. `Err` is a module the session cannot
    /// run: one wasmtime refuses, an import not on wasmtime yet, or a
    /// session import no earlier input exports. Such an input does not run.
    pub fn run_input(&mut self, wasm: &[u8]) -> Result<Outcome, String> {
        self.flag.store(false, Ordering::Relaxed);
        let module = self.load(wasm)?;
        // A panic or standard error of an earlier input does not hide this
        // one's internal error.
        self.run.store.data_mut().reported = false;
        define_session(&mut self.linker, &module, self.profile, &mut self.hosts)?;
        let instance = match self.linker.instantiate(&mut self.run.store, &module) {
            Ok(instance) => instance,
            Err(error)
                if limits::is_heap_exhausted(&error)
                    || matches!(
                        error.downcast_ref::<Stop>(),
                        Some(Stop::TimeLimit | Stop::Interrupted)
                    ) =>
            {
                self.run.done = None;
                let outcome = self.run.trapped(&error);
                self.reset()?;
                return Ok(outcome);
            }
            Err(error) => return Err(format!("{error:#}")),
        };
        self.run
            .enter(instance, "hd.init", "hd.poll")
            .map_err(|e| match e {
                StartError::Refused(m) | StartError::Engine(m) => m,
            })?;
        let outcome = match self.run.init() {
            Outcome::Exit(0) => hd_run::drive(&mut self.run, usize::MAX),
            other => other,
        };
        let outcome = match outcome {
            Outcome::Panic(report) => {
                let host = self.run.store.data_mut();
                host.err(&format!("panic: {}: {}\n", report.category, report.message));
                Outcome::Exit(3)
            }
            other => other,
        };
        if outcome == Outcome::Exit(0) {
            self.add(instance)?;
        } else {
            self.reset()?;
        }
        Ok(outcome)
    }

    /// A handle that stops the running input from another thread.
    #[must_use]
    pub fn stopper(&self) -> Stopper {
        Stopper(Arc::clone(&self.flag))
    }

    /// Compiles an input, or names the first import that does not run on
    /// wasmtime yet. Session imports are the linker's to check.
    fn load(&self, wasm: &[u8]) -> Result<Arc<wasmtime::Module>, String> {
        if wasm.get(..4) != Some(b"\0asm") {
            return Err("not a Wasm module".into());
        }
        let imports = hd_run::module_imports(wasm)?;
        if let Some((m, n)) = imports
            .iter()
            .find(|(m, n)| m != SESSION_MODULE && !supported(m, n, self.profile))
        {
            return Err(unsupported_message(m, n));
        }
        let shared = cache::ondemand(&self.config)?;
        shared
            .cache
            .get_or_compile(&shared.engine, wasm)
            .map_err(|e| format!("{e:#}"))
    }

    /// Adds the session keys `instance` exports to the linker, for later
    /// inputs to import; not its entry exports, which every input has.
    /// Every input exports the memory; the first one's is the session's.
    fn add(&mut self, instance: wasmtime::Instance) -> Result<(), String> {
        let store = &mut self.run.store;
        let runtime: Vec<String> = hd_wasm::rt::runtime_globals()
            .iter()
            .map(global_key)
            .collect();
        let exports: Vec<(String, Extern)> = instance
            .exports(&mut *store)
            .map(|e| (e.name().to_owned(), e.into_extern()))
            .collect();
        for (name, export) in exports {
            if !is_session_key(&name) || (name == MEMORY_KEY && self.memory) {
                continue;
            }
            self.memory |= name == MEMORY_KEY;
            if let Extern::Global(global) = &export
                && runtime.contains(&name)
            {
                self.runtime.push(*global);
            }
            self.linker
                .define(&*store, SESSION_MODULE, &name, export)
                .map_err(|e| format!("{e:#}"))?;
        }
        Ok(())
    }

    /// After an input that did not end with status 0: sets every runtime
    /// global the session's inputs share back to its initial value. A
    /// panic can leave them set: the forbidden-context counter after a
    /// panic inside `defer` or a facts body (so the next input's `println`
    /// would panic), the facts label, the entry's root suspension and the
    /// wake table. Before the first input that ends with status 0 there are
    /// none: the next input defines its own.
    fn reset(&mut self) -> Result<(), String> {
        let store = &mut self.run.store;
        for global in &self.runtime {
            let ty = global.ty(&*store);
            let initial = Val::default_for_ty(ty.content())
                .ok_or_else(|| format!("a runtime global of type {ty:?} has no default"))?;
            global
                .set(&mut *store, initial)
                .map_err(|e| format!("{e:#}"))?;
        }
        Ok(())
    }
}
