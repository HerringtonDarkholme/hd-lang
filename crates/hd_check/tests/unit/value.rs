use hd_base::{Fuel, HashMap};
use hd_check::testing::{ProjectionCycles, inline_kids};
use hd_types::{AdtId, Ty, TyData};

#[test]
fn inline_containment_preserves_product_rest_boundary() {
    let pool = hd_types::InternPool::new();
    let fixed = pool.list(&[Ty::I32]);
    for (ty, expected) in [
        (pool.product(fixed, Some(Ty::STRING)), vec![Ty::I32]),
        (pool.product(fixed, None), vec![Ty::I32]),
        (
            pool.product(hd_types::TyList::EMPTY, Some(Ty::STRING)),
            vec![],
        ),
        (
            pool.intern_ty(&TyData::Adt(AdtId::Sum, pool.list(&[Ty::I32, Ty::STRING]))),
            vec![Ty::I32, Ty::STRING],
        ),
    ] {
        let mut out = Vec::new();
        inline_kids(pool.types(), ty, None, &mut out);
        assert_eq!(out, expected);
    }
}

#[test]
fn projection_graph_work_depends_only_on_reachable_width_and_depth() {
    // A thousand roots share a depth-20 suffix. A repeated check adds
    // no expansion, even with ten times as much unrelated memo state.
    for unrelated in [100, 1000] {
        let mut done = HashMap::default();
        for i in 0..unrelated {
            done.insert(Ty(100_000 + i), None);
        }
        let mut fuel = Fuel::new(100_000);
        let mut expanded = 0;
        let mut expand = |t: Ty| {
            expanded += 1;
            if t.0 < 1000 {
                vec![Ty(5000 + t.0)]
            } else if t.0 < 1019 {
                // Shared branches must not cause exponential retries.
                vec![Ty(t.0 + 1), Ty(t.0 + 1)]
            } else {
                vec![]
            }
        };
        for _ in 0..2 {
            for i in 0..1000 {
                assert_eq!(
                    ProjectionCycles::search(
                        &mut done,
                        Ty(i),
                        &mut fuel,
                        &mut |_| false,
                        &mut Vec::new(),
                        &mut |t| if (5000..6000).contains(&t.0) {
                            Ty(1000)
                        } else {
                            t
                        },
                        &mut expand
                    ),
                    Ok(None)
                );
            }
        }
        assert_eq!(expanded, 1020);
        // The first projection enters the suffix's root directly;
        // only its other nineteen nodes need separate enter events.
        assert_eq!(fuel.spent(), 2019);
    }
}

#[test]
fn a_thousand_node_projected_ring_is_iterative_and_memoized() {
    let mut done = HashMap::default();
    let mut fuel = Fuel::new(2000);
    let mut expanded = 0;
    let mut expand = |t: Ty| {
        expanded += 1;
        vec![Ty(t.0 + 1)]
    };
    assert_eq!(
        ProjectionCycles::search(
            &mut done,
            Ty(0),
            &mut fuel,
            &mut |_| false,
            &mut Vec::new(),
            &mut |t| if t == Ty(1000) { Ty(0) } else { t },
            &mut expand
        ),
        Ok(Some(Ty(0)))
    );
    for i in 0..1000 {
        assert_eq!(
            ProjectionCycles::search(
                &mut done,
                Ty(i),
                &mut fuel,
                &mut |_| false,
                &mut Vec::new(),
                &mut |t| if t == Ty(1000) { Ty(0) } else { t },
                &mut expand
            ),
            Ok(Some(Ty(0)))
        );
    }
    assert_eq!(expanded, 1000);
    assert_eq!(fuel.spent(), 1001);
}

#[test]
fn exhaustion_does_not_cache_an_unfinished_graph_as_valid() {
    let mut done = HashMap::default();
    let mut expand = |t: Ty| vec![Ty(t.0 + 1)];
    let mut fuel = Fuel::new(20);
    assert_eq!(
        ProjectionCycles::search(
            &mut done,
            Ty(0),
            &mut fuel,
            &mut |_| false,
            &mut Vec::new(),
            &mut |t| t,
            &mut expand
        ),
        Err(())
    );
    assert!(done.is_empty());
}
