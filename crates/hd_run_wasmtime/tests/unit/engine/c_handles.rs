//! C providers' returned handles (`module.host.package.c-handles`, owner
//! decision 1 of #2388): a C-bound root's method returns the library's
//! handle object, whose trait's table comes from a second entry of the
//! same library, checked as a root's is. The handle lives in the
//! instance's handle registry; a method call on it reaches its table's
//! slot with the object as `self`, a forged, stale or other-trait number
//! calls no slot (`native-handle-check`, `host-contract`), and its last
//! release runs `destroy` (`native-handle-release`). The library is
//! `c_handles/cursor.rs`, built by `rustc` inside the test.

use hd_host_abi::c::{Method as CMethod, Ret, Table};
use hd_host_abi::schema::{Contract, Interface, Method, Value};
use hd_run::{Consent, GrantValue, Grants, HostSetup, Limits, Outcome};
use hd_run_wasmtime::internals::Shared;
use hd_run_wasmtime::native::c::Library;
use hd_run_wasmtime::{EngineConfig, HostIo};
use wasm_encoder::{
    CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection, ImportSection,
    Instruction as I, MemorySection, MemoryType, Module as Wasm, TypeSection, ValType,
};

const STORE: &str = "hd:pkg:store";
const CURSOR: &str = "hd:pkg:cursor";

fn identity(trait_path: &str) -> hd_host_abi::schema::Identity {
    hd_host_abi::schema::Identity {
        package: "example.org/sdk".into(),
        version: "1.0.0".into(),
        trait_path: trait_path.into(),
    }
}

fn method(name: &str, result: Value) -> Method {
    Method {
        name: name.into(),
        member: name.into(),
        params: Vec::new(),
        result,
        suspends: false,
    }
}

fn contract() -> Contract {
    Contract {
        interfaces: vec![
            Interface {
                identity: identity("store.Store"),
                module: STORE.into(),
                selectors: vec!["dep.sdk.store.Store".into()],
                handle: false,
                methods: vec![method("open", Value::Handle(CURSOR.into()))],
            },
            Interface {
                identity: identity("store.Cursor"),
                module: CURSOR.into(),
                selectors: Vec::new(),
                handle: true,
                methods: vec![method(
                    "next",
                    Value::Int {
                        bits: 32,
                        signed: true,
                    },
                )],
            },
        ],
    }
}

fn tables() -> Vec<Table> {
    let row = |name: &str, result| CMethod {
        name: name.into(),
        params: Vec::new(),
        result,
        suspends: false,
    };
    vec![
        Table {
            module: STORE.into(),
            trait_name: "Store".into(),
            handle: false,
            methods: vec![row("open", Ret::Handle("Cursor".into()))],
        },
        Table {
            module: CURSOR.into(),
            trait_name: "Cursor".into(),
            handle: true,
            methods: vec![row("next", Ret::Num(hd_host_abi::c::Num::I32))],
        },
    ]
}

/// A module importing `open() -> i32`, `next(i32) -> i32` and
/// `$release(i32)`, whose `hd.init` runs `init` and whose `hd.poll` is
/// done at once.
fn module(init: &[I<'_>]) -> Vec<u8> {
    let mut types = TypeSection::new();
    types.ty().function([], [ValType::I32]);
    types.ty().function([ValType::I32], [ValType::I32]);
    types.ty().function([ValType::I32], []);
    types.ty().function([], []);
    types.ty().function([ValType::I32], [ValType::I32]);
    let mut imports = ImportSection::new();
    imports.import(STORE, "open", EntityType::Function(0));
    imports.import(CURSOR, "next", EntityType::Function(1));
    imports.import(CURSOR, "$release", EntityType::Function(2));
    let mut functions = FunctionSection::new();
    functions.function(3).function(4);
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
    exports.export("hd.init", ExportKind::Func, 3);
    exports.export("hd.poll", ExportKind::Func, 4);
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
        data: hd_host_abi::c::encode(&tables()).into(),
    });
    wasm.finish()
}

fn hash_text(table: &Table) -> String {
    let bytes: Vec<String> = table.hash().iter().map(|b| format!("{b:#04x}")).collect();
    format!("[{}]", bytes.join(", "))
}

/// The cursor library, built once per test into its own folder.
fn library(name: &str) -> (Loaded, Library) {
    library_for(name, STORE, &tables())
}

/// The test's own handle on a built library, opened before the run and
/// held after it. The run's handle is then not the last one: on Windows
/// the last `FreeLibrary` unloads the DLL and its counters start again at
/// 0, which an unloaded `dlclose` on Unix never showed.
struct Loaded(libloading::Library);

impl Loaded {
    /// The exported counter `name`.
    #[allow(unsafe_code)]
    fn read<T>(&self, name: &[u8]) -> T {
        // SAFETY: the test's own library, whose counters take nothing and
        // return `T`.
        unsafe {
            let f = self
                .0
                .get::<unsafe extern "C" fn() -> T>(name)
                .expect("a counter");
            f()
        }
    }

    /// The cursor destroys and `next` calls so far, the high and low
    /// halves of its `counts`.
    #[allow(clippy::cast_possible_truncation)]
    fn counts(&self) -> (u32, u32) {
        let n: u64 = self.read(b"counts");
        ((n >> 32) as u32, n as u32)
    }

    /// How many providers the library created.
    fn created(&self) -> u32 {
        self.read(b"created")
    }
}

/// [`library`], selected for the root interface `module`, with the
/// contract hashes of `tables`.
#[allow(unsafe_code)]
fn library_for(name: &str, module: &str, tables: &[Table]) -> (Loaded, Library) {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("c-handles-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("library folder");
    let source = include_str!("c_handles/cursor.rs")
        .replace("__STORE__", &hash_text(&tables[0]))
        .replace("__CURSOR__", &hash_text(&tables[1]));
    std::fs::write(dir.join("lib.rs"), source).expect("library source");
    let file = dir.join(if cfg!(windows) {
        "hd_store.dll"
    } else if cfg!(target_vendor = "apple") {
        "libhd_store.dylib"
    } else {
        "libhd_store.so"
    });
    hd_testkit::cdylib::build(&dir.join("lib.rs"), &file);
    // SAFETY: the test's own library, whose loading runs no initializer.
    let loaded = Loaded(unsafe { libloading::Library::new(&file) }.expect("the library"));
    let library = Library {
        module: module.into(),
        path: file,
        name: "hd_store".into(),
        package: "sdk".into(),
        triple: "test".into(),
    };
    (loaded, library)
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
            &[("dep.sdk.store.Store".to_owned(), GrantValue::All)],
            &[],
        )),
        limits: Limits::default(),
        args: vec![],
        program: "c-handles".into(),
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

/// `open` twice issues 1 and 2; `next` on each reaches its own object;
/// releasing both runs `destroy` twice.
#[test]
fn a_returned_c_handle_is_called_and_destroyed_at_its_release() {
    let (file, library) = library("round-trip");
    let (outcome, error) = run(
        &library,
        &[
            I::Call(0),
            I::Drop,
            I::Call(0),
            I::Drop,
            I::I32Const(1),
            I::Call(1),
            I::Drop,
            I::I32Const(2),
            I::Call(1),
            I::Drop,
            I::I32Const(1),
            I::Call(2),
            I::I32Const(2),
            I::Call(2),
        ],
    );
    assert_eq!(outcome, Outcome::Exit(0), "{error}");
    assert_eq!(file.counts(), (2, 2), "two calls, two destroys");
}

/// A number this instance never issued, one already released, and one of
/// another trait call no slot and fail the run with `host-contract`.
#[test]
fn a_forged_or_stale_c_handle_calls_no_slot() {
    let (file, library) = library("forged");
    let call = "panic: host-contract: package host: call of an invalid handle";
    let release = "panic: host-contract: package host: release of an invalid handle";
    let cases: [(&str, &[I<'_>], &str); 3] = [
        ("never issued", &[I::I32Const(7), I::Call(1), I::Drop], call),
        (
            "released",
            &[
                I::Call(0),
                I::Drop,
                I::I32Const(1),
                I::Call(2),
                I::I32Const(1),
                I::Call(1),
                I::Drop,
            ],
            call,
        ),
        (
            "released twice",
            &[
                I::Call(0),
                I::Drop,
                I::I32Const(1),
                I::Call(2),
                I::I32Const(1),
                I::Call(2),
            ],
            release,
        ),
    ];
    for (case, body, message) in cases {
        let (outcome, error) = run(&library, body);
        assert_eq!(outcome, Outcome::Exit(3), "{case}: {error}");
        assert!(error.starts_with(message), "{case}: {error}");
    }
    assert_eq!(file.counts().1, 0, "no `next` slot ran");
    // Each released cursor was destroyed once.
    assert_eq!(file.counts().0, 2);
}

/// A module of `contract` and `tables` whose `hd.init` traps, so a guest
/// that started would end in a trap, not the refusal.
fn guarded(contract: &Contract, tables: &[Table]) -> Vec<u8> {
    let mut types = TypeSection::new();
    types.ty().function([], []);
    types.ty().function([ValType::I32], [ValType::I32]);
    let mut functions = FunctionSection::new();
    functions.function(0).function(1);
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
    exports.export("hd.init", ExportKind::Func, 0);
    exports.export("hd.poll", ExportKind::Func, 1);
    let mut init = Function::new([]);
    init.instruction(&I::Unreachable).instruction(&I::End);
    let mut poll = Function::new([]);
    poll.instruction(&I::I32Const(0)).instruction(&I::End);
    let mut code = CodeSection::new();
    code.function(&init).function(&poll);
    let mut wasm = Wasm::new();
    wasm.section(&types)
        .section(&functions)
        .section(&memory)
        .section(&exports)
        .section(&code);
    wasm.section(&wasm_encoder::CustomSection {
        name: hd_host_abi::schema::SECTION.into(),
        data: contract.encode().into(),
    });
    wasm.section(&wasm_encoder::CustomSection {
        name: hd_host_abi::c::SECTION.into(),
        data: hd_host_abi::c::encode(tables).into(),
    });
    wasm.finish()
}

/// The refusal of `wasm` with `libraries` bound and both roots consented,
/// and what the guest wrote to standard output.
#[allow(unsafe_code)]
fn refused(wasm: &[u8], libraries: &[Library]) -> (String, Vec<u8>) {
    let out = Shared::default();
    let io = HostIo {
        out: Box::new(out.clone()),
        err: Box::new(Shared::default()),
        input: None,
    };
    let setup = HostSetup {
        grants: Grants::with_consent(Consent::new(
            &[
                ("dep.sdk.store.Store".to_owned(), GrantValue::All),
                ("dep.sdk.shelf.Store".to_owned(), GrantValue::All),
            ],
            &[],
        )),
        limits: Limits::default(),
        args: vec![],
        program: "c-handles".into(),
    };
    // SAFETY: a portable Wasm image, and the test's own libraries.
    let ran = unsafe {
        hd_run_wasmtime::run_with_c_providers(
            EngineConfig::default(),
            hd_run::artifact::Image::Wasm(wasm),
            &setup,
            io,
            libraries,
            None,
            None,
        )
    };
    let Err(message) = ran else {
        panic!("not refused: {ran:?}");
    };
    (message, out.take())
}

/// `cli.host.c.unseen-conflict`: a conflict no one check saw refuses the
/// run before the guest starts and before any `create` runs. Two root
/// traits of two libraries return one handle trait
/// (`cli.host.c.handle-library`), and a handle trait takes its library's
/// entry name of a root trait (`cli.host.c.handle-entry`).
#[test]
fn a_conflict_no_check_saw_refuses_the_run_before_any_create() {
    let mut two = tables();
    let shelf = "hd:pkg:shelf";
    // A second root, `shelf.Store`, of another library file, also returns
    // `Cursor`.
    two.push(Table {
        module: shelf.into(),
        ..two[0].clone()
    });
    let mut roots = contract();
    roots.interfaces.push(Interface {
        identity: identity("shelf.Store"),
        module: shelf.into(),
        selectors: vec!["dep.sdk.shelf.Store".into()],
        ..roots.interfaces[0].clone()
    });
    let (store_file, store) = library_for("conflict-store", STORE, &two);
    let (shelf_file, shelf_library) = library_for("conflict-shelf", shelf, &two);
    let (message, out) = refused(&guarded(&roots, &two), &[store, shelf_library]);
    assert!(
        message.contains("returned by C providers of two libraries"),
        "{message}"
    );
    assert!(message.contains("store.Cursor"), "{message}");
    assert!(out.is_empty(), "the guest wrote {out:?}");
    assert_eq!((store_file.created(), shelf_file.created()), (0, 0));

    // `Cursor`'s table named `Store`: the root's entry symbol.
    let mut tables = tables();
    tables[1].trait_name = "Store".into();
    tables[0].methods[0].result = Ret::Handle("Store".into());
    let (file, library) = library_for("conflict-entry", STORE, &tables);
    let (message, out) = refused(&guarded(&contract(), &tables), &[library]);
    assert!(
        message.contains("would both take the entry symbol `hd_store_Store`"),
        "{message}"
    );
    assert!(out.is_empty(), "the guest wrote {out:?}");
    assert_eq!(file.created(), 0);
}
