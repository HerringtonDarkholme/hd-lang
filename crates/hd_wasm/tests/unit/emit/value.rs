mod object_glue_tests {

    use hd_wasm::testing::op::Op as I;

    use hd_base::Hash128;
    use hd_wasm::testing::layout::Leaf;
    use hd_wasm::testing::module::{Ins, Module};
    use hd_wasm::testing::module::{Sym, Val};
    use hd_wasm::testing::rt::Helper;
    use hd_wasm::testing::rt::{Counted, ShimSyms};

    fn calls(f: &hd_wasm::testing::module::Func) -> Vec<Sym> {
        f.body
            .iter()
            .filter_map(|ins| match ins {
                Ins::Call(id, _) => Some(f.symbol(*id).clone()),
                _ => None,
            })
            .collect()
    }

    /// An object's release glue counts each part through its type: a
    /// part with a hook calls its release shim, which runs the hook, on
    /// that part's values; a known address releases with no shim.
    #[test]
    fn object_glue_counts_each_part_through_its_type() {
        let word = |offset| Leaf {
            val: Val::I32,
            offset,
            bytes: 4,
            signed: false,
        };
        let leaves = vec![word(0), word(4), word(8)];
        let mut module = Module::default();
        let hooked = Helper::DropParts {
            leaves: leaves.clone(),
            parts: vec![(
                1,
                Counted::Shims(Box::new(ShimSyms {
                    retain: Hash128(1),
                    release: Hash128(2),
                    retain_value: Hash128(1),
                    release_value: Hash128(2),
                    width: 2,
                    hooked: true,
                    release_at: false,
                    retain_at: false,
                    in_place: None,
                    small: None,
                })),
            )],
        };
        let f = hd_wasm::testing::rt::helper(&hooked, &mut module);
        assert_eq!(calls(&f), [Sym::Shim(Hash128(2), false, 2, true)]);
        let address = Helper::DropParts {
            leaves,
            parts: vec![(
                2,
                Counted::Address {
                    index: 0,
                    known: true,
                },
            )],
        };
        let f = hd_wasm::testing::rt::helper(&address, &mut module);
        assert!(calls(&f).iter().all(|s| !matches!(s, Sym::Shim(..))));
        assert!(
            f.body
                .iter()
                .any(|ins| matches!(ins, Ins::Op(I::I32Load(a)) if a.offset == 8))
        );
    }
}
