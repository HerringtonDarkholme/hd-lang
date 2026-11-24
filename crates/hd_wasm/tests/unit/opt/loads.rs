mod tests {
    use hd_wasm::testing::module::{Func, Ins, Val};
    use hd_wasm::testing::op::Op as I;
    use hd_wasm::testing::opt::loads::*;
    use wasm_encoder::BlockType::Empty;
    use wasm_encoder::MemArg;

    fn at(offset: u64) -> MemArg {
        MemArg {
            offset,
            align: 2,
            memory_index: 0,
        }
    }

    /// Parameters: two addresses (0, 1) and a test (2); one spare local (3).
    fn function(body: Vec<I>) -> Func {
        let mut f = Func::default();
        f.params = vec![Val::I32, Val::I32, Val::I32];
        f.locals = vec![Val::I32];
        f.body = body.into_iter().map(Ins::Op).collect();
        f.origins = Some((0..f.body.len()).map(|i| u32::try_from(i).ok()).collect());
        f
    }

    fn loads(f: &Func) -> usize {
        f.body
            .iter()
            .filter(|i| matches!(i, Ins::Op(I::I32Load(_) | I::I64Load(_))))
            .count()
    }

    fn reused(body: Vec<I>) -> (bool, Func) {
        let mut f = function(body);
        let changed = run(&mut f, &vec![]);
        assert_eq!(f.origins.as_ref().map(Vec::len), Some(f.body.len()));
        (changed, f)
    }

    const READ: [I; 3] = [I::LocalGet(0), I::I32Load(MEM), I::Drop];
    const MEM: MemArg = MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    };

    #[test]
    fn a_fact_crosses_an_arm_that_writes_nothing_and_dies_at_one_that_stores() {
        let arm = |inner: &[I]| {
            let mut body = READ.to_vec();
            body.extend([I::LocalGet(2), I::If(Empty)]);
            body.extend_from_slice(inner);
            body.push(I::End);
            body.extend(READ);
            body
        };
        let (changed, f) = reused(arm(&READ));
        assert!(changed);
        assert_eq!(loads(&f), 1);
        let (_, f) = reused(arm(&[I::LocalGet(1), I::I32Const(5), I::I32Store(at(8))]));
        assert_eq!(loads(&f), 2, "a store through another address may alias");
    }

    #[test]
    fn a_loop_entered_after_the_fact_keeps_it_only_when_its_body_writes_nothing() {
        let looped = |store: bool| {
            let mut body = READ.to_vec();
            body.push(I::Loop(Empty));
            body.extend(READ);
            if store {
                body.extend([I::LocalGet(1), I::I32Const(5), I::I32Store(MEM)]);
            }
            body.extend([I::LocalGet(2), I::BrIf(0), I::End]);
            body
        };
        assert_eq!(loads(&reused(looped(false)).1), 1);
        // The store after the read reaches it again through the back edge.
        let (changed, f) = reused(looped(true));
        assert!(!changed);
        assert_eq!(loads(&f), 2);
    }

    #[test]
    fn an_arms_fact_dies_at_its_else_and_its_end() {
        let mut body = vec![I::LocalGet(2), I::If(Empty)];
        body.extend(READ);
        body.push(I::Else);
        body.extend(READ);
        body.push(I::End);
        body.extend(READ);
        let (changed, f) = reused(body);
        assert!(!changed);
        assert_eq!(loads(&f), 3);
    }

    #[test]
    fn a_write_of_the_address_local_ends_the_fact_and_a_copy_carries_it() {
        let mut body = READ.to_vec();
        body.extend([I::LocalGet(1), I::LocalSet(0)]);
        body.extend(READ);
        assert!(!reused(body).0);
        // `local.tee 3` copies the loaded address; a load through the copy
        // is the load through the original.
        let (changed, f) = reused(vec![
            I::LocalGet(0),
            I::I32Load(MEM),
            I::LocalTee(3),
            I::I32Load(at(4)),
            I::Drop,
            I::LocalGet(0),
            I::I32Load(MEM),
            I::I32Load(at(4)),
            I::Drop,
            I::LocalGet(3),
            I::I32Load(at(4)),
            I::Drop,
        ]);
        assert!(changed);
        assert_eq!(loads(&f), 2);
    }

    #[test]
    fn a_store_forwards_its_operand_only_to_a_load_of_its_width() {
        let (changed, f) = reused(vec![
            I::LocalGet(0),
            I::LocalGet(1),
            I::I32Store(at(4)),
            I::LocalGet(0),
            I::I32Load(at(4)),
        ]);
        assert!(changed);
        assert_eq!(loads(&f), 0);
        assert!(matches!(f.body.last(), Some(Ins::Op(I::LocalGet(1)))));
        for store in [I::I32Store8(at(4)), I::I32Store16(at(4))] {
            let (changed, _) = reused(vec![
                I::LocalGet(0),
                I::LocalGet(1),
                store,
                I::LocalGet(0),
                I::I32Load(at(4)),
            ]);
            assert!(!changed, "a narrow store keeps the wider load");
        }
        let (changed, _) = reused(vec![
            I::LocalGet(0),
            I::I64Const(1),
            I::I64Store(MEM),
            I::LocalGet(0),
            I::I32Load(MEM),
        ]);
        assert!(!changed, "an i64 store keeps an i32 load");
    }

    #[test]
    fn atomic_accesses_end_every_fact() {
        for barrier in [
            vec![I::AtomicFence],
            vec![I::LocalGet(1), I::I32AtomicLoad(MEM), I::Drop],
        ] {
            let mut body = READ.to_vec();
            body.extend(barrier);
            body.extend(READ);
            assert!(!reused(body).0);
        }
    }

    #[test]
    fn work_is_linear_at_width_1000_and_depth_20() {
        for (width, depth) in [(1_000, 20), (10_000, 20), (1_000, 1_000)] {
            let mut body = Vec::new();
            for _ in 0..depth {
                body.extend([I::LocalGet(2), I::If(Empty), I::Loop(Empty)]);
            }
            body.extend(READ);
            for _ in 0..width {
                body.extend(READ);
            }
            for _ in 0..depth {
                body.extend([I::End, I::End]);
            }
            let n = body.len();
            let mut f = function(body);
            let mut visits = 0;
            assert!(run_with(&mut f, &vec![], &mut || visits += 1));
            assert_eq!(loads(&f), 1);
            assert!(visits <= 3 * n, "{visits} visits for {n} instructions");
        }
    }
}
