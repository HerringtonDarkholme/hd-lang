use super::{DefId, HashMap, InternPool, Ty, TyData, TyList, holds_coll_walk};

#[test]
fn collection_query_reaches_past_the_old_cap() {
    let pool = InternPool::new();
    let types: Vec<Ty> = (0..65)
        .map(|n| {
            pool.intern_ty(&TyData::Adt {
                def: DefId::from_raw(n),
                args: TyList::EMPTY,
            })
        })
        .collect();
    let positions: HashMap<Ty, usize> = types
        .iter()
        .copied()
        .enumerate()
        .map(|(n, t)| (t, n))
        .collect();
    let mut queries = 0;
    assert!(holds_coll_walk(types[0], |t| {
        queries += 1;
        let n = positions[&t];
        (n == 64, types.get(n + 1).copied().into_iter().collect())
    }));
    assert_eq!(queries, 65);
}

#[test]
fn collection_query_shares_width_1000_and_depth_20() {
    for (width, depth) in [(1usize, 1usize), (1000, 1), (1, 20), (1000, 20)] {
        for present in [false, true] {
            let pool = InternPool::new();
            let types: Vec<Ty> = (0..=depth)
                .map(|n| {
                    pool.intern_ty(&TyData::Adt {
                        // ruled out: this generated depth is 1 or 20.
                        def: DefId::from_raw(u32::try_from(n).expect("small depth")),
                        args: TyList::EMPTY,
                    })
                })
                .collect();
            let positions: HashMap<Ty, usize> = types
                .iter()
                .copied()
                .enumerate()
                .map(|(n, t)| (t, n))
                .collect();
            let mut queries = 0;
            let mut edges = 0;
            assert_eq!(
                holds_coll_walk(types[0], |t| {
                    queries += 1;
                    let n = positions[&t];
                    let children = if n < depth {
                        vec![types[n + 1]; width]
                    } else {
                        Vec::new()
                    };
                    edges += children.len();
                    (present && n == depth, children)
                }),
                present
            );
            assert_eq!(queries, depth + 1);
            assert_eq!(edges, width * depth);
        }
    }
}

#[test]
fn collection_query_uses_full_instantiated_identity_and_handles_cycles() {
    let pool = InternPool::new();
    let wrapper = DefId::from_raw(1);
    let outer = pool.intern_ty(&TyData::Adt {
        def: wrapper,
        args: pool.list(&[Ty::STRING]),
    });
    let inner = pool.intern_ty(&TyData::Adt {
        def: wrapper,
        args: pool.list(&[Ty::I32]),
    });
    let mut queries = Vec::new();
    assert!(holds_coll_walk(outer, |t| {
        queries.push(t);
        if t == outer {
            (false, vec![outer, inner])
        } else {
            (true, Vec::new())
        }
    }));
    assert_eq!(queries, vec![outer, inner]);
    let mut queries = 0;
    assert!(!holds_coll_walk(outer, |_| {
        queries += 1;
        (false, vec![outer, inner])
    }));
    assert_eq!(queries, 2);
}

#[test]
fn collection_query_walks_a_deep_chain_without_native_recursion() {
    let mut queries = 0;
    assert!(holds_coll_walk(Ty(0), |t| {
        queries += 1;
        (
            t.0 == 20_000,
            if t.0 < 20_000 {
                vec![Ty(t.0 + 1)]
            } else {
                Vec::new()
            },
        )
    }));
    assert_eq!(queries, 20_001);
}
