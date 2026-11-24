mod tests {
    use hd_wasm::testing::module::Ins;
    use hd_wasm::testing::op::Op as I;
    use hd_wasm::testing::opt::constant::*;

    use hd_wasm::testing::module::Func;

    use hd_wasm::testing::module::Val;
    use wasm_encoder::BlockType;

    fn function(body: Vec<I>) -> Func {
        let mut f = Func::default();
        f.params = vec![Val::I32];
        f.locals = vec![Val::I32, Val::I64];
        f.body = body.into_iter().map(Ins::Op).collect();
        f.origins = Some((0..f.body.len()).map(|i| u32::try_from(i).ok()).collect());
        f
    }

    #[test]
    fn constants_cross_selected_arms_but_effects_and_labels_stay() {
        let mut f = function(vec![
            I::I32Const(32),
            I::LocalTee(0),
            I::LocalSet(1),
            I::LocalGet(0),
            I::I32Const(128),
            I::I32LtU,
            I::If(BlockType::Empty),
            I::LocalGet(1),
            I::I32Const(64),
            I::I32GtU,
            I::If(BlockType::Empty),
            I::Unreachable,
            I::End,
            I::I64Const(7),
            I::LocalSet(2),
            I::Else,
            I::Unreachable,
            I::End,
            I::LocalGet(2),
            I::I32Const(0),
            I::I64ExtendI32U,
            I::I64Shl,
        ]);
        assert!(run(&mut f));
        assert!(!f.body.iter().any(|i| matches!(
            i,
            Ins::Op(I::If(_) | I::Else | I::Unreachable | I::I64Shl | I::I32LtU | I::I32GtU)
        )));
        assert!(f.body.iter().any(|i| matches!(i, Ins::Op(I::LocalSet(2)))));
        assert!(matches!(f.body.last(), Some(Ins::Op(I::I64Const(7)))));
        assert_eq!(f.origins.as_ref().map(Vec::len), Some(f.body.len()));
    }

    #[test]
    fn unknown_joins_loop_backedges_and_label_exits_discard_facts() {
        for control in [
            I::If(BlockType::Empty),
            I::Loop(BlockType::Empty),
            I::Block(BlockType::Empty),
        ] {
            let mut f = function(vec![
                I::I32Const(7),
                I::LocalSet(1),
                I::LocalGet(0),
                control,
                I::I32Const(8),
                I::LocalSet(1),
                I::Br(0),
                I::I32Const(9),
                I::LocalSet(1),
                I::End,
                I::LocalGet(1),
            ]);
            run(&mut f);
            assert!(matches!(f.body.last(), Some(Ins::Op(I::LocalGet(1)))));
        }
        // A no-else conditional joins its skipped path too.
        let mut f = function(vec![
            I::LocalGet(0),
            I::If(BlockType::Empty),
            I::I32Const(3),
            I::LocalSet(1),
            I::End,
            I::LocalGet(1),
        ]);
        run(&mut f);
        assert!(matches!(f.body.last(), Some(Ins::Op(I::LocalGet(1)))));
    }

    #[test]
    fn signed_unsigned_and_masked_shifts_match_wasm_integer_rules() {
        for (op, expected) in [
            (I::I32LtU, 0),
            (I::I32LtS, 1),
            (I::I32GtU, 1),
            (I::I32GeS, 0),
        ] {
            assert!(
                matches!(evaluate(op, &[Ins::Op(I::I32Const(-1)), Ins::Op(I::I32Const(2))]), Some((2, I::I32Const(value))) if value == expected)
            );
        }
        assert!(matches!(
            evaluate(
                I::I64ShrU,
                &[Ins::Op(I::I64Const(-1)), Ins::Op(I::I64Const(65))]
            ),
            Some((2, I::I64Const(i64::MAX)))
        ));
        let mut f = function(vec![I::LocalGet(0), I::I32Const(32), I::I32Shl]);
        assert!(run(&mut f));
        assert!(matches!(f.body.as_slice(), [Ins::Op(I::LocalGet(0))]));
        // Keep trapping division for runtime; Wasm MIN % -1 is zero.
        assert!(
            evaluate(
                I::I32DivS,
                &[Ins::Op(I::I32Const(1)), Ins::Op(I::I32Const(0))]
            )
            .is_none()
        );
    }

    #[test]
    fn safe_division_and_remainders_fold_without_hiding_traps() {
        for (op, expected) in [(I::I32DivS, -3), (I::I32RemS, -2)] {
            assert!(
                matches!(evaluate(op, &[Ins::Op(I::I32Const(-17)), Ins::Op(I::I32Const(5))]), Some((2, I::I32Const(value))) if value == expected)
            );
        }
        assert!(matches!(
            evaluate(
                I::I64DivU,
                &[Ins::Op(I::I64Const(-1)), Ins::Op(I::I64Const(7))]
            ),
            Some((2, I::I64Const(2_635_249_153_387_078_802)))
        ));
        assert!(matches!(
            evaluate(
                I::I64RemS,
                &[Ins::Op(I::I64Const(i64::MIN)), Ins::Op(I::I64Const(-1))]
            ),
            Some((2, I::I64Const(0)))
        ));
        assert!(
            evaluate(
                I::I64DivS,
                &[Ins::Op(I::I64Const(i64::MIN)), Ins::Op(I::I64Const(-1))]
            )
            .is_none()
        );
        for op in [I::I64DivS, I::I64DivU, I::I64RemS, I::I64RemU] {
            assert!(evaluate(op, &[Ins::Op(I::I64Const(7)), Ins::Op(I::I64Const(0))]).is_none());
        }
    }

    #[test]
    fn unconsumed_tees_stay_compact_in_one_walk() {
        for width in [1, 1_000, 10_000] {
            let mut body = vec![I::I32Const(24)];
            body.extend(std::iter::repeat_n(I::LocalTee(1), width));
            body.push(I::Drop);
            let mut f = function(body);
            let n = f.body.len();
            let mut visits = 0;
            assert!(!run_with(&mut f, &mut || visits += 1));
            assert_eq!(f.body.len(), n);
            assert_eq!(
                f.body
                    .iter()
                    .filter(|i| matches!(i, Ins::Op(I::LocalTee(1))))
                    .count(),
                width
            );
            assert!(visits <= 5 * n);
            assert_eq!(f.origins.as_ref().map(Vec::len), Some(n));
        }
    }

    #[test]
    fn surviving_local_reads_avoid_rematerializing_constants() {
        let mut f = function(vec![
            I::I32Const(24),
            I::LocalSet(1),
            I::LocalGet(0),
            I::LocalGet(1),
            I::LocalSet(0),
            I::Drop,
        ]);
        run(&mut f);
        assert!(matches!(f.body[3], Ins::Op(I::LocalGet(1))));
        let mut f = function(vec![
            I::I32Const(3),
            I::LocalSet(1),
            I::LocalGet(1),
            I::I32Eqz,
        ]);
        run(&mut f);
        assert!(matches!(f.body.last(), Some(Ins::Op(I::I32Const(0)))));
    }

    #[test]
    fn different_literal_bits_keep_the_store() {
        use wasm_encoder::{Ieee32, Ieee64};
        for (a, b) in [
            (I::I32Const(24), I::I32Const(25)),
            (I::I64Const(24), I::I64Const(25)),
            (
                I::F32Const(Ieee32::new(0)),
                I::F32Const(Ieee32::new(1 << 31)),
            ),
            (
                I::F64Const(Ieee64::new(0x7ff8_0000_0000_0001)),
                I::F64Const(Ieee64::new(0x7ff8_0000_0000_0002)),
            ),
        ] {
            let mut f = function(vec![a, I::LocalSet(1), b, I::Drop]);
            compact_tees(&mut f, &[], &mut || {});
            assert!(matches!(f.body[1], Ins::Op(I::LocalSet(1))));
            assert_eq!(f.body.len(), 4);
        }
    }

    #[test]
    fn generated_width_and_depth_count_linear_work() {
        for (width, depth) in [(1_000, 20), (10_000, 20), (1_000, 1_000)] {
            let mut body = vec![I::Block(BlockType::Empty); depth];
            for _ in 0..width {
                body.extend([I::I32Const(3), I::LocalSet(1), I::LocalGet(1), I::Drop]);
            }
            body.extend(std::iter::repeat_n(I::End, depth));
            let mut f = function(body);
            let n = f.body.len();
            let mut visits = 0;
            run_with(&mut f, &mut || visits += 1);
            assert_eq!(visits, 3 * n);
            assert_eq!(f.origins.as_ref().map(Vec::len), Some(f.body.len()));
        }
    }
}
