use hd_base::{DefId, NodeIdx};
use hd_check::testing::{DeferredTry, Joins, with_checker};
use hd_tir::ir::{BodyKind, NONE, Ref, TirBuilder, TirSink};
use hd_types::{AdtId, Ty, TyData, VarKind};

#[test]
fn join_shape_unification_at_depth_1000_shares_pairs_and_keeps_bindings() {
    std::thread::Builder::new()
        .stack_size(96 << 10)
        // cost: one 96 KiB thread for this generated depth test.
        .spawn(|| {
            with_checker(|ck| {
                let p = ck.pool();
                let reference = |inner, mutable| {
                    p.intern_ty(&TyData::Ref {
                        inner,
                        mutable,
                        counted: mutable,
                    })
                };
                let tuple = |elems: &[Ty], rest| p.product(p.list(elems), rest);
                for depth in [100, 1000] {
                    let x = ck.infer().fresh(p, VarKind::General);
                    let (mut a, mut b) = (reference(x, true), reference(Ty::I32, false));
                    for _ in 0..depth {
                        a = tuple(&[a, a], None);
                        b = tuple(&[b, b], None);
                    }
                    let snapshot = ck.infer().snapshot();
                    assert_eq!(
                        hd_types::work::count_type_nodes(|| ck.unify_join(a, b)),
                        (true, depth + 1)
                    );
                    assert_eq!(ck.infer().shallow(p, x), Ty::I32);
                    ck.infer().rollback(snapshot);
                    assert_eq!(ck.infer().shallow(p, x), x);
                }
                let (a, b) = (reference(Ty::I32, true), reference(Ty::I32, false));
                let list_def = ck.cx().names.known.list;
                let list = |t| p.intern_ty(&TyData::Adt(AdtId::Declared(list_def), p.list(&[t])));
                // A successful permission-ignoring pair cannot satisfy
                // the same pair's invariant query beneath List.
                assert!(!ck.unify_join(tuple(&[a, list(a)], None), tuple(&[b, list(b)], None)));
                let x = ck.infer().fresh(p, VarKind::General);
                let snapshot = ck.infer().snapshot();
                assert!(!ck.unify_join(tuple(&[x], Some(list(Ty::I32))), tuple(&[Ty::I32], None)));
                assert_eq!(
                    ck.infer().shallow(p, x),
                    Ty::I32,
                    "fixed elements bind before a rest-presence mismatch"
                );
                ck.infer().rollback(snapshot);
                assert_eq!(ck.infer().shallow(p, x), x);
            });
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn tuple_meets_and_conversions_at_depth_1000_share_pair_answers() {
    std::thread::Builder::new()
        .stack_size(96 << 10)
        // cost: one 96 KiB thread for both generated depth tests.
        .spawn(|| {
            with_checker(|ck| {
                let p = ck.pool();
                for am in [false, true] {
                    for ac in [false, true] {
                        for bm in [false, true] {
                            for bc in [false, true] {
                                let reference = |mutable, counted| {
                                    p.intern_ty(&TyData::Ref {
                                        inner: Ty::I32,
                                        mutable,
                                        counted,
                                    })
                                };
                                for depth in [100, 1000] {
                                    let (mut a, mut b, mut wanted) = (
                                        reference(am, ac),
                                        reference(bm, bc),
                                        reference(am && bm, ac && bc),
                                    );
                                    for i in 0..depth {
                                        let tuple = |t| {
                                            p.product(
                                                p.list(&[t, t]),
                                                (i % 2 == 0).then_some(t),
                                            )
                                        };
                                        a = tuple(a);
                                        b = tuple(b);
                                        wanted = tuple(wanted);
                                    }
                                    let work = if a == b { 0 } else { depth + 1 };
                                    assert_eq!(
                                        hd_types::work::count_type_nodes(
                                            || ck.lct_meet(a, b, true)
                                        ),
                                        (Some(wanted), work)
                                    );
                                    assert_eq!(
                                        hd_types::work::count_type_nodes(
                                            || ck.lct_converts(a, b, true)
                                        ),
                                        ((am || !bm) && (ac || !bc), work)
                                    );
                                }
                            }
                        }
                    }
                }
            });
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn lct_rows_join_only_at_the_top_and_arguments_stay_invariant() {
    with_checker(|ck| {
        let p = ck.pool();
        let key = |name| {
            p.intern_ty(&TyData::TraitValue {
                def: ck.cx().names.item("app.main", name),
                args: hd_types::TyList::EMPTY,
                bindings: Vec::new(),
            })
        };
        let row = |keys| {
            p.row(&hd_types::RowData {
                keys,
                params: Vec::new(),
            })
        };
        let left = row(vec![key("Left")]);
        let right = row(vec![key("Right")]);
        let union = row(vec![key("Left"), key("Right")]);
        let function = |row| {
            p.intern_ty(&TyData::Fn {
                params: p.list(&[Ty::I32]),
                result: Ty::I32,
                row,
                suspends: false,
                inputs: hd_types::Inputs::Fixed,
                deps: hd_types::FnDeps::NONE,
            })
        };
        let (a, b) = (function(left), function(right));
        assert_eq!(ck.lct_meet(a, b, true), Some(function(union)));
        assert!(ck.lct_converts(a, b, true));
        assert_eq!(ck.lct_meet(a, b, false), None);
        assert!(!ck.lct_converts(a, b, false));
        let tuple = |t, rest| p.product(p.list(&[t]), rest);
        let adt = |t| {
            p.intern_ty(&TyData::Adt(
                AdtId::Declared(ck.cx().names.known.list),
                p.list(&[t]),
            ))
        };
        for (a, b) in [
            (tuple(a, None), tuple(b, None)),
            (adt(a), adt(b)),
            (tuple(Ty::I32, Some(Ty::I32)), tuple(Ty::I32, None)),
            (
                tuple(Ty::I32, None),
                p.product(hd_types::TyList::EMPTY, None),
            ),
        ] {
            assert_eq!(ck.lct_meet(a, b, true), None);
            assert!(!ck.lct_converts(a, b, true));
        }
    });
}

#[test]
fn deferred_tries_visit_only_their_return_site_and_roll_back() {
    for unrelated in [100, 1000] {
        let mut joins = Joins::default();
        let mut builder = TirBuilder::new(DefId::NONE, BodyKind::Fn);
        let deferred = DeferredTry {
            operand: (Ref(NONE), Ty::VOID),
            payload: Ty::VOID,
            slot: builder.reserve(),
            at: (NodeIdx::NONE, NodeIdx::NONE),
        };
        for i in 0..unrelated {
            let var = Ty(i + 128);
            joins.open(var);
            joins.defer(var, deferred);
        }
        let target = Ty(5000);
        joins.open(target);
        joins.defer(target, deferred);
        let mark = joins.mark();
        joins.defer(target, deferred);
        assert_eq!(joins.deferred(target).len(), 2);
        joins.rollback(mark);
        assert_eq!(joins.deferred(target).len(), 1);
        let mut visited = 0;
        for _ in joins.deferred(target) {
            visited += 1;
        }
        assert_eq!(visited, 1, "unrelated state: {unrelated}");
    }
}
