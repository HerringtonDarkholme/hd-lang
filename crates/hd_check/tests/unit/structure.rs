mod structure_input_tests {
    use hd_base::{DefId, NotImplemented, Stage, Symbol};
    use hd_check::testing::{adt_args, structure_decl, with_checker};
    use hd_resolve::{Field, Item, ItemData, TypeAttrs, Variant};
    use hd_types::{Ty, TyData};

    fn internal_error(error: &NotImplemented) {
        assert_eq!(error.stage, Stage::Body);
        assert!(error.bug);
        assert!(!error.out_of_fuel);
    }

    #[test]
    fn selected_declarations_borrow_struct_enum_and_shared_fields() {
        let field = Field {
            name: Symbol::from_raw(1),
            ty: Ty::I32,
            public: true,
            has_default: false,
            embedded: false,
            lifetimes: Vec::new(),
            fn_lifetimes: Vec::new(),
        };
        let data = [
            ItemData::Struct(vec![field.clone()], TypeAttrs::default()),
            ItemData::Enum {
                shared: vec![field.clone()],
                variants: vec![Variant {
                    name: Symbol::from_raw(2),
                    def: DefId::from_raw(2),
                    fields: vec![field],
                }],
                attrs: TypeAttrs::default(),
            },
        ];
        for data in data {
            let item = Item::new(DefId::from_raw(1), Symbol::from_raw(1), false, data);
            let (header, fields, variants) =
                structure_decl(Some(&item)).expect("selected declaration");
            assert!(std::ptr::eq(header, &raw const item));
            match (variants, &item.data) {
                (None, ItemData::Struct(expected, _)) => {
                    assert!(std::ptr::eq(fields, expected.as_slice()));
                }
                (
                    Some(variants),
                    ItemData::Enum {
                        shared: expected_shared,
                        variants: expected_variants,
                        ..
                    },
                ) => {
                    assert!(std::ptr::eq(fields, expected_shared.as_slice()));
                    assert!(std::ptr::eq(variants, expected_variants.as_slice()));
                }
                _ => panic!("declaration kind changed"),
            }
        }
    }

    #[test]
    fn missing_and_non_data_declarations_report_an_internal_error() {
        let item = Item::new(
            DefId::from_raw(1),
            Symbol::from_raw(1),
            false,
            ItemData::Builtin,
        );
        for selected in [None, Some(&item)] {
            internal_error(&structure_decl(selected).expect_err("invalid selection"));
        }
    }

    #[test]
    fn selected_adt_arguments_preserve_the_interned_list() {
        with_checker(|ck| {
            let pool = ck.pool();
            let args = pool.list(&[Ty::I32, Ty::VOID]);
            let ty = pool.intern_ty(&TyData::Adt(
                hd_types::AdtId::Declared(DefId::from_raw(1)),
                args,
            ));
            assert_eq!(adt_args(&pool.get(ty)).expect("ADT arguments"), args);
            let rest = pool.applied(ck.cx().names.known.list, Ty::I32);
            for other in [
                Ty::I32,
                Ty::VOID,
                Ty::POISON,
                pool.intern_ty(&TyData::sum(args)),
                pool.product(args, None),
                pool.product(args, Some(rest)),
                pool.intern_ty(&TyData::Adt(hd_types::AdtId::Declared(DefId::NONE), args)),
            ] {
                internal_error(&adt_args(&pool.get(other)).expect_err("non-nominal ADT"));
            }
        });
    }
}

mod self_reference_walk_tests {
    use hd_check::testing::with_checker_items;
    use hd_resolve::{Field, Generic, Item, ItemData, TypeAttrs};
    use hd_types::{ParamRef, Ty, TyData, TyList};

    #[test]
    fn refers_at_depth_1000_and_growing_declaration_cycles_stay_linear() {
        std::thread::Builder::new()
            .stack_size(96 << 10)
            // cost: one 96 KiB thread for the generated graph cases.
            .spawn(|| {
                with_checker_items(
                    |names| {
                        let p = names.pool.types();
                        let target = p.intern_ty(&TyData::Adt(
                            hd_types::AdtId::Declared(names.item("app.main", "Target")),
                            TyList::EMPTY,
                        ));
                        let field = |ty| Field {
                            name: names.syms.intern("field"),
                            ty,
                            public: true,
                            has_default: false,
                            embedded: false,
                            lifetimes: Vec::new(),
                            fn_lifetimes: Vec::new(),
                        };
                        let growing = |name, more: &[Ty]| {
                            let def = names.item("app.main", name);
                            let param = p.intern_ty(&TyData::Param(ParamRef {
                                owner: def,
                                index: 0,
                            }));
                            let list = p.intern_ty(&TyData::Adt(
                                hd_types::AdtId::Declared(names.known.list),
                                p.list(&[param]),
                            ));
                            let again = p.intern_ty(&TyData::Adt(
                                hd_types::AdtId::Declared(def),
                                p.list(&[list]),
                            ));
                            let again = p.intern_ty(&TyData::Ref {
                                inner: again,
                                mutable: false,
                                counted: true,
                            });
                            let mut fields = vec![field(again)];
                            fields.extend(more.iter().copied().map(field));
                            let mut item = Item::new(
                                def,
                                names.syms.intern(name),
                                true,
                                ItemData::Struct(fields, TypeAttrs::default()),
                            );
                            item.generics.push(Generic::plain(names.syms.intern("T")));
                            item
                        };
                        let a = growing("Grow", &[]);
                        let a_ty = p.intern_ty(&TyData::Adt(
                            hd_types::AdtId::Declared(a.def),
                            p.list(&[Ty::I32]),
                        ));
                        let b = Item::new(
                            names.item("app.main", "Hidden"),
                            names.syms.intern("Hidden"),
                            true,
                            ItemData::Struct(
                                vec![field(a_ty), field(target)],
                                TypeAttrs::default(),
                            ),
                        );
                        let b_ty = p.intern_ty(&TyData::Adt(
                            hd_types::AdtId::Declared(b.def),
                            TyList::EMPTY,
                        ));
                        vec![a, b, growing("Positive", &[b_ty])]
                    },
                    |ck| {
                        let p = ck.cx().names.pool.types();
                        let data = ck.cx().names.item("app.main", "Target");
                        let target = p.intern_ty(&TyData::Adt(
                            hd_types::AdtId::Declared(data),
                            TyList::EMPTY,
                        ));
                        for depth in [100, 1000] {
                            let refs = ck.self_refs(data);
                            for leaf in [target, Ty::I32] {
                                let mut root = leaf;
                                for _ in 0..depth {
                                    root = p.product(p.list(&[root, root]), None);
                                }
                                let positive = leaf == target;
                                assert_eq!(refs.needs(root), positive);
                                let work = (depth + 1) * if positive { 2 } else { 1 };
                                assert_eq!(
                                    hd_types::work::count_type_nodes(|| refs.refers(root)),
                                    (positive, work)
                                );
                                assert_eq!(
                                    hd_types::work::count_type_nodes(|| refs.refers(root)),
                                    (positive, 0)
                                );
                            }
                        }
                        let refs = ck.self_refs(data);
                        let adt = |name: &str, args: &[Ty]| {
                            p.intern_ty(&TyData::Adt(
                                hd_types::AdtId::Declared(ck.cx().names.item("app.main", name)),
                                p.list(args),
                            ))
                        };
                        let negative = adt("Grow", &[Ty::I32]);
                        let (answer, work) =
                            hd_types::work::count_type_nodes(|| refs.refers(negative));
                        assert!(!answer);
                        assert!(work < 20, "raw generic cycle is finite: {work}");
                        assert!(
                            refs.refers(adt("Grow", &[target])),
                            "actual arguments always contribute"
                        );
                        assert!(refs.refers(adt("Hidden", &[])));
                        assert!(refs.refers(adt("Positive", &[Ty::I32])));
                        assert!(!refs.refers(negative));
                        for mutable in [false, true] {
                            for counted in [false, true] {
                                assert!(refs.refers(p.intern_ty(&TyData::Ref {
                                    inner: target,
                                    mutable,
                                    counted
                                })));
                            }
                        }
                        for data in [
                            TyData::sum(p.list(&[target])),
                            TyData::TraitValue {
                                def: data,
                                args: p.list(&[target]),
                                bindings: Vec::new(),
                            },
                            TyData::Opaque {
                                owner: data,
                                site: 0,
                                borrows: true,
                                args: p.list(&[target]),
                                bound: target,
                            },
                            TyData::Fn {
                                params: p.list(&[target]),
                                result: target,
                                row: hd_types::RowId::EMPTY,
                                suspends: false,
                                inputs: hd_types::Inputs::Fixed,
                                deps: hd_types::FnDeps::NONE,
                            },
                        ] {
                            assert!(!refs.refers(p.intern_ty(&data)), "original boundary");
                        }
                        for width in [100, 1000] {
                            let refs = ck.self_refs(data);
                            let mut root = Ty::I32;
                            for _ in 0..20 {
                                root = p.product(p.list(&[root, root]), None);
                            }
                            assert!(!refs.refers(root));
                            for i in 0..width {
                                let unrelated = adt(&format!("Unrelated{i}"), &[]);
                                assert_eq!(
                                    hd_types::work::count_type_nodes(|| refs.refers(unrelated)),
                                    (false, 2)
                                );
                            }
                            assert_eq!(
                                hd_types::work::count_type_nodes(|| refs.refers(root)),
                                (false, 0)
                            );
                        }
                    },
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
