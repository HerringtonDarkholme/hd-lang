use hd_check::testing::with_checker;
use hd_types::{Ty, TyData, VarKind};

#[test]
fn strict_returns_at_depth_1000_fit_a_small_stack() {
    std::thread::Builder::new()
        // cost: one 96 KiB thread for this test.
        .stack_size(96 << 10)
        .spawn(|| {
            with_checker(|ck| {
                let pool = ck.pool();
                let strict = ck.infer().fresh(pool, VarKind::General);
                let other = ck.infer().fresh(pool, VarKind::General);
                ck.strict_rets().push(strict);
                for depth in [100, 1_000] {
                    let mut chain = [strict, other];
                    for _ in 0..depth {
                        for t in &mut chain {
                            *t = pool.product(pool.list(&[*t, *t]), None);
                        }
                    }
                    for (t, expected) in [(chain[0], true), (chain[1], false)] {
                        let (answer, work) =
                            hd_types::work::count_type_nodes(|| ck.strict_opaque_return(t));
                        assert_eq!(answer, expected);
                        assert_eq!(work, depth + 2, "one graph walk and one strict-slot read");
                    }
                }
                let mut deep = strict;
                for _ in 0..1_000 {
                    deep = pool.intern_ty(&TyData::Ref {
                        inner: deep,
                        mutable: false,
                        counted: true,
                    });
                }
                assert!(ck.constrain_opaque_return(deep, deep));
                let owner = ck.cx().names.item("app.main", "Shape");
                let row = pool.row(&hd_types::RowData {
                    keys: vec![strict],
                    params: vec![],
                });
                for t in [
                    pool.intern_ty(&TyData::Adt(
                        hd_types::AdtId::Declared(owner),
                        pool.list(&[strict]),
                    )),
                    pool.product(hd_types::TyList::EMPTY, Some(strict)),
                    pool.intern_ty(&TyData::Ref {
                        inner: strict,
                        mutable: false,
                        counted: false,
                    }),
                    pool.intern_ty(&TyData::Fn {
                        params: hd_types::TyList::EMPTY,
                        result: Ty::I32,
                        row,
                        suspends: false,
                        inputs: hd_types::Inputs::Fixed,
                        deps: hd_types::FnDeps::NONE,
                    }),
                    pool.intern_ty(&TyData::TraitValue {
                        def: owner,
                        args: hd_types::TyList::EMPTY,
                        bindings: vec![(owner, strict)],
                    }),
                    pool.intern_ty(&TyData::Assoc {
                        assoc: owner,
                        trait_: owner,
                        self_ty: strict,
                        args: hd_types::TyList::EMPTY,
                    }),
                    pool.intern_ty(&TyData::Row(row)),
                    pool.intern_ty(&TyData::Context(row)),
                ] {
                    assert!(
                        ck.strict_opaque_return(t),
                        "the original holder forms are traversed"
                    );
                }
                let opaque = pool.intern_ty(&TyData::Opaque {
                    owner,
                    site: 0,
                    args: pool.list(&[strict]),
                    bound: strict,
                    borrows: false,
                });
                let union = pool.intern_ty(&TyData::sum(pool.list(&[strict, Ty::BOOL])));
                for t in [opaque, union] {
                    assert!(
                        !ck.strict_opaque_return(t),
                        "the original opaque/union boundaries stay leaves"
                    );
                }
                let root = pool.product(pool.list(&[other]), None);
                assert!(!ck.strict_opaque_return(root));
                let mark = ck.infer().snapshot();
                ck.infer().unify(pool, strict, other).unwrap();
                assert!(ck.strict_opaque_return(root));
                ck.infer().rollback(mark);
                assert!(!ck.strict_opaque_return(root));
                ck.infer().unify(pool, strict, other).unwrap();
                assert!(
                    ck.strict_opaque_return(root),
                    "a new query sees merged classes"
                );
                ck.infer().unify(pool, other, Ty::I32).unwrap();
                assert!(
                    ck.strict_opaque_return(root),
                    "a solved occurrence stays strict"
                );
            });
        })
        .expect("thread")
        .join()
        .expect("no stack overflow");
}

#[test]
fn strict_return_slot_index_is_lazy_and_linear() {
    for width in [100, 1_000] {
        with_checker(|ck| {
            let pool = ck.pool();
            for _ in 0..width {
                let slot = ck.infer().fresh(pool, VarKind::General);
                ck.strict_rets().push(slot);
            }
            let others: Vec<_> = (0..width)
                .map(|_| ck.infer().fresh(pool, VarKind::General))
                .collect();
            let root = pool.product(pool.list(&others), None);
            let (answer, work) = hd_types::work::count_type_nodes(|| ck.strict_opaque_return(root));
            assert!(!answer);
            assert_eq!(work, 2 * width + 1);
            let first = pool.product(pool.list(&[ck.strict_rets()[0]]), None);
            let (answer, work) =
                hd_types::work::count_type_nodes(|| ck.strict_opaque_return(first));
            assert!(answer);
            assert_eq!(work, 3, "the first match does not index unrelated slots");
        });
    }
}
