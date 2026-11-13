//! Incremental SCC summaries over fully explored type edges. Interned type
//! shapes have a deterministic structural height. Use that well-founded
//! rank to recognize finite descent without repeated containment searches.
use hd_base::{DefId, HashMap};
use hd_types::{InternPool, Ty};
use std::collections::BTreeSet;

/// Each interned type child/edge is read at most twice, across all roots.
/// Type allocation order and parallel interning do not affect this rank.
#[derive(Default)]
pub(crate) struct Ranks {
    done: HashMap<Ty, u32>,
    #[cfg(test)]
    visits: usize,
}
impl Ranks {
    pub fn get(&mut self, pool: &InternPool, root: Ty) -> u32 {
        #[cfg(test)]
        {
            self.visits += 1;
        }
        if let Some(&rank) = self.done.get(&root) {
            return rank;
        }
        let mut work = vec![(root, false)];
        let mut children = Vec::new();
        while let Some((ty, ready)) = work.pop() {
            #[cfg(test)]
            {
                self.visits += 1;
            }
            if self.done.contains_key(&ty) {
                continue;
            }
            children.clear();
            pool.types().children(ty, &mut children);
            #[cfg(test)]
            {
                self.visits += children.len();
            }
            if ready || children.is_empty() {
                let rank = children
                    .iter()
                    .map(|t| {
                        // ruled out: every argument child completed before
                        // its parent's postorder frame.
                        *crate::emit::required(self.done.get(t), "completed rank child")
                    })
                    .max()
                    .map_or(0, |height| height.saturating_add(1));
                self.done.insert(ty, rank);
            } else {
                work.push((ty, true));
                work.extend(children.iter().rev().map(|&t| (t, false)));
            }
        }
        // ruled out: the immutable type DAG was completed in postorder.
        *crate::emit::required(self.done.get(&root), "completed type rank")
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Facts {
    pub value: bool,
    pub object: bool,
    pub recursive: bool,
    unknown: bool,
}

pub(crate) struct Shape {
    pub parts: Vec<Ty>,
    pub value_open: bool,
    pub object_open: bool,
    pub declaration: Option<DefId>,
    pub rank: u32,
}

struct Node {
    ty: Ty,
    shape: Shape,
    low: usize,
    component: usize,
    next: usize,
    active: bool,
    cut: bool,
}

#[derive(Default)]
pub(crate) struct Graph {
    done: HashMap<Ty, Facts>,
    #[cfg(test)]
    pub visits: usize,
}

impl Graph {
    /// Each new type/edge is visited once. Ordered ancestor queries inspect
    /// at most `limit` entries after O(log depth) lookup. No native recursion.
    pub fn get(&mut self, root: Ty, limit: usize, mut shape: impl FnMut(Ty) -> Shape) -> Facts {
        #[cfg(test)]
        {
            self.visits += 1;
        }
        if let Some(&facts) = self.done.get(&root) {
            return facts;
        }
        let mut nodes: Vec<Node> = Vec::new();
        let mut ids = HashMap::default();
        let mut pending = vec![root];
        let mut frames: Vec<usize> = Vec::new();
        let mut components = Vec::new();
        let mut ancestors: HashMap<DefId, BTreeSet<(u32, u32)>> = HashMap::default();
        loop {
            if let Some(ty) = pending.pop() {
                let mut data = shape(ty);
                #[cfg(test)]
                {
                    self.visits += 1;
                }
                let cut = data.declaration.is_some_and(|def| {
                    ancestors.get(&def).is_some_and(|types| {
                        types.range(..=(data.rank, u32::MAX)).take(limit).count() == limit
                    })
                });
                if cut {
                    // Unfolding is unknown, never a proof of non-recursion.
                    data.parts.clear();
                    data.value_open = true;
                    data.object_open = true;
                }
                if let Some(def) = data.declaration {
                    ancestors.entry(def).or_default().insert((data.rank, ty.0));
                }
                let at = nodes.len();
                ids.insert(ty, at);
                nodes.push(Node {
                    ty,
                    shape: data,
                    low: at,
                    component: usize::MAX,
                    next: 0,
                    active: true,
                    cut,
                });
                frames.push(at);
                components.push(at);
            }
            let Some(&at) = frames.last() else {
                break;
            };
            if let Some(&child) = nodes[at].shape.parts.get(nodes[at].next) {
                nodes[at].next += 1;
                #[cfg(test)]
                {
                    self.visits += 1;
                }
                if self.done.contains_key(&child) {
                    continue;
                }
                if let Some(&to) = ids.get(&child) {
                    if nodes[to].active {
                        nodes[at].low = nodes[at].low.min(to);
                    }
                } else {
                    pending.push(child);
                }
                continue;
            }
            #[cfg(test)]
            {
                self.visits += 1;
            }
            frames.pop();
            if let Some(def) = nodes[at].shape.declaration
                && let Some(types) = ancestors.get_mut(&def)
            {
                types.remove(&(nodes[at].shape.rank, nodes[at].ty.0));
            }
            if nodes[at].low == at {
                let mut members = Vec::new();
                while let Some(member) = components.pop() {
                    #[cfg(test)]
                    {
                        self.visits += 1;
                    }
                    nodes[member].active = false;
                    nodes[member].component = at;
                    members.push(member);
                    if member == at {
                        break;
                    }
                }
                let mut value = false;
                let mut unknown = false;
                let cyclic = members.len() > 1 || nodes[at].shape.parts.contains(&nodes[at].ty);
                for &member in &members {
                    #[cfg(test)]
                    {
                        self.visits += 1;
                    }
                    value |= nodes[member].shape.value_open;
                    unknown |= nodes[member].cut;
                    for child in &nodes[member].shape.parts {
                        #[cfg(test)]
                        {
                            self.visits += 1;
                        }
                        if ids.get(child).is_some_and(|&to| nodes[to].component == at) {
                            continue;
                        }
                        // ruled out: an edge leaving a completed SCC reaches
                        // an already completed SCC or an earlier cached query.
                        let facts = crate::emit::required(
                            self.done.get(child),
                            "completed cycle successor",
                        );
                        value |= facts.value;
                        unknown |= facts.unknown;
                    }
                }
                for &member in &members {
                    #[cfg(test)]
                    {
                        self.visits += 1;
                    }
                    let mut object = nodes[member].shape.object_open;
                    for child in &nodes[member].shape.parts {
                        #[cfg(test)]
                        {
                            self.visits += 1;
                        }
                        object |= if ids.get(child).is_some_and(|&to| nodes[to].component == at) {
                            value
                        } else {
                            // ruled out: the component's outgoing facts were
                            // read successfully in the preceding loop.
                            crate::emit::required(
                                self.done.get(child),
                                "completed object successor",
                            )
                            .value
                        };
                    }
                    self.done.insert(
                        nodes[member].ty,
                        Facts {
                            value,
                            object,
                            recursive: cyclic || unknown,
                            unknown,
                        },
                    );
                }
            }
            if let Some(&parent) = frames.last() {
                nodes[parent].low = nodes[parent].low.min(nodes[at].low);
            }
        }
        // ruled out: the root's DFS frame closed its SCC before the work ended.
        *crate::emit::required(self.done.get(&root), "completed cycle root")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        use hd_types::TyData;
        for (width, depth) in [(1000, 20), (1, 1000), (1, 10000)] {
            let pool = InternPool::default();
            let mut root = Ty::I32;
            let mut roots = Vec::new();
            for _ in 0..depth {
                root = pool.intern_ty(&TyData::Tuple {
                    elems: pool.list(&vec![root; width]),
                    rest: None,
                });
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
