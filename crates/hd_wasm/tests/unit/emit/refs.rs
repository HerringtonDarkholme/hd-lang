mod tests {
    use hd_base::{DefId, LocalId};
    use hd_tir::ir::{BodyKind, Place, TirBuilder};
    use hd_wasm::testing::emit::refs::bare_local;

    #[test]
    fn memory_trait_value_projection_loads_both_leaves_and_counts_nothing() {
        use hd_wasm::testing::layout::Leaf;
        use hd_wasm::testing::module::{Func, Ins, Val};
        use hd_wasm::testing::op::Op;
        use hd_wasm::testing::rt::RefLayout;

        // A legal backend representation: two i32 leaves in memory,
        // data first and vtable second, with a count on the data address.
        // This exercises representation emission; it does not claim a
        // checked user expression currently produces this storage form.
        let value = RefLayout {
            size: 8,
            leaves: [0, 4]
                .map(|offset| Leaf {
                    offset,
                    val: Val::I32,
                    bytes: 4,
                    signed: false,
                })
                .into(),
            counted: hd_wasm::testing::rt::Counted::Address {
                index: 0,
                known: false,
            },
        };
        for fat in [false, true] {
            let mut function = Func::default();
            let address = function.local(Val::I32);
            hd_wasm::testing::emit::refs::project_reference(&mut function, address, &value, fat);
            let loads: Vec<_> = function
                .body
                .iter()
                .filter_map(|instruction| {
                    if let Ins::Op(Op::I32Load(argument)) = instruction {
                        Some(argument.offset)
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(loads, if fat { vec![0, 4] } else { vec![] });
            let calls: Vec<_> = function
                .body
                .iter()
                .filter_map(|instruction| {
                    if let Ins::Call(symbol, _) = instruction {
                        Some((*function.symbol(*symbol)).clone())
                    } else {
                        None
                    }
                })
                .collect();
            // A counted reference's count is its stated `Retain`.
            assert!(calls.is_empty(), "a projection must not acquire a count");
            let results = if fat { 2 } else { 1 };
            assert!(
                function
                    .body
                    .iter()
                    .rev()
                    .take(results)
                    .all(|instruction| matches!(instruction, Ins::Op(Op::LocalGet(_))))
            );
        }
    }

    #[test]
    fn bare_local_decodes_canonical_whole_locals() {
        let mut builder = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        for local in [0, 7, 1024] {
            let word = builder.place(&Place::local(LocalId::from_raw(local)));
            assert_eq!(bare_local(builder.body(), word), local as usize);
        }
    }

    #[cfg(debug_assertions)]
    #[test]
    fn bare_local_rejects_other_place_encodings() {
        // Imported here: the test exists only with debug assertions, so the
        // release test build would warn about them at module level.
        use hd_tir::ir::{PlaceRoot, Proj, Ref, place};
        let mut builder = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let record_local = builder.words_record(&[place::ROOT_LOCAL, 7]);
        let projected = builder.place(&Place {
            root: PlaceRoot::Local(LocalId::from_raw(7)),
            proj: vec![Proj::Field(0)],
        });
        let value = builder.place(&Place {
            root: PlaceRoot::Value(Ref(0)),
            proj: vec![],
        });
        for word in [record_local, projected, value, place::BARE_LOCAL - 1] {
            assert!(std::panic::catch_unwind(|| bare_local(builder.body(), word)).is_err());
        }
    }
}
