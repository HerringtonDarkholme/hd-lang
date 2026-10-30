//! Const generic syntax reaches resolution without being encoded as a fake
//! type argument before the interface slice exists.

use std::sync::Arc;

use hd_base::{FileId, Stage};
use hd_diag::{Code, DiagBuf};
use hd_intern::{PathTable, ShardedInterner};
use hd_project::UseRoots;
use hd_resolve::{Cx, FolderIface, KnownItems, ModIn, Names, Src, World, build_folder};
use hd_syntax::parse;
use hd_types::InternPool;

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
fn const_declarations_and_arguments_reach_the_not_yet_boundary() {
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
    let result = build_folder(&cx, &[input], None, Stage::FolderIface, &mut diags);

    let Err(unsupported) = result else {
        panic!("const generics unexpectedly reached an interface");
    };
    assert!(
        matches!(
            unsupported.what.as_str(),
            "const generic parameters" | "const generic arguments"
        ),
        "{}",
        unsupported.what
    );
    assert_eq!(
        diags
            .content_order()
            .into_iter()
            .map(|index| diags.code[index])
            .collect::<Vec<_>>(),
        vec![
            Code::GenericKindMismatch,
            Code::GenericKindMismatch,
            Code::GenericKindMismatch,
            Code::GenericKindMismatch,
            Code::GenericKindMismatch,
        ]
    );
}
