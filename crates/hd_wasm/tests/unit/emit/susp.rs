mod origin_tests {

    use hd_wasm::testing::op::Op as I;

    use hd_wasm::testing::module::{Ins, Val};

    #[test]
    fn resident_save_area_has_no_ephemeral_local_holes() {
        for count in [100_u32, 1_000] {
            let values = vec![Val::I64; count as usize];
            let skip: Vec<u32> = (0..count - 2).collect();
            let (area, size) = hd_wasm::testing::emit::susp::save_area(&values, &skip, 32);
            assert_eq!(area, [(count - 2, 32, Val::I64), (count - 1, 40, Val::I64)]);
            assert_eq!(size, 48);
        }
    }

    #[test]
    fn resident_spill_rewrites_get_set_tee_and_preserves_origins() {
        let m = hd_wasm::testing::module::Module::default();
        let mut f = m.func("resident", vec![Val::I32], vec![]);
        let value = f.local(Val::I32);
        let base = f.local(Val::I32);
        let stack = f.local(Val::I32);
        f.body = vec![
            Ins::LoadAll,
            Ins::Op(I::LocalGet(value)),
            Ins::Op(I::LocalTee(value)),
            Ins::Op(I::LocalSet(value)),
            Ins::Op(I::LocalGet(hd_wasm::testing::emit::susp::FRAME)),
            Ins::SpillBase,
            Ins::Op(I::Drop),
        ];
        f.origins = Some(vec![Some(10); 7]);
        hd_wasm::testing::emit::susp_spill::resident(
            &mut f,
            hd_wasm::testing::emit::susp_spill::Plan {
                area: &[(value, 32, Val::I32)],
                selected: &[false, true, false, false],
                start: 32,
                size: 40,
            },
            base,
            stack,
            0,
        );
        assert!(
            !f.body
                .iter()
                .any(|i| matches!(i, Ins::LoadAll | Ins::SpillBase))
        );
        assert!(!f.body.iter().any(|i| matches!(i,
            Ins::Op(I::LocalGet(l) | I::LocalSet(l) | I::LocalTee(l)) if *l == value)));
        assert_eq!(f.origins.as_ref().map(Vec::len), Some(f.body.len()));
    }
}
