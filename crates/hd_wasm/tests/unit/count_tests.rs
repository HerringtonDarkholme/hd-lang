mod tests {
    use hd_base::Hash128;
    use hd_wasm::testing::count_tests::*;
    use hd_wasm::testing::module::Module;
    use hd_wasm::testing::module::{Func, Ins, Sym, Val};
    use hd_wasm::testing::op::Op as I;
    use hd_wasm::testing::rt::Helper;
    use wasm_encoder::BlockType;

    fn config() -> Counts {
        Counts {
            retain: Hash128(7),
            acyclic: false,
            known_inlined: false,
            skipped: Some(Hash128(8)),
        }
    }

    fn function() -> Func {
        Module::default().func("count operands", vec![Val::I32], vec![])
    }

    fn guards(f: &Func) -> usize {
        f.body
            .iter()
            .filter(|i| matches!(i, Ins::Op(I::If(_))))
            .count()
    }

    #[test]
    fn existing_local_producers_and_zero_width_markers_need_no_temporary() {
        for tee in [false, true] {
            let mut f = function();
            let source = if tee { f.local(Val::I32) } else { 0 };
            f.get(0);
            if tee {
                f.op(I::LocalTee(source));
            }
            let dependency = f.intern(Sym::Helper(Helper::ReleaseRef));
            f.push(Ins::DrainGlue(dependency));
            f.at = Some(17);
            f.call(Sym::Helper(Helper::ReleaseKnown));
            f.origins = Some(
                (0..f.body.len())
                    .map(|i| Some(u32::try_from(i).unwrap()))
                    .collect(),
            );
            f.origin_at = Some(99);
            f.at = Some(117);
            let original_locals = f.locals.len();
            let call_origin = f.origins.as_ref().unwrap().last().copied().unwrap();
            assert!(lower(&mut f, config()));
            assert_eq!(f.locals.len(), original_locals);
            assert_eq!(guards(&f), 2);
            assert_eq!(
                f.body
                    .iter()
                    .filter(|i| matches!(i, Ins::Op(I::LocalTee(_))))
                    .count(),
                usize::from(tee)
            );
            assert!(f.body.iter().any(|i| matches!(i, Ins::Call(id, Some(17))
                if matches!(f.symbol(*id), Sym::Helper(Helper::ReleaseKnown)))));
            assert!(
                f.body
                    .iter()
                    .any(|i| matches!(i, Ins::DrainGlue(s) if *s == dependency))
            );
            assert!(
                f.body
                    .iter()
                    .filter_map(|i| if let Ins::Op(I::LocalGet(l)) = i {
                        Some(l)
                    } else {
                        None
                    })
                    .all(|l| *l == 0 || *l == source)
            );
            assert_eq!(f.origin_at, Some(99));
            assert_eq!(f.at, Some(117));
            let origins = f.origins.as_ref().unwrap();
            assert_eq!(origins.len(), f.body.len());
            let first_guard = f
                .body
                .iter()
                .position(|i| matches!(i, Ins::Op(I::If(_))))
                .unwrap();
            assert!(origins[first_guard..].iter().all(|o| *o == call_origin));
        }
    }

    #[test]
    fn computed_operands_have_independent_single_definition_temporaries() {
        let mut f = function();
        for offset in [8, 16] {
            f.get(0);
            f.i32(offset);
            f.op(I::I32Add);
            f.call(Sym::Helper(Helper::ReleaseRef));
        }
        assert!(lower(&mut f, config()));
        let temps: Vec<_> = f
            .body
            .iter()
            .filter_map(|i| match i {
                Ins::Op(I::LocalTee(l)) => Some(*l),
                _ => None,
            })
            .collect();
        assert_eq!(temps, [1, 2]);
        assert_eq!(f.locals, [Val::I32, Val::I32]);
    }

    #[test]
    fn modes_keep_known_inlining_tracing_and_skipped_count_behavior() {
        for acyclic in [false, true] {
            for known_inlined in [false, true] {
                for checked in [false, true] {
                    let mut f = function();
                    for symbol in [
                        Sym::Helper(Helper::RetainKnown),
                        Sym::Helper(Helper::ReleaseKnown),
                        Sym::Helper(Helper::ReleaseRef),
                        Sym::Inst(Hash128(7)),
                    ] {
                        f.get(0);
                        f.call(symbol);
                    }
                    let c = Counts {
                        acyclic,
                        known_inlined,
                        skipped: checked.then_some(Hash128(8)),
                        ..config()
                    };
                    assert!(lower(&mut f, c));
                    assert!(f.locals.is_empty());
                    let known = usize::from(!known_inlined);
                    assert_eq!(guards(&f), 2 + 4 * known);
                    assert_eq!(
                        f.body
                            .iter()
                            .filter(|i| matches!(i, Ins::GlobalGet(_)))
                            .count(),
                        usize::from(!acyclic && !known_inlined)
                    );
                    assert_eq!(
                        f.body
                            .iter()
                            .filter(|i| matches!(i, Ins::Op(I::Else)))
                            .count(),
                        usize::from(checked) * (2 + 4 * known)
                    );
                }
            }
        }
        let mut f = function();
        f.get(0);
        f.call(Sym::Inst(Hash128(99)));
        assert!(!lower(&mut f, config()));
        assert!(f.locals.is_empty());
    }

    #[test]
    fn generated_width_and_depth_have_linear_visits_and_fixed_guard_expansion() {
        for width in [1000, 10000] {
            for computed in [false, true] {
                let mut f = function();
                for _ in 0..20 {
                    f.op(I::Block(BlockType::Empty));
                }
                for _ in 0..width {
                    f.get(0);
                    if computed {
                        f.i32(8);
                        f.op(I::I32Add);
                    }
                    f.call(Sym::Helper(Helper::ReleaseKnown));
                }
                for _ in 0..20 {
                    f.op(I::End);
                }
                let input = f.body.len();
                let mut visits = 0;
                assert!(lower_with_work(&mut f, config(), &mut || visits += 1));
                assert!(visits <= 2 * input, "{width}/{computed}: {visits}/{input}");
                assert!(f.body.len() <= input + 18 * width);
                assert_eq!(f.locals.len(), if computed { width } else { 0 });
                assert_eq!(guards(&f), 2 * width);
            }
        }
    }
}
