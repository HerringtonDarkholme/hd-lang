//! Wall deadlines must interrupt Wasm and reactor waits, not hang the test runner.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use hd_cache::MemoryStore;
use hd_run::{Engine, Grants, HostSetup, Instance, Limits, Outcome};
use hd_run_wasmtime::{EngineConfig, HostIo, WasmtimeEngine};
use wasm_encoder::{
    BlockType, CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection,
    ImportSection, Instruction as I, MemorySection, MemoryType, Module, TypeSection, ValType,
};

const CHILD: &str = "HD_M21_TIME_LIMIT_CHILD";
const CHILD_TIMEOUT: Duration = Duration::from_secs(5);
const TIME_PANIC: &[u8] = b"panic: time-limit: the program exceeded its time limit\n";

#[derive(Clone, Copy)]
enum Program {
    BusyInit,
    BusyPoll,
    ClockBlock(i64),
    ClockDrive,
    InputBlock,
    Ready,
}

fn module(program: Program) -> Vec<u8> {
    let mut types = TypeSection::new();
    types.ty().function([], []); // init
    types.ty().function([], [ValType::I32]); // poll/block
    types
        .ty()
        .function([ValType::I64], [ValType::I32, ValType::I32]); // sleep.start
    types.ty().function([ValType::I32], [ValType::I32]); // finish
    types.ty().function([], [ValType::I32, ValType::I32]); // read_line.start
    let mut imports = ImportSection::new();
    let waiting = matches!(
        program,
        Program::ClockBlock(_) | Program::ClockDrive | Program::InputBlock
    );
    let import_count = if waiting {
        let (provider, start, finish, ty) = if matches!(program, Program::InputBlock) {
            ("hd:ConsoleInput", "read_line.start", "read_line.finish", 4)
        } else {
            ("hd:Clock", "sleep.start", "sleep.finish", 2)
        };
        imports.import(provider, start, EntityType::Function(ty));
        imports.import("hd:rt", "block", EntityType::Function(1));
        imports.import(provider, finish, EntityType::Function(3));
        3
    } else {
        0
    };
    let mut functions = FunctionSection::new();
    functions.function(0).function(1);
    let mut memory = MemorySection::new();
    memory.memory(MemoryType {
        minimum: 1,
        maximum: Some(1),
        memory64: false,
        shared: false,
        page_size_log2: None,
    });
    let mut exports = ExportSection::new();
    exports.export("hd.x", ExportKind::Memory, 0);
    exports.export("hd.init", ExportKind::Func, import_count);
    exports.export("hd.poll", ExportKind::Func, import_count + 1);
    let mut init = Function::new([(1, ValType::I32)]);
    let mut poll = Function::new([]);
    if matches!(program, Program::BusyInit) {
        infinite_loop(&mut init);
    }
    if waiting {
        if let Program::ClockBlock(ms) = program {
            init.instruction(&I::I64Const(ms));
        } else if matches!(program, Program::ClockDrive) {
            init.instruction(&I::I64Const(10_000));
        }
        init.instruction(&I::Call(0))
            .instruction(&I::LocalSet(0))
            .instruction(&I::I32Const(1))
            .instruction(&I::I32Ne)
            .instruction(&I::If(BlockType::Empty))
            .instruction(&I::Unreachable)
            .instruction(&I::End);
        if !matches!(program, Program::ClockDrive) {
            init.instruction(&I::Call(1))
                .instruction(&I::Drop)
                .instruction(&I::LocalGet(0))
                .instruction(&I::Call(2))
                .instruction(&I::Drop);
        }
    }
    init.instruction(&I::End);
    if matches!(program, Program::BusyPoll) {
        infinite_loop(&mut poll);
    }
    poll.instruction(&I::I32Const(if matches!(program, Program::ClockDrive) {
        -1
    } else {
        7
    }))
    .instruction(&I::End);
    let mut code = CodeSection::new();
    code.function(&init).function(&poll);
    let mut module = Module::new();
    module
        .section(&types)
        .section(&imports)
        .section(&functions)
        .section(&memory)
        .section(&exports)
        .section(&code);
    module.finish()
}

fn infinite_loop(function: &mut Function) {
    function
        .instruction(&I::Loop(BlockType::Empty))
        .instruction(&I::Br(0))
        .instruction(&I::End);
}

fn setup(wall_ms: Option<u64>) -> HostSetup {
    HostSetup {
        providers: vec![],
        grants: Grants::default(),
        limits: Limits {
            memory_bytes: u64::MAX,
            fuel: None,
            wall_ms,
        },
        args: vec![],
        program: String::new(),
    }
}

fn run(program: Program, wall_ms: Option<u64>, expected: u8) {
    let outcome = hd_run_wasmtime::run(
        EngineConfig::default(),
        &module(program),
        &setup(wall_ms),
        HostIo::process(),
    )
    .expect("run raw Wasm");
    assert_eq!(outcome, Outcome::Exit(expected));
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn bounded(test: &str, expected_error: &[u8], body: impl FnOnce()) {
    if std::env::var_os(CHILD).is_some() {
        body();
        std::process::exit(0);
    }
    let mut child = ChildGuard(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--quiet"])
            .env(CHILD, "1")
            // Keep the write end open and empty until the child exits. An EOF
            // would make the ConsoleInput test finish without exercising a wait.
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start bounded native child"),
    );
    let mut stdout = child.0.stdout.take().unwrap();
    let mut stderr = child.0.stderr.take().unwrap();
    let output = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let errors = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let deadline = Instant::now() + CHILD_TIMEOUT;
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "{test}: deadline enforcement failed; killing child"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    let stdout = output.join().unwrap();
    let stderr = errors.join().unwrap();
    assert!(
        status.success(),
        "{test}: {status}\n{}",
        String::from_utf8_lossy(&stderr)
    );
    assert_eq!(stdout, b"\nrunning 1 test\n", "{test}: no program output");
    assert_eq!(stderr, expected_error, "{test}: exact panic report");
}

#[test]
fn deadline_interrupts_busy_init() {
    bounded("deadline_interrupts_busy_init", TIME_PANIC, || {
        run(Program::BusyInit, Some(50), 3);
    });
}

#[test]
fn deadline_interrupts_busy_poll() {
    bounded("deadline_interrupts_busy_poll", TIME_PANIC, || {
        run(Program::BusyPoll, Some(50), 3);
    });
}

#[test]
fn retained_instance_keeps_its_deadline_after_the_engine_is_dropped() {
    bounded(
        "retained_instance_keeps_its_deadline_after_the_engine_is_dropped",
        TIME_PANIC,
        || {
            let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
            let module = engine
                .load(&module(Program::BusyPoll), &MemoryStore::default())
                .unwrap();
            let mut instance = engine
                .instantiate_with(&module, &setup(Some(200)), HostIo::process())
                .unwrap();
            assert_eq!(instance.init(), Outcome::Exit(0));
            drop(module);
            drop(engine);
            // The Store's internal Engine clone is not the outer Arc used by
            // the ticker. Keep only the instance, then enter a CPU-only loop.
            let start = Instant::now();
            assert_eq!(instance.poll(), std::task::Poll::Ready(Outcome::Exit(3)));
            assert!(start.elapsed() < Duration::from_secs(1));
            assert_eq!(
                instance.poll(),
                std::task::Poll::Ready(Outcome::Exit(3)),
                "the retained ended instance does not report another panic"
            );
        },
    );
}

#[test]
fn deadline_interrupts_clock_block_before_timer_finishes() {
    bounded(
        "deadline_interrupts_clock_block_before_timer_finishes",
        TIME_PANIC,
        || {
            run(Program::ClockBlock(10_000), Some(50), 3);
        },
    );
}

#[test]
fn deadline_interrupts_parked_driver_before_timer_finishes() {
    bounded(
        "deadline_interrupts_parked_driver_before_timer_finishes",
        TIME_PANIC,
        || {
            run(Program::ClockDrive, Some(50), 3);
        },
    );
}

#[test]
fn deadline_interrupts_console_input_with_stdin_open_and_empty() {
    bounded(
        "deadline_interrupts_console_input_with_stdin_open_and_empty",
        TIME_PANIC,
        || {
            run(Program::InputBlock, Some(50), 3);
        },
    );
}

#[test]
fn zero_deadline_panics_before_ready_program_runs() {
    bounded(
        "zero_deadline_panics_before_ready_program_runs",
        TIME_PANIC,
        || {
            run(Program::Ready, Some(0), 3);
        },
    );
}

#[test]
fn omitted_deadline_allows_clock_wait_to_finish() {
    bounded("omitted_deadline_allows_clock_wait_to_finish", b"", || {
        let start = Instant::now();
        run(Program::ClockBlock(120), None, 7);
        assert!(
            start.elapsed() >= Duration::from_millis(120),
            "sleep really completed"
        );
    });
}

#[test]
fn expired_store_does_not_shorten_fresh_or_unlimited_stores() {
    bounded(
        "expired_store_does_not_shorten_fresh_or_unlimited_stores",
        TIME_PANIC,
        || {
            let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
            let cache = MemoryStore::default();
            let busy = engine.load(&module(Program::BusyInit), &cache).unwrap();
            let sleeper = engine
                .load(&module(Program::ClockBlock(120)), &cache)
                .unwrap();
            let mut expired = engine
                .instantiate_with(&busy, &setup(Some(50)), HostIo::process())
                .unwrap();
            assert_eq!(expired.init(), Outcome::Exit(3));
            // Keep the expired instance alive: all stores share the same engine
            // and epoch ticker, but neither its wall deadline nor trap leaks.
            for limit in [Some(500), None] {
                let mut fresh = engine
                    .instantiate_with(&sleeper, &setup(limit), HostIo::process())
                    .unwrap();
                let start = Instant::now();
                assert_eq!(fresh.init(), Outcome::Exit(0));
                assert!(
                    start.elapsed() >= Duration::from_millis(120),
                    "fresh store completes its timer"
                );
                assert_eq!(hd_run::drive(&mut fresh, usize::MAX), Outcome::Exit(7));
            }
        },
    );
}
