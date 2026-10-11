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

mod cache;
mod codec;
mod core;
mod deadline;
mod fs;
mod host;
mod http;
mod limits;
mod native_io;
mod net;
pub mod prim;
mod profiles;
mod property;
mod reactor;
mod test_engine;
pub mod testing;
mod testing_files;
mod testing_json;
#[cfg(windows)]
mod windows_pipe;

use std::sync::Arc;
use std::task::Poll;

use hd_cache::CacheStore;
use hd_run::{Engine, HostSetup, Instance, LoadError, Outcome, StartError};
use wasmtime::{Linker, Store, Trap, TypedFunc};

pub use host::HostIo;
use host::{Host, Stop};
pub use profiles::RuntimeProfile;
pub use test_engine::{
    NamedProgram, NativeBodyRunner, PreparedTest, TestEngine, TestEnvironment, TestHostKind,
    TestModule, TestPoolConfig,
};

/// The wasmtime configuration of §18.1, as data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineConfig {
    pub cranelift_opt: OptLevel,
    /// `Config::max_wasm_stack`: the profile's stack limit.
    pub max_wasm_stack: usize,
    /// The initial GC heap of a store (§17.8; E9 decides the default).
    pub gc_heap_initial: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    profile: RuntimeProfile,
}

impl WasmtimeEngine {
    /// The engine of `config`, or the reason wasmtime refused it.
    pub fn new(config: EngineConfig) -> Result<WasmtimeEngine, String> {
        Self::new_with_profile(config, RuntimeProfile::Default)
    }

    /// Creates an engine with explicitly scoped conformance fixture providers.
    /// Ordinary engines never accept the fixture-only imports.
    pub fn new_with_profile(
        config: EngineConfig,
        profile: RuntimeProfile,
    ) -> Result<WasmtimeEngine, String> {
        host::start_clock();
        let shared = cache::ondemand(&config)?;
        Ok(WasmtimeEngine {
            config,
            engine: Arc::clone(&shared.engine),
            profile,
        })
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
        self.instantiate_host(m, host)
    }

    fn instantiate_host(&self, m: &Module, host: Host) -> Result<WasmtimeInstance, StartError> {
        let mut store = new_store(&self.engine, host)?;
        let mut linker = Linker::new(&self.engine);
        host::define(&mut linker, &m.0, self.profile).map_err(StartError::Engine)?;
        let instance = match linker.instantiate(&mut store, &m.0) {
            Ok(instance) => instance,
            Err(error)
                if limits::is_heap_exhausted(&error)
                    || matches!(error.downcast_ref::<Stop>(), Some(Stop::TimeLimit)) =>
            {
                let mut stopped = WasmtimeInstance::stopped(store, Arc::clone(&self.engine));
                stopped.trapped(&error);
                return Ok(stopped);
            }
            Err(error) => return Err(StartError::Engine(error.to_string())),
        };
        WasmtimeInstance::from_instance(
            store,
            Arc::clone(&self.engine),
            instance,
            "hd.init",
            "hd.poll",
        )
    }
}

pub(crate) fn wasmtime_config(config: EngineConfig) -> wasmtime::Config {
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
    c
}

fn new_store(engine: &Arc<wasmtime::Engine>, host: Host) -> Result<Store<Host>, StartError> {
    // Store metadata is engine infrastructure, outside the guest's
    // linear/GC backing-byte budget. Its typed allocation failure is a
    // startup error, not a guest heap-exhausted panic or a Rust panic.
    let mut store = Store::try_new(engine, host).map_err(|error| {
        StartError::Engine(format!("cannot create the wasmtime store: {error:#}"))
    })?;
    store.limiter(|host| &mut host.heap);
    if store.data().deadline().is_some() {
        deadline::register(engine).map_err(StartError::Engine)?;
        store.epoch_deadline_callback(|cx| {
            cx.data().check_deadline()?;
            Ok(wasmtime::UpdateDeadline::Continue(1))
        });
        store.set_epoch_deadline(1);
    } else {
        store.epoch_deadline_trap();
        store.set_epoch_deadline(u64::MAX / 2);
    }
    Ok(store)
}

impl WasmtimeInstance {
    fn stopped(store: Store<Host>, engine: Arc<wasmtime::Engine>) -> Self {
        Self {
            store,
            _engine: engine,
            init: None,
            instance: None,
            poll: None,
            wake: None,
            done: None,
            close_on_end: true,
            last_trapped: false,
        }
    }

    fn from_instance(
        mut store: Store<Host>,
        engine: Arc<wasmtime::Engine>,
        instance: wasmtime::Instance,
        init_name: &str,
        poll_name: &str,
    ) -> Result<Self, StartError> {
        store.data_mut().memory = instance.get_memory(&mut store, "hd.x");
        let export = |store: &mut Store<Host>, name: &str| {
            instance
                .get_func(&mut *store, name)
                .ok_or_else(|| StartError::Engine(format!("the module has no export `{name}`")))
        };
        let init = export(&mut store, init_name)?
            .typed::<(), ()>(&store)
            .map_err(|e| StartError::Engine(format!("`{init_name}`: {e}")))?;
        let poll = export(&mut store, poll_name)?
            .typed::<(), i32>(&store)
            .map_err(|e| StartError::Engine(format!("`{poll_name}`: {e}")))?;
        let wake = match instance.get_func(&mut store, "hd.wake") {
            Some(f) => Some(
                f.typed::<i32, ()>(&store)
                    .map_err(|e| StartError::Engine(format!("`hd.wake`: {e}")))?,
            ),
            None => None,
        };
        Ok(WasmtimeInstance {
            store,
            _engine: engine,
            init: Some(init),
            instance: Some(instance),
            poll: Some(poll),
            wake,
            done: None,
            close_on_end: true,
            last_trapped: false,
        })
    }
}

/// A compiled module. The artifact is shared: loading identical bytes
/// returns the same compilation (see `cache`).
pub struct Module(Arc<wasmtime::Module>);

impl Engine for WasmtimeEngine {
    type Module = Module;
    fn load(&self, wasm: &[u8], _cache: &dyn CacheStore) -> Result<Module, LoadError> {
        if wasm.get(..4) != Some(b"\0asm") {
            return Err(LoadError::NotHd("not a Wasm module".into()));
        }
        let imports = hd_run::module_imports(wasm).map_err(LoadError::NotHd)?;
        if let Some((m, n)) = imports
            .iter()
            .find(|(m, n)| !host::supported(m, n, self.profile))
        {
            return Err(LoadError::Engine(host::unsupported_message(m, n)));
        }
        let shared = cache::ondemand(&self.config).map_err(LoadError::Engine)?;
        shared
            .cache
            .get_or_compile(&shared.engine, wasm)
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
    instance: Option<wasmtime::Instance>,
    poll: Option<TypedFunc<(), i32>>,
    wake: Option<TypedFunc<i32, ()>>,
    /// How the run ended, once it has.
    done: Option<Outcome>,
    /// A property keeps successful instances and their host state between bodies.
    close_on_end: bool,
    last_trapped: bool,
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
        self.finish(host::Control::Exit {
            status,
            close: self.close_on_end,
        })
    }

    fn finish(&mut self, event: host::Control) -> Outcome {
        let o = self
            .store
            .data_mut()
            .control(event)
            .expect("a run termination returns its outcome");
        self.done = Some(o.clone());
        o
    }

    /// Reports a trap as the Node host does and ends the run with status 3.
    fn trapped(&mut self, e: &wasmtime::Error) -> Outcome {
        if matches!(e.downcast_ref::<Stop>(), Some(Stop::EndCase)) {
            return self.end(0);
        }
        self.last_trapped = true;
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
                    let report = host.time_limit_report();
                    host.err(&report);
                }
                Stop::Internal(m) => host.err(&format!("internal error: {m}\n")),
                Stop::Deadlock => {}
                Stop::EndCase => unreachable!("handled before panic reporting"),
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
        self.finish(host::Control::Trap {
            close: self.close_on_end,
        })
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
        let n = match host::deliver_batch(&mut self.store, handles) {
            Ok(count) => count,
            Err(error) => {
                self.trapped(&error);
                return;
            }
        };
        if let Err(e) = wake.call(&mut self.store, n) {
            self.trapped(&e);
        }
    }
    fn call_test(&mut self, export: u32) -> Poll<Outcome> {
        if let Some(outcome) = &self.done {
            return Poll::Ready(outcome.clone());
        }
        let name = format!("hd.test.{export}");
        let Some(instance) = self.instance else {
            return Poll::Ready(Outcome::Internal("the instance has no live exports".into()));
        };
        let poll = match instance.get_typed_func::<(), i32>(&mut self.store, &name) {
            Ok(poll) => poll,
            Err(error) => return Poll::Ready(Outcome::Internal(format!("`{name}`: {error}"))),
        };
        self.poll = Some(poll);
        self.step()
    }
    /// Waits for at least one completion. The returned batch is delivered
    /// only by wake; no exchange-buffer preload is required or performed.
    fn completed(&mut self) -> Vec<u32> {
        loop {
            match host::complete(&mut self.store) {
                Ok(handles) if handles.is_empty() => {}
                Ok(handles) => return handles,
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
    run_captured_with_profile(config, RuntimeProfile::Default, wasm, setup)
}

/// Runs a module with explicitly selected conformance fixture providers.
/// Standard streams are captured and standard input is closed.
pub fn run_captured_with_profile(
    config: EngineConfig,
    profile: RuntimeProfile,
    wasm: &[u8],
    setup: &HostSetup,
) -> Result<Captured, String> {
    let (out, err) = (Shared::default(), Shared::default());
    let io = HostIo {
        out: Box::new(out.clone()),
        err: Box::new(err.clone()),
        input: None,
    };
    let outcome = run_with_profile(config, profile, wasm, setup, io)?;
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
    run_with_profile(config, RuntimeProfile::Default, wasm, setup, io)
}

/// Runs a module with explicitly selected conformance fixture providers and
/// caller-supplied standard streams, on a thread with the required stack.
pub fn run_with_profile(
    config: EngineConfig,
    profile: RuntimeProfile,
    wasm: &[u8],
    setup: &HostSetup,
    io: HostIo,
) -> Result<Outcome, String> {
    run_profile_with_guards(config, profile, wasm, setup, io, || (), || ())
}

/// Runs with caller-owned guards around native compilation and guest execution.
/// The guest guard remains alive until the instance and its host are dropped.
pub fn run_with_guards<P, G>(
    config: EngineConfig,
    wasm: &[u8],
    setup: &HostSetup,
    io: HostIo,
    preparing: impl FnOnce() -> P + Send,
    guest: impl FnOnce() -> G + Send,
) -> Result<Outcome, String> {
    run_profile_with_guards(
        config,
        RuntimeProfile::Default,
        wasm,
        setup,
        io,
        preparing,
        guest,
    )
}

fn run_profile_with_guards<P, G>(
    mut config: EngineConfig,
    profile: RuntimeProfile,
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
                let engine = WasmtimeEngine::new_with_profile(config, profile)?;
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

    fn malformed_reply_module(
        module: &str,
        name: &str,
        payload: &[u8],
        results: &[wasm_encoder::ValType],
    ) -> Vec<u8> {
        use wasm_encoder::{
            CodeSection, ConstExpr, DataSection, EntityType, ExportKind, ExportSection, Function,
            FunctionSection, ImportSection, Instruction as I, MemorySection, MemoryType,
            Module as Wasm, TypeSection, ValType,
        };
        let mut types = TypeSection::new();
        types.ty().function([ValType::I32], results.iter().copied());
        types.ty().function([], []);
        types.ty().function([], [ValType::I32]);
        let mut imports = ImportSection::new();
        imports.import(module, name, EntityType::Function(0));
        let mut functions = FunctionSection::new();
        functions.function(1).function(2);
        let mut memory = MemorySection::new();
        memory.memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        let mut exports = ExportSection::new();
        exports.export("hd.x", ExportKind::Memory, 0);
        exports.export("hd.init", ExportKind::Func, 1);
        exports.export("hd.poll", ExportKind::Func, 2);
        let mut init = Function::new([]);
        init.instruction(&I::I32Const(i32::try_from(payload.len()).unwrap()))
            .instruction(&I::Call(0));
        for _ in results {
            init.instruction(&I::Drop);
        }
        init.instruction(&I::End);
        let mut poll = Function::new([]);
        poll.instruction(&I::I32Const(0)).instruction(&I::End);
        let mut code = CodeSection::new();
        code.function(&init).function(&poll);
        let mut data = DataSection::new();
        data.active(0, &ConstExpr::i32_const(0), payload.iter().copied());
        let mut wasm = Wasm::new();
        wasm.section(&types)
            .section(&imports)
            .section(&functions)
            .section(&memory)
            .section(&exports)
            .section(&code)
            .section(&data);
        wasm.finish()
    }

    #[test]
    fn malformed_reply_shapes_fail_before_output_or_filesystem_effects() {
        use hd_run::{HostSetup, Instance, Outcome};
        use wasm_encoder::ValType;

        let owner = crate::testing::State::new(crate::testing::TestCase::default());
        let root = std::path::PathBuf::from(owner.temp_dir().unwrap());
        let path = root.join("must-not-be-created");
        let mut request = crate::codec::Enc::default();
        request
            .str(path.to_str().unwrap())
            .str("must-not-be-written");
        let marker = b"must-not-be-output";
        let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
        for (module, name, payload, results) in [
            (
                "hd:Console",
                "write_line.start",
                marker.as_slice(),
                vec![ValType::I32],
            ),
            (
                "hd:Console",
                "write_line.start",
                marker.as_slice(),
                vec![ValType::I32, ValType::F64],
            ),
            ("hd:rt", "stderr", marker.as_slice(), vec![ValType::I32]),
            ("hd:rt", "stderr", marker.as_slice(), vec![ValType::F64]),
            (
                "hd:FsWrite",
                "write_text.start",
                request.done(),
                vec![ValType::I64],
            ),
            (
                "hd:FsWrite",
                "write_text.start",
                request.done(),
                vec![ValType::I32, ValType::F64],
            ),
        ] {
            let wasm = malformed_reply_module(module, name, payload, &results);
            let program = engine.load(&wasm, &MemoryStore::default()).unwrap();
            let out = super::Shared::default();
            let err = super::Shared::default();
            let mut instance = engine
                .instantiate_with(
                    &program,
                    &HostSetup {
                        providers: Vec::new(),
                        grants: hd_run::Grants::default(),
                        limits: hd_run::Limits::default(),
                        args: Vec::new(),
                        program: "malformed-reply-regression".into(),
                    },
                    super::HostIo {
                        out: Box::new(out.clone()),
                        err: Box::new(err.clone()),
                        input: None,
                    },
                )
                .unwrap();
            assert_eq!(instance.init(), Outcome::Exit(3), "{module} {name}");
            // Console start must not even enqueue work that was silently
            // lost at teardown; stdout emptiness alone cannot prove this.
            assert!(!instance.store.data().waiting(), "{module} {name}");
            assert!(out.take().is_empty(), "{module} {name}");
            let error = String::from_utf8(err.take()).unwrap();
            assert!(error.starts_with("panic: host-contract:"), "{error}");
            assert!(!error.contains("must-not-be-output"), "{error}");
            assert!(!path.exists(), "{module} {name}");
        }
    }

    #[test]
    fn wake_delivers_the_supplied_batch_without_a_completed_preload() {
        use hd_run::{HostSetup, Instance, Outcome};
        use std::task::Poll;
        use wasm_encoder::{
            CodeSection, ConstExpr, DataSection, ExportKind, ExportSection, Function,
            FunctionSection, GlobalSection, GlobalType, Instruction as I, MemArg, MemorySection,
            MemoryType, Module as Wasm, TypeSection, ValType,
        };

        let mut types = TypeSection::new();
        types.ty().function([], []);
        types.ty().function([], [ValType::I32]);
        types.ty().function([ValType::I32], []);
        let mut functions = FunctionSection::new();
        functions.function(0).function(1).function(2);
        let mut memory = MemorySection::new();
        memory.memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        let mut globals = GlobalSection::new();
        globals.global(
            GlobalType {
                val_type: ValType::I32,
                mutable: true,
                shared: false,
            },
            &ConstExpr::i32_const(0),
        );
        let mut exports = ExportSection::new();
        exports.export("hd.x", ExportKind::Memory, 0);
        exports.export("hd.init", ExportKind::Func, 0);
        exports.export("hd.poll", ExportKind::Func, 1);
        exports.export("hd.wake", ExportKind::Func, 2);
        let mut init = Function::new([]);
        init.instruction(&I::End);
        let mut poll = Function::new([]);
        poll.instruction(&I::GlobalGet(0)).instruction(&I::End);
        let mut wake = Function::new([]);
        // The guest observes the count and both little-endian handles,
        // not merely memory the host test inspects after the call.
        wake.instruction(&I::LocalGet(0))
            .instruction(&I::I32Const(0))
            .instruction(&I::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }))
            .instruction(&I::I32Add)
            .instruction(&I::I32Const(4))
            .instruction(&I::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }))
            .instruction(&I::I32Add)
            .instruction(&I::GlobalSet(0))
            .instruction(&I::End);
        let mut code = CodeSection::new();
        code.function(&init).function(&poll).function(&wake);
        let mut data = DataSection::new();
        data.active(0, &ConstExpr::i32_const(0), [9, 0, 0, 0, 10, 0, 0, 0]);
        let mut wasm = Wasm::new();
        wasm.section(&types)
            .section(&functions)
            .section(&memory)
            .section(&globals)
            .section(&exports)
            .section(&code)
            .section(&data);
        let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
        let module = engine
            .load(&wasm.finish(), &MemoryStore::default())
            .unwrap();
        let mut instance = engine
            .instantiate_with(
                &module,
                &HostSetup {
                    providers: Vec::new(),
                    grants: hd_run::Grants::default(),
                    limits: hd_run::Limits::default(),
                    args: Vec::new(),
                    program: "wake-batch-regression".into(),
                },
                super::HostIo {
                    out: Box::new(std::io::sink()),
                    err: Box::new(std::io::sink()),
                    input: None,
                },
            )
            .unwrap();
        assert_eq!(instance.init(), Outcome::Exit(0));
        // Do not call completed(): these are supplied/recorded handles,
        // replacing deliberately stale exchange bytes from the data segment.
        instance.wake(&[41, 42]);
        assert_eq!(instance.poll(), Poll::Ready(Outcome::Exit(85)));
    }

    #[test]
    fn empty_wake_batch_does_not_require_an_exchange_buffer() {
        use hd_run::{HostSetup, Instance, Outcome};
        use std::task::Poll;
        use wasm_encoder::{
            BlockType, CodeSection, ExportKind, ExportSection, Function, FunctionSection,
            Instruction as I, Module as Wasm, TypeSection, ValType,
        };

        let mut types = TypeSection::new();
        types.ty().function([], []);
        types.ty().function([], [ValType::I32]);
        types.ty().function([ValType::I32], []);
        let mut functions = FunctionSection::new();
        functions.function(0).function(1).function(2);
        let mut exports = ExportSection::new();
        exports.export("hd.init", ExportKind::Func, 0);
        exports.export("hd.poll", ExportKind::Func, 1);
        exports.export("hd.wake", ExportKind::Func, 2);
        let mut init = Function::new([]);
        init.instruction(&I::End);
        let mut poll = Function::new([]);
        poll.instruction(&I::I32Const(7)).instruction(&I::End);
        let mut wake = Function::new([]);
        // An empty delivery still invokes hd.wake with count zero.
        wake.instruction(&I::LocalGet(0))
            .instruction(&I::If(BlockType::Empty))
            .instruction(&I::Unreachable)
            .instruction(&I::End)
            .instruction(&I::End);
        let mut code = CodeSection::new();
        code.function(&init).function(&poll).function(&wake);
        let mut wasm = Wasm::new();
        wasm.section(&types)
            .section(&functions)
            .section(&exports)
            .section(&code);
        let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
        let module = engine
            .load(&wasm.finish(), &MemoryStore::default())
            .unwrap();
        let mut instance = engine
            .instantiate_with(
                &module,
                &HostSetup {
                    providers: Vec::new(),
                    grants: hd_run::Grants::default(),
                    limits: hd_run::Limits::default(),
                    args: Vec::new(),
                    program: "empty-wake-regression".into(),
                },
                super::HostIo {
                    out: Box::new(std::io::sink()),
                    err: Box::new(std::io::sink()),
                    input: None,
                },
            )
            .unwrap();
        assert_eq!(instance.init(), Outcome::Exit(0));
        instance.wake(&[]);
        assert_eq!(instance.poll(), Poll::Ready(Outcome::Exit(7)));
    }

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
