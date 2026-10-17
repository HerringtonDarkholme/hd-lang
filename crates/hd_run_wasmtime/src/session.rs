//! A REPL session on wasmtime (`f1c55020:future-work/compiler/repl-plan.md` §2.3 and §2.4, job S1).
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
//!
//! A `Stopper` interrupts the input that runs (job S2 and S3). While Wasm
//! runs, the input traps at the next epoch tick or host call and is
//! `stopped` (`cli.repl.stop`). While it waits on the host, the wait wakes
//! at once and the session calls the input's `hd.cancel`, which cancels
//! its root suspension by `req.cancel.steps`: its `defer` suites run and
//! it is `cancelled` (`cli.repl.cancel`). Either way it then ends as a
//! panicking input does, and the host operations it left are dropped.
//!
//! The session records a journal in memory (`docs/future-work/live-execution.md` §4, job
//! J1): an `Input` row as each input starts, a row for each of its host
//! calls, written where the host dispatches imports, its `Wake` and
//! `Cancel` driver events, and an `End` row with how it ended. It is the
//! record a rebuild serves host results from (`cli.repl.rebuild`, job J2,
//! `session_rebuild.rs`). Past
//! `hd_run::journal::SESSION_CAP` recording stops; the REPL then says once
//! that the session can no longer be rebuilt (`take_journal_notice`).
//!
//! wasmtime frees no instance before its store, so each input holds its
//! instance, about 89 KiB, until the session ends. Once more than
//! `REBUILD_AFTER` inputs ran since the session started or last rebuilt,
//! `wants_rebuild` says so, and the REPL rebuilds the session silently: a
//! rebuild holds the inputs it replays in one instance
//! (`session_rebuild.rs`).

use hd_base::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use hd_run::journal::{End, Recorder, SESSION_CAP};
use hd_run::{HostSetup, Instance, Outcome, StartError};
use hd_wasm::{MEMORY_KEY, SESSION_MODULE, global_key, is_session_key};
use wasmtime::{Extern, Global, Linker, Val};

use crate::engine::{EngineConfig, WasmtimeEngine, new_store};
use crate::host::{Host, HostIo, Stop, define_session, supported, unsupported_message};
use crate::instance::WasmtimeInstance;
use crate::profiles::RuntimeProfile;
use crate::runtime::reactor::Waker;
use crate::{cache, limits};

#[path = "session_rebuild.rs"]
mod rebuild;
pub use rebuild::{Rebuilder, Rebuilt};

/// How many inputs a session runs, after it starts or rebuilds, before it
/// wants a rebuild to free their instances' memory (#228, decided
/// 2026-10-16): about 87 MiB of instances at 89 KiB each.
pub const REBUILD_AFTER: usize = 1000;

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
    /// Set by a `Stopper`; the store's epoch callback, every host call and
    /// the host's wait read it.
    flag: Arc<AtomicBool>,
    /// The instances the store holds.
    instances: usize,
    /// The inputs, not the prelude, that `run_input` started since the
    /// store did.
    ran: usize,
    /// `REBUILD_AFTER`, or a test's lower bound.
    rebuild_after: usize,
}

/// Interrupts the input a `WasmtimeSession` is running. It is `Send +
/// Sync` and cheap to clone, for a Ctrl-C handler on another thread. An
/// input that computes traps at the next epoch tick (about ten
/// milliseconds) or host call, reported as `stopped` (`cli.repl.stop`).
/// An input that waits on the host is cancelled at once, its `defer`
/// suites run, reported as `cancelled` (`cli.repl.cancel`). Either way it
/// ends as a panicking input does. A stop while no input runs does
/// nothing: the flag clears when the next input starts.
#[derive(Clone)]
pub struct Stopper {
    flag: Arc<AtomicBool>,
    waker: Waker,
}

impl Stopper {
    pub fn stop(&self) {
        self.flag.store(true, Ordering::Relaxed);
        self.waker.wake();
    }

    /// Whether the running input waits on the host now, so a stop cancels
    /// it rather than stopping it.
    #[must_use]
    pub fn waiting(&self) -> bool {
        self.waker.parked()
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
        );
        let flag = Arc::new(AtomicBool::new(false));
        host.interrupt = Some(Arc::clone(&flag));
        let execution = crate::sys::random(16)
            .ok()
            .and_then(|b| <[u8; 16]>::try_from(b).ok())
            .map_or(0, u128::from_le_bytes);
        host.journal = Some(Recorder::new(SESSION_CAP, execution));
        let mut run =
            WasmtimeInstance::stopped(new_store(&self.engine, host)?, Arc::clone(&self.engine));
        // Sockets and HTTP clients outlive an input: a later input may use
        // a handle an earlier one opened.
        run.close_on_end = false;
        Ok(WasmtimeSession {
            run,
            linker: Linker::new(&self.engine),
            hosts: HashSet::default(),
            memory: false,
            runtime: Vec::new(),
            config: self.config,
            profile: self.profile,
            flag,
            instances: 0,
            ran: 0,
            rebuild_after: REBUILD_AFTER,
        })
    }
}

impl WasmtimeSession {
    /// Runs the input module `wasm`, numbered `number` in the session, of
    /// the source text `source`, to its end and answers how it ended, as
    /// `hd run` would: status 0, a program's own status, or 3 after a
    /// panic, which is reported on standard error. Its standard output is
    /// written before this returns. `Err` is a module the session cannot
    /// run: one wasmtime refuses, an import not on wasmtime yet, or a
    /// session import no earlier input exports. Such an input does not run.
    pub fn run_input(&mut self, number: u32, source: &str, wasm: &[u8]) -> Result<Outcome, String> {
        let (outcome, end) = self.run_ended(number, source, wasm);
        if end.is_some() && number != 0 {
            self.ran += 1;
        }
        outcome
    }

    /// Whether more inputs than the bound (`REBUILD_AFTER`) ran since the
    /// session started or last rebuilt, so a rebuild would free the
    /// memory of their instances.
    #[must_use]
    pub fn wants_rebuild(&self) -> bool {
        self.ran > self.rebuild_after
    }

    /// Sets the bound `wants_rebuild` reads, `REBUILD_AFTER` until then.
    pub fn set_rebuild_after(&mut self, inputs: usize) {
        self.rebuild_after = inputs;
    }

    /// The instances the session's store holds: one per input that ran,
    /// or after a rebuild one for each run of inputs it held together.
    #[must_use]
    pub fn instances(&self) -> usize {
        self.instances
    }

    /// Runs an input as `run_input` does, and answers how it ended too, as
    /// its `End` row says: `None` for an input that did not start.
    fn run_ended(
        &mut self,
        number: u32,
        source: &str,
        wasm: &[u8],
    ) -> (Result<Outcome, String>, Option<End>) {
        self.flag.store(false, Ordering::Relaxed);
        let module = match self.load(wasm) {
            Ok(module) => module,
            Err(e) => return (Err(e), None),
        };
        // A panic or standard error of an earlier input does not hide this
        // one's internal error.
        self.run.store.data_mut().reported = false;
        if let Err(e) = define_session(&mut self.linker, &module, self.profile, &mut self.hosts) {
            return (Err(e), None);
        }
        if let Some(journal) = self.run.store.data_mut().journal.as_mut() {
            journal.input(number, source.as_bytes(), wasm);
        }
        self.run.halt = None;
        self.run.last_trapped = false;
        let result = self.execute(&module);
        let end = self.ended(&result);
        (result, Some(end))
    }

    /// How the input that ran ended, as its `End` row says, which this
    /// writes.
    fn ended(&mut self, result: &Result<Outcome, String>) -> End {
        let end = match (result, self.run.halt) {
            (Ok(_), Some(halt)) => halt,
            (Ok(Outcome::Exit(_)), None) if self.run.last_trapped => End::Trapped,
            (Ok(Outcome::Exit(status)), None) => End::Exit(*status),
            (Ok(Outcome::Panic(_)), None) => End::Trapped,
            (Ok(Outcome::Internal(_)) | Err(_), _) => End::Failed,
        };
        if let Some(journal) = self.run.store.data_mut().journal.as_mut() {
            journal.end(end);
        }
        end
    }

    /// Instantiates and runs an input whose host imports the linker
    /// defines.
    fn execute(&mut self, module: &wasmtime::Module) -> Result<Outcome, String> {
        let instance = match self.instantiate(module)? {
            Ok(instance) => instance,
            Err(outcome) => return Ok(outcome),
        };
        let outcome = self.run_entry(instance, "hd.init", "hd.poll")?;
        if outcome == Outcome::Exit(0) {
            self.add(instance)?;
        } else {
            self.reset()?;
        }
        Ok(outcome)
    }

    /// Instantiates a module whose host imports the linker defines, or
    /// answers how its input ended when that ran out of heap or time or
    /// was stopped. `Err` is a module the session cannot instantiate.
    fn instantiate(
        &mut self,
        module: &wasmtime::Module,
    ) -> Result<Result<wasmtime::Instance, Outcome>, String> {
        match self.linker.instantiate(&mut self.run.store, module) {
            Ok(instance) => {
                self.instances += 1;
                Ok(Ok(instance))
            }
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
                Ok(Err(outcome))
            }
            Err(error) => Err(format!("{error:#}")),
        }
    }

    /// Runs an input from `instance`'s entry exports `init` and `poll`, as
    /// a program's entry runs, and answers how it ended. A panic is
    /// reported on standard error and ends it with status 3.
    fn run_entry(
        &mut self,
        instance: wasmtime::Instance,
        init: &str,
        poll: &str,
    ) -> Result<Outcome, String> {
        self.run.enter(instance, init, poll).map_err(|e| match e {
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
                self.run.last_trapped = true;
                Outcome::Exit(3)
            }
            other => other,
        };
        Ok(outcome)
    }

    /// The session's journal so far, or `None` once recording stopped at
    /// its cap.
    #[must_use]
    pub fn journal(&self) -> Option<&[u8]> {
        self.run.store.data().journal.as_ref()?.journal()
    }

    /// Changes the journal's cap, `hd_run::journal::SESSION_CAP` until
    /// then; a journal already past it stops at its next row.
    pub fn set_journal_cap(&mut self, cap: usize) {
        if let Some(journal) = self.run.store.data_mut().journal.as_mut() {
            journal.set_cap(cap);
        }
    }

    /// True once, after the input in which the journal passed its cap and
    /// recording stopped: the REPL then says that the session can no
    /// longer be rebuilt.
    pub fn take_journal_notice(&mut self) -> bool {
        self.run
            .store
            .data_mut()
            .journal
            .as_mut()
            .is_some_and(Recorder::take_notice)
    }

    /// The host calls the session made, not served from a record.
    #[cfg(test)]
    pub(crate) fn host_calls(&self) -> u64 {
        self.run.store.data().calls
    }

    /// Whether a host operation is pending in the session's reactor.
    #[cfg(test)]
    pub(crate) fn pending(&self) -> bool {
        self.run.store.data().waiting()
    }

    /// Starts a new journal, or with `on` false records none from now on:
    /// for measuring what recording costs.
    #[cfg(test)]
    pub(crate) fn record(&mut self, on: bool) {
        self.run.store.data_mut().journal = on.then(|| Recorder::new(SESSION_CAP, 0));
    }

    /// A handle that stops or cancels the running input from another
    /// thread.
    #[must_use]
    pub fn stopper(&self) -> Stopper {
        Stopper {
            flag: Arc::clone(&self.flag),
            waker: self.run.store.data().reactor.waker(),
        }
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

    /// After an input that did not end with status 0: drops the host
    /// operations it left pending, a panicking or stopped input's or a
    /// timer a cancel did not abort, so none wakes a later input or hides
    /// its deadlock. Inputs run one at a time and one that ends with
    /// status 0 has finished every operation it started, so each one left
    /// is this input's.
    ///
    /// Then sets every runtime global the session's inputs share back to
    /// its initial value. A panic can leave them set: the forbidden-context
    /// counter after a panic inside `defer` or a facts body (so the next
    /// input's `println` would panic), the facts label, the entry's root
    /// suspension and the wake table. Before the first input that ends with
    /// status 0 there are none: the next input defines its own.
    fn reset(&mut self) -> Result<(), String> {
        let store = &mut self.run.store;
        store.data_mut().drop_pending();
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
