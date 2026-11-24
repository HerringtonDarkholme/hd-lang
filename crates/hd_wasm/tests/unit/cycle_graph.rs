mod tests {
    use hd_base::DefId;
    use hd_types::{InternPool, Ty};
    use hd_wasm::testing::cycle_graph::*;

    fn shape(parts: Vec<Ty>) -> Shape {
        Shape {
            parts,
            value_open: false,
            object_open: false,
            declaration: None,
            rank: 0,
        }
    }
    #[test]
    fn structural_rank_work_is_linear_and_cached_across_overlapping_roots() {
        for (width, depth) in [(1000, 20), (1, 1000), (1, 10000)] {
            let pool = InternPool::default();
            let mut root = Ty::I32;
            let mut roots = Vec::new();
            for _ in 0..depth {
                root = pool.product(pool.list(&vec![root; width]), None);
                roots.push(root);
            }
            let mut ranks = Ranks::default();
            assert_eq!(ranks.get(&pool, root), u32::try_from(depth).unwrap());
            assert!(ranks.visits <= 4 * (depth * (width + 1) + 1));
            let before = ranks.visits;
            for root in roots {
                ranks.get(&pool, root);
            }
            assert_eq!(
                ranks.visits - before,
                depth,
                "cached queries do not walk accumulated types"
            );
        }
    }

    #[test]
    fn components_distinguish_a_cycle_from_a_parent_that_only_reaches_it() {
        let mut graph = Graph::default();
        let classify = |ty: Ty| match ty.0 {
            1 => shape(vec![Ty(2), Ty(3)]),
            2 => shape(vec![Ty(2)]),
            _ => Shape {
                value_open: true,
                object_open: true,
                ..shape(vec![])
            },
        };
        let root = graph.get(Ty(1), 4, classify);
        assert!(root.value && !root.recursive);
        let cycle = graph.get(Ty(2), 4, classify);
        assert!(cycle.recursive && !cycle.value);
    }
    #[test]
    fn deep_and_wide_queries_share_linear_work_in_both_query_orders() {
        for (width, depth) in [(1000u32, 20u32), (1, 1000), (1, 10000)] {
            let count = width * depth;
            let classify = |ty: Ty| {
                let parts = if ty.0 == count + 1 {
                    (0..width).map(|i| Ty((i + 1) * depth)).collect()
                } else if ty.0 % depth == 1 {
                    vec![]
                } else {
                    vec![Ty(ty.0 - 1)]
                };
                Shape {
                    declaration: Some(DefId::from_raw(7)),
                    rank: ty.0,
                    ..shape(parts)
                }
            };
            for reverse in [false, true] {
                let mut graph = Graph::default();
                let order: Vec<_> = if reverse {
                    (1..=count + 1).rev().collect()
                } else {
                    (1..=count + 1).collect()
                };
                for n in order {
                    let facts = graph.get(Ty(n), 4, classify);
                    assert!(!facts.value && !facts.recursive);
                }
                assert!(
                    graph.visits <= 12 * (count + width + 1) as usize,
                    "{} visits for width {width}, depth {depth}",
                    graph.visits
                );
            }
        }
    }
    #[test]
    fn every_three_node_graph_matches_transitive_closure_in_both_query_orders() {
        for edges in 0..512u32 {
            let adjacent = |a: usize, b: usize| edges & (1 << (3 * a + b)) != 0;
            let mut reach = [[false; 3]; 3];
            for (a, row) in reach.iter_mut().enumerate() {
                for (b, edge) in row.iter_mut().enumerate() {
                    *edge = adjacent(a, b);
                }
            }
            for via in 0..3 {
                for a in 0..3 {
                    for b in 0..3 {
                        reach[a][b] |= reach[a][via] && reach[via][b];
                    }
                }
            }
            for terminals in 0..8u32 {
                let capable = |a: usize| {
                    terminals & (1 << a) != 0
                        || (0..3).any(|b| reach[a][b] && terminals & (1 << b) != 0)
                };
                for order in [[0, 1, 2], [2, 1, 0]] {
                    let mut graph = Graph::default();
                    for a in order {
                        let result = graph.get(Ty(a), 4, |ty| Shape {
                            parts: (0..3)
                                .filter(|&b| adjacent(ty.0 as usize, b))
                                .map(|b| Ty(u32::try_from(b).unwrap()))
                                .collect(),
                            value_open: terminals & (1 << ty.0) != 0,
                            object_open: false,
                            declaration: None,
                            rank: 0,
                        });
                        assert_eq!(
                            result.value,
                            capable(a as usize),
                            "edges {edges}, terminals {terminals}, node {a}"
                        );
                        assert_eq!(
                            result.object,
                            (0..3).any(|b| adjacent(a as usize, b) && capable(b))
                        );
                        assert_eq!(result.recursive, reach[a as usize][a as usize]);
                    }
                }
            }
        }
    }

    #[test]
    fn growing_families_stop_and_remain_conservative() {
        let mut graph = Graph::default();
        let result = graph.get(Ty(1), 4, |ty| Shape {
            declaration: Some(DefId::from_raw(7)),
            ..shape(vec![Ty(ty.0 + 1)])
        });
        assert!(result.value && result.recursive);
        assert!(graph.visits <= 80);
    }
}
