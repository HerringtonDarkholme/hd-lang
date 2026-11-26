mod interpolation_metadata_tests {
    use hd_base::{DefId, Stage, Symbol};
    use hd_check::testing::{interpolation_buffer_field, with_checker_items};
    use hd_resolve::{Field, Item, ItemData, TypeAttrs};
    use hd_types::Ty;

    fn item(data: ItemData) -> Item {
        Item::new(DefId::from_raw(1), Symbol::from_raw(1), false, data)
    }

    fn field(ty: Ty) -> Field {
        Field {
            name: Symbol::from_raw(2),
            ty,
            public: false,
            has_default: false,
            embedded: false,
            lifetimes: Vec::new(),
            fn_lifetimes: Vec::new(),
        }
    }

    #[test]
    fn a_buffer_field_keeps_its_resolved_type() {
        for ty in [Ty::I32, Ty::STRING] {
            let declaration = item(ItemData::Struct(vec![field(ty)], TypeAttrs::default()));
            assert_eq!(
                interpolation_buffer_field(Some(&declaration)).expect("buffer"),
                ty
            );
        }
    }

    #[test]
    fn missing_or_wrong_builder_shapes_are_internal_errors() {
        let builtin = item(ItemData::Builtin);
        let empty = item(ItemData::Struct(vec![], TypeAttrs::default()));
        let extra = item(ItemData::Struct(
            vec![field(Ty::I32), field(Ty::STRING)],
            TypeAttrs::default(),
        ));
        let enum_item = item(ItemData::Enum {
            shared: vec![field(Ty::I32)],
            variants: vec![],
            attrs: TypeAttrs::default(),
        });
        for selected in [
            None,
            Some(&builtin),
            Some(&empty),
            Some(&extra),
            Some(&enum_item),
        ] {
            let error = interpolation_buffer_field(selected).expect_err("invalid shape");
            assert_eq!(error.stage, Stage::Body);
            assert!(error.bug);
            assert!(!error.out_of_fuel);
        }
    }

    #[test]
    fn missing_runtime_selections_propagate_without_emitting_tir() {
        for has_builder in [false, true] {
            with_checker_items(
                |names| {
                    if has_builder {
                        vec![Item::new(
                            names.known.string_builder,
                            names.syms.intern("StringBuilder"),
                            false,
                            ItemData::Struct(vec![field(Ty::I32)], TypeAttrs::default()),
                        )]
                    } else {
                        Vec::new()
                    }
                },
                |ck| {
                    let before = (
                        ck.b().body().len(),
                        ck.b().body().extra.len(),
                        ck.b().body().local_ty.len(),
                    );
                    let results = [
                        (
                            if has_builder { "new" } else { "StringBuilder" },
                            ck.new_interpolation_builder(),
                        ),
                        (
                            "value_new",
                            ck.string_builder_helper("value_new").map(|_| ()),
                        ),
                        ("build", ck.builder_call("build", &[]).map(|_| ())),
                        ("new", ck.interpolation_inherent(Ty::I32, "new").map(|_| ())),
                    ];
                    for (needed, result) in results {
                        let error = result.expect_err("missing runtime selection");
                        assert_eq!(error.stage, Stage::Body);
                        assert!(error.bug);
                        assert!(!error.out_of_fuel);
                        assert!(error.what.contains(needed), "{}", error.what);
                    }
                    assert_eq!(
                        (
                            ck.b().body().len(),
                            ck.b().body().extra.len(),
                            ck.b().body().local_ty.len()
                        ),
                        before,
                    );
                },
            );
        }
    }
}

mod map_loop_metadata_tests {
    use hd_base::Stage;
    use hd_check::testing::with_checker;
    use hd_tir::ir::IntrinsicOp;
    use hd_types::{Ty, TyData};

    #[test]
    fn map_pair_selects_the_existing_entry_operation() {
        with_checker(|ck| {
            let pool = ck.pool();
            for value in [Ty::I32, Ty::STRING] {
                for (field, expected) in [
                    (value, IntrinsicOp::MapSlotEntry),
                    (
                        pool.intern_ty(&TyData::Ref {
                            inner: value,
                            mutable: false,
                            counted: false,
                        }),
                        IntrinsicOp::MapSlotEntry,
                    ),
                    (
                        pool.intern_ty(&TyData::Ref {
                            inner: value,
                            mutable: true,
                            counted: false,
                        }),
                        IntrinsicOp::MapSlotEntryMut,
                    ),
                    (
                        pool.intern_ty(&TyData::Ref {
                            inner: value,
                            mutable: false,
                            counted: true,
                        }),
                        IntrinsicOp::MapSlotEntry,
                    ),
                    (
                        pool.intern_ty(&TyData::Ref {
                            inner: value,
                            mutable: true,
                            counted: true,
                        }),
                        IntrinsicOp::MapSlotEntryMut,
                    ),
                ] {
                    let pair = pool.product(pool.list(&[Ty::I32, field]), None);
                    assert_eq!(ck.map_loop_entry(pair).expect("map pair"), expected);
                }
            }
        });
    }

    #[test]
    fn malformed_map_pair_fails_before_loop_state_or_callback() {
        with_checker(|ck| {
            let pool = ck.pool();
            let mut invalid = vec![Ty::I32, Ty::POISON];
            for fields in [&[][..], &[Ty::I32][..], &[Ty::I32, Ty::I32, Ty::I32][..]] {
                invalid.push(pool.product(pool.list(fields), None));
            }
            invalid.push(pool.product(pool.list(&[Ty::I32, Ty::STRING]), Some(Ty::I32)));
            let before = (
                ck.b().body().len(),
                ck.b().body().extra.len(),
                ck.b().body().local_ty.len(),
                std::hint::black_box(hd_check::testing::Checker::depths)(ck).0,
                ck.depths().1,
                ck.infer().var_count(),
            );
            let mut calls = 0;
            for item in invalid {
                let error = ck
                    .map_loop(item, &mut || {
                        calls += 1;
                        Ok(())
                    })
                    .expect_err("malformed map item");
                assert_eq!(error.stage, Stage::Body);
                assert!(error.bug);
                assert!(!error.out_of_fuel);
                assert_eq!(error.what, "map loop requires a fixed key/value pair item");
                assert_eq!(
                    (
                        ck.b().body().len(),
                        ck.b().body().extra.len(),
                        ck.b().body().local_ty.len(),
                        ck.depths().0,
                        ck.depths().1,
                        ck.infer().var_count()
                    ),
                    before
                );
            }
            assert_eq!(calls, 0);
        });
    }
}
