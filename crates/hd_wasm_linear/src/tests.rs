//! The module builder's output validates and has the entry exports.

use hd_base::Hash128;

use crate::boundary::{LEnum, LList, LTy};
use crate::module::{Module, Sym, Val};
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
    let main_func = m.func("main", vec![], vec![]);
    m.define(Sym::Inst(main), main_func);
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
    use crate::op::Op as I;
    let n = u32::try_from(crate::wide::WIDTH).expect("width") + 36;
    let vals = vec![Val::I32; n as usize];
    let wide = Sym::Inst(Hash128(9));
    // Returns its arguments reversed, early when the first is zero.
    let mut m = Module::default();
    let mut w = m.func("wide", vals.clone(), vals.clone());
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
    m.define(wide.clone(), w);
    let main = Hash128(7);
    let mut f = m.func("main", vec![], vec![]);
    for indirect in [false, true] {
        for k in 0..n {
            f.i32(i32::try_from(k).expect("k"));
        }
        if indirect {
            f.table_slot(wide.clone());
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
    let mut f = m.func("main", vec![Val::I32; 4], vec![]);
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
    f.op(crate::op::Op::Drop);
    f.i32(8);
    f.helper(Helper::Alloc);
    f.op(crate::op::Op::Drop);
    for signed in [false, true] {
        f.op(crate::op::Op::I64Const(-5));
        f.helper(Helper::FmtInt { signed });
        f.op(crate::op::Op::Drop);
        f.op(crate::op::Op::Drop);
    }
    for _ in 0..4 {
        f.i32(0);
    }
    f.helper(Helper::StrEq);
    f.op(crate::op::Op::Drop);
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

#[test]
fn instruction_origins_do_not_change_bytes_and_offsets_match_operators() {
    for helper in [Helper::RefIsView, Helper::RefViewsAbsent] {
        let make = |tracked: bool| {
            let mut m = Module::default();
            m.track_origins = tracked;
            let mut f = m.func("mapped", vec![Val::I32], vec![Val::I32]);
            if tracked {
                f.origins = Some(Vec::new());
                f.origin_item = Some("app/main/mapped".into());
                f.origin_root = Some(99);
            }
            if helper == Helper::RefIsView {
                f.origin_at = Some(10);
                f.get(0);
            }
            f.origin_at = Some(11);
            f.call(Sym::Helper(helper.clone()));
            m.define(Sym::Inst(Hash128(7)), f);
            m.export("mapped", Sym::Inst(Hash128(7)));
            m
        };
        let plain = make(false).finish().expect("ordinary module");
        let mapped = make(true).finish_with_origins().expect("mapped module");
        assert_eq!(plain, mapped.wasm);
        validate(&mapped.wasm);
        let mut index = 0;
        for payload in wasmparser::Parser::new(0).parse_all(&mapped.wasm) {
            match payload.unwrap() {
                wasmparser::Payload::ImportSection(imports) => {
                    for import in imports.into_imports() {
                        if matches!(import.unwrap().ty, wasmparser::TypeRef::Func(_)) {
                            index += 1;
                        }
                    }
                }
                wasmparser::Payload::CodeSectionEntry(body) => {
                    let source = mapped
                        .functions
                        .iter()
                        .find(|f| f.index == index)
                        .expect("function map");
                    let start = body.range().start;
                    let mut reader = body.get_operators_reader().unwrap();
                    let mut offsets = Vec::new();
                    while !reader.eof() {
                        offsets.push(u32::try_from(reader.original_position() - start).unwrap());
                        reader.read().unwrap();
                    }
                    assert_eq!(
                        offsets,
                        source
                            .instructions
                            .iter()
                            .map(|i| i.offset)
                            .collect::<Vec<_>>()
                    );
                    assert_eq!(source.before_opt.last(), Some(&Some(11)));
                    // `RefIsView` folds to a dropped argument and a
                    // constant, and the dropped read goes with its drop.
                    assert_eq!(source.instructions.first().unwrap().origin, Some(11));
                    assert_eq!(source.instructions.last().unwrap().origin, Some(99));
                    index += 1;
                }
                _ => {}
            }
        }
    }
}

/// The cold paths are calls through the table (`Func::cold_helper`): from
/// a function that changes a list, pushes onto one and panics with a
/// formatted message, direct calls reach none of the copy, growth and
/// formatting helpers, and the direct calls among the helpers have no
/// cycle (#165): V8 inlines direct calls without call feedback, and an
/// unrolled cycle filled a hot caller's budget.
#[test]
fn cold_paths_are_no_direct_calls() {
    let mut m = Module::default();
    let main = Hash128(11);
    let elem = string_area();
    let mut f = m.func("main", vec![], vec![]);
    let l = f.local(Val::I32);
    f.get(l);
    f.helper(Helper::ListUnique { elem: elem.clone() });
    f.op(crate::op::Op::Drop);
    f.get(l);
    f.cold_helper(Helper::ListGrow { elem }, vec![Val::I32], vec![Val::I32]);
    f.op(crate::op::Op::Drop);
    f.helper(Helper::Panic("index-out-of-bounds: x".into()));
    for _ in 0..4 {
        f.get(l);
    }
    f.helper(Helper::PanicWith);
    m.define(Sym::Inst(main), f);
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
    // Each defined function's name and the functions it calls directly.
    let (mut imports, mut names, mut calls) = (0, Vec::new(), Vec::new());
    for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
        match payload.expect("payload") {
            wasmparser::Payload::ImportSection(r) => {
                for i in r.into_imports() {
                    if matches!(i.expect("import").ty, wasmparser::TypeRef::Func(_)) {
                        imports += 1;
                    }
                }
            }
            wasmparser::Payload::CodeSectionEntry(body) => {
                let mut out = Vec::new();
                let mut ops = body.get_operators_reader().expect("operators");
                while !ops.eof() {
                    if let wasmparser::Operator::Call { function_index } = ops.read().expect("op") {
                        out.extend(function_index.checked_sub(imports));
                    }
                }
                calls.push(out);
            }
            wasmparser::Payload::CustomSection(c) if c.name() == "name" => {
                let wasmparser::KnownCustom::Name(r) = c.as_known() else {
                    continue;
                };
                for n in r {
                    if let wasmparser::Name::Function(map) = n.expect("names") {
                        for x in map {
                            let x = x.expect("name");
                            if let Some(k) = x.index.checked_sub(imports) {
                                names.push((k, x.name.to_owned()));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let name = |k: u32| {
        names
            .iter()
            .find(|(i, _)| *i == k)
            .map_or("?", |(_, n)| n.as_str())
            .to_owned()
    };
    let root = names.iter().find(|(_, n)| n == "main").expect("main").0;
    // Depth-first from `main`, a function on the path again being a cycle.
    let mut reached = Vec::new();
    let mut stack = vec![(root, vec![root])];
    while let Some((k, path)) = stack.pop() {
        reached.push(name(k));
        for &c in &calls[k as usize] {
            assert!(
                !path.contains(&c),
                "a cycle of direct calls through {}",
                name(c)
            );
            stack.push((c, [path.clone(), vec![c]].concat()));
        }
    }
    for cold in ["rt:list_copy", "rt:list_grow", "rt:panic_with", "rt:alloc"] {
        assert!(
            !reached.iter().any(|n| n.starts_with(cold)),
            "{cold} is a direct call from main: {reached:?}"
        );
    }
    assert!(reached.iter().any(|n| n.starts_with("rt:list_unique")));
}

/// A data relocation must never reinterpret an integer whose value happens
/// to equal an address. The relocated operand keeps its source mapping.
#[test]
fn session_static_relocation_preserves_integer_constants_and_origins() {
    use crate::module::{Global, Ins, STATIC_BASE};
    use crate::op::Op as I;

    let m = Module::default();
    let mut f = m.func("extension", vec![], vec![Val::I32; 3]);
    f.origins = Some(Vec::new());
    f.origin_at = Some(7);
    f.u32(STATIC_BASE + 8);
    f.origin_at = Some(9);
    f.static_addr(STATIC_BASE);
    f.origin_at = Some(11);
    f.static_addr(STATIC_BASE + 8);
    let base = Global::Storage(Hash128(646), 0, Val::I32);
    let global = f.intern_global(base);
    f.resolve_statics(Some(base));

    assert!(matches!(f.body.as_slice(), [
        Ins::Op(I::I32Const(n)), Ins::GlobalGet(a), Ins::GlobalGet(b),
        Ins::Op(I::I32Const(8)), Ins::Op(I::I32Add),
    ] if *n == (STATIC_BASE + 8).cast_signed() && *a == global && *b == global));
    assert_eq!(
        f.origins,
        Some(vec![Some(7), Some(9), Some(11), Some(11), Some(11)])
    );
    assert_eq!(f.origin_at, Some(11));
}

/// Ordinary modules lower the explicit operand before optimizations, so the
/// resulting bytes remain exactly those of the original constant emission.
#[test]
fn session_static_operands_keep_ordinary_wasm_identical() {
    fn emit(symbolic: bool) -> Vec<u8> {
        let mut m = Module::default();
        let address = m.static_bytes(b"a static literal", 1);
        let mut f = m.func("literal", vec![], vec![Val::I32]);
        if symbolic {
            f.static_addr(address);
        } else {
            f.u32(address);
        }
        let key = Sym::Inst(Hash128(646));
        m.define(key.clone(), f);
        m.export("literal", key);
        m.finish().expect("link")
    }
    let ordinary = emit(false);
    validate(&ordinary);
    assert_eq!(emit(true), ordinary);
}

/// Even a scalar first input must provide the shared shadow stack and a
/// growable table: a later input may introduce a wide call or new glue.
#[test]
fn session_base_exports_shared_storage_and_keeps_its_table_growable() {
    use crate::module::Global;
    let mut m = Module::default();
    let key = Sym::Inst(Hash128(646));
    let global = Global::Storage(Hash128(17), 0, Val::I32);
    let mut f = m.func("counter", vec![], vec![Val::I32]);
    f.i32(1);
    f.global_set(global);
    f.global_get(global);
    m.define(key.clone(), f);
    m.export("counter", key);
    let (mapped, _) = m.finish_session(true).expect("session base");
    validate(&mapped.wasm);
    let mut memories = 0;
    let mut globals = 0;
    let mut exported_globals = 0;
    let mut exports = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(&mapped.wasm) {
        match payload.expect("payload") {
            wasmparser::Payload::MemorySection(r) => memories = r.count(),
            wasmparser::Payload::GlobalSection(r) => globals = r.count(),
            wasmparser::Payload::TableSection(r) => {
                let table = r.into_iter().next().expect("shared table").expect("table");
                assert_eq!(table.ty.initial, 0);
                assert_eq!(table.ty.maximum, None);
            }
            wasmparser::Payload::ExportSection(r) => {
                for e in r {
                    let e = e.expect("export");
                    exported_globals += u32::from(e.kind == wasmparser::ExternalKind::Global);
                    exports.push(e.name.to_owned());
                }
            }
            _ => {}
        }
    }
    assert_eq!(memories, 3);
    assert_eq!(globals, 2); // counter storage and the shared stack pointer
    assert_eq!(exported_globals, globals);
    for name in [
        "hd.x",
        "hd.session.heap",
        "hd.session.stack",
        "hd.session.table",
        "hd.session.f.0",
    ] {
        assert!(exports.iter().any(|e| e == name), "missing {name}");
    }
}

/// A base cannot specialize away capabilities just because input 1 never
/// uses them: later modules must see the same view/cancellation runtime.
#[test]
fn session_base_does_not_assume_future_views_and_cancellation_are_absent() {
    let mut m = Module::default();
    let key = Sym::Inst(Hash128(646));
    let mut f = m.func("future_features", vec![], vec![Val::I32; 2]);
    f.helper(Helper::RefViewsAbsent);
    f.helper(Helper::CanCancel);
    m.define(key.clone(), f);
    m.export("future_features", key);
    let (mapped, _) = m.finish_session(true).expect("session base");
    validate(&mapped.wasm);
    let mut constants = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(&mapped.wasm) {
        if let wasmparser::Payload::CodeSectionEntry(body) = payload.expect("payload") {
            for op in body.get_operators_reader().expect("operators") {
                if let wasmparser::Operator::I32Const { value } = op.expect("operator") {
                    constants.push(value);
                }
            }
        }
    }
    assert_eq!(constants, [0, 1]);
}

/// An open-world base gives every allocation an exact descriptor alias before
/// a later input first constructs a weak value. Extensions reuse that alias
/// and type id, while the next concrete type appends both identities.
#[test]
fn session_weak_descriptor_aliases_and_type_ids_stay_stable() {
    use crate::module::{DescriptorKey, Ins};

    fn module(probe: u128, descriptors: &[Hash128], weak: &[Hash128]) -> Module {
        let mut m = Module::default();
        m.open_world = true;
        m.uses_weak = !weak.is_empty();
        m.weak_types.extend(weak.iter().copied());
        let glue = Sym::Inst(Hash128(7200));
        let drop = m.func("drop", vec![Val::I32], vec![]);
        m.define(glue.clone(), drop);
        let mut f = m.func(
            "weak_session_probe",
            vec![],
            vec![Val::I32; descriptors.len() + weak.len()],
        );
        for ty in descriptors {
            f.descriptor(glue.clone(), DescriptorKey::weak(*ty));
        }
        for ty in weak {
            f.push(Ins::WeakType(*ty));
        }
        let probe = Sym::Inst(Hash128(probe));
        m.define(probe.clone(), f);
        m.export("probe", probe);
        m.export("hd.init", Sym::Helper(Helper::EntryInit { inits: vec![] }));
        m
    }

    fn probe_constants(wasm: &[u8]) -> Vec<i32> {
        let mut imports = 0;
        let mut probe = None;
        let mut bodies = Vec::new();
        for payload in wasmparser::Parser::new(0).parse_all(wasm) {
            match payload.expect("payload") {
                wasmparser::Payload::ImportSection(r) => {
                    for import in r.into_imports() {
                        if matches!(import.expect("import").ty, wasmparser::TypeRef::Func(_)) {
                            imports += 1;
                        }
                    }
                }
                wasmparser::Payload::ExportSection(r) => {
                    probe = r.into_iter().find_map(|e| {
                        let e = e.expect("export");
                        (e.name == "probe").then_some(e.index)
                    });
                }
                wasmparser::Payload::CodeSectionEntry(body) => bodies.push(body),
                _ => {}
            }
        }
        let body = &bodies[usize::try_from(probe.expect("probe") - imports).expect("body")];
        body.get_operators_reader()
            .expect("operators")
            .into_iter()
            .filter_map(|op| match op.expect("operator") {
                wasmparser::Operator::I32Const { value } => Some(value),
                _ => None,
            })
            .collect()
    }

    let (a, b) = (Hash128(7201), Hash128(7202));
    let (base, manifest) = module(7210, &[a], &[])
        .finish_session(true)
        .expect("open-world base");
    validate(&base.wasm);
    let base_a = probe_constants(&base.wasm)[0];

    let (first, manifest) = module(7211, &[a], &[a])
        .finish_extension(&manifest)
        .expect("first weak type");
    validate(&first.wasm);
    let first = probe_constants(&first.wasm);
    assert_eq!(first[0], base_a);

    let (second, _) = module(7212, &[a, b], &[a, b])
        .finish_extension(&manifest)
        .expect("second weak type");
    validate(&second.wasm);
    let second = probe_constants(&second.wasm);
    assert_eq!(second[0], base_a);
    assert_eq!(second[1], base_a + 1);
    assert_eq!(second[2], first[1]);
    assert_eq!(second[3], first[1] + 1);
}

/// Runs a real base and extension on Node. The extension calls the old
/// function through its old table slot, shares its mutable global, relocates
/// fresh static data through an allocation, and appends a new table entry.
#[test]
fn session_extension_shares_state_and_appends_without_reinitializing() {
    use crate::module::{Address, Global, HEAP, Ins, STATIC_BASE, mem};
    use crate::op::Op as I;

    fn bump(m: &Module) -> crate::module::Func {
        let mut f = m.func("bump", vec![], vec![Val::I32]);
        let counter = Address::Binding(Hash128(646));
        let value = f.local(Val::I32);
        f.address(counter);
        f.op(I::I32Load(mem(HEAP, 0, 2)));
        f.i32(1);
        f.op(I::I32Add);
        f.set(value);
        f.address(counter);
        f.get(value);
        f.op(I::I32Store(mem(HEAP, 0, 2)));
        f.get(value);
        f
    }
    let old = Sym::Inst(Hash128(646));
    let fresh = Sym::Inst(Hash128(647));
    let tail = Sym::Inst(Hash128(648));
    let mut base = Module::default();
    base.static_bytes(b"BASE", 1);
    let counter_slot = base.static_slot(4, 4);
    base.binding_slots.insert(Hash128(646), counter_slot);
    base.define(old.clone(), bump(&base));
    base.export("bump", old.clone());
    base.export("alloc", Sym::Helper(Helper::Alloc));
    let mut address = base.func("old_slot", vec![], vec![Val::I32]);
    address.table_slot(old.clone());
    let address_key = Sym::Inst(Hash128(649));
    base.define(address_key.clone(), address);
    base.export("old_slot", address_key);
    let (base, manifest) = base.finish_session(true).expect("base");
    validate(&base.wasm);

    let mut extension = Module::default();
    extension.define(old.clone(), bump(&extension));
    let literal = extension.static_bytes(b"TAIL", 1);
    let new_binding = Hash128(651);
    let new_binding_slot = extension.static_slot(4, 4);
    extension
        .binding_slots
        .insert(new_binding, new_binding_slot);
    let mut f = extension.func("fresh", vec![], vec![Val::I32]);
    let fresh_counter = Global::Storage(Hash128(647), 0, Val::I32);
    f.global_get(fresh_counter);
    f.i32(17);
    f.op(I::I32Add);
    f.global_set(fresh_counter);
    f.global_get(fresh_counter);
    extension.define(fresh.clone(), f);
    let parent = extension.vtable(vec![old.clone()]);
    let child = extension.vtable_with(vec![fresh.clone()], vec![parent]);
    let mut f = extension.func("tail", vec![], vec![Val::I32; 7]);
    f.table_slot(old);
    f.call_indirect(vec![], vec![Val::I32]);
    f.static_addr(literal);
    f.op(I::I32Load8U(mem(HEAP, 0, 0)));
    f.u32(STATIC_BASE); // Same number as `literal`, deliberately not relocated.
    f.table_slot(fresh.clone());
    f.push(Ins::VTable(child));
    f.address(Address::Binding(Hash128(646)));
    f.address(Address::Binding(new_binding));
    extension.define(tail.clone(), f);
    extension.export("tail", tail);
    let (extension, manifest) = extension.finish_extension(&manifest).expect("extension");
    validate(&extension.wasm);
    // No old function body is redefined in the extension.
    let definitions: u32 = wasmparser::Parser::new(0)
        .parse_all(&extension.wasm)
        .filter_map(|p| match p.expect("payload") {
            wasmparser::Payload::FunctionSection(r) => Some(r.count()),
            _ => None,
        })
        .sum();
    assert_eq!(definitions, 2);
    let mut third = Module::default();
    let mut f = third.func("third", vec![], vec![Val::I32; 3]);
    f.call(Sym::Inst(Hash128(646)));
    f.global_get(fresh_counter);
    f.call(fresh);
    let third_key = Sym::Inst(Hash128(650));
    third.define(third_key.clone(), f);
    third.export("third", third_key);
    let (third, _) = third.finish_extension(&manifest).expect("third module");
    validate(&third.wasm);
    let script = format!(
        r"
const baseModule = new WebAssembly.Module(new Uint8Array({base:?}));
const imports = {{}};
for (const i of WebAssembly.Module.imports(baseModule)) {{
    (imports[i.module] ??= {{}})[i.name] = () => {{ throw Error('unexpected host call'); }};
}}
const base = new WebAssembly.Instance(baseModule, imports).exports;
if (base.bump() !== 1 || base.old_slot() !== 0) throw Error('bad base');
const module = new WebAssembly.Module(new Uint8Array({tail:?}));
const metadata = new DataView(WebAssembly.Module.customSections(module, 'hd.session')[0]);
// Static storage comes from the base's private bump allocator.
const block = base.alloc(metadata.getUint32(8, true) + 8);
new Uint8Array(base['hd.session.heap'].buffer, block, 8).fill(0);
const data = block + 8;
const table = base['hd.session.table'];
const prefixLength = table.length;
table.grow(metadata.getUint32(12, true) - table.length);
const extension = new WebAssembly.Instance(module, {{
    'hd:session:0': base,
    'hd:session': {{ data: new WebAssembly.Global({{value:'i32', mutable:false}}, data) }},
}}).exports;
const heapView = new DataView(base['hd.session.heap'].buffer);
for (let i = 0; i < metadata.getUint32(16, true); i++) {{
    const at = data + metadata.getUint32(20 + i * 4, true);
    heapView.setUint32(at, heapView.getUint32(at, true) + data - 16, true);
}}
const values = extension.tail();
if (JSON.stringify(values.slice(0,3)) !== '[2,84,16]' || values[3] !== prefixLength) throw Error('bad tail: ' + values);
if (heapView.getUint32(values[4], true) !== values[3]) throw Error('child table slot');
const parent = heapView.getUint32(values[4] + 4, true);
if (parent < data || parent >= data + metadata.getUint32(8, true)) throw Error('parent pointer was not relocated');
if (heapView.getUint32(parent, true) !== 0) throw Error('old parent table slot moved');
if (heapView.getUint32(values[5], true) !== 2) throw Error('binding address does not see old calls');
heapView.setUint32(values[5], 100, true);
if (base.bump() !== 101) throw Error('old code does not see the new alias');
if (values[6] !== data + 4 || heapView.getUint32(values[6], true) !== 0) throw Error('new binding initializer was not relocated');
if (table.get(values[3])() !== 17) throw Error('new table slot');
const heap = new Uint8Array(base['hd.session.heap'].buffer);
if (String.fromCharCode(...heap.slice(16,20)) !== 'BASE') throw Error('base data overwritten');
if (String.fromCharCode(...heap.slice(data,data+4)) !== 'TAIL') throw Error('missing extension data');
const thirdModule = new WebAssembly.Module(new Uint8Array({third:?}));
const thirdMetadata = new DataView(WebAssembly.Module.customSections(thirdModule, 'hd.session')[0]);
const thirdData = base.alloc(thirdMetadata.getUint32(8, true) + 8) + 8;
table.grow(thirdMetadata.getUint32(12, true) - table.length);
const third = new WebAssembly.Instance(thirdModule, {{
    'hd:session:0': base, 'hd:session:1': extension,
    'hd:session': {{data: new WebAssembly.Global({{value:'i32', mutable:false}}, thirdData)}},
}}).exports;
if (JSON.stringify(third.third()) !== '[102,17,34]') throw Error('third module lost an earlier global or function');
console.log('session extension passed');
",
        base = base.wasm,
        tail = extension.wasm,
        third = third.wasm
    );
    let output = std::process::Command::new("node")
        .args(["--input-type=module", "-e", &script])
        .output()
        .expect("Node available");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "session extension passed"
    );
}

/// Earlier wide functions must be imported after both modules agree on the
/// lowered ABI, and both callers must use the same shadow stack pointer.
#[test]
fn session_extension_reuses_a_lowered_wide_function() {
    use crate::op::Op as I;
    fn define_wide(m: &mut Module, key: Sym, width: usize) {
        let mut f = m.func("wide", vec![Val::I32; width], vec![Val::I32]);
        f.get(0);
        f.get(u32::try_from(width - 1).expect("width"));
        f.op(I::I32Add);
        m.define(key, f);
    }
    fn caller(m: &mut Module, wide: Sym, width: usize, offset: u32, key: u128) {
        let mut f = m.func("call", vec![], vec![Val::I32]);
        for i in 0..u32::try_from(width).expect("width") {
            f.u32(i + offset);
        }
        f.call(wide);
        let key = Sym::Inst(Hash128(key));
        m.define(key.clone(), f);
        m.export("call", key);
    }
    let width = crate::wide::WIDTH + 3;
    let wide = Sym::Inst(Hash128(670));
    let mut base = Module::default();
    define_wide(&mut base, wide.clone(), width);
    caller(&mut base, wide.clone(), width, 0, 671);
    let (base, manifest) = base.finish_session(true).expect("base");
    validate(&base.wasm);
    let mut extension = Module::default();
    define_wide(&mut extension, wide.clone(), width);
    caller(&mut extension, wide, width, 100, 672);
    let (extension, _) = extension.finish_extension(&manifest).expect("extension");
    validate(&extension.wasm);
    let script = format!(
        r"
const base = new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array({base:?}))).exports;
const extension = new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array({extension:?})), {{
    'hd:session:0': base,
    'hd:session': {{data: new WebAssembly.Global({{value:'i32', mutable:false}}, 0)}},
}}).exports;
if (base.call() !== 66 || extension.call() !== 266 || base.call() !== 66) throw Error('wide ABI or shared shadow stack');
",
        base = base.wasm,
        extension = extension.wasm
    );
    run_session_node(&script);
}

fn run_session_node(script: &str) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("node")
        .args(["--input-type=module"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Node available");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(script.as_bytes())
        .expect("script");
    let output = child.wait_with_output().expect("Node result");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Import discovery must include aliases and vtable-only targets, which
/// have no call instruction in any newly defined body.
#[test]
fn session_extension_imports_export_only_and_vtable_only_functions() {
    use crate::module::Ins;
    let first = Sym::Inst(Hash128(680));
    let second = Sym::Inst(Hash128(681));
    let mut base = Module::default();
    for (key, value) in [(first.clone(), 42), (second.clone(), 43)] {
        let mut f = base.func("value", vec![], vec![Val::I32]);
        f.i32(value);
        base.define(key, f);
    }
    base.export("value", first.clone());
    base.retain_instances(&[Hash128(681)]);
    let (base, manifest) = base.finish_session(true).expect("base");
    let mut extension = Module::default();
    extension.export("alias", first);
    let table = extension.vtable(vec![second]);
    let mut f = extension.func("vtable", vec![], vec![Val::I32]);
    f.push(Ins::VTable(table));
    let key = Sym::Inst(Hash128(682));
    extension.define(key.clone(), f);
    extension.export("vtable", key);
    let (extension, _) = extension.finish_extension(&manifest).expect("extension");
    validate(&extension.wasm);
    let script = format!(
        r"
const base = new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array({base:?}))).exports;
base['hd.session.table'].grow(1);
const extension = new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array({extension:?})), {{
    'hd:session:0': base,
    'hd:session': {{data: new WebAssembly.Global({{value:'i32', mutable:false}}, 4096)}},
}}).exports;
if (extension.alias() !== 42) throw Error('export-only function');
const slot = new DataView(base['hd.session.heap'].buffer).getUint32(extension.vtable(), true);
if (base['hd.session.table'].get(slot)() !== 43) throw Error('vtable-only function');
",
        base = base.wasm,
        extension = extension.wasm
    );
    run_session_node(&script);
}

/// The real JS session restores initialized storage after reinstantiating
/// data segments, and replays only the tail without repeating host calls.
#[test]
fn session_host_restores_checkpoint_and_active_tail_without_effects() {
    use crate::module::{Address, Global, HEAP, Import, mem};
    use crate::op::Op as I;

    let counter = Global::Storage(Hash128(690), 0, Val::I32);
    let place = Hash128(691);
    let mut previous = None;
    let mut base_manifest = None;
    let mut modules = Vec::new();
    for number in 1_u32..=4 {
        let mut m = Module::default();
        if number == 1 {
            let slot = m.static_slot(4, 4);
            m.binding_slots.insert(place, slot);
            m.export("hd.session.alloc", Sym::Helper(Helper::Alloc));
        }
        m.static_bytes(&[u8::try_from(number).unwrap(); 16], 1);
        let mut init = m.func("init", vec![], vec![]);
        init.call(Sym::Import(Import::new(
            "hd:Clock",
            "now",
            &[],
            &[Val::I64],
        )));
        init.op(I::Drop);
        init.global_get(counter);
        init.i32(1);
        init.op(I::I32Add);
        init.global_set(counter);
        init.address(Address::Binding(place));
        init.global_get(counter);
        init.op(I::I32Store(mem(HEAP, 0, 2)));
        let key = Sym::Inst(Hash128(700 + u128::from(number)));
        m.define(key.clone(), init);
        if number == 2 {
            m.export("hd.init.21", key.clone());
        }
        m.export(format!("hd.init.{number}"), key);
        let mut poll = m.func("poll", vec![Val::I32], vec![Val::I32]);
        for memory in [false, true] {
            if memory {
                poll.address(Address::Binding(place));
                poll.op(I::I32Load(mem(HEAP, 0, 2)));
            } else {
                poll.global_get(counter);
            }
            poll.u32(number);
            poll.op(I::I32Ne);
            poll.op(I::If(wasm_encoder::BlockType::Empty));
            poll.op(I::Unreachable);
            poll.op(I::End);
        }
        poll.i32(0);
        let key = Sym::Inst(Hash128(710 + u128::from(number)));
        m.define(key.clone(), poll);
        if number == 2 {
            m.export("hd.poll.21", key.clone());
        }
        m.export(format!("hd.poll.{number}"), key);
        let (mapped, added) = match &previous {
            None => m.finish_session(true),
            Some(previous) => m.finish_extension(previous),
        }
        .expect("linked module");
        validate(&mapped.wasm);
        modules.push(mapped.wasm);
        if number == 1 {
            base_manifest = Some(added.clone());
        }
        previous = Some(added);
    }
    let mut cancelled = Module::default();
    let mut init = cancelled.func("waiting_init", vec![], vec![]);
    init.i32(100);
    init.global_set(counter);
    init.address(Address::Binding(place));
    init.i32(100);
    init.op(I::I32Store(mem(HEAP, 0, 2)));
    init.op(I::I64Const(30_000));
    init.call(Sym::Import(Import::new(
        "hd:Clock",
        "sleep.start",
        &[Val::I64],
        &[Val::I32, Val::I32],
    )));
    init.op(I::Drop);
    init.op(I::Drop);
    let key = Sym::Inst(Hash128(720));
    cancelled.define(key.clone(), init);
    cancelled.export("hd.init.20", key);
    let mut poll = cancelled.func("waiting_poll", vec![Val::I32], vec![Val::I32]);
    poll.i32(-1);
    let key = Sym::Inst(Hash128(721));
    cancelled.define(key.clone(), poll);
    cancelled.export("hd.poll.20", key);
    let mut cleanup = cancelled.func("cancel_cleanup", vec![], vec![]);
    cleanup.i32(200);
    cleanup.global_set(counter);
    let key = Sym::Inst(Hash128(722));
    cancelled.define(key.clone(), cleanup);
    cancelled.export("hd.cancel", key);
    let (cancelled, _) = cancelled
        .finish_extension(base_manifest.as_ref().unwrap())
        .expect("cancelled extension");
    validate(&cancelled.wasm);
    let host = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../host/core.mts")
        .canonicalize()
        .unwrap();
    let script = format!(
        r"
import assert from 'node:assert/strict';
import {{pathToFileURL}} from 'node:url';
const {{createSession}} = await import(new URL('session.mjs', pathToFileURL({host:?})));
const modules = {modules:?}.map(bytes => new Uint8Array(bytes));
const cancelled = new Uint8Array({cancelled:?});
for (const cap of [1, 300, 100000]) {{
    const errors = [];
    const session = createSession({{out: t => errors.push(t), err: t => errors.push(t)}}, {{}}, null, {{journalCap: cap}});
    const sources = [];
    try {{
        for (const [epoch, wasm] of modules.entries()) {{
            sources.push([epoch + 1, `input ${{epoch + 1}}`]);
            const ran = await session.run(wasm, sources, epoch + 1);
            assert.equal(ran.live.status, 0);
            const calls = session.hostCalls();
            const journal = session.journal();
            const restored = await session.run(wasm, sources, null);
            assert.deepEqual(restored.kept, sources.map(([n]) => n));
            assert.equal(restored.live, null);
            assert.equal(session.hostCalls(), calls);
            assert.deepEqual(session.journal(), journal);
            assert.deepEqual(await session.run(wasm, sources, null, journal), restored);
            await assert.rejects(session.run(wasm, sources, null, new Uint8Array()), /current journal/);
            assert.deepEqual(errors, []);
        }}
        assert.equal(session.hostCalls(), 4);
        assert.equal(session.checkpointStats().epochs, 4);
        assert.equal(session.checkpointStats().archivedJournals > 0, cap === 1);
        await assert.rejects(session.run(modules[0], sources, null), /changed session code/);
    }} finally {{ session.close(); }}
}}
for (const cap of [1, 100000]) {{
    const errors = [];
    const session = createSession({{out: () => {{}}, err: t => errors.push(t)}}, {{}}, null, {{journalCap: cap}});
    try {{
        assert.equal((await session.run(modules[0], [[1, 'base']], 1)).live.status, 0);
        const waiting = session.run(cancelled, [[1, 'base'], [20, 'cancelled']], 20);
        const timer = setTimeout(() => session.stop(), 5);
        const ended = await waiting;
        clearTimeout(timer);
        assert.equal(ended.live.end.kind, 'cancelled');
        const next = await session.run(modules[1], [[1, 'base'], [21, 'next']], 21);
        assert.deepEqual(next.skipped, [20]);
        assert.equal(next.live.status, 0);
        assert.equal(session.hostCalls(), 3, 'the cancelled host call is never retried');
        assert.deepEqual(errors, ['cancelled\n']);
    }} finally {{ session.close(); }}
}}
const foreign = createSession({{out: () => {{}}, err: () => {{}}}});
try {{
    await assert.rejects(foreign.run(modules[3], [], null), /original provider session/);
}} finally {{ foreign.close(); }}
",
        host = host.to_string_lossy(),
        cancelled = cancelled.wasm,
    );
    run_session_node(&script);
}
