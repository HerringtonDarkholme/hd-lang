use hd_check::testing::{variant_list, with_checker};

#[test]
fn range_comparisons_preserve_single_bounds_and_reject_missing_bounds() {
    with_checker(|ck| {
        let before = (ck.b().body().len(), ck.b().body().extra.len());
        for (lo, hi, expected) in [
            (Some(hd_tir::ir::Ref(31)), None, hd_tir::ir::Ref(31)),
            (None, Some(hd_tir::ir::Ref(47)), hd_tir::ir::Ref(47)),
        ] {
            assert_eq!(
                ck.range_comparison(lo, hi).expect("one written bound"),
                expected,
            );
        }
        let error = ck
            .range_comparison(None, None)
            .expect_err("no written bounds");
        assert_eq!(error.stage, hd_base::Stage::Body);
        assert!(error.bug);
        assert!(!error.out_of_fuel);
        assert_eq!(
            error.what,
            "range-pattern comparison requires at least one bound"
        );
        assert_eq!((ck.b().body().len(), ck.b().body().extra.len()), before);
    });
}

#[test]
fn two_range_comparisons_emit_one_ordered_and() {
    use hd_tir::ir::PrimOp;
    with_checker(|ck| {
        let lo = ck.b().const_value(hd_types::Ty::BOOL, 0);
        let hi = ck.b().const_value(hd_types::Ty::BOOL, 1);
        let before = ck.b().body().len();
        let result = ck
            .range_comparison(Some(lo), Some(hi))
            .expect("two written bounds");
        let body = ck.b().body();
        assert_eq!(body.len(), before + 1);
        assert_eq!(result.0 as usize, before);
        assert_eq!(body.tags[before], hd_tir::ir::Tag::Prim);
        assert_eq!(body.data[before][0], PrimOp::And as u32);
        assert_eq!(body.ty[before], hd_types::Ty::BOOL);
        let start = body.data[before][1] as usize;
        assert_eq!(&body.extra[start..start + 3], &[2, lo.0, hi.0]);
    });
}

#[test]
fn unsettled_non_pattern_input_returns_an_error_without_a_decision_chain() {
    with_checker(|ck| {
        let node = ck.root();
        let before = (
            ck.b().body().len(),
            ck.b().body().extra.len(),
            ck.b().body().local_ty.len(),
            ck.infer().var_count(),
        );
        let (result, stack_empty) = ck.decide_unsettled(node);
        let error = result.expect_err("unsettled non-test cell");
        assert_eq!(error.stage, hd_base::Stage::Body);
        assert!(error.bug);
        assert!(!error.out_of_fuel);
        assert_eq!(
            error.what,
            format!(
                "pattern decision requires a settled test, found {:?}",
                node.kind()
            )
        );
        assert!(stack_empty);
        assert_eq!(
            (
                ck.b().body().len(),
                ck.b().body().extra.len(),
                ck.b().body().local_ty.len(),
                ck.infer().var_count()
            ),
            before
        );
    });
}

#[test]
fn variant_lists_stay_short() {
    assert_eq!(variant_list(&["Only"]), "its only variant is `.Only`");
    assert_eq!(
        variant_list(&["Ok", "Err"]),
        "its variants are `.Ok` and `.Err`"
    );
    assert_eq!(
        variant_list(&["A", "B", "C", "D", "E", "F"]),
        "its variants are `.A`, `.B`, `.C`, `.D` and 2 more"
    );
}
