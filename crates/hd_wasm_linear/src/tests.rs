//! The module builder's output validates and has the entry exports.

use hd_base::Hash128;

use crate::boundary::{LEnum, LList, LTy};
use crate::module::{Func, Module, Sym, Val};
use crate::rt::{Elem, Helper};

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
            bang: None,
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

/// A function of more parameters and results than `wide::WIDTH`, called
/// directly and through the table, early return included, lowers to frames
/// on the shadow stack: the module validates, no function type is wider
/// than the bound, and the stack is a third memory.
#[test]
fn wide_signatures_pass_through_the_shadow_stack() {
    use wasm_encoder::Instruction as I;
    let n = u32::try_from(crate::wide::WIDTH).expect("width") + 36;
    let vals = vec![Val::I32; n as usize];
    let wide = Sym::Inst(Hash128(9));
    // Returns its arguments reversed, early when the first is zero.
    let mut w = Func::new("wide", vals.clone(), vals.clone());
    w.get(0);
    w.op(I::I32Eqz);
    w.op(I::If(wasm_encoder::BlockType::Empty));
    for k in (0..n).rev() {
        w.get(k);
    }
    w.op(I::Return);
    w.op(I::End);
    for k in (0..n).rev() {
        w.get(k);
    }
    let mut m = Module::default();
    m.define(wide.clone(), w);
    let main = Hash128(7);
    let mut f = Func::new("main", vec![], vec![]);
    for indirect in [false, true] {
        for k in 0..n {
            f.i32(i32::try_from(k).expect("k"));
        }
        if indirect {
            f.body.push(crate::module::Ins::TableSlot(wide.clone()));
            f.call_indirect(vals.clone(), vals.clone());
        } else {
            f.call(wide.clone());
        }
        for _ in 0..n {
            f.op(I::Drop);
        }
    }
    m.define(Sym::Inst(main), f);
    m.export("hd.init", Sym::Inst(main));
    let wasm = m.finish().expect("link");
    validate(&wasm);
    let mut memories = 0;
    for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
        match payload.expect("payload") {
            wasmparser::Payload::MemorySection(r) => memories += r.count(),
            wasmparser::Payload::TypeSection(r) => {
                for t in r.into_iter_err_on_gc_types() {
                    let t = t.expect("type");
                    assert!(t.params().len() <= crate::wide::WIDTH);
                    assert!(t.results().len() <= crate::wide::WIDTH);
                }
            }
            _ => {}
        }
    }
    assert_eq!(memories, 3);
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
            args: vec![crate::task::HostArg::Str],
            area: string_area(),
            res: crate::task::HostRes::Tags,
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
    let mut f = Func::new("main", vec![Val::I32; 4], vec![]);
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
        args: vec![crate::task::HostArg::Str],
        area: string_area(),
        res: crate::task::HostRes::Tags,
    })]);
    // A host call of every boundary type, in both directions, and a
    // waiting one through a handle's slot.
    let every = every_boundary_type();
    let call = Helper::HostCall {
        module: "hd:Probe".into(),
        method: "echo".into(),
        args: vec![
            crate::task::HostArg::Scalar(Val::I64),
            crate::task::HostArg::Buf(every.clone()),
        ],
        result: crate::boundary::CallRes::Buf(every.clone()),
    };
    let mut params = vec![Val::I64];
    params.extend(every.vals());
    let leaf = Helper::HostCold {
        module: "hd:Probe".into(),
        method: "wait".into(),
        args: vec![
            crate::task::HostArg::Scalar(Val::I32),
            crate::task::HostArg::Buf(LTy::Str),
        ],
        area: Elem {
            size: 12,
            leaves: vec![word(0), word(4), word(8)],
            walk: None,
            colls: vec![],
            cyclic: false,
        },
        res: crate::task::HostRes::Buf(every.clone()),
    };
    let probe = m.vtable(vec![
        Sym::Helper(call),
        Sym::Helper(Helper::HandleSlot {
            inner: Box::new(leaf),
            params: vec![Val::I32, Val::I32],
            results: vec![Val::I32],
        }),
    ]);
    m.export("hd.init", Sym::Helper(Helper::EntryInit { inits: vec![] }));
    m.export(
        "hd.poll",
        Sym::Helper(Helper::EntryPoll {
            providers: vec![(console, None), (probe, None)],
            main,
            results: 0,
            report: None,
            then: vec![],
            bang: None,
        }),
    );
    validate(&m.finish().expect("link"));
}

/// A 4-byte `i32` leaf at `offset`.
fn word(offset: u32) -> crate::layout::Leaf {
    crate::layout::Leaf {
        offset,
        val: Val::I32,
        bytes: 4,
        signed: false,
    }
}

/// A leaf frame's area of one string argument.
fn string_area() -> Elem {
    Elem {
        size: 8,
        leaves: vec![word(0), word(4)],
        walk: Some(crate::layout::Walk::Ref(0)),
        colls: vec![],
        cyclic: false,
    }
}

/// A record of every kind of boundary type the codecs build.
fn every_boundary_type() -> LTy {
    let opt = LTy::Enum(Box::new(LEnum {
        vals: vec![Val::I32, Val::I32, Val::I32, Val::I64],
        variants: vec![
            vec![],
            vec![(vec![1, 2], LTy::Str)],
            vec![(
                vec![3],
                LTy::Int {
                    bits: 64,
                    signed: true,
                },
            )],
        ],
    }));
    LTy::Record(vec![
        LTy::Void,
        LTy::Int {
            bits: 8,
            signed: false,
        },
        LTy::Int {
            bits: 16,
            signed: true,
        },
        LTy::Int {
            bits: 1,
            signed: false,
        },
        LTy::F32,
        LTy::F64,
        LTy::Str,
        LTy::Bytes,
        LTy::List(Box::new(LList {
            elem: Elem {
                size: 8,
                leaves: vec![word(0), word(4)],
                walk: None,
                colls: vec![],
                cyclic: false,
            },
            ty: LTy::Str,
        })),
        opt,
    ])
}
