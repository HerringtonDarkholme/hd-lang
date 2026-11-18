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
    let mut out = Sccs {
        comp: vec![UNREACHED; n],
        order: Vec::with_capacity(n),
        starts: vec![0],
        cyclic: Vec::new(),
        work: 0,
    };
    out.work = traverse(n, roots, |event| {
        match event {
            Event::Enter(v, edges) => succ(v, edges),
            Event::Component(members, cyclic) => {
                let c = node(out.cyclic.len());
                for &m in members {
                    out.comp[m as usize] = c;
                }
                out.order.extend_from_slice(members);
                out.starts.push(node(out.order.len()));
                out.cyclic.push(cyclic);
            }
            Event::Edge(_, _) | Event::Finish(_) => {}
        }
        true
    });
    out
}

/// An online walk's callbacks. Returning false from `Enter` skips a node;
/// it must not append successors when skipping. Other return values are ignored.
pub enum Event<'a, N> {
    /// Append this node's successors, in traversal order, once per entered node.
    Enter(N, &'a mut Vec<N>),
    /// An edge, after its child traversal (if any), before the next edge.
    Edge(N, N),
    /// A node's DFS postorder position, before its component callback.
    Finish(N),
    /// A finished component in discovery order, and whether it is cyclic.
    /// The parent's walk resumes only after this callback returns.
    Component(&'a [N], bool),
}

/// Walk dense nodes `0..n`, sending the same events as [`Online`].
/// Dense indexing costs O(n) initialization; traversal costs only reached nodes
/// and edges, plus one probe per root. Returns entered nodes plus followed edges.
pub fn traverse(
    n: usize,
    roots: impl IntoIterator<Item = u32>,
    mut event: impl FnMut(Event<'_, u32>) -> bool,
) -> u64 {
    let mut walk = Walk::new(vec![usize::MAX; n]);
    for root in roots {
        walk.visit(root, &mut event);
    }
    walk.work
}

/// Persistent sparse SCC discovery, for lazy graphs and multiple seeds.
/// Like LLVM's `SCCIterator`, it yields components before resuming parents.
/// Only reached nodes are indexed; completed nodes persist across visits.
/// Expected cost: O(seeds + entered nodes + followed edges), plus callbacks;
/// space O(entered nodes + edges of open frames), with no native recursion.
pub struct Online<N> {
    walk: Walk<N, crate::HashMap<N, usize>>,
}

impl<N: Copy + Eq + std::hash::Hash> Default for Online<N> {
    fn default() -> Self {
        Self {
            walk: Walk::new(crate::HashMap::default()),
        }
    }
}

impl<N: Copy + Eq + std::hash::Hash> Online<N> {
    /// Visit a seed not already entered, retaining earlier discovery state.
    pub fn visit(&mut self, root: N, mut event: impl FnMut(Event<'_, N>) -> bool) {
        self.walk.visit(root, &mut event);
    }

    /// Nodes entered plus edges followed, cumulative across seeds.
    #[must_use]
    pub fn work(&self) -> u64 {
        self.walk.work
    }
}

// Batch graphs keep dense indexing; online graphs pay only for reached names.
// Both use exactly the same traversal below.
trait Indices<N> {
    fn get(&self, node: N) -> Option<usize>;
    fn insert(&mut self, node: N, index: usize);
}
impl Indices<u32> for Vec<usize> {
    fn get(&self, node: u32) -> Option<usize> {
        (self[node as usize] != usize::MAX).then_some(self[node as usize])
    }
    fn insert(&mut self, node: u32, index: usize) {
        self[node as usize] = index;
    }
}
impl<N: Eq + std::hash::Hash> Indices<N> for crate::HashMap<N, usize> {
    fn get(&self, node: N) -> Option<usize> {
        self.get(&node).copied()
    }
    fn insert(&mut self, node: N, index: usize) {
        self.insert(node, index);
    }
}

struct Node<N> {
    name: N,
    low: usize,
    base: usize,
}
struct Frame<N> {
    v: usize,
    at: usize,
    from: usize,
    child: Option<N>,
}
struct Walk<N, I> {
    index: I,
    nodes: Vec<Node<N>>,
    stack: Vec<usize>,
    frames: Vec<Frame<N>>,
    edges: Vec<N>,
    members: Vec<N>,
    work: u64,
}
impl<N: Copy + Eq, I: Indices<N>> Walk<N, I> {
    fn new(index: I) -> Self {
        Self {
            index,
            nodes: Vec::new(),
            stack: Vec::new(),
            frames: Vec::new(),
            edges: Vec::new(),
            members: Vec::new(),
            work: 0,
        }
    }
    fn enter(&mut self, name: N, event: &mut impl FnMut(Event<'_, N>) -> bool) -> bool {
        let from = self.edges.len();
        if !event(Event::Enter(name, &mut self.edges)) {
            self.edges.truncate(from);
            return false;
        }
        let v = self.nodes.len();
        self.index.insert(name, v);
        self.nodes.push(Node {
            name,
            low: v,
            base: self.stack.len(),
        });
        self.stack.push(v);
        self.frames.push(Frame {
            v,
            at: from,
            from,
            child: None,
        });
        self.work += 1;
        true
    }
    fn visit(&mut self, root: N, event: &mut impl FnMut(Event<'_, N>) -> bool) {
        if self.index.get(root).is_some() || !self.enter(root, event) {
            return;
        }
        while let Some(frame) = self.frames.last_mut() {
            let v = frame.v;
            let name = self.nodes[v].name;
            if let Some(child) = frame.child.take() {
                event(Event::Edge(name, child));
            }
            if frame.at < self.edges.len() {
                let child = self.edges[frame.at];
                frame.at += 1;
                self.work += 1;
                if let Some(w) = self.index.get(child) {
                    if self.nodes[w].low != usize::MAX {
                        self.nodes[v].low = self.nodes[v].low.min(w);
                    }
                } else {
                    frame.child = Some(child);
                    if self.enter(child, event) {
                        continue;
                    }
                    if let Some(frame) = self.frames.last_mut() {
                        frame.child = None;
                    }
                }
                event(Event::Edge(name, child));
                continue;
            }
            let from = frame.from;
            let low = self.nodes[v].low;
            event(Event::Finish(name));
            if low == v {
                let base = self.nodes[v].base;
                self.members.clear();
                for m in self.stack.drain(base..) {
                    self.nodes[m].low = usize::MAX;
                    self.members.push(self.nodes[m].name);
                }
                let cyclic = self.members.len() > 1 || self.edges[from..].contains(&name);
                event(Event::Component(&self.members, cyclic));
            }
            self.frames.pop();
            self.edges.truncate(from);
            if let Some(parent) = self.frames.last() {
                self.nodes[parent.v].low = self.nodes[parent.v].low.min(low);
            }
        }
    }
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

    #[test]
    fn online_callbacks_resume_parents_and_retain_seeds() {
        let mut walk = Online::default();
        let mut log = Vec::new();
        let mut finished = crate::HashSet::<u32>::default();
        let mut run = |root| {
            walk.visit(root, |event| {
                match event {
                    Event::Enter(v, out) => {
                        log.push(format!("enter {v}"));
                        if v == 9 {
                            return false;
                        }
                        match v {
                            0 => out.extend([1, 2, 9]),
                            1 => out.push(1),
                            2 => {
                                assert!(finished.contains(&1));
                                out.extend([1, 0]);
                            }
                            3 => out.push(0),
                            _ => {}
                        }
                    }
                    Event::Edge(v, w) => log.push(format!("edge {v} {w}")),
                    Event::Finish(v) => log.push(format!("finish {v}")),
                    Event::Component(m, cyclic) => {
                        log.push(format!("component {m:?} {cyclic}"));
                        finished.extend(m);
                    }
                }
                true
            });
        };
        run(0);
        run(3);
        run(0);
        assert_eq!(
            log,
            [
                "enter 0",
                "enter 1",
                "edge 1 1",
                "finish 1",
                "component [1] true",
                "edge 0 1",
                "enter 2",
                "edge 2 1",
                "edge 2 0",
                "finish 2",
                "edge 0 2",
                "enter 9",
                "edge 0 9",
                "finish 0",
                "component [0, 2] true",
                "enter 3",
                "edge 3 0",
                "finish 3",
                "component [3] false"
            ]
        );
        assert_eq!(walk.work(), 4 + 7);
    }

    #[test]
    fn online_and_dense_components_match_every_three_node_graph() {
        for bits in 0..512 {
            let edges: Vec<Vec<usize>> = (0..3)
                .map(|v| (0..3).filter(|w| bits & (1 << (3 * v + w)) != 0).collect())
                .collect();
            let dense = sccs_of(&edges);
            let mut online = Online::default();
            let mut groups = Vec::new();
            for root in 0..3 {
                online.visit(root, |event| {
                    match event {
                        Event::Enter(v, out) => out.extend_from_slice(&edges[v]),
                        Event::Component(m, cyclic) => groups.push((m.to_vec(), cyclic)),
                        _ => {}
                    }
                    true
                });
            }
            let expected: Vec<_> = dense
                .iter()
                .enumerate()
                .map(|(c, m)| (m.iter().map(|&v| v as usize).collect(), dense.cyclic(c)))
                .collect();
            assert_eq!(groups, expected, "graph {bits}");
            // Independent transitive closure checks the partition itself.
            let mut reach = [[false; 3]; 3];
            for v in 0..3 {
                for &w in &edges[v] {
                    reach[v][w] = true;
                }
            }
            for k in 0..3 {
                for v in 0..3 {
                    for w in 0..3 {
                        reach[v][w] |= reach[v][k] && reach[k][w];
                    }
                }
            }
            for (v, row) in reach.iter().enumerate() {
                for (w, other) in reach.iter().enumerate() {
                    assert_eq!(dense.comp[v] == dense.comp[w], v == w || row[w] && other[v]);
                }
            }
            assert_eq!(online.work(), dense.work);
        }
    }

    #[test]
    fn online_work_is_linear_across_wide_deep_and_repeated_seeds() {
        std::thread::Builder::new()
            .stack_size(96 * 1024)
            // cost: one thread, one O(nodes + edges + seeds) walk per generated graph.
            .spawn(|| {
                for (width, depth) in [(1000, 20), (10000, 20), (1, 10000)] {
                    let n = width * depth;
                    let mut walk = Online::default();
                    let mut components = 0;
                    for root in 0..n {
                        walk.visit(root, |event| {
                            match event {
                                Event::Enter(v, out) => {
                                    out.push(v / width * width + (v + 1) % width);
                                    if v + width < n {
                                        out.push(v + width);
                                    }
                                }
                                Event::Component(m, cyclic) => {
                                    assert_eq!(m.len(), width);
                                    assert!(cyclic);
                                    components += 1;
                                }
                                _ => {}
                            }
                            true
                        });
                    }
                    assert_eq!(components, depth);
                    assert_eq!(walk.work(), (3 * n - width) as u64);
                    let before = walk.work();
                    for root in 0..n {
                        walk.visit(root, |_| panic!("cached seed"));
                    }
                    assert_eq!(walk.work(), before);
                }
            })
            .unwrap()
            .join()
            .unwrap();
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
