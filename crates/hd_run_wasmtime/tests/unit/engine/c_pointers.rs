//! A C provider's pointer arguments are aligned for their number type
//! (`module.host.package.c-aligned`): a `&mut i64` or a `&List[f64]` at an
//! offset that is not a multiple of 8 fails the call with `host-contract`,
//! with the JavaScript glue's message, and no slot runs. The library is
//! `c_pointers/lib.rs`, built inside the test.

use hd_host_abi::c::{Method as CMethod, Num, Param, Ret, Table};
use hd_host_abi::schema::{Contract, Interface, Method, Value};
use hd_run::{Consent, GrantValue, Grants, HostSetup, Limits, Outcome};
use hd_run_wasmtime::internals::Shared;
use hd_run_wasmtime::native::c::Library;
use hd_run_wasmtime::{EngineConfig, HostIo};
use wasm_encoder::{
    CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection, ImportSection,
    Instruction as I, MemorySection, MemoryType, Module as Wasm, TypeSection, ValType,
};

const CELL: &str = "hd:pkg:cell";

fn contract() -> Contract {
    let method = |name: &str| Method {
        name: name.into(),
        member: name.into(),
        params: Vec::new(),
        result: Value::Int {
            bits: 32,
            signed: true,
        },
        suspends: false,
    };
    Contract {
        interfaces: vec![Interface {
            identity: hd_host_abi::schema::Identity {
                package: "example.org/sdk".into(),
                version: "1.0.0".into(),
                trait_path: "cell.Cell".into(),
            },
            module: CELL.into(),
            selectors: vec!["dep.sdk.cell.Cell".into()],
            handle: false,
            methods: vec![method("bump"), method("sum")],
        }],
    }
}

fn table() -> Table {
    let row = |name: &str, params| CMethod {
        name: name.into(),
        params,
        result: Ret::Num(Num::I32),
        suspends: false,
    };
    Table {
        module: CELL.into(),
        trait_name: "Cell".into(),
        handle: false,
        methods: vec![
            row(
                "bump",
                vec![(
                    "at".into(),
                    Param::Ref {
                        num: Num::I64,
                        mutable: true,
                    },
                )],
            ),
            row(
                "sum",
                vec![(
                    "xs".into(),
                    Param::List {
                        num: Num::F64,
                        mutable: false,
                    },
                )],
            ),
        ],
    }
}

/// A module importing `bump(i32) -> i32` and `sum(i32, i32) -> i32`, whose
/// `hd.init` runs `init` and whose `hd.poll` is done at once.
fn module(init: &[I<'_>]) -> Vec<u8> {
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32], [ValType::I32]);
    types
        .ty()
        .function([ValType::I32, ValType::I32], [ValType::I32]);
    types.ty().function([], []);
    types.ty().function([ValType::I32], [ValType::I32]);
    let mut imports = ImportSection::new();
    imports.import(CELL, "bump", EntityType::Function(0));
    imports.import(CELL, "sum", EntityType::Function(1));
    let mut functions = FunctionSection::new();
    functions.function(2).function(3);
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
    exports.export("hd.heap", ExportKind::Memory, 0);
    exports.export("hd.init", ExportKind::Func, 2);
    exports.export("hd.poll", ExportKind::Func, 3);
    let mut body = Function::new([]);
    for instruction in init {
        body.instruction(instruction);
    }
    body.instruction(&I::End);
    let mut poll = Function::new([]);
    poll.instruction(&I::I32Const(0)).instruction(&I::End);
    let mut code = CodeSection::new();
    code.function(&body).function(&poll);
    let mut wasm = Wasm::new();
    wasm.section(&types)
        .section(&imports)
        .section(&functions)
        .section(&memory)
        .section(&exports)
        .section(&code);
    wasm.section(&wasm_encoder::CustomSection {
        name: hd_host_abi::schema::SECTION.into(),
        data: contract().encode().into(),
    });
    wasm.section(&wasm_encoder::CustomSection {
        name: hd_host_abi::c::SECTION.into(),
        data: hd_host_abi::c::encode(&[table()]).into(),
    });
    wasm.finish()
}

/// The library, built into its own folder, and the test's own handle on
/// it, held across the runs so its counter survives them on Windows.
#[allow(unsafe_code)]
fn library() -> (libloading::Library, Library) {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("c-pointers");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("library folder");
    let hash: Vec<String> = table().hash().iter().map(|b| format!("{b:#04x}")).collect();
    let source =
        include_str!("c_pointers/lib.rs").replace("__HASH__", &format!("[{}]", hash.join(", ")));
    std::fs::write(dir.join("lib.rs"), source).expect("library source");
    let file = dir.join(if cfg!(windows) {
        "hd_cell.dll"
    } else if cfg!(target_vendor = "apple") {
        "libhd_cell.dylib"
    } else {
        "libhd_cell.so"
    });
    hd_testkit::cdylib::build(&dir.join("lib.rs"), &file);
    // SAFETY: the test's own library, whose loading runs no initializer.
    let held = unsafe { libloading::Library::new(&file) }.expect("the library");
    let library = Library {
        module: CELL.into(),
        path: file,
        name: "hd_cell".into(),
        package: "sdk".into(),
        triple: "test".into(),
    };
    (held, library)
}

/// How many slot calls the library ran.
#[allow(unsafe_code)]
fn calls(held: &libloading::Library) -> u32 {
    // SAFETY: the test's own library; `calls` takes nothing.
    unsafe {
        let calls = held
            .get::<unsafe extern "C" fn() -> u32>(b"calls")
            .expect("calls");
        calls()
    }
}

/// Runs `init` with the library bound: its outcome and standard error.
#[allow(unsafe_code)]
fn run(library: &Library, init: &[I<'_>]) -> (Outcome, String) {
    let err = Shared::default();
    let io = HostIo {
        out: Box::new(Shared::default()),
        err: Box::new(err.clone()),
        input: None,
    };
    let setup = HostSetup {
        grants: Grants::with_consent(Consent::new(
            &[("dep.sdk.cell.Cell".to_owned(), GrantValue::All)],
            &[],
        )),
        limits: Limits::default(),
        args: vec![],
        program: "c-pointers".into(),
    };
    let wasm = module(init);
    // SAFETY: a portable Wasm image, and the test's own library.
    let (outcome, _) = unsafe {
        hd_run_wasmtime::run_with_c_providers(
            EngineConfig::default(),
            hd_run::artifact::Image::Wasm(&wasm),
            &setup,
            io,
            std::slice::from_ref(library),
            None,
            None,
        )
    }
    .expect("the run");
    (outcome, String::from_utf8(err.take()).unwrap())
}

#[test]
fn a_misaligned_pointer_argument_fails_the_call_and_runs_no_slot() {
    let (held, library) = library();
    let message = "panic: host-contract: package host: misaligned C pointer argument";
    // `&mut i64` at 4, and `&List[f64]` of one element at 12.
    let cases: [(&str, &[I<'_>]); 2] = [
        ("&mut i64", &[I::I32Const(4), I::Call(0), I::Drop]),
        (
            "&List[f64]",
            &[I::I32Const(12), I::I32Const(1), I::Call(1), I::Drop],
        ),
    ];
    for (case, body) in cases {
        let (outcome, error) = run(&library, body);
        assert_eq!(outcome, Outcome::Exit(3), "{case}: {error}");
        assert!(error.starts_with(message), "{case}: {error}");
    }
    assert_eq!(calls(&held), 0, "no slot ran");
    // The same calls at multiples of 8 reach the slots.
    let (outcome, error) = run(
        &library,
        &[
            I::I32Const(8),
            I::Call(0),
            I::Drop,
            I::I32Const(16),
            I::I32Const(1),
            I::Call(1),
            I::Drop,
        ],
    );
    assert_eq!(outcome, Outcome::Exit(0), "{error}");
    assert_eq!(calls(&held), 2);
}
