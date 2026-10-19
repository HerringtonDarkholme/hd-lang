//! The module builder's output validates and has the entry exports.

use hd_base::Hash128;

use crate::module::{Func, Module, Sym, Val};
use crate::rt::Helper;

fn validate(wasm: &[u8]) {
    let mut v = wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all());
    v.validate_all(wasm).expect("valid module");
}

/// An entry with nothing to run: `hd.init` and `hd.poll` over an empty
/// `main`, the exchange buffer exported as `hd.x`, and the heap as a
/// second memory.
#[test]
fn an_empty_entry_links_two_memories_and_the_entry_exports() {
    let mut m = Module::default();
    let main = Hash128(7);
    m.define(Sym::Inst(main), Func::new("main", vec![], vec![]));
    m.export("hd.init", Sym::Helper(Helper::EntryInit { inits: vec![] }));
    m.export(
        "hd.poll",
        Sym::Helper(Helper::EntryPoll {
            providers: vec![],
            main,
            results: 0,
            report: None,
            then: vec![],
        }),
    );
    let wasm = m.finish().expect("link");
    validate(&wasm);
    let mut memories = 0;
    let mut exports = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
        match payload.expect("payload") {
            wasmparser::Payload::MemorySection(r) => memories += r.count(),
            wasmparser::Payload::ExportSection(r) => {
                for e in r {
                    exports.push(e.expect("export").name.to_owned());
                }
            }
            _ => {}
        }
    }
    assert_eq!(memories, 2);
    assert_eq!(exports, ["hd.init", "hd.poll", "hd.x"]);
}

/// Static data is placed once per content, aligned, from `STATIC_BASE`,
/// and the same slots make one vtable.
#[test]
fn static_bytes_and_vtables_are_shared_by_content() {
    let mut m = Module::default();
    let a = m.static_bytes(b"abc", 1);
    let b = m.static_bytes(b"xy", 8);
    assert_eq!(a, crate::module::STATIC_BASE);
    assert_eq!(b, crate::module::STATIC_BASE + 8);
    assert_eq!(m.static_bytes(b"abc", 1), a);
    let host = || {
        Sym::Helper(Helper::HostCold {
            module: "hd:Console".into(),
            method: "write_line".into(),
        })
    };
    let v = m.vtable(vec![host(), host()]);
    assert_eq!(m.vtable(vec![host(), host()]), v);
    assert_ne!(m.vtable(vec![host()]), v);
}

/// Every runtime helper an instance or a vtable names is built, and the
/// module with all of them validates: the panics, the buffer copy, the
/// bump allocation, integer text, string equality, the forbidden-context
/// check and a host provider's cold call and poll through the table.
#[test]
fn every_runtime_helper_validates() {
    let mut m = Module::default();
    let main = Hash128(9);
    let mut f = Func::new("main", vec![Val::I32, Val::I32], vec![]);
    for h in [
        Helper::Panic("explicit-panic: x".into()),
        Helper::ForbidCheck,
    ] {
        f.helper(h);
    }
    f.i32(0);
    f.i32(0);
    f.helper(Helper::PanicStr);
    for _ in 0..4 {
        f.i32(0);
    }
    f.helper(Helper::PanicWith);
    f.i32(0);
    f.i32(0);
    f.helper(Helper::ToBuf);
    f.op(wasm_encoder::Instruction::Drop);
    f.i32(8);
    f.helper(Helper::Alloc);
    f.op(wasm_encoder::Instruction::Drop);
    for signed in [false, true] {
        f.op(wasm_encoder::Instruction::I64Const(-5));
        f.helper(Helper::FmtInt { signed });
        f.op(wasm_encoder::Instruction::Drop);
        f.op(wasm_encoder::Instruction::Drop);
    }
    for _ in 0..4 {
        f.i32(0);
    }
    f.helper(Helper::StrEq);
    f.op(wasm_encoder::Instruction::Drop);
    m.define(Sym::Inst(main), f);
    let console = m.vtable(vec![Sym::Helper(Helper::HostCold {
        module: "hd:Console".into(),
        method: "write_line".into(),
    })]);
    m.export("hd.init", Sym::Helper(Helper::EntryInit { inits: vec![] }));
    m.export(
        "hd.poll",
        Sym::Helper(Helper::EntryPoll {
            providers: vec![(console, None)],
            main,
            results: 0,
            report: None,
            then: vec![],
        }),
    );
    validate(&m.finish().expect("link"));
}
