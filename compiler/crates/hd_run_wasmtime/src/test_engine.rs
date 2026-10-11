//! Compiled test programs and pre-resolved host kinds. Workers own stores;
//! modules, import resolution, and the bounded pool are shared by clones.

use std::collections::HashMap;
use std::io::{self, Cursor, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use std::time::Instant;

use hd_cache::MemoryStore;
use hd_run::{Engine, Grants, HostSetup, Instance, Outcome};
use wasmtime::{InstanceAllocationStrategy, InstancePre, Linker, PoolingAllocationConfig};

use crate::codec::{Dec, Enc};
use crate::host::{self, Host, HostIo, Stop};
use crate::testing::{BodyResult, BodyRunner, State, TestCase};
use crate::{EngineConfig, Module, RuntimeProfile, Shared, WasmtimeEngine, WasmtimeInstance};

/// Test-only providers are explicit, and never accepted by ordinary engines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TestHostKind {
    Unit,
    Integration,
    Doc(RuntimeProfile),
}

impl TestHostKind {
    pub(crate) fn profile(self) -> RuntimeProfile {
        match self {
            Self::Doc(profile) => profile,
            Self::Unit | Self::Integration => RuntimeProfile::Default,
        }
    }
}

/// Slots bound simultaneous stores, not the amount of committed memory.
/// The combined linear/GC backing-byte cap remains a per-store limiter.
#[derive(Clone, Copy, Debug)]
pub struct TestPoolConfig {
    pub workers: u32,
    pub shrink_spares: u32,
    pub max_memory_bytes: u64,
}

struct Inner {
    engine: Arc<wasmtime::Engine>,
    max_memory_bytes: u64,
}

#[derive(Clone)]
pub struct TestEngine(Arc<Inner>);

struct Compiled {
    module: wasmtime::Module,
    engine: Arc<Inner>,
    prepared: Mutex<HashMap<TestHostKind, InstancePre<Host>>>,
}

#[derive(Clone)]
pub struct TestModule(Arc<Compiled>);

#[derive(Clone)]
pub struct PreparedTest {
    engine: Arc<Inner>,
    pre: InstancePre<Host>,
}

impl TestEngine {
    pub fn new(mut config: EngineConfig, pool: TestPoolConfig) -> Result<Self, String> {
        if pool.workers == 0 {
            return Err("the test pool needs at least one worker".into());
        }
        let slots = pool
            .workers
            .checked_add(pool.shrink_spares)
            .ok_or_else(|| "the test pool slot count overflows".to_owned())?;
        let memories = slots
            .checked_mul(2)
            .ok_or_else(|| "the test memory slot count overflows".to_owned())?;
        // Test heaps grow lazily. A fixed initial heap would falsely consume
        // small explicit budgets before a case allocates anything.
        config.gc_heap_initial = 0;
        let mut config = crate::wasmtime_config(config);
        let mut allocator = PoolingAllocationConfig::default();
        allocator
            .total_core_instances(slots)
            // One exchange linear memory and one GC backing memory per store.
            // Wasmtime 49 allocates both from the same memory-slot pool.
            .total_memories(memories)
            .total_tables(slots)
            .total_gc_heaps(slots);
        // Pool GC and linear reservations must agree in pinned Wasmtime 49.
        // These are virtual reservations, not eagerly committed 4 GiB heaps.
        config
            .memory_reservation(1 << 32)
            .gc_heap_reservation(1 << 32)
            .memory_reservation_for_growth(0)
            .gc_heap_reservation_for_growth(0)
            .allocation_strategy(InstanceAllocationStrategy::Pooling(allocator));
        let engine = Arc::new(wasmtime::Engine::new(&config).map_err(|error| error.to_string())?);
        crate::deadline::register(&engine)?;
        Ok(Self(Arc::new(Inner {
            engine,
            max_memory_bytes: pool.max_memory_bytes,
        })))
    }

    /// Compiles once; kind-specific imports are checked while preparing.
    pub fn compile(&self, wasm: &[u8]) -> Result<TestModule, String> {
        if wasm.get(..4) != Some(b"\0asm") {
            return Err("not a Wasm module".into());
        }
        let module =
            wasmtime::Module::new(&self.0.engine, wasm).map_err(|error| format!("{error:#}"))?;
        Ok(TestModule(Arc::new(Compiled {
            module,
            engine: Arc::clone(&self.0),
            prepared: Mutex::default(),
        })))
    }

    /// Resolves one import table per program/kind, shared by every worker.
    pub fn prepare(&self, module: &TestModule, kind: TestHostKind) -> Result<PreparedTest, String> {
        if !Arc::ptr_eq(&self.0, &module.0.engine) {
            return Err("the test module belongs to a different engine".into());
        }
        let mut prepared = module
            .0
            .prepared
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let pre = if let Some(pre) = prepared.get(&kind) {
            pre.clone()
        } else {
            let mut linker = Linker::new(&self.0.engine);
            host::define_test(&mut linker, &module.0.module, kind)?;
            let pre = linker
                .instantiate_pre(&module.0.module)
                .map_err(|error| format!("{error:#}"))?;
            prepared.insert(kind, pre.clone());
            pre
        };
        Ok(PreparedTest {
            engine: Arc::clone(&self.0),
            pre,
        })
    }
}

/// A package executable compiled once, on an ordinary on-demand engine.
/// Running it cannot exhaust a parent's test-pool slot while that case waits.
#[derive(Clone)]
pub struct NamedProgram {
    name: String,
    engine: Arc<WasmtimeEngine>,
    module: Arc<Module>,
    grants: Grants,
}

impl NamedProgram {
    pub fn compile(
        mut config: EngineConfig,
        name: String,
        wasm: &[u8],
        grants: Grants,
    ) -> Result<Self, String> {
        config.gc_heap_initial = 0;
        let engine = Arc::new(WasmtimeEngine::new(config)?);
        let module = Arc::new(
            engine
                .load(wasm, &MemoryStore::default())
                .map_err(|error| format!("{error:?}"))?,
        );
        Ok(Self {
            name,
            engine,
            module,
            grants,
        })
    }
}

/// Per-host cwd avoids process-global chdir, including concurrent packages.
#[derive(Clone)]
pub struct TestEnvironment {
    pub cwd: PathBuf,
    pub program: String,
    pub children: Vec<NamedProgram>,
}

pub struct NativeBodyRunner {
    prepared: PreparedTest,
    case: TestCase,
    environment: TestEnvironment,
    setup: HostSetup,
    instance: Option<WasmtimeInstance>,
    out: Shared,
    err: Shared,
    deadline: Option<(u32, Option<Instant>)>,
}

impl NativeBodyRunner {
    #[must_use]
    pub fn new(
        prepared: PreparedTest,
        case: TestCase,
        environment: TestEnvironment,
        setup: HostSetup,
    ) -> Self {
        Self {
            prepared,
            case,
            environment,
            setup,
            instance: None,
            out: Shared::default(),
            err: Shared::default(),
            deadline: None,
        }
    }

    fn fresh(&self, state: State) -> Result<WasmtimeInstance, String> {
        let mut host = Host::new(
            HostIo {
                out: Box::new(self.out.clone()),
                err: Box::new(self.err.clone()),
                input: None,
            },
            self.setup.args.clone(),
            self.environment.program.clone(),
            self.setup.grants.clone(),
            None,
            self.setup
                .limits
                .memory_bytes
                .min(self.prepared.engine.max_memory_bytes),
        );
        host.testing = Some(state);
        host.environment = Some(self.environment.clone());
        host.set_deadline(self.deadline.and_then(|(_, deadline)| deadline));
        let mut store = crate::new_store(&self.prepared.engine.engine, host)
            .map_err(|error| format!("{error:?}"))?;
        // Dynamic report_timeout works even when no CLI limit was supplied.
        store.epoch_deadline_callback(|cx| {
            cx.data().check_deadline()?;
            Ok(wasmtime::UpdateDeadline::Continue(1))
        });
        store.set_epoch_deadline(1);
        let instance = match self.prepared.pre.instantiate(&mut store) {
            Ok(instance) => instance,
            Err(error)
                if crate::limits::is_heap_exhausted(&error)
                    || matches!(error.downcast_ref::<Stop>(), Some(Stop::TimeLimit)) =>
            {
                let mut instance =
                    WasmtimeInstance::stopped(store, Arc::clone(&self.prepared.engine.engine));
                instance.trapped(&error);
                return Ok(instance);
            }
            Err(error) => return Err(format!("{error:#}")),
        };
        let mut instance = WasmtimeInstance::from_instance(
            store,
            Arc::clone(&self.prepared.engine.engine),
            instance,
            &format!("hd.init.{}", self.case.init),
            &format!("hd.test.{}", self.case.test),
        )
        .map_err(|error| format!("{error:?}"))?;
        instance.close_on_end = false;
        Ok(instance)
    }
}

impl BodyRunner for NativeBodyRunner {
    fn run_body(&mut self, state: State, reuse: bool) -> Result<BodyResult, String> {
        let start = Instant::now();
        let row = state.current_row();
        if self.deadline.is_none_or(|(previous, _)| previous != row) {
            self.deadline = Some((
                row,
                self.setup
                    .limits
                    .wall_ms
                    .and_then(|millis| start.checked_add(Duration::from_millis(millis))),
            ));
        }
        state.set_io_deadline(self.deadline.and_then(|(_, deadline)| deadline));
        let reusable = reuse
            && self.instance.as_ref().is_some_and(|instance| {
                instance.done == Some(Outcome::Exit(0)) && !instance.last_trapped
            });
        if reusable {
            let instance = self.instance.as_mut().expect("reusable instance");
            instance.done = None;
            instance.store.data_mut().reported = false;
            instance.store.data_mut().begin_test_body();
        } else {
            self.instance = None;
            self.instance = Some(self.fresh(state.clone())?);
        }
        let instance = self.instance.as_mut().expect("instance created");
        let outcome = if reusable || instance.init() == Outcome::Exit(0) {
            hd_run::drive(instance, usize::MAX)
        } else {
            instance
                .done
                .clone()
                .unwrap_or_else(|| Outcome::Internal("test init did not finish".into()))
        };
        instance.store.data_mut().flush();
        let mut trapped = instance.last_trapped;
        let status = match outcome {
            Outcome::Exit(status) => i64::from(status),
            Outcome::Panic(report) => {
                trapped = true;
                instance
                    .store
                    .data_mut()
                    .err(&format!("panic: {}: {}\n", report.category, report.message));
                3
            }
            Outcome::Internal(message) => return Err(message),
        };
        let ended = state.ended();
        let result = BodyResult {
            status,
            trapped,
            ended,
            stdout: String::from_utf8_lossy(&self.out.take()).into_owned(),
            stderr: String::from_utf8_lossy(&self.err.take()).into_owned(),
            us: u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX),
            panic: None,
        };
        if trapped || status != 0 || ended {
            self.instance = None;
        }
        Ok(result)
    }

    fn finish(&mut self) {
        self.instance = None;
    }
}

/// Child output is bounded across both streams. The flag is checked after
/// each host call and at epoch checks, so an overflowing child is interrupted.
struct ProcessSink {
    bytes: Shared,
    count: Arc<AtomicUsize>,
    overflow: Arc<AtomicBool>,
    limit: usize,
}

impl Write for ProcessSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let start = self.count.fetch_add(bytes.len(), Ordering::Relaxed);
        let kept = bytes.len().min(self.limit.saturating_sub(start));
        self.bytes.write_all(&bytes[..kept])?;
        if kept != bytes.len() {
            self.overflow.store(true, Ordering::Relaxed);
            return Err(io::ErrorKind::FileTooLarge.into());
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn process(
    bytes: &[u8],
    grants: &Grants,
    environment: &TestEnvironment,
    deadline: Option<Instant>,
    memory_bytes: u64,
) -> wasmtime::Result<Enc> {
    process_with_limit(bytes, grants, environment, deadline, memory_bytes, 1 << 30)
}

fn process_with_limit(
    bytes: &[u8],
    grants: &Grants,
    environment: &TestEnvironment,
    deadline: Option<Instant>,
    memory_bytes: u64,
    output_limit: usize,
) -> wasmtime::Result<Enc> {
    let mut arguments = Dec::new(bytes);
    let name = arguments.str()?;
    let args = arguments.strings()?;
    let input = arguments.bytes()?.to_vec();
    let mut encoded = Enc::default();
    if !grants.covers_name("Process", &name) {
        encoded.leb(1).leb(2);
        return Ok(encoded);
    }
    let Some(child) = environment.children.iter().find(|child| child.name == name) else {
        encoded.leb(1).leb(0);
        return Ok(encoded);
    };
    let out = Shared::default();
    let err = Shared::default();
    let count = Arc::new(AtomicUsize::new(0));
    let overflow = Arc::new(AtomicBool::new(false));
    let result = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("hd-test-process".into())
            .stack_size(crate::thread_stack(&child.engine.config))
            .spawn_scoped(scope, || {
                let mut host = Host::new(
                    HostIo {
                        out: Box::new(ProcessSink {
                            bytes: out.clone(),
                            count: Arc::clone(&count),
                            overflow: Arc::clone(&overflow),
                            limit: output_limit,
                        }),
                        err: Box::new(ProcessSink {
                            bytes: err.clone(),
                            count: Arc::clone(&count),
                            overflow: Arc::clone(&overflow),
                            limit: output_limit,
                        }),
                        input: Some(Box::new(Cursor::new(input))),
                    },
                    args,
                    name.clone(),
                    child.grants.clone(),
                    None,
                    memory_bytes,
                );
                host.environment = Some(TestEnvironment {
                    cwd: environment.cwd.clone(),
                    program: name.clone(),
                    children: Vec::new(),
                });
                // A named child uses the ordinary Process provider, not test names.
                host.set_deadline(deadline);
                host.output_overflow = Some(Arc::clone(&overflow));
                host.output_budget = Some((Arc::clone(&count), output_limit));
                let mut instance = child
                    .engine
                    .instantiate_host(&child.module, host)
                    .map_err(|error| format!("{error:?}"))?;
                Ok::<_, String>(match instance.init() {
                    Outcome::Exit(0) => hd_run::drive(&mut instance, usize::MAX),
                    other => other,
                })
            })
            .map_err(|error| error.to_string())?
            .join()
            .map_err(|_| "the test child thread panicked".to_owned())?
    });
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Err(Stop::TimeLimit.into());
    }
    if overflow.load(Ordering::Relaxed) {
        encoded
            .leb(1)
            .leb(3)
            .str("process output exceeds the host limit");
        return Ok(encoded);
    }
    match result {
        Ok(Outcome::Exit(status)) => {
            encoded
                .leb(0)
                .str(&String::from_utf8_lossy(&out.take()))
                .str(&String::from_utf8_lossy(&err.take()))
                .zz(i64::from(status));
        }
        Ok(Outcome::Panic(_)) => {
            encoded
                .leb(0)
                .str(&String::from_utf8_lossy(&out.take()))
                .str(&String::from_utf8_lossy(&err.take()))
                .zz(3);
        }
        Ok(Outcome::Internal(message)) | Err(message) => {
            encoded.leb(1).leb(3).str(&message);
        }
    }
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hd_run::Limits;
    use wasm_encoder::{
        BlockType, CodeSection, ConstExpr, DataSection, EntityType, ExportKind, ExportSection,
        Function, FunctionSection, GlobalSection, GlobalType, ImportSection, Instruction as I,
        MemorySection, MemoryType, Module as Wasm, TypeSection, ValType,
    };

    #[derive(Clone, Copy)]
    enum Body {
        Counter,
        Timeout,
        ExitThree,
        Snapshot,
        Regression,
        ProcessCwd,
        HeapGrow,
    }

    fn wasm(body: Body, standard: bool) -> Vec<u8> {
        let mut types = TypeSection::new();
        types.ty().function([], []);
        types.ty().function([], [ValType::I32]);
        types.ty().function([ValType::I32], []);
        types.ty().function([ValType::I64], []);
        types
            .ty()
            .function([ValType::I32, ValType::I32, ValType::I32], [ValType::I32]);
        types.ty().function([ValType::I32], [ValType::I32]);
        types
            .ty()
            .function([ValType::I32], [ValType::I32, ValType::I32]);
        let mut imports = ImportSection::new();
        imports.import("hd:rt", "stderr", EntityType::Function(2));
        let count = match body {
            Body::Timeout => {
                imports.import("hd:TestRunner", "report_timeout", EntityType::Function(3));
                2
            }
            Body::Snapshot => {
                imports.import("hd:TestRunner", "snapshot_check", EntityType::Function(5));
                2
            }
            Body::Regression => {
                imports.import("hd:PropertyRunner", "start", EntityType::Function(4));
                2
            }
            Body::ProcessCwd => {
                imports.import("hd:Process", "run.start", EntityType::Function(6));
                2
            }
            _ => 1,
        };
        let mut functions = FunctionSection::new();
        functions.function(0).function(1).function(1);
        let mut globals = GlobalSection::new();
        globals.global(
            GlobalType {
                val_type: ValType::I32,
                mutable: true,
                shared: false,
            },
            &ConstExpr::i32_const(0),
        );
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
        exports.export(
            if standard { "hd.init" } else { "hd.init.2" },
            ExportKind::Func,
            count,
        );
        exports.export(
            if standard { "hd.poll" } else { "hd.test.3" },
            ExportKind::Func,
            count + 1,
        );
        exports.export("hd.test.9", ExportKind::Func, count + 2);
        let mut init = Function::new([]);
        init.instruction(&I::I32Const(48))
            .instruction(&I::GlobalSet(0))
            .instruction(&I::End);
        let mut poll = Function::new([]);
        match body {
            Body::Counter => {
                poll.instruction(&I::GlobalGet(0))
                    .instruction(&I::I32Const(1))
                    .instruction(&I::I32Add)
                    .instruction(&I::GlobalSet(0));
                poll.instruction(&I::I32Const(0))
                    .instruction(&I::GlobalGet(0))
                    .instruction(&I::I32Store8(wasm_encoder::MemArg {
                        offset: 0,
                        align: 0,
                        memory_index: 0,
                    }));
                poll.instruction(&I::I32Const(1)).instruction(&I::Call(0));
            }
            Body::Timeout => {
                poll.instruction(&I::I64Const(10)).instruction(&I::Call(1));
                busy(&mut poll);
            }
            Body::ExitThree => {}
            Body::Snapshot => {
                poll.instruction(&I::I32Const(1))
                    .instruction(&I::Call(1))
                    .instruction(&I::Drop);
            }
            Body::HeapGrow => {
                poll.instruction(&I::I32Const(1))
                    .instruction(&I::MemoryGrow(0))
                    .instruction(&I::Drop);
            }
            Body::Regression => {
                poll.instruction(&I::I32Const(1))
                    .instruction(&I::I32Const(0))
                    .instruction(&I::I32Const(0))
                    .instruction(&I::Call(1))
                    .instruction(&I::Drop);
            }
            Body::ProcessCwd => {
                poll.instruction(&I::I32Const(i32::try_from(pwd_arguments().len()).unwrap()))
                    .instruction(&I::Call(1))
                    .instruction(&I::Call(0))
                    .instruction(&I::Drop);
            }
        }
        poll.instruction(&I::I32Const(if matches!(body, Body::ExitThree) {
            3
        } else {
            0
        }))
        .instruction(&I::End);
        let mut selected = Function::new([]);
        selected.instruction(&I::I32Const(7)).instruction(&I::End);
        let mut code = CodeSection::new();
        code.function(&init).function(&poll).function(&selected);
        let mut data = DataSection::new();
        data.active(0, &ConstExpr::i32_const(0), *b"?");
        if matches!(body, Body::ProcessCwd) {
            data.active(0, &ConstExpr::i32_const(0), pwd_arguments());
        }
        let mut module = Wasm::new();
        module
            .section(&types)
            .section(&imports)
            .section(&functions)
            .section(&memory)
            .section(&globals)
            .section(&exports)
            .section(&code)
            .section(&data);
        module.finish()
    }

    fn busy(function: &mut Function) {
        function
            .instruction(&I::Loop(BlockType::Empty))
            .instruction(&I::Br(0))
            .instruction(&I::End);
    }

    fn pwd_arguments() -> Vec<u8> {
        let mut arguments = Enc::default();
        arguments.str("/bin/pwd").leb(0).bytes(&[]);
        arguments.done().to_vec()
    }

    #[cfg(unix)]
    #[test]
    fn named_child_external_process_uses_case_cwd_without_changing_parent() {
        let owner = State::new(TestCase::default());
        let root = std::path::PathBuf::from(owner.temp_dir().unwrap());
        let parent = std::env::current_dir().unwrap();
        let child = NamedProgram::compile(
            EngineConfig::default(),
            "child".into(),
            &wasm(Body::ProcessCwd, true),
            Grants::default(),
        )
        .unwrap();
        let environment = TestEnvironment {
            cwd: root.clone(),
            program: "test".into(),
            children: vec![child],
        };
        let mut arguments = Enc::default();
        arguments.str("child").leb(0).bytes(&[]);
        let result = process(
            arguments.done(),
            &Grants::default(),
            &environment,
            Instant::now().checked_add(Duration::from_secs(2)),
            64 << 20,
        )
        .unwrap();
        let mut outer = Dec::new(result.done());
        assert_eq!(outer.leb().unwrap(), 0);
        assert_eq!(outer.str().unwrap(), "");
        let nested = outer.str().unwrap();
        assert_eq!(outer.zz().unwrap(), 0);
        let mut inner = Dec::new(nested.as_bytes());
        assert_eq!(inner.leb().unwrap(), 0);
        assert_eq!(
            inner.str().unwrap().trim(),
            root.canonicalize().unwrap().to_str().unwrap()
        );
        assert_eq!(inner.str().unwrap(), "");
        assert_eq!(inner.zz().unwrap(), 0);
        assert_eq!(std::env::current_dir().unwrap(), parent);
    }

    fn engine() -> TestEngine {
        TestEngine::new(
            EngineConfig::default(),
            TestPoolConfig {
                workers: 1,
                shrink_spares: 0,
                max_memory_bytes: u64::MAX,
            },
        )
        .unwrap()
    }

    fn case() -> TestCase {
        TestCase {
            init: 2,
            test: 3,
            kind: "test".into(),
            ..TestCase::default()
        }
    }

    fn runner(engine: &TestEngine, wasm: &[u8], limits: Limits) -> NativeBodyRunner {
        let module = engine.compile(wasm).unwrap();
        let prepared = engine.prepare(&module, TestHostKind::Unit).unwrap();
        NativeBodyRunner::new(
            prepared,
            case(),
            TestEnvironment {
                cwd: std::env::current_dir().unwrap(),
                program: "test".into(),
                children: Vec::new(),
            },
            HostSetup { limits, ..setup() },
        )
    }

    fn setup() -> HostSetup {
        HostSetup {
            providers: Vec::new(),
            grants: Grants::default(),
            limits: Limits::default(),
            args: Vec::new(),
            program: "test".into(),
        }
    }

    #[test]
    fn passing_bodies_retain_globals_without_reinitialization_and_clear_output() {
        let engine = engine();
        let mut runner = runner(&engine, &wasm(Body::Counter, false), Limits::default());
        let state = State::new(case());
        for (ordinal, expected) in ["1", "2", "3"].into_iter().enumerate() {
            state.begin_body(0, ordinal as u64);
            let result = runner.run_body(state.clone(), ordinal != 0).unwrap();
            assert_eq!(result.stderr, expected);
            assert_eq!(result.status, 0);
            assert!(!result.trapped);
        }
        runner.finish();
        assert!(runner.instance.is_none());
    }

    #[test]
    fn replacement_bodies_and_rows_get_fresh_globals() {
        let engine = engine();
        let mut runner = runner(&engine, &wasm(Body::Counter, false), Limits::default());
        let state = State::new(case());
        for ordinal in 0..3 {
            state.begin_body(ordinal, u64::from(ordinal));
            assert_eq!(runner.run_body(state.clone(), false).unwrap().stderr, "1");
        }
    }

    #[test]
    fn execute_case_releases_the_only_slot_while_runner_is_retained() {
        let engine = engine();
        let wasm = wasm(Body::Counter, false);
        let mut first = runner(&engine, &wasm, Limits::default());
        let mut results = Vec::new();
        crate::testing::execute_case(&case(), &mut first, &mut |result| results.push(result))
            .unwrap();
        assert_eq!(results.len(), 1);
        assert!(first.instance.is_none());
        let mut second = runner(&engine, &wasm, Limits::default());
        assert_eq!(
            second.run_body(State::new(case()), false).unwrap().stderr,
            "1"
        );
    }

    #[test]
    fn exit_three_is_not_a_trap() {
        let engine = engine();
        let mut runner = runner(&engine, &wasm(Body::ExitThree, false), Limits::default());
        let result = runner.run_body(State::new(case()), false).unwrap();
        assert_eq!(result.status, 3);
        assert!(!result.trapped);
        assert!(result.stderr.is_empty());
    }

    #[test]
    fn initial_test_memory_over_the_case_cap_is_a_heap_panic() {
        let engine = engine();
        let mut runner = runner(
            &engine,
            &wasm(Body::Counter, false),
            Limits {
                memory_bytes: 0,
                ..Limits::default()
            },
        );
        let result = runner.run_body(State::new(case()), false).unwrap();
        assert_eq!(result.status, 3);
        assert!(result.trapped);
        assert!(
            result.stderr.starts_with("panic: heap-exhausted:"),
            "{}",
            result.stderr
        );
        runner.finish();
        assert!(runner.instance.is_none());
    }

    #[test]
    fn memory_growth_is_capped_independently_for_each_test_row() {
        let engine = engine();
        let mut runner = runner(
            &engine,
            &wasm(Body::HeapGrow, false),
            Limits {
                memory_bytes: 64 << 10,
                ..Limits::default()
            },
        );
        let state = State::new(case());
        for row in 0..2 {
            state.begin_body(row, u64::from(row));
            let result = runner.run_body(state.clone(), false).unwrap();
            assert_eq!(result.status, 3);
            assert!(result.trapped);
            assert!(
                result.stderr.starts_with("panic: heap-exhausted:"),
                "{}",
                result.stderr
            );
        }
    }

    fn bounded_timeout(body: Body, wall_ms: Option<u64>, expected: &'static str) {
        let (send, receive) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let engine = engine();
            let mut runner = runner(
                &engine,
                &wasm(body, false),
                Limits {
                    wall_ms,
                    ..Limits::default()
                },
            );
            send.send(runner.run_body(State::new(case()), false))
                .unwrap();
        });
        let result = receive
            .recv_timeout(Duration::from_secs(3))
            .expect("test deadline must interrupt the worker")
            .unwrap();
        worker.join().unwrap();
        assert_eq!(result.status, 3);
        assert!(result.trapped);
        assert_eq!(result.stderr, expected);
    }

    #[test]
    fn dynamic_report_timeout_interrupts_a_busy_loop_without_a_cli_limit() {
        bounded_timeout(
            Body::Timeout,
            None,
            "panic: time-limit: the test case ran longer than its timeout of 10ms\n",
        );
    }

    #[test]
    fn zero_cli_deadline_wins_over_body_timeout() {
        bounded_timeout(
            Body::Timeout,
            Some(0),
            "panic: time-limit: the program exceeded its time limit\n",
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_file_imports_interrupt_fifos_with_the_time_limit_report() {
        for body in [Body::Snapshot, Body::Regression] {
            let owner = State::new(TestCase::default());
            let root = std::path::PathBuf::from(owner.temp_dir().unwrap());
            let path = root.join("blocked-1.snap");
            nix::unistd::mkfifo(
                &path,
                nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
            )
            .unwrap();
            let mut case = case();
            if matches!(body, Body::Snapshot) {
                case.snapshot = Some(crate::testing::Snapshot {
                    dir: root,
                    shown: None,
                    slug: "blocked".into(),
                    update: false,
                });
            } else {
                case.regression = Some(path);
                case.kind = "property".into();
            }
            let (send, receive) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                let engine = engine();
                let module = engine.compile(&wasm(body, false)).unwrap();
                let prepared = engine.prepare(&module, TestHostKind::Unit).unwrap();
                let mut runner = NativeBodyRunner::new(
                    prepared,
                    case.clone(),
                    TestEnvironment {
                        cwd: std::env::current_dir().unwrap(),
                        program: "test".into(),
                        children: Vec::new(),
                    },
                    HostSetup {
                        limits: Limits {
                            wall_ms: Some(20),
                            ..Limits::default()
                        },
                        ..setup()
                    },
                );
                send.send(runner.run_body(State::new(case), false)).unwrap();
            });
            let result = receive
                .recv_timeout(Duration::from_secs(3))
                .expect("a blocked test-provider file import must respect the deadline")
                .unwrap();
            worker.join().unwrap();
            assert_eq!(result.status, 3);
            assert!(result.trapped);
            assert_eq!(
                result.stderr,
                "panic: time-limit: the program exceeded its time limit\n"
            );
        }
    }

    #[test]
    fn absolute_case_deadline_survives_poisoned_store_replacements() {
        let engine = engine();
        let mut runner = runner(
            &engine,
            &wasm(Body::Counter, false),
            Limits {
                wall_ms: Some(20),
                ..Limits::default()
            },
        );
        let state = State::new(case());
        state.begin_body(0, 0);
        assert_eq!(runner.run_body(state.clone(), false).unwrap().status, 0);
        std::thread::sleep(Duration::from_millis(30));
        state.begin_body(0, 1);
        let result = runner.run_body(state, false).unwrap();
        assert_eq!(result.status, 3);
        assert!(result.trapped);
    }

    #[test]
    fn test_imports_are_explicit_and_kind_specific() {
        let engine = engine();
        let bytes = wasm(Body::Timeout, false);
        let module = engine.compile(&bytes).unwrap();
        engine.prepare(&module, TestHostKind::Unit).unwrap();
        engine.prepare(&module, TestHostKind::Unit).unwrap();
        assert_eq!(module.0.prepared.lock().unwrap().len(), 1);
        assert!(
            WasmtimeEngine::new(EngineConfig::default())
                .unwrap()
                .load(&bytes, &MemoryStore::default())
                .is_err()
        );
        let mut types = TypeSection::new();
        types.ty().function([], [ValType::I32]);
        let mut imports = ImportSection::new();
        imports.import("hd:Random", "next_u64", EntityType::Function(0));
        let mut denied = Wasm::new();
        denied.section(&types).section(&imports);
        let denied = engine.compile(&denied.finish()).unwrap();
        assert!(engine.prepare(&denied, TestHostKind::Unit).is_err());
        assert!(engine.prepare(&denied, TestHostKind::Integration).is_ok());
        let other = self::engine();
        assert!(other.prepare(&module, TestHostKind::Unit).is_err());
    }

    #[test]
    fn ordinary_instance_call_test_selects_the_named_export_after_init() {
        let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
        let module = engine
            .load(&wasm(Body::Counter, true), &MemoryStore::default())
            .unwrap();
        let mut instance = engine
            .instantiate_with(
                &module,
                &setup(),
                HostIo {
                    out: Box::new(io::sink()),
                    err: Box::new(io::sink()),
                    input: None,
                },
            )
            .unwrap();
        assert_eq!(instance.init(), Outcome::Exit(0));
        assert!(matches!(
            instance.call_test(99),
            std::task::Poll::Ready(Outcome::Internal(_))
        ));
        assert_eq!(
            instance.call_test(9),
            std::task::Poll::Ready(Outcome::Exit(7))
        );
        assert_eq!(
            instance.call_test(9),
            std::task::Poll::Ready(Outcome::Exit(7))
        );
    }
}
