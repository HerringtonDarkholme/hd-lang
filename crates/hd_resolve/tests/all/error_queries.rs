//! Error syntax shapes and resolved field lists stay aligned.

use hd_base::FileId;
use hd_intern::{PathTable, ShardedInterner};
use hd_resolve::error_type::{Message, fields, interpolated, label, shape};
use hd_resolve::{KnownItems, Names, Src};
use hd_types::InternPool;

#[test]
fn error_shapes_align_with_resolved_fields_and_ignore_other_declarations() {
    let text = include_str!("inputs/error_queries.hd");
    let parsed = hd_syntax::parse(text.as_bytes());
    assert!(parsed.is_ok(), "{:?}", parsed.diagnostics);
    let src = Src {
        parse: &parsed,
        text,
        file: FileId::from_raw(0),
    };
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
    let (items, _) = super::const_generics::resolve_in(names, text);
    let mut inspected = Vec::new();
    for decl in src.items() {
        let name = src.text(decl.name(&parsed.tokens).expect("named source declaration"));
        let def = names.item("app", name);
        let item = items
            .iter()
            .find(|i| i.def == def)
            .expect("resolved declaration");
        let field_lists = fields(&item.data);
        let Some(error) = shape(&src, decl) else {
            assert_eq!(name, "plain");
            assert!(field_lists.is_empty());
            inspected.push(name);
            continue;
        };
        assert_eq!(error.is_enum, name == "Status");
        assert!(!error.has_cause());
        assert_eq!(error.variants.len(), field_lists.len());
        let mut labels = Vec::new();
        let mut types = Vec::new();
        let mut messages = Vec::new();
        for (variant, fields) in error.variants.iter().zip(field_lists) {
            assert_eq!(variant.members.len(), fields.len());
            labels.push(fields.iter().map(|f| label(&names, f)).collect::<Vec<_>>());
            types.push(
                fields
                    .iter()
                    .map(|f| hd_resolve::show_ty(&names, f.ty))
                    .collect::<Vec<_>>(),
            );
            messages.push(match variant.message {
                Message::Text(message) => interpolated(&src, message),
                Message::Absent => Vec::new(),
                Message::Transparent => panic!("this fixture has no transparent message"),
            });
        }
        if name == "Lost" {
            assert_eq!(labels, [vec!["code"]]);
            assert_eq!(types, [vec!["i32"]]);
            assert_eq!(messages, [vec!["code"]]);
        } else {
            assert_eq!(name, "Status");
            assert_eq!(labels, [vec![], vec!["code", "name"]]);
            assert_eq!(types, [vec![], vec!["i32", "string"]]);
            assert_eq!(messages, [vec![], vec!["code", "name"]]);
        }
        inspected.push(name);
    }
    assert_eq!(inspected, ["Lost", "Status", "plain"]);
}
