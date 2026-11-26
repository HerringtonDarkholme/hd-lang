mod returned_variable_tests {
    use hd_check::testing::{Var, returned_origins};

    #[test]
    fn linked_origins_are_indexed_in_linear_width_and_depth() {
        for depth in [1u32, 20] {
            let mut small = 0;
            for width in [1_000u32, 10_000] {
                let mut links = Vec::new();
                for branch in 0..width {
                    let mut previous = Var::Life(0);
                    for level in 0..depth {
                        let next = Var::Life(1 + branch * depth + level);
                        links.push((previous, next));
                        previous = next;
                    }
                }
                let mut work = 0;
                let result = returned_origins(&[Var::Life(0)], &links, || work += 1);
                assert_eq!(result.len(), (width * depth + 1) as usize);
                assert!(!result.contains(&Var::Held));
                if width == 1_000 {
                    small = work;
                } else {
                    assert!(work <= 10 * small);
                }
            }
        }
    }
}

mod line_up_tests {
    use hd_base::DefId;
    use hd_check::testing::{Key, Layouts, line_up, line_up_with};
    use hd_types::{InternPool, ParamRef, Ty, TyData};

    fn tuple(pool: &InternPool, elems: &[Ty]) -> Ty {
        pool.product(pool.list(elems), None)
    }

    #[test]
    fn doubling_pairs_are_checked_once() {
        let pool = InternPool::new();
        let lifetimed = |_: DefId| false;
        let ls = Layouts::new(pool.types(), DefId::from_raw(2), &lifetimed);
        for depth in [13, 26] {
            let mut pair = [Ty::I32, Ty::BOOL];
            for _ in 0..depth {
                pair = pair.map(|t| tuple(&pool, &[t, t]));
            }
            for call in [false, true] {
                let mut work = 0;
                let keys = line_up_with(&ls, pair[0], pair[1], call, || work += 1);
                assert_eq!(keys, Some(Vec::new()));
                assert_eq!(work, depth + 1);
            }
            let mismatch = tuple(&pool, &[pair[1], pair[1], Ty::I32]);
            assert!(line_up(&ls, pair[0], mismatch).is_none());
        }
    }

    #[test]
    fn repeated_pairs_rebase_declared_slots_but_keep_parameter_keys() {
        let pool = InternPool::new();
        let p = ParamRef {
            owner: DefId::from_raw(1),
            index: 0,
        };
        let param = pool.intern_ty(&TyData::Param(p));
        let r = pool.intern_ty(&TyData::Ref {
            inner: Ty::I32,
            mutable: false,
            counted: false,
        });
        let d = tuple(&pool, &[r, param]);
        let t = tuple(&pool, &[r, r]);
        let lifetimed = |_: DefId| false;
        let ls = Layouts::new(pool.types(), DefId::from_raw(2), &lifetimed);
        assert_eq!(
            line_up(&ls, tuple(&pool, &[d, d]), tuple(&pool, &[t, t])),
            Some(vec![
                Key::Decl(0),
                Key::Param(p, 0),
                Key::Decl(1),
                Key::Param(p, 0)
            ])
        );
        let wide = tuple(&pool, &vec![r; 1_000]);
        assert_eq!(
            line_up(&ls, wide, wide),
            Some((0..1_000).map(Key::Decl).collect())
        );
    }

    #[test]
    fn a_cached_pair_still_obeys_the_type_depth_limit() {
        let pool = InternPool::new();
        let shallow = tuple(&pool, &[Ty::I32]);
        let mut deep = shallow;
        for _ in 0..hd_base::limits::TYPE_DEPTH {
            deep = tuple(&pool, &[deep]);
        }
        let root = tuple(&pool, &[shallow, deep]);
        let lifetimed = |_: DefId| false;
        let ls = Layouts::new(pool.types(), DefId::from_raw(2), &lifetimed);
        assert!(line_up(&ls, root, root).is_none());
    }

    /// [`line_up`] keys a type on its own stack: a declared type 1,000
    /// levels deep, a local reference beside the next level at each, with
    /// a type parameter at the bottom, lines up with its instance on a
    /// 96 KiB thread.
    #[test]
    fn a_line_up_at_the_type_depth_fits_a_small_stack() {
        std::thread::Builder::new()
            // cost: one 96 KiB thread for this test.
            .stack_size(96 << 10)
            .spawn(|| {
                let pool = InternPool::new();
                let r = pool.intern_ty(&TyData::Ref {
                    inner: Ty::I32,
                    mutable: false,
                    counted: false,
                });
                let p = ParamRef {
                    owner: DefId::from_raw(1),
                    index: 0,
                };
                let mut deep = (pool.intern_ty(&TyData::Param(p)), r);
                for _ in 1..hd_base::limits::TYPE_DEPTH {
                    let pair = |x| pool.product(pool.list(&[r, x]), None);
                    deep = (pair(deep.0), pair(deep.1));
                }
                let lifetimed = |_: DefId| false;
                let ls = Layouts::new(pool.types(), DefId::from_raw(2), &lifetimed);
                let keys = line_up(&ls, deep.0, deep.1).expect("the two line up");
                let n = hd_base::limits::TYPE_DEPTH as usize;
                assert_eq!(keys.len(), n);
                assert_eq!(keys[n - 2], Key::Decl(n - 2));
                assert_eq!(keys[n - 1], Key::Param(p, 0));
            })
            .expect("thread")
            .join()
            .expect("no stack overflow");
    }
}

mod projection_lifetime_tests {
    use hd_base::{DefId, HashMap};
    use hd_check::testing::{Layouts, with_checker_items};
    use hd_resolve::{Generic, Item, ItemData, TraitData};
    use hd_types::{ParamRef, TyData, TyList};

    #[test]
    fn a_thousand_projection_lifetimes_share_bound_queries_on_a_small_stack() {
        std::thread::Builder::new()
            .stack_size(96 << 10)
            // cost: one 96 KiB thread for this generated projection-chain test.
            .spawn(|| {
                for lifetime in [0, 7] {
                    with_checker_items(
                        |names| {
                            let p = names.pool.types();
                            let shelf = names.item("app.main", "Shelf");
                            let out = names.member(shelf, hd_intern::PathKind::Member, "Out");
                            let holder = names.item("app.main", "Holder");
                            let bound = p.intern_ty(&TyData::TraitValue {
                                def: shelf,
                                args: p.list(&[hd_resolve::lifetime_arg(p)]),
                                bindings: Vec::new(),
                            });
                            let mut life = Generic::plain(names.syms.intern("a"));
                            life.lifetime = true;
                            let mut tr = Item::new(
                                shelf,
                                names.syms.intern("Shelf"),
                                true,
                                ItemData::Trait(TraitData::default()),
                            );
                            tr.generics.push(life);
                            let associated = Item::new(
                                out,
                                names.syms.intern("Out"),
                                true,
                                ItemData::AssocType {
                                    owner: shelf,
                                    bounds: vec![bound],
                                    bound_lifetimes: vec![vec![1]],
                                },
                            );
                            let mut t = Generic::plain(names.syms.intern("T"));
                            t.bounds.push(bound);
                            t.bound_lifetimes.push(vec![lifetime]);
                            let mut root = Item::new(
                                holder,
                                names.syms.intern("Holder"),
                                true,
                                ItemData::Builtin,
                            );
                            root.generics.push(t);
                            vec![tr, associated, root]
                        },
                        |ck| {
                            let p = ck.pool();
                            let shelf = ck.cx().names.item("app.main", "Shelf");
                            let assoc =
                                ck.cx()
                                    .names
                                    .member(shelf, hd_intern::PathKind::Member, "Out");
                            let owner = ck.cx().names.item("app.main", "Holder");
                            let other = ck.cx().names.item("app.main", "Other");
                            let lifetimed = |_: DefId| false;
                            let ls = Layouts::new(p, ck.cx().names.known.suspend, &lifetimed);
                            for depth in [100, 1000] {
                                let mut root =
                                    p.intern_ty(&TyData::Param(ParamRef { owner, index: 0 }));
                                for _ in 0..depth {
                                    root = p.intern_ty(&TyData::Assoc {
                                        assoc,
                                        trait_: shelf,
                                        self_ty: root,
                                        args: p.list(&[hd_resolve::lifetime_arg(p)]),
                                    });
                                }
                                let wanted = if lifetime == 0 {
                                    Vec::new()
                                } else {
                                    vec![u32::from(lifetime)]
                                };
                                assert_eq!(
                                    hd_types::work::count_type_nodes(
                                        || ck.proj_lifetimes(&ls, root)
                                    ),
                                    (wanted, 2 * depth + 1)
                                );
                                let mut memo = HashMap::default();
                                let ids = if lifetime == 0 {
                                    Vec::new()
                                } else {
                                    vec![vec![u32::from(lifetime)]]
                                };
                                assert_eq!(
                                    hd_types::work::count_type_nodes(
                                        || ck.bound_ids_in(&ls, root, shelf, &mut memo)
                                    ),
                                    (ids.clone(), depth + 1)
                                );
                                assert_eq!(
                                    hd_types::work::count_type_nodes(
                                        || ck.bound_ids_in(&ls, root, shelf, &mut memo)
                                    ),
                                    (ids, 0)
                                );
                                assert_eq!(
                                    hd_types::work::count_type_nodes(
                                        || ck.bound_ids_in(&ls, root, other, &mut memo)
                                    ),
                                    (Vec::<Vec<u32>>::new(), 1)
                                );
                                let scalar = p.product(TyList::EMPTY, None);
                                assert!(ck.proj_lifetimes(&ls, scalar).is_empty());
                            }
                        },
                    );
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }
}

mod implementation_slot_tests {
    use hd_base::DefId;
    use hd_check::testing::{Checker, Layouts, with_checker_items};
    use hd_resolve::{ImplKind, Item, ItemData};
    use hd_types::{Ty, TyData};

    #[test]
    fn nonborrowing_opaque_metadata_does_not_invent_trait_lifetimes() {
        with_checker_items(
            |names| {
                let implementation = names.item("app.main", "implementation");
                let opaque = names.pool.intern_ty(&TyData::Opaque {
                    owner: implementation,
                    site: 0,
                    args: hd_types::TyList::EMPTY,
                    bound: Ty::I32,
                    borrows: false,
                });
                vec![Item::new(
                    implementation,
                    names.syms.intern("implementation"),
                    false,
                    ItemData::Impl {
                        trait_: names.item("app.main", "Trait"),
                        trait_args: names.pool.list(&[opaque]),
                        trait_lifetimes: vec![],
                        self_lifetimes: vec![],
                        trait_slots: vec![],
                        self_ty: Ty::I32,
                        methods: vec![],
                        assoc: vec![],
                        assoc_lifetimes: vec![],
                        by: None,
                        kind: ImplKind::Written,
                        fills_defaults: false,
                        skips: vec![],
                    },
                )]
            },
            |ck| {
                let pool = ck.pool();
                let lifetimed = |_: DefId| false;
                let layouts = Layouts::new(pool, ck.cx().names.known.suspend, &lifetimed);
                let implementation = ck.cx().names.item("app.main", "implementation");
                let implementation_slots = std::hint::black_box(Checker::implementation_slots);
                for mutable in [false, true] {
                    let reference = pool.intern_ty(&TyData::Ref {
                        inner: Ty::I32,
                        mutable,
                        counted: false,
                    });
                    assert_eq!(
                        implementation_slots(ck, &layouts, implementation, Ty::I32, &[reference]),
                        (vec![vec![], vec![None]], 0),
                    );
                }
            },
        );
    }
}
