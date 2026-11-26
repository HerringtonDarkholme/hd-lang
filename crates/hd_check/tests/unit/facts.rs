use hd_base::{DefId, Stage, Symbol};
use hd_check::testing::facts_items;
use hd_resolve::{Field, Item, ItemData, TypeAttrs};
use hd_types::Ty;

fn item(data: ItemData) -> Item {
    Item::new(DefId::from_raw(1), Symbol::from_raw(1), false, data)
}

#[test]
fn items_type_is_the_first_resolved_field() {
    let field = |ty| Field {
        name: Symbol::from_raw(2),
        ty,
        public: true,
        has_default: false,
        embedded: false,
        lifetimes: Vec::new(),
        fn_lifetimes: Vec::new(),
    };
    for ty in [Ty::I32, Ty::VOID] {
        let declaration = item(ItemData::Struct(
            vec![field(ty), field(Ty::POISON)],
            TypeAttrs::default(),
        ));
        assert_eq!(facts_items(Some(&declaration)).expect("items type"), ty);
    }
}

#[test]
fn absent_non_struct_and_empty_metadata_are_internal_errors() {
    let wrong = item(ItemData::Builtin);
    let empty = item(ItemData::Struct(Vec::new(), TypeAttrs::default()));
    let enum_item = item(ItemData::Enum {
        shared: Vec::new(),
        variants: Vec::new(),
        attrs: TypeAttrs::default(),
    });
    for declaration in [None, Some(&wrong), Some(&empty), Some(&enum_item)] {
        let error = facts_items(declaration).expect_err("invalid Facts metadata");
        assert_eq!(error.stage, Stage::Body);
        assert!(error.bug);
        assert!(!error.out_of_fuel);
    }
}

#[test]
fn a_literal_marker_without_its_decorator_keeps_the_checker_unchanged() {
    use hd_check::testing::{Checker, with_checker_items};
    use hd_resolve::iface::{LITERAL_PREFIX, LITERAL_SUFFIX};
    use hd_resolve::{FnAttrs, FnSig};
    for kind in [LITERAL_SUFFIX, LITERAL_PREFIX] {
        with_checker_items(
            |names| {
                vec![Item::new(
                    names.item("app.main", "marked"),
                    Symbol::NONE,
                    false,
                    ItemData::Fn(
                        FnSig::simple(vec![], vec![], Ty::I32),
                        FnAttrs {
                            literal_fn: kind,
                            ..FnAttrs::default()
                        },
                    ),
                )]
            },
            |ck| {
                let def = ck.cx().names.item("app.main", "marked");
                let node = ck.root();
                let before = ck.b().body().len();
                std::hint::black_box(Checker::check_literal_marker)(ck, def, node);
                assert_eq!(ck.b().body().len(), before);
            },
        );
    }
}
