//! Iterative, shared plans for flattened reference slots. Unary field
//! chains compose offsets; they do not copy all their descendant slots.

use hd_base::HashMap;
use hd_types::Ty;
use std::hash::Hash;

pub(super) enum Shape<K> {
    Leaf,
    Fields(Vec<(usize, K)>),
    Mapped(K, Vec<usize>),
}

impl<K: Copy> Shape<K> {
    fn children(&self) -> Vec<K> {
        match self {
            Self::Leaf => Vec::new(),
            Self::Fields(fields) => fields.iter().map(|&(_, ty)| ty).collect(),
            Self::Mapped(inner, _) => vec![*inner],
        }
    }
}

#[derive(Clone, Copy)]
struct Edge {
    first: usize,
    node: usize,
}

enum Node<K> {
    Leaf(K),
    Branch(Vec<Edge>),
}

enum Task<K> {
    Enter(K),
    Exit(K, Shape<K>),
}

fn join<K>(nodes: &mut Vec<Node<K>>, mut edges: Vec<Edge>) -> Option<Edge> {
    match edges.len() {
        0 => None,
        1 => edges.pop(),
        _ => {
            let node = nodes.len();
            nodes.push(Node::Branch(edges));
            Some(Edge { first: 0, node })
        }
    }
}

fn leaves<K: Copy>(nodes: &[Node<K>], root: Option<Edge>, mut leaf: impl FnMut(usize, usize, K)) {
    let mut work: Vec<Edge> = root.into_iter().collect();
    while let Some(edge) = work.pop() {
        match &nodes[edge.node] {
            Node::Leaf(key) => leaf(edge.first, edge.node, *key),
            Node::Branch(children) => work.extend(children.iter().rev().map(|child| Edge {
                first: edge.first + child.first,
                node: child.node,
            })),
        }
    }
}

// O(T+E+M+R) work/storage: distinct types, field edges, optional mapping
// entries plus remapped references, and returned slots. Unary chains collapse; every
// branch has at least two children, so leaf enumeration is output-linear.
// Layout/substitution costs belong to the caller's existing queries.
struct Plans<K> {
    done: HashMap<K, Option<Edge>>,
    nodes: Vec<Node<K>>,
}

impl<K> Default for Plans<K> {
    fn default() -> Self {
        Self {
            done: HashMap::default(),
            nodes: Vec::new(),
        }
    }
}

fn slots<K: Copy>(
    nodes: &[Node<K>],
    root: Option<Edge>,
    visit: &mut impl FnMut(),
) -> Vec<(usize, K)> {
    let mut result = Vec::new();
    leaves(nodes, root, |slot, _, key| {
        visit();
        result.push((slot, key));
    });
    result
}

impl<K: Copy + Eq + Hash> Plans<K> {
    fn collect(
        &mut self,
        root: K,
        mut shape: impl FnMut(K) -> Shape<K>,
        visit: &mut impl FnMut(),
    ) -> Vec<(usize, K)> {
        let Self { done, nodes } = self;
        if let Some(&root) = done.get(&root) {
            return slots(nodes, root, visit);
        }
        let first = shape(root);
        visit();
        let terminal = match &first {
            Shape::Leaf => Some(vec![(0, root)]),
            Shape::Fields(fields) => fields.is_empty().then(Vec::new),
            Shape::Mapped(_, positions) => positions.is_empty().then(Vec::new),
        };
        if let Some(result) = terminal {
            let edge = if result.is_empty() {
                None
            } else {
                let node = nodes.len();
                nodes.push(Node::Leaf(root));
                Some(Edge { first: 0, node })
            };
            done.insert(root, edge);
            return result;
        }
        let children = first.children();
        let mut tasks = vec![Task::Exit(root, first)];
        tasks.extend(children.into_iter().rev().map(Task::Enter));
        while let Some(task) = tasks.pop() {
            visit();
            match task {
                Task::Enter(key) => {
                    if done.contains_key(&key) {
                        continue;
                    }
                    let next = shape(key);
                    let children = next.children();
                    tasks.push(Task::Exit(key, next));
                    // ruled out: cycles through value fields are rejected by layout;
                    // counted references are terminal shapes, not value children.
                    tasks.extend(children.into_iter().rev().map(Task::Enter));
                }
                Task::Exit(key, next) => {
                    // ruled out: a child missing from done; its Enter/Exit tasks
                    // precede this Exit on the explicit postorder stack.
                    let result = match next {
                        Shape::Leaf => {
                            let node = nodes.len();
                            nodes.push(Node::Leaf(key));
                            Some(Edge { first: 0, node })
                        }
                        Shape::Fields(fields) => {
                            let edges = fields
                                .into_iter()
                                .filter_map(|(first, child)| {
                                    done[&child].map(|edge| Edge {
                                        first: first + edge.first,
                                        node: edge.node,
                                    })
                                })
                                .collect();
                            join(nodes, edges)
                        }
                        Shape::Mapped(inner, positions) => {
                            let mut edges = Vec::new();
                            leaves(nodes, done[&inner], |slot, node, _| {
                                visit();
                                if let Some(&first) = positions.get(slot) {
                                    edges.push(Edge { first, node });
                                }
                            });
                            join(nodes, edges)
                        }
                    };
                    done.insert(key, result);
                }
            }
        }
        // ruled out: the root is absent; its Exit was the last task.
        slots(nodes, done[&root], visit)
    }
}

thread_local! {
    static CACHE: std::cell::RefCell<Option<Plans<Ty>>> = const { std::cell::RefCell::new(None) };
}

/// Share plans for exactly one layout/link scope, restoring an outer scope
/// even on unwind. No type IDs or plans survive their pool's link lifetime.
pub(crate) fn cached<R>(link: impl FnOnce() -> R) -> R {
    struct Reset(Option<Plans<Ty>>);
    impl Drop for Reset {
        fn drop(&mut self) {
            CACHE.set(self.0.take());
        }
    }
    let _reset = Reset(CACHE.take());
    CACHE.set(Some(Plans::default()));
    link()
}

pub(super) fn for_type(root: Ty, shape: impl FnMut(Ty) -> Shape<Ty>) -> Vec<(usize, Ty)> {
    let held = CACHE.take();
    let linked = held.is_some();
    let mut plans = held.unwrap_or_default();
    let result = plans.collect(root, shape, &mut || {});
    if linked {
        CACHE.set(Some(plans));
    }
    result
}

#[cfg(test)]
fn collect<K: Copy + Eq + Hash>(
    root: K,
    shape: impl FnMut(K) -> Shape<K>,
    visit: &mut impl FnMut(),
) -> Vec<(usize, K)> {
    Plans::default().collect(root, shape, visit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_queries_only_enumerate_their_returned_slots() {
        let mut plans = Plans::default();
        let got = plans.collect(
            20usize,
            |key| {
                if key == 0 {
                    Shape::Leaf
                } else {
                    Shape::Fields(vec![(1, key - 1)])
                }
            },
            &mut || {},
        );
        let mut work = 0;
        for _ in 0..1000 {
            assert_eq!(
                plans.collect(20, |_| panic!("cached shape re-expanded"), &mut || work +=
                    1),
                got
            );
        }
        assert_eq!(work, 1000);
    }

    #[test]
    fn link_scopes_isolate_plans_and_restore_the_outer_cache_on_unwind() {
        use std::panic::{AssertUnwindSafe, catch_unwind};
        assert!(CACHE.with(|cache| cache.borrow().is_none()));
        cached(|| {
            assert_eq!(
                for_type(Ty::BOOL, |key| {
                    if key == Ty::BOOL {
                        Shape::Fields(vec![(2, Ty::I32)])
                    } else {
                        Shape::Leaf
                    }
                }),
                [(2, Ty::I32)]
            );
            let failed = catch_unwind(AssertUnwindSafe(|| {
                cached(|| {
                    assert!(for_type(Ty::BOOL, |_| Shape::Fields(Vec::new())).is_empty());
                    panic!("nested link");
                });
            }));
            assert!(failed.is_err());
            assert_eq!(
                for_type(Ty::BOOL, |_| panic!("outer plan lost")),
                [(2, Ty::I32)]
            );
        });
        assert!(CACHE.with(|cache| cache.borrow().is_none()));
        cached(|| {
            assert!(for_type(Ty::BOOL, |_| Shape::Fields(Vec::new())).is_empty());
            assert!(for_type(Ty::BOOL, |_| panic!("empty type re-expanded")).is_empty());
        });
    }

    #[test]
    fn shared_fields_and_optional_positions_keep_their_slots_and_types() {
        let got = collect(
            3u32,
            |key| match key {
                0 => Shape::Leaf,
                1 => Shape::Fields(vec![(1, 0), (3, 0)]),
                2 => Shape::Mapped(1, vec![9, 5, 7, 3]),
                3 => Shape::Fields(vec![(4, 2), (1, 0), (0, 4)]),
                4 => Shape::Fields(Vec::new()),
                _ => panic!("unexpected test type"),
            },
            &mut || {},
        );
        assert_eq!(got, [(9, 0), (7, 0), (1, 0)]);
    }

    #[test]
    fn sharing_does_not_multiply_chain_depth_at_width_1000_depth_20() {
        for (width, depth) in [(1usize, 1usize), (1000, 1), (1, 20), (1000, 20)] {
            let mut expanded = vec![0; depth + 2];
            let mut work = 0;
            let got = collect(
                depth + 1,
                |key| {
                    expanded[key] += 1;
                    if key == 0 {
                        Shape::Leaf
                    } else if key <= depth {
                        Shape::Fields(vec![(1, key - 1)])
                    } else {
                        Shape::Fields((0..width).map(|slot| (slot, depth)).collect())
                    }
                },
                &mut || work += 1,
            );
            assert_eq!(got, (0..width).map(|i| (i + depth, 0)).collect::<Vec<_>>());
            assert!(expanded.iter().all(|&n| n == 1));
            assert!(work <= 4 * (width + depth + 2), "{work}");
        }
    }

    #[test]
    fn repeated_empty_subgraphs_are_not_expanded_as_value_occurrences() {
        let width = 1000;
        let depth = 20;
        let mut expansions = 0;
        let mut work = 0;
        let got = collect(
            depth,
            |key| {
                expansions += 1;
                Shape::Fields(if key == 0 {
                    Vec::new()
                } else {
                    vec![(0, key - 1); width]
                })
            },
            &mut || work += 1,
        );
        assert!(got.is_empty());
        assert_eq!(expansions, depth + 1);
        assert!(work <= width * depth + 2 * (depth + 1));
    }

    #[test]
    fn a_20000_field_chain_uses_no_native_recursion_or_depth_cutoff() {
        let mut work = 0;
        let got = collect(
            20_000usize,
            |key| {
                if key == 0 {
                    Shape::Leaf
                } else {
                    Shape::Fields(vec![(1, key - 1)])
                }
            },
            &mut || work += 1,
        );
        assert_eq!(got, [(20_000, 0)]);
        assert_eq!(work, 2 * 20_001 + 1);
    }
}
