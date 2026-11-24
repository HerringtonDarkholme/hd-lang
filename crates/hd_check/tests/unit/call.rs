use hd_base::{DefId, HashMap};
use hd_check::testing::{Checker, with_checker};
use hd_intern::PathKind;
use hd_types::{AdtId, Inputs, ParamRef, Ty, TyData, TyList, VarKind};

fn projection(ck: &Checker<'_, '_, '_>, base: Ty, binding: Option<Ty>) -> (Ty, DefId, Ty) {
    let pool = ck.pool();
    let owner = ck.cx().names.item("app.main", "Shelf");
    let assoc = ck.cx().names.member(owner, PathKind::Member, "Out");
    let self_ty = binding.map_or(base, |value| {
        pool.intern_ty(&TyData::TraitValue {
            def: owner,
            args: TyList::EMPTY,
            bindings: vec![(assoc, value)],
        })
    });
    (
        pool.intern_ty(&TyData::Assoc {
            assoc,
            trait_: owner,
            self_ty,
            args: TyList::EMPTY,
        }),
        owner,
        self_ty,
    )
}

#[test]
fn normalization_at_depth_1000_fits_a_small_stack_and_shares_answers() {
    std::thread::Builder::new()
        .stack_size(96 << 10)
        // cost: one 96 KiB thread for this generated depth test.
        .spawn(|| {
            with_checker(|ck| {
                let p = ck.pool();
                let leaf = projection(ck, Ty::I32, Some(Ty::I32)).0;
                for depth in [100, 1000] {
                    let (mut root, mut wanted) = (leaf, Ty::I32);
                    for _ in 0..depth {
                        let tuple = |t| p.product(p.list(&[t, t]), None);
                        root = tuple(root);
                        wanted = tuple(wanted);
                    }
                    let mut memo = HashMap::default();
                    let (found, work) =
                        hd_types::work::count_type_nodes(|| ck.normalize_deep_in(root, &mut memo));
                    assert_eq!(found.unwrap(), wanted);
                    assert_eq!(work, depth + 1);
                    assert_eq!(
                        hd_types::work::count_type_nodes(|| ck
                            .normalize_deep_in(root, &mut memo)
                            .unwrap()),
                        (wanted, 0)
                    );
                }
                let owner = ck.cx().names.item("app.main", "Holder");
                let row = p.row(&hd_types::RowData {
                    keys: vec![p.intern_ty(&TyData::TraitValue {
                        def: owner,
                        args: p.list(&[leaf]),
                        bindings: Vec::new(),
                    })],
                    params: Vec::new(),
                });
                let wrap = |t, i| {
                    p.intern_ty(&match i % 4 {
                        0 => TyData::Adt(AdtId::Declared(owner), p.list(&[t, t])),
                        1 => TyData::Ref {
                            inner: t,
                            mutable: true,
                            counted: true,
                        },
                        2 => TyData::Adt(AdtId::Product { rest: true }, p.list(&[t, t, leaf])),
                        _ => TyData::Fn {
                            params: p.list(&[t, t]),
                            result: t,
                            row,
                            suspends: true,
                            inputs: Inputs::Fixed,
                            deps: hd_types::FnDeps::NONE,
                        },
                    })
                };
                let (mut root, mut wanted) = (leaf, Ty::I32);
                for i in 0..1000 {
                    root = wrap(root, i);
                    wanted = wrap(wanted, i);
                }
                assert_eq!(ck.normalize_deep(root).unwrap(), wanted);
                for data in [
                    TyData::sum(p.list(&[leaf])),
                    TyData::Opaque {
                        owner,
                        site: 0,
                        borrows: true,
                        args: p.list(&[leaf]),
                        bound: leaf,
                    },
                    TyData::TraitValue {
                        def: owner,
                        args: p.list(&[leaf]),
                        bindings: vec![(owner, leaf)],
                    },
                    TyData::Row(row),
                    TyData::Context(row),
                ] {
                    let ty = p.intern_ty(&data);
                    assert_eq!(ck.normalize_deep(ty).unwrap(), ty);
                }
                let base = ck.infer().fresh(p, VarKind::General);
                let projected = projection(ck, base, None).0;
                let mut deep = projected;
                for _ in 0..1_000 {
                    deep = p.intern_ty(&TyData::Ref {
                        inner: deep,
                        mutable: false,
                        counted: true,
                    });
                }
                let opened = ck.open_result_projections(deep);
                assert!(!p.has_assoc(opened));
                assert_eq!(ck.depths().2, 1);
                let pair = p.product(p.list(&[projected, projected]), Some(projected));
                let opened = ck.open_result_projections(pair);
                let (elems, rest) = p.product_parts(opened).expect("product");
                assert_ne!(elems[0], elems[1]);
                assert_eq!(rest, Some(projected));
                assert_eq!(ck.depths().2, 3);
            });
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn a_thousand_rigid_projection_bases_normalize_on_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(96 << 10)
        // cost: one 96 KiB thread for this generated projection-chain test.
        .spawn(|| {
            for depth in [100, 1000] {
                with_checker(|ck| {
                    let p = ck.pool();
                    let mut root = p.intern_ty(&TyData::Param(ParamRef {
                        owner: ck.cx().names.item("app.main", "f"),
                        index: 0,
                    }));
                    for _ in 0..depth {
                        let (next, trait_, _) = projection(ck, root, None);
                        ck.env()
                            .push(root, trait_, TyList::EMPTY, Vec::new(), false);
                        root = next;
                    }
                    let (found, work) =
                        hd_types::work::count_type_nodes(|| ck.normalize_deep(root));
                    assert_eq!(found.unwrap(), root);
                    assert!(
                        work < 10 * depth + 100,
                        "projection-chain work stays linear: {work}"
                    );
                });
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn bounds_scans_wake_only_for_the_missing_clause_and_survive_rollback() {
    std::thread::Builder::new()
        .stack_size(96 << 10)
        // cost: one 96 KiB thread for this generated depth test.
        .spawn(|| {
            for depth in [100, 1000] {
                with_checker(|ck| {
                    let p = ck.pool();
                    let base = p.intern_ty(&TyData::Param(ParamRef {
                        owner: ck.cx().names.item("app.main", "f"),
                        index: 0,
                    }));
                    let (proj, trait_, _) = projection(ck, base, None);
                    let mut root = proj;
                    for _ in 0..depth {
                        root = p.product(p.list(&[root, root]), None);
                    }
                    let query = |ck: &mut Checker<'_, '_, '_>| {
                        hd_types::work::count_type_nodes(|| ck.add_projection_bounds(root))
                    };
                    assert_eq!(query(ck), (false, depth + 1));
                    assert_eq!(query(ck), (false, 0));
                    // Even clauses on the same self type cannot wake an
                    // unrelated trait/argument dependency.
                    for i in 0..1000 {
                        let unrelated = ck.cx().names.item("app.main", &format!("Other{i}"));
                        ck.env()
                            .push(base, unrelated, TyList::EMPTY, Vec::new(), false);
                        assert_eq!(query(ck), (false, 0));
                    }
                    let wrong = ck.env().len();
                    let wrong_index = u32::try_from(wrong).unwrap();
                    ck.env()
                        .push(base, trait_, p.list(&[Ty::I32]), Vec::new(), false);
                    assert_eq!(ck.env().first_clause(base, trait_), Some(wrong_index));
                    assert_eq!(query(ck), (false, 0));
                    let before = ck.env().len();
                    ck.env()
                        .push(base, trait_, TyList::EMPTY, Vec::new(), false);
                    assert_eq!(ck.env().first_clause(base, trait_), Some(wrong_index));
                    assert_eq!(query(ck), (false, depth + 1));
                    assert!(ck.env().elaborated(proj));
                    assert_eq!(query(ck), (false, 0));
                    ck.env().truncate(before);
                    assert_eq!(ck.env().first_clause(base, trait_), Some(wrong_index));
                    assert!(!ck.env().elaborated(proj));
                    assert_eq!(query(ck), (false, depth + 1));
                    ck.env()
                        .push(base, trait_, TyList::EMPTY, Vec::new(), false);
                    assert_eq!(query(ck), (false, depth + 1));
                    assert!(ck.env().elaborated(proj));
                    ck.env().truncate(wrong);
                    assert_eq!(ck.env().first_clause(base, trait_), None);
                    assert_eq!(query(ck), (false, depth + 1));
                    ck.env()
                        .push(base, trait_, TyList::EMPTY, Vec::new(), false);
                    assert_eq!(ck.env().first_clause(base, trait_), Some(wrong_index));
                    assert_eq!(query(ck), (false, depth + 1));
                });
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
