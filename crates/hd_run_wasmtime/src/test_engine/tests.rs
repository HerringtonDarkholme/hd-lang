use super::{NativeBodyRunner, TestEngine, TestEnvironment, TestHostKind, TestPoolConfig};
use crate::host::HostIo;
use crate::testing::{BodyRunner, State, TestCase};
use crate::{EngineConfig, WasmtimeEngine};
use hd_cache::MemoryStore;
use hd_run::{Engine, Grants, HostSetup, Instance, Limits, Outcome};
use std::io;
use std::sync::Arc;
use std::time::Duration;
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
    // Blocked on a FIFO, which only Unix has.
    #[cfg(unix)]
    Snapshot,
    #[cfg(unix)]
    Regression,
    // Runs `/bin/pwd`, which only Unix has.
    #[cfg(unix)]
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
        #[cfg(unix)]
        Body::Snapshot => {
            imports.import("hd:TestRunner", "snapshot_check", EntityType::Function(5));
            2
        }
        #[cfg(unix)]
        Body::Regression => {
            imports.import("hd:PropertyRunner", "start", EntityType::Function(4));
            2
        }
        #[cfg(unix)]
        Body::ProcessCwd => {
            imports.import("hd:Process", "run.start", EntityType::Function(6));
            imports.import("hd:rt", "block", EntityType::Function(1));
            imports.import("hd:Process", "run.finish", EntityType::Function(5));
            4
        }
        _ => 1,
    };
    let mut functions = FunctionSection::new();
    functions.function(0).function(5).function(5);
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
    // `ProcessCwd` keeps a start's status and value in locals 1 and 2.
    let mut poll = Function::new(poll_locals(body));
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
        #[cfg(unix)]
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
        #[cfg(unix)]
        Body::Regression => {
            poll.instruction(&I::I32Const(1))
                .instruction(&I::I32Const(0))
                .instruction(&I::I32Const(0))
                .instruction(&I::Call(1))
                .instruction(&I::Drop);
        }
        #[cfg(unix)]
        Body::ProcessCwd => {
            // A pending run waits in `block`, then `.finish` gives its result.
            poll.instruction(&I::I32Const(i32::try_from(pwd_arguments().len()).unwrap()))
                .instruction(&I::Call(1))
                .instruction(&I::LocalSet(2))
                .instruction(&I::LocalSet(1))
                .instruction(&I::LocalGet(1))
                .instruction(&I::If(BlockType::Empty))
                .instruction(&I::Call(2))
                .instruction(&I::Drop)
                .instruction(&I::LocalGet(2))
                .instruction(&I::Call(3))
                .instruction(&I::LocalSet(2))
                .instruction(&I::End)
                .instruction(&I::LocalGet(2))
                .instruction(&I::Call(0));
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
    #[cfg(unix)]
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

fn poll_locals(body: Body) -> Vec<(u32, ValType)> {
    #[cfg(unix)]
    if matches!(body, Body::ProcessCwd) {
        return vec![(2, ValType::I32)];
    }
    let _ = body;
    vec![]
}

#[cfg(unix)]
fn pwd_arguments() -> Vec<u8> {
    use crate::codec::Enc;
    let mut arguments = Enc::default();
    arguments.str("/bin/pwd").leb(0).bytes(&[]);
    arguments.done().to_vec()
}

#[cfg(unix)]
#[test]
fn named_child_external_process_uses_case_cwd_without_changing_parent() {
    use super::{NamedProgram, process};
    use crate::codec::{Dec, Enc};
    use std::time::Instant;
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

/// `hd test` records no journal: only a REPL session does.
#[test]
fn a_test_body_records_no_journal() {
    let engine = engine();
    let mut runner = runner(&engine, &wasm(Body::Counter, false), Limits::default());
    let state = State::new(case());
    state.begin_body(0, 0);
    assert_eq!(runner.run_body(state, false).unwrap().stderr, "1");
    let instance = runner.instance.as_ref().expect("a retained instance");
    assert!(instance.store.data().journal.is_none());
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
    crate::testing::execute_case(&case(), &mut first, &mut |result| results.push(result)).unwrap();
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
fn identical_bytes_share_one_compilation_per_engine() {
    let pool = engine();
    let bytes = wasm(Body::Counter, false);
    let first = pool.compile(&bytes).unwrap();
    let second = pool.compile(&bytes).unwrap();
    assert!(Arc::ptr_eq(&first.0.module, &second.0.module));
    // Pool capacity is per engine, so another engine of the same shape
    // compiles separately — while each wrapper still owns its import
    // table, as before.
    let other = engine();
    let third = other.compile(&bytes).unwrap();
    assert!(!Arc::ptr_eq(&first.0.module, &third.0.module));
    assert!(other.prepare(&first, TestHostKind::Unit).is_err());
    assert!(other.prepare(&third, TestHostKind::Unit).is_ok());
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
