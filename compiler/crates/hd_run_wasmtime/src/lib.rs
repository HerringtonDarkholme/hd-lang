#![forbid(unsafe_code)]
//! `hd_run_wasmtime`: the wasmtime engine and the default profile's host
//! (codegen.md §11.4; engines-and-test-runner.md §18; runtime-and-host.md
//! §17.5).
//!
//! A program runs in-process: its module is compiled by Cranelift with
//! Wasm GC on, instantiated with the imports of `host.rs`, and driven by
//! `hd.init`, then `hd.poll` and `hd.wake` until it is done, as the Node
//! host (`compiler/host/core.mjs`) drives it. Its standard output, error
//! and status are the Node host's: a panic stub's report and status 3, an
//! entry's own status (1 for `.Err`), and `internal error: ...` with
//! status 3 for any other trap. Two traps Node cannot report are panics
//! here, as the spec asks: a stack overflow is `stack-exhausted`
//! (`flow.panic.stack-exhausted`), a refused heap allocation is
//! `heap-exhausted`, and a wall deadline is `time-limit`.
//!
//! The host traits of the default profile that run here are `Console`,
//! `ConsoleInput`, `Clock`, `Args`, `Env`, `FsRead`, `FsWrite`, `Process`,
//! `Random`, `Sys`, `Http`, `Net`, their handle traits, the `hd:rt` runtime
//! imports and the `hd:prim` primitives.
//! A module that imports any other is refused at
//! load with a message naming it; `hd` runs those on Node.

mod codec;
mod core;
mod deadline;
mod fs;
mod host;
mod http;
mod limits;
mod net;
pub mod prim;
mod reactor;
#[cfg(windows)]
mod windows_pipe;

use std::sync::Arc;
use std::task::Poll;

use hd_cache::CacheStore;
use hd_run::{Engine, HostSetup, Instance, LoadError, Outcome, StartError};
use wasmtime::{Linker, Store, Trap, TypedFunc};

pub use host::HostIo;
use host::{Host, Stop};

/// The wasmtime configuration of §18.1, as data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineConfig {
    pub cranelift_opt: OptLevel,
    /// `Config::max_wasm_stack`: the profile's stack limit.
    pub max_wasm_stack: usize,
    /// The initial GC heap of a store (§17.8; E9 decides the default).
    pub gc_heap_initial: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptLevel {
    /// The dev pipeline: no optimization, backtracking register allocator
    /// (spike T1 has not chosen the single-pass allocator; §18.6).
    None,
    /// The optimized pipeline, `--release`.
    Speed,
}

impl EngineConfig {
    /// The dev pipeline (`hd run`), or with `release` the optimized one.
    #[must_use]
    pub fn pipeline(release: bool) -> EngineConfig {
        EngineConfig {
            cranelift_opt: if release {
                OptLevel::Speed
            } else {
                OptLevel::None
            },
            max_wasm_stack: 4 << 20,
            gc_heap_initial: 64 << 20,
        }
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig::pipeline(false)
    }
}

/// The native stack of the thread a program runs on: its Wasm stack limit
/// and room for the host and the compiler. A caller runs `run` (or the
/// `Engine` methods) on a thread with at least this much stack.
#[must_use]
pub fn thread_stack(config: &EngineConfig) -> usize {
    config.max_wasm_stack + (8 << 20)
}

pub struct WasmtimeEngine {
    pub config: EngineConfig,
    engine: Arc<wasmtime::Engine>,
}

impl WasmtimeEngine {
    /// The engine of `config`, or the reason wasmtime refused it.
    pub fn new(config: EngineConfig) -> Result<WasmtimeEngine, String> {
        host::start_clock();
        let mut c = wasmtime::Config::new();
        c.wasm_gc(true)
            .wasm_function_references(true)
            .wasm_reference_types(true)
            .wasm_multi_value(true)
            .wasm_bulk_memory(true)
            .wasm_extended_const(true)
            .wasm_tail_call(false)
            .wasm_exceptions(false)
            .wasm_stack_switching(false)
            .collector(wasmtime::Collector::Copying)
            .gc_heap_initial_size(config.gc_heap_initial)
            .epoch_interruption(true)
            .max_wasm_stack(config.max_wasm_stack)
            // No fiber runs Wasm (no async support); wasmtime still checks
            // the Wasm stack against this size.
            .async_stack_size(config.max_wasm_stack)
            .parallel_compilation(true)
            .cranelift_opt_level(match config.cranelift_opt {
                OptLevel::None => wasmtime::OptLevel::None,
                OptLevel::Speed => wasmtime::OptLevel::Speed,
            });
        let engine = Arc::new(wasmtime::Engine::new(&c).map_err(|e| e.to_string())?);
        Ok(WasmtimeEngine { config, engine })
    }

    /// Instantiates `m` with its standard streams on `io`.
    pub fn instantiate_with(
        &self,
        m: &Module,
        setup: &HostSetup,
        io: HostIo,
    ) -> Result<WasmtimeInstance, StartError> {
        let host = Host::new(
            io,
            setup.args.clone(),
            setup.program.clone(),
            setup.grants.clone(),
            setup.limits.wall_ms,
            setup.limits.memory_bytes,
        );
        // Store metadata is engine infrastructure, outside the guest's
        // linear/GC backing-byte budget. Its typed allocation failure is a
        // startup error, not a guest heap-exhausted panic or a Rust panic.
        let mut store = Store::try_new(&self.engine, host).map_err(|error| {
            StartError::Engine(format!("cannot create the wasmtime store: {error:#}"))
        })?;
        store.limiter(|host| &mut host.heap);
        if setup.limits.wall_ms.is_some() {
            deadline::register(&self.engine).map_err(StartError::Engine)?;
            store.epoch_deadline_callback(|cx| {
                cx.data().check_deadline()?;
                Ok(wasmtime::UpdateDeadline::Continue(1))
            });
            store.set_epoch_deadline(1);
        } else {
            store.epoch_deadline_trap();
            store.set_epoch_deadline(u64::MAX / 2);
        }
        let mut linker = Linker::new(&self.engine);
        host::define(&mut linker, &m.0).map_err(StartError::Engine)?;
        let instance = match linker.instantiate(&mut store, &m.0) {
            Ok(instance) => instance,
            Err(error)
                if limits::is_heap_exhausted(&error)
                    || matches!(error.downcast_ref::<Stop>(), Some(Stop::TimeLimit)) =>
            {
                let mut stopped = WasmtimeInstance {
                    store,
                    _engine: Arc::clone(&self.engine),
                    init: None,
                    poll: None,
                    wake: None,
                    done: None,
                };
                stopped.trapped(&error);
                return Ok(stopped);
            }
            Err(error) => return Err(StartError::Engine(error.to_string())),
        };
        let memory = instance.get_memory(&mut store, "hd.x");
        store.data_mut().memory = memory;
        let export = |store: &mut Store<Host>, name: &str| {
            instance
                .get_func(&mut *store, name)
                .ok_or_else(|| StartError::Engine(format!("the module has no export `{name}`")))
        };
        let init = export(&mut store, "hd.init")?
            .typed::<(), ()>(&store)
            .map_err(|e| StartError::Engine(format!("`hd.init`: {e}")))?;
        let poll = export(&mut store, "hd.poll")?
            .typed::<(), i32>(&store)
            .map_err(|e| StartError::Engine(format!("`hd.poll`: {e}")))?;
        let wake = match instance.get_func(&mut store, "hd.wake") {
            Some(f) => Some(
                f.typed::<i32, ()>(&store)
                    .map_err(|e| StartError::Engine(format!("`hd.wake`: {e}")))?,
            ),
            None => None,
        };
        Ok(WasmtimeInstance {
            store,
            _engine: Arc::clone(&self.engine),
            init: Some(init),
            poll: Some(poll),
            wake,
            done: None,
        })
    }
}

/// A compiled module.
pub struct Module(wasmtime::Module);

impl Engine for WasmtimeEngine {
    type Module = Module;
    fn load(&self, wasm: &[u8], _cache: &dyn CacheStore) -> Result<Module, LoadError> {
        if wasm.get(..4) != Some(b"\0asm") {
            return Err(LoadError::NotHd("not a Wasm module".into()));
        }
        let imports = hd_run::module_imports(wasm).map_err(LoadError::NotHd)?;
        if let Some((m, n)) = imports.iter().find(|(m, n)| !host::supported(m, n)) {
            return Err(LoadError::Engine(host::unsupported_message(m, n)));
        }
        wasmtime::Module::new(&self.engine, wasm)
            .map(Module)
            .map_err(|e| LoadError::Engine(format!("{e:#}")))
    }
    fn instantiate(&self, m: &Module, host: &HostSetup) -> Result<Box<dyn Instance>, StartError> {
        Ok(Box::new(self.instantiate_with(
            m,
            host,
            HostIo::process(),
        )?))
    }
}

/// A running program.
pub struct WasmtimeInstance {
    store: Store<Host>,
    /// Keep the ticker's weak registration alive for the entire instance.
    /// Store owns Wasmtime's internal Engine clone, not this outer Arc; an
    /// embedding may drop `WasmtimeEngine` while its instance is still running.
    _engine: Arc<wasmtime::Engine>,
    init: Option<TypedFunc<(), ()>>,
    poll: Option<TypedFunc<(), i32>>,
    wake: Option<TypedFunc<i32, ()>>,
    /// How the run ended, once it has.
    done: Option<Outcome>,
}

/// V8's text of a trap, which the Node host prints as an internal error.
fn trap_text(trap: Trap) -> String {
    match trap {
        Trap::UnreachableCodeReached => "unreachable",
        Trap::MemoryOutOfBounds => "memory access out of bounds",
        Trap::IntegerDivisionByZero => "divide by zero",
        Trap::IntegerOverflow => "divide result unrepresentable",
        Trap::BadConversionToInteger => "float unrepresentable in integer range",
        Trap::NullReference => "dereferencing a null pointer",
        Trap::CastFailure => "illegal cast",
        Trap::ArrayOutOfBounds => "array element access out of bounds",
        Trap::AllocationTooLarge => "requested new array is too large",
        other => return other.to_string(),
    }
    .to_owned()
}

impl WasmtimeInstance {
    /// Ends the run with status `status`, writing the held output.
    fn end(&mut self, status: i32) -> Outcome {
        self.store.data_mut().flush();
        self.store.data_mut().close();
        let code = u8::try_from(status & 0xff).unwrap_or(1);
        let o = Outcome::Exit(code);
        self.done = Some(o.clone());
        o
    }

    /// Reports a trap as the Node host does and ends the run with status 3.
    fn trapped(&mut self, e: &wasmtime::Error) -> Outcome {
        let host = self.store.data_mut();
        host.flush();
        if limits::is_heap_exhausted(e) {
            host.err("panic: heap-exhausted: the program heap cannot allocate more memory\n");
        } else if let Some(stop) = e.downcast_ref::<Stop>() {
            match stop {
                Stop::Contract(m) => host.err(&format!("panic: host-contract: {m}\n")),
                Stop::HeapExhausted => {
                    host.err("panic: heap-exhausted: the exchange buffer cannot grow\n");
                }
                Stop::TimeLimit => {
                    host.err("panic: time-limit: the program exceeded its time limit\n");
                }
                Stop::Internal(m) => host.err(&format!("internal error: {m}\n")),
                Stop::Deadlock => {}
            }
        } else if let Some(Trap::StackOverflow) = e.downcast_ref::<Trap>() {
            host.err(
                "panic: stack-exhausted: the call stack ran out; check for recursion that never ends\n",
            );
        } else if !host.reported {
            let text = match e.downcast_ref::<Trap>() {
                Some(t) => trap_text(*t),
                None => format!("{e:#}"),
            };
            host.err(&format!("internal error: {text}\n"));
        }
        self.end(3)
    }

    /// One `hd.poll`: the status, pending, or the end of a trap.
    fn step(&mut self) -> Poll<Outcome> {
        if let Err(error) = self.store.data().check_deadline() {
            return Poll::Ready(self.trapped(&error));
        }
        match self
            .poll
            .as_ref()
            .expect("a live instance has hd.poll")
            .call(&mut self.store, ())
        {
            Ok(-1) => {
                if self.store.data().waiting() {
                    Poll::Pending
                } else {
                    let e = host::deadlock(self.store.data_mut());
                    Poll::Ready(self.trapped(&e))
                }
            }
            Ok(status) => match self.store.data().check_deadline() {
                Ok(()) => Poll::Ready(self.end(status)),
                Err(error) => Poll::Ready(self.trapped(&error)),
            },
            Err(e) => Poll::Ready(self.trapped(&e)),
        }
    }
}

impl Instance for WasmtimeInstance {
    fn init(&mut self) -> Outcome {
        if let Some(outcome) = &self.done {
            return outcome.clone();
        }
        if let Err(error) = self.store.data().check_deadline() {
            return self.trapped(&error);
        }
        match self
            .init
            .as_ref()
            .expect("a live instance has hd.init")
            .call(&mut self.store, ())
        {
            Ok(()) => match self.store.data().check_deadline() {
                Ok(()) => Outcome::Exit(0),
                Err(error) => self.trapped(&error),
            },
            Err(e) => self.trapped(&e),
        }
    }
    fn poll(&mut self) -> Poll<Outcome> {
        if let Some(o) = &self.done {
            return Poll::Ready(o.clone());
        }
        self.step()
    }
    fn wake(&mut self, handles: &[u32]) {
        let Some(wake) = self.wake.clone().filter(|_| self.done.is_none()) else {
            return;
        };
        let n = i32::try_from(handles.len()).unwrap_or(i32::MAX);
        if let Err(e) = wake.call(&mut self.store, n) {
            self.trapped(&e);
        }
    }
    fn call_test(&mut self, _: u32) -> Poll<Outcome> {
        Poll::Ready(Outcome::Internal(
            "test cases run on the Node engine".into(),
        ))
    }
    /// Waits for at least one completion and writes the handles to the
    /// exchange buffer, as `hd.wake` reads them. `poll` checked that
    /// something is pending.
    fn completed(&mut self) -> Vec<u32> {
        loop {
            match host::complete(&mut self.store) {
                Ok(handles) if handles.is_empty() => {}
                Ok(handles) => {
                    return handles
                        .into_iter()
                        .map(|h| u32::try_from(h).unwrap_or(0))
                        .collect();
                }
                // The run is over; the next poll returns how.
                Err(e) => {
                    self.trapped(&e);
                    return vec![0];
                }
            }
            match self.store.data().wait() {
                Ok(true) => {}
                Ok(false) => return Vec::new(),
                Err(error) => {
                    self.trapped(&error);
                    return vec![0];
                }
            }
        }
    }
}

/// The streams and status of a run whose output a caller keeps.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Captured {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub status: u8,
}

/// A shared buffer that a run writes one stream to.
#[derive(Clone, Default)]
struct Shared(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Shared {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("poisoned"))?
            .extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Shared {
    fn take(&self) -> Vec<u8> {
        self.0
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default()
    }
}

/// Runs `wasm` to its end on a thread of its own, with its standard
/// output and error kept and its standard input closed: what `hd run`
/// prints and its status, for tests and the conformance runner. `Err` is
/// a module wasmtime does not run (an import not on wasmtime yet, or a
/// module it rejects).
pub fn run_captured(
    config: EngineConfig,
    wasm: &[u8],
    setup: &HostSetup,
) -> Result<Captured, String> {
    let (out, err) = (Shared::default(), Shared::default());
    let io = HostIo {
        out: Box::new(out.clone()),
        err: Box::new(err.clone()),
        input: None,
    };
    let outcome = run(config, wasm, setup, io)?;
    let status = match outcome {
        Outcome::Exit(code) => code,
        Outcome::Panic(p) => {
            return Err(format!("panic: {}: {}", p.category, p.message));
        }
        Outcome::Internal(e) => return Err(e),
    };
    Ok(Captured {
        stdout: out.take(),
        stderr: err.take(),
        status,
    })
}

/// Runs `wasm` to its end on a thread with the stack it needs, its
/// standard streams on `io`.
pub fn run(
    config: EngineConfig,
    wasm: &[u8],
    setup: &HostSetup,
    io: HostIo,
) -> Result<Outcome, String> {
    run_with_guards(config, wasm, setup, io, || (), || ())
}

/// Runs with caller-owned guards around native compilation and guest execution.
/// The guest guard remains alive until the instance and its host are dropped.
pub fn run_with_guards<P, G>(
    mut config: EngineConfig,
    wasm: &[u8],
    setup: &HostSetup,
    io: HostIo,
    preparing: impl FnOnce() -> P + Send,
    guest: impl FnOnce() -> G + Send,
) -> Result<Outcome, String> {
    // A bounded store grows lazily rather than reserving the profile's
    // initial 64 MiB before the guest has allocated anything.
    if setup.limits.memory_bytes != u64::MAX {
        config.gc_heap_initial = 0;
    }
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("hd-wasmtime".into())
            .stack_size(thread_stack(&config))
            .spawn_scoped(scope, move || {
                let preparing = preparing();
                let engine = WasmtimeEngine::new(config)?;
                let module = engine
                    .load(wasm, &hd_cache::MemoryStore::default())
                    .map_err(|e| match e {
                        LoadError::NotHd(m) | LoadError::Engine(m) => m,
                    })?;
                drop(preparing);
                let _guest = guest();
                let mut inst =
                    engine
                        .instantiate_with(&module, setup, io)
                        .map_err(|e| match e {
                            StartError::Refused(m) | StartError::Engine(m) => m,
                        })?;
                Ok(match inst.init() {
                    Outcome::Exit(0) => hd_run::drive(&mut inst, usize::MAX),
                    other => other,
                })
            })
            .map_err(|e| e.to_string())?
            .join()
            .map_err(|_| "the wasmtime thread panicked".to_owned())?
    })
}

#[cfg(test)]
mod tests {
    use super::{EngineConfig, WasmtimeEngine};
    use hd_cache::MemoryStore;
    use hd_run::Engine;

    #[test]
    fn refuses_what_is_not_wasm_and_imports_not_on_wasmtime() {
        let e = WasmtimeEngine::new(EngineConfig::default()).expect("engine");
        assert!(e.load(b"not wasm", &MemoryStore::default()).is_err());
        // The test-runner providers have not been ported yet.
        let mut m = b"\0asm\x01\0\0\0".to_vec();
        m.extend([1, 4, 1, 0x60, 0, 0]);
        let module = b"hd:TestRunner";
        let name = b"temp_dir";
        let mut body = vec![1, u8::try_from(module.len()).expect("len")];
        body.extend(module);
        body.push(u8::try_from(name.len()).expect("len"));
        body.extend(name);
        body.extend([0, 0]);
        m.push(2);
        m.push(u8::try_from(body.len()).expect("len"));
        m.extend(body);
        let Err(hd_run::LoadError::Engine(msg)) = e.load(&m, &MemoryStore::default()) else {
            panic!("an import not on wasmtime loads");
        };
        assert!(
            msg.contains("`TestRunner`") && msg.contains("not yet on wasmtime"),
            "{msg}"
        );
    }
}
