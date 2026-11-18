//! Strongly connected components: the workspace's one Tarjan walk.
//!
//! Tarjan 1972 in the shape of rustc's `rustc_data_structures::graph::scc`:
//! dense per-node arrays and an explicit frame stack, no native recursion,
//! so a chain of a million nodes takes no stack per node. A node's
//! successors are written into one shared buffer when the walk enters it;
//! its frame reads them from there, a child's successors follow its
//! parent's, and they are dropped when the child's frame closes. Each node
//! is entered once and each edge followed once: O(nodes + edges) time and
//! O(nodes + the successors of the open frames) memory.

/// The component of a node no root reaches.
pub const UNREACHED: u32 = u32::MAX;

/// A node number: graphs are program items, values or regions, far fewer
/// than `u32::MAX`, which marks a node no root reaches.
#[must_use]
pub fn node(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(UNREACHED)
}

/// The strongly connected components of a graph, numbered in the order
/// Tarjan's walk finishes them: each after every component it reaches.
pub struct Sccs {
    /// Each node's component, [`UNREACHED`] for a node no root reaches.
    pub comp: Vec<u32>,
    /// The members of component `c`, `order[starts[c]..starts[c + 1]]`,
    /// each component's in the order the walk entered them.
    order: Vec<u32>,
    starts: Vec<u32>,
    /// Per component, whether it is a cycle: two or more members, or one
    /// with an edge to itself.
    cyclic: Vec<bool>,
    /// The nodes entered plus the edges followed, for cost tests.
    pub work: u64,
}

impl Sccs {
    /// The number of components.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cyclic.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cyclic.is_empty()
    }

    /// The members of component `c`, in the order the walk entered them.
    #[must_use]
    pub fn members(&self, c: usize) -> &[u32] {
        &self.order[self.starts[c] as usize..self.starts[c + 1] as usize]
    }

    /// Whether component `c` is a cycle.
    #[must_use]
    pub fn cyclic(&self, c: usize) -> bool {
        self.cyclic[c]
    }

    /// The components' members, in finishing order.
    #[must_use]
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &[u32]> + '_ {
        (0..self.len()).map(|c| self.members(c))
    }
}

/// The components of the graph over nodes `0..n` whose successors `succ`
/// appends to its buffer, walking from every node in order.
pub fn sccs(n: usize, succ: impl FnMut(u32, &mut Vec<u32>)) -> Sccs {
    sccs_from(n, (0..n).map(node), succ)
}

/// The components of a graph given by successor lists.
#[must_use]
pub fn sccs_of(lists: &[Vec<usize>]) -> Sccs {
    sccs(lists.len(), |v, out| {
        out.extend(lists[v as usize].iter().map(|&w| node(w)));
    })
}

/// The components of the graph over nodes `0..n` that `roots` reach,
/// whose successors `succ` appends to its buffer.
pub fn sccs_from(
    n: usize,
    roots: impl IntoIterator<Item = u32>,
    mut succ: impl FnMut(u32, &mut Vec<u32>),
) -> Sccs {
    let mut index = vec![UNREACHED; n];
    let mut low = vec![0u32; n];
    let mut comp = vec![UNREACHED; n];
    let mut stack: Vec<u32> = Vec::new();
    // Per open frame: its node, its next successor in `buf`, and where its
    // successors start there. The top frame's successors end at the
    // buffer's end.
    let mut frames: Vec<(u32, usize, usize)> = Vec::new();
    let mut buf: Vec<u32> = Vec::new();
    let mut out = Sccs {
        comp: Vec::new(),
        order: Vec::with_capacity(n),
        starts: vec![0],
        cyclic: Vec::new(),
        work: 0,
    };
    let mut next = 0u32;
    for root in roots {
        if index[root as usize] != UNREACHED {
            continue;
        }
        let mut pending = Some(root);
        loop {
            if let Some(v) = pending.take() {
                out.work += 1;
                (index[v as usize], low[v as usize]) = (next, next);
                next += 1;
                stack.push(v);
                let from = buf.len();
                succ(v, &mut buf);
                frames.push((v, from, from));
            }
            let Some(&mut (v, ref mut at, from)) = frames.last_mut() else {
                break;
            };
            let vi = v as usize;
            if *at < buf.len() {
                let w = buf[*at];
                *at += 1;
                out.work += 1;
                if index[w as usize] == UNREACHED {
                    pending = Some(w);
                } else if comp[w as usize] == UNREACHED {
                    // Entered and in no finished component: on the stack.
                    low[vi] = low[vi].min(index[w as usize]);
                }
                continue;
            }
            if low[vi] == index[vi] {
                // ruled out: Tarjan's invariant keeps `v` on the stack until
                // its component closes, and it closes here.
                let at = stack.iter().rposition(|&m| m == v).unwrap_or(0);
                let c = node(out.cyclic.len());
                for &m in &stack[at..] {
                    comp[m as usize] = c;
                }
                out.cyclic
                    .push(stack.len() - at > 1 || buf[from..].contains(&v));
                out.order.extend_from_slice(&stack[at..]);
                out.starts.push(node(out.order.len()));
                stack.truncate(at);
            }
            frames.pop();
            buf.truncate(from);
            if let Some(&(p, ..)) = frames.last() {
                low[p as usize] = low[p as usize].min(low[vi]);
            }
        }
    }
    out.comp = comp;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn groups(s: &Sccs) -> Vec<Vec<u32>> {
        s.iter().map(<[u32]>::to_vec).collect()
    }

    #[test]
    fn components_finish_after_what_they_reach() {
        // 0 -> 1 <-> 2 -> 3, 3 -> 3, 4 alone.
        let s = sccs_of(&[vec![1], vec![2], vec![1, 3], vec![3], vec![]]);
        assert_eq!(groups(&s), [vec![3], vec![1, 2], vec![0], vec![4]]);
        assert_eq!(s.comp, [2, 1, 1, 0, 3]);
        let cyclic: Vec<bool> = (0..s.len()).map(|c| s.cyclic(c)).collect();
        assert_eq!(cyclic, [true, true, false, false]);
        assert_eq!(s.work, 5 + 5);
    }

    #[test]
    fn roots_limit_the_walk() {
        let s = sccs_from(4, [2], |v, out| {
            if v < 3 {
                out.push(v + 1);
            }
        });
        assert_eq!(groups(&s), [vec![3], vec![2]]);
        assert_eq!(s.comp, [UNREACHED, UNREACHED, 1, 0]);
        assert!(!s.is_empty());
    }

    /// A walk over `n` nodes on a 256 KiB stack.
    fn on_small_stack(n: usize, cycle: bool) -> Sccs {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            // cost: one thread, one walk of `n` nodes and `n` edges.
            .spawn(move || {
                sccs(n, |v, out| {
                    let w = v as usize + 1;
                    if w < n {
                        out.push(node(w));
                    } else if cycle {
                        out.push(0);
                    }
                })
            })
            .unwrap()
            .join()
            .unwrap()
    }

    #[test]
    fn a_million_node_chain_takes_no_stack_per_node() {
        let s = on_small_stack(1_000_000, false);
        assert_eq!(s.len(), 1_000_000);
        assert_eq!(s.comp[0], 999_999);
        assert!(!s.cyclic(0));
        let s = on_small_stack(1_000_000, true);
        assert_eq!(s.len(), 1);
        assert!(s.cyclic(0));
        assert_eq!(s.work, 2_000_000);
    }
}
