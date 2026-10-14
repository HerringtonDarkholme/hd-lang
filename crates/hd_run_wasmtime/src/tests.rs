use super::{EngineConfig, OptLevel, WasmtimeEngine};
use hd_cache::MemoryStore;
use hd_run::Engine;

/// M41: the build profile selects the Cranelift pipeline. Dev stays
/// unoptimized (`hd test`, conformance); release optimizes
/// (`hd run --release`).
#[test]
fn profile_selects_cranelift_pipeline() {
    assert_eq!(
        EngineConfig::pipeline(hd_base::OptLevel::Dev).cranelift_opt,
        OptLevel::None
    );
    assert_eq!(
        EngineConfig::pipeline(hd_base::OptLevel::Optimized).cranelift_opt,
        OptLevel::Speed
    );
    assert_eq!(
        EngineConfig::default(),
        EngineConfig::pipeline(hd_base::OptLevel::Dev)
    );
}

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
    types.ty().function([ValType::I32], [ValType::I32]);
    let mut imports = ImportSection::new();
    imports.import(module, name, EntityType::Function(0));
    let mut functions = FunctionSection::new();
    functions.function(1).function(3);
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
        (
            "hd:Process",
            "run.start",
            marker.as_slice(),
            vec![ValType::I32],
        ),
        (
            "hd:Http",
            "send.start",
            marker.as_slice(),
            vec![ValType::I32],
        ),
        (
            "hd:Net",
            "connect.start",
            marker.as_slice(),
            vec![ValType::I32],
        ),
        (
            "hd:Clock",
            "sleep.start",
            marker.as_slice(),
            vec![ValType::I32],
        ),
        (
            "hd:ConsoleInput",
            "read_line.start",
            marker.as_slice(),
            vec![ValType::I32],
        ),
        (
            "hd:rt",
            "block",
            marker.as_slice(),
            vec![ValType::I32, ValType::I32],
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
        // A rejected shape must cause no provider effects at all, not
        // merely empty output: no queued work, no process spawn, no
        // sleep armed, no reader thread, no socket opened.
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
        CodeSection, ConstExpr, DataSection, ExportKind, ExportSection, Function, FunctionSection,
        GlobalSection, GlobalType, Instruction as I, MemArg, MemorySection, MemoryType,
        Module as Wasm, TypeSection, ValType,
    };

    let mut types = TypeSection::new();
    types.ty().function([], []);
    types.ty().function([ValType::I32], [ValType::I32]);
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
        BlockType, CodeSection, ConstExpr, ExportKind, ExportSection, Function, FunctionSection,
        GlobalSection, GlobalType, Instruction as I, Module as Wasm, TypeSection, ValType,
    };

    let mut types = TypeSection::new();
    types.ty().function([], []);
    types.ty().function([ValType::I32], [ValType::I32]);
    types.ty().function([ValType::I32], []);
    let mut functions = FunctionSection::new();
    functions.function(0).function(1).function(2);
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
    exports.export("hd.init", ExportKind::Func, 0);
    exports.export("hd.poll", ExportKind::Func, 1);
    exports.export("hd.wake", ExportKind::Func, 2);
    let mut init = Function::new([]);
    init.instruction(&I::End);
    let mut poll = Function::new([]);
    poll.instruction(&I::GlobalGet(0))
        .instruction(&I::I32Const(7))
        .instruction(&I::I32Add)
        .instruction(&I::End);
    let mut wake = Function::new([]);
    // An empty delivery still invokes hd.wake with count zero: the
    // marker proves the call happened, and the absent trap proves the
    // count was zero.
    wake.instruction(&I::I32Const(5))
        .instruction(&I::GlobalSet(0))
        .instruction(&I::LocalGet(0))
        .instruction(&I::If(BlockType::Empty))
        .instruction(&I::Unreachable)
        .instruction(&I::End)
        .instruction(&I::End);
    let mut code = CodeSection::new();
    code.function(&init).function(&poll).function(&wake);
    let mut wasm = Wasm::new();
    wasm.section(&types)
        .section(&functions)
        .section(&globals)
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
    assert_eq!(instance.poll(), Poll::Ready(Outcome::Exit(12)));
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
