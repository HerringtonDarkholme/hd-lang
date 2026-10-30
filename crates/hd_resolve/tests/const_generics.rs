//! Const generic parameters and arguments reach interfaces: a bare
//! parameter as itself, and a closed const expression as its value.

use std::sync::Arc;

use hd_base::{FileId, Stage};
use hd_diag::{Code, DiagBuf};
use hd_intern::{PathTable, ShardedInterner};
use hd_project::UseRoots;
use hd_resolve::{Cx, FolderIface, KnownItems, ModIn, Names, Src, World, build_folder, show_ty};
use hd_syntax::parse;
use hd_types::{ConstValue, InternPool, ParamRef, Prim, TyData};

struct OneFolder;

impl World for OneFolder {
    fn module_folder(&self, module: &str) -> Option<u32> {
        (module == "app").then_some(0)
    }

    fn usable(&self, module: &str) -> bool {
        module == "app"
    }

    fn test_only(&self, _module: &str) -> bool {
        false
    }

    fn iface(&self, _folder: u32) -> Option<Arc<FolderIface>> {
        None
    }
}

#[test]
fn const_declarations_and_arguments_resolve_with_kind_errors() {
    let text = "struct Buffer[T, N < const usize]:\n    value: T\n\nstruct Plain[T]:\n    value: T\n\nstruct BadKind[N < const f32]:\n    value: i32\n\nstruct WrongDefault[T = 4]:\n    value: T\n\ntype Four[T] = Buffer[T, 2 + 2]\ntype Repeated[T, N < const usize] = Buffer[T, N]\ntype ConstAsType[N < const usize] = N\ntype WrongArgument[T, N < const usize] = Buffer[T, T]\ntype WrongSlot = Plain[4]\n";
    let parsed = parse(text.as_bytes());
    assert!(parsed.is_ok());

    let pool = InternPool::new();
    let paths = PathTable::new();
    let syms = ShardedInterner::default();
    let known = KnownItems::new(&paths);
    let names = Names {
        pool: &pool,
        paths: &paths,
        syms: &syms,
        known: &known,
    };
    let cx = Cx {
        names,
        folder: 0,
        world: &OneFolder,
    };
    let src = Src {
        parse: &parsed,
        text,
        file: FileId::from_raw(0),
    };
    let input = ModIn {
        path: "app".to_owned(),
        roots: UseRoots::default(),
        src,
        seeds: Vec::new(),
        tests: false,
        session: None,
    };
    let mut diags = DiagBuf::default();
    let out = build_folder(&cx, &[input], None, Stage::FolderIface, &mut diags)
        .unwrap_or_else(|gap| panic!("stopped at {}", gap.what));
    assert_eq!(
        diags
            .content_order()
            .into_iter()
            .map(|index| diags.code[index])
            .collect::<Vec<_>>(),
        vec![Code::GenericKindMismatch; 5]
    );

    // `2 + 2` is evaluated as the parameter's `usize` (`types.generic.const.evaluate`).
    let four = out
        .modules
        .iter()
        .flat_map(|m| &m.items)
        .find(|i| names.text(i.name) == "Four")
        .expect("Four");
    let hd_resolve::ItemData::Alias(body) = four.data else {
        panic!("`Four` is not an alias");
    };
    names.record_params(four);
    assert_eq!(show_ty(&names, body), "Buffer[T, 4]");
    let TyData::Adt { args, .. } = pool.get(body) else {
        panic!("not a struct type");
    };
    assert_eq!(
        pool.get(pool.list_items(args)[1]),
        TyData::Const(ConstValue::Int {
            prim: Prim::Usize,
            bits: 4
        })
    );
}

/// The declarations resolved in a fresh session from `text`, and the
/// codes it reports.
fn resolve_with(
    names: Names<'_>,
    text: &str,
) -> (Vec<hd_resolve::Item>, Vec<hd_resolve::Export>, Vec<Code>) {
    let parsed = parse(text.as_bytes());
    assert!(parsed.is_ok());
    let cx = Cx {
        names,
        folder: 0,
        world: &OneFolder,
    };
    let input = ModIn {
        path: "app".to_owned(),
        roots: UseRoots::default(),
        src: Src {
            parse: &parsed,
            text,
            file: FileId::from_raw(0),
        },
        seeds: Vec::new(),
        tests: false,
        session: None,
    };
    let mut diags = DiagBuf::default();
    let out = build_folder(&cx, &[input], None, Stage::FolderIface, &mut diags)
        .unwrap_or_else(|gap| panic!("stopped at {}", gap.what));
    let codes = diags
        .content_order()
        .into_iter()
        .map(|index| diags.code[index])
        .collect();
    let items = out.modules.into_iter().flat_map(|m| m.items).collect();
    (items, out.exports, codes)
}

/// [`resolve_with`] for text that reports nothing.
fn resolve_in(names: Names<'_>, text: &str) -> (Vec<hd_resolve::Item>, Vec<hd_resolve::Export>) {
    let (items, exports, codes) = resolve_with(names, text);
    assert_eq!(codes, []);
    (items, exports)
}

#[test]
fn closed_const_expressions_evaluate_in_the_parameter_type() {
    let text = r#"struct Big[N < const i64]: pass
struct Small[N < const u8]: pass
struct Text[S < const string]: pass
struct Flag[B < const bool]: pass

LIMIT := 1 << 3

type Lowest = Big[-9223372036854775808]
type Named = Small[LIMIT * 2 + 0x0f]
type Joined = Text["a\n" + "b"]
type Short = Flag[false && 1 / 0 == 0]
type Negative = Small[-1]
type Wide = Big[2 ** 63]
"#;
    let (pool, paths, syms) = (
        InternPool::new(),
        PathTable::new(),
        ShardedInterner::default(),
    );
    let known = KnownItems::new(&paths);
    let names = Names {
        pool: &pool,
        paths: &paths,
        syms: &syms,
        known: &known,
    };
    let (items, _, codes) = resolve_with(names, text);
    assert_eq!(codes, [Code::TypeMismatch, Code::ConstEvaluationFailed]);
    let shown = |name: &str| {
        let item = items
            .iter()
            .find(|i| names.text(i.name) == name)
            .unwrap_or_else(|| panic!("no item `{name}`"));
        let hd_resolve::ItemData::Alias(body) = item.data else {
            panic!("`{name}` is not an alias");
        };
        show_ty(&names, body)
    };
    assert_eq!(shown("Lowest"), "Big[-9223372036854775808]");
    assert_eq!(shown("Named"), "Small[31]");
    assert_eq!(shown("Joined"), r#"Text["a\nb"]"#);
    assert_eq!(shown("Short"), "Flag[false]");
}

#[test]
fn const_parameters_and_parameter_arguments_survive_an_interface() {
    let text = "pub struct Buffer[T, N < const usize]:\n    value: T\n\npub struct Flag[B < const bool, S < const string]:\n    value: i32\n\npub fn keep[T, N < const usize](value: Buffer[T, N]) -> Buffer[T, N]:\n    value\n";
    let (pool, paths, syms) = (
        InternPool::new(),
        PathTable::new(),
        ShardedInterner::default(),
    );
    let known = KnownItems::new(&paths);
    let names = Names {
        pool: &pool,
        paths: &paths,
        syms: &syms,
        known: &known,
    };
    let (items, exports) = resolve_in(names, text);
    let bytes = hd_resolve::encode_items(&names, &items, &exports).expect("encode");

    // A fresh session, whose tables number paths and symbols differently.
    let (pool, paths, syms) = (
        InternPool::new(),
        PathTable::new(),
        ShardedInterner::default(),
    );
    paths.item("other", "x", "Y");
    syms.intern("an earlier symbol");
    let known = KnownItems::new(&paths);
    let names = Names {
        pool: &pool,
        paths: &paths,
        syms: &syms,
        known: &known,
    };
    let (read, _) = hd_resolve::decode_items(&names, &bytes).expect("decode");
    let item = |name: &str| {
        read.iter()
            .find(|i| names.text(i.name) == name)
            .unwrap_or_else(|| panic!("no item `{name}`"))
    };
    let kinds = |gs: &[hd_resolve::Generic]| gs.iter().map(|g| g.const_type).collect::<Vec<_>>();
    assert_eq!(kinds(&item("Buffer").generics), [None, Some(Prim::Usize)]);
    assert_eq!(
        kinds(&item("Flag").generics),
        [Some(Prim::Bool), Some(Prim::String)]
    );

    // `N` in `Buffer[T, N]` is `keep`'s own parameter, by path and position.
    let keep = item("keep");
    let hd_resolve::ItemData::Fn(sig) = &keep.data else {
        panic!("`keep` is not a function");
    };
    assert_eq!(kinds(&sig.generics), [None, Some(Prim::Usize)]);
    let TyData::Adt { args, .. } = pool.get(sig.params[0].1) else {
        panic!("not a struct type");
    };
    assert_eq!(
        pool.get(pool.list_items(args)[1]),
        TyData::Param(ParamRef {
            owner: keep.def,
            index: 1,
        })
    );
    names.record_params(keep);
    assert_eq!(show_ty(&names, sig.ret), "Buffer[T, N]");
}

#[test]
fn const_arguments_display_as_canonical_literals() {
    let (pool, paths, syms) = (
        InternPool::new(),
        PathTable::new(),
        ShardedInterner::default(),
    );
    let known = KnownItems::new(&paths);
    let names = Names {
        pool: &pool,
        paths: &paths,
        syms: &syms,
        known: &known,
    };
    let shown = |v| show_ty(&names, pool.intern_ty(&TyData::Const(v)));
    let int = |prim, bits| ConstValue::Int { prim, bits };
    assert_eq!(shown(int(Prim::I32, u64::MAX - 6)), "-7");
    assert_eq!(shown(int(Prim::U64, u64::MAX)), "18446744073709551615");
    assert_eq!(shown(ConstValue::Bool(false)), "false");
    assert_eq!(shown(ConstValue::Char('\'')), "'\\''");
    assert_eq!(shown(ConstValue::Char('\n')), "'\\n'");
    assert_eq!(
        shown(ConstValue::Str(syms.intern("say \"$x\"\t"))),
        "\"say \\\"\\$x\\\"\\t\""
    );
}
