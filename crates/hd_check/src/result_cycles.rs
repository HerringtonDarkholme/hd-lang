//! Result-inference dependencies, shared by item and local functions.
//!
//! Like LLVM's `SCCIterator`, the existing `hd_base` Tarjan walk uses explicit
//! frames. Recording costs expected O(1) per declaration/read; finishing a
//! completed inference group costs O(vertices + edges), then drops that group.
//! Written results are boundaries. No body is checked again by this graph.

use hd_base::{HashMap, HashSet, NodeIdx};
use hd_types::Ty;

#[derive(Default)]
pub(crate) struct Cycles {
    ids: HashMap<NodeIdx, u32>,
    nodes: Vec<(NodeIdx, usize)>,
    edges: Vec<Vec<u32>>,
    unique: HashSet<(u32, u32)>,
    frames: Vec<Frame>,
    active: HashMap<NodeIdx, usize>,
    /// An unfinished result read supplied poison; suppress follow-on
    /// inference errors until this group's cycle diagnostics are emitted.
    pub(crate) poisoned: bool,
}

pub(crate) enum Read {
    Inactive,
    Poison,
    Live(Ty),
}

struct Frame {
    at: NodeIdx,
    vertex: Option<u32>,
    /// One past the nearest written-result frame, or zero.
    boundary: usize,
    live: Option<Ty>,
    /// Only lexical body reads add edges, not row-settling queries after it.
    recording: bool,
}

impl Cycles {
    fn vertex(&mut self, at: NodeIdx, position: usize) -> u32 {
        if let Some(&v) = self.ids.get(&at) {
            return v;
        }
        let v = hd_base::scc::node(self.nodes.len());
        self.ids.insert(at, v);
        self.nodes.push((at, position));
        self.edges.push(Vec::new());
        v
    }

    fn edge(&mut self, to: u32) {
        if let Some(from) = self
            .frames
            .last()
            .filter(|f| f.recording)
            .and_then(|f| f.vertex)
            && self.unique.insert((from, to))
        {
            self.edges[from as usize].push(to);
        }
    }

    /// A local declaration's omitted result is inferred in its enclosing
    /// body's check. Item dependencies instead come from signature reads.
    pub(crate) fn enter(
        &mut self,
        at: NodeIdx,
        position: usize,
        omitted: bool,
        local: bool,
        live: Option<Ty>,
    ) -> usize {
        let mark = self.frames.len();
        let vertex = omitted.then(|| self.vertex(at, position));
        if local && let Some(v) = vertex {
            self.edge(v);
        }
        let boundary = if omitted {
            self.frames.last().map_or(0, |f| f.boundary)
        } else {
            mark + 1
        };
        if at != NodeIdx::NONE {
            self.active.insert(at, mark);
        }
        self.frames.push(Frame {
            at,
            vertex,
            boundary,
            live,
            recording: true,
        });
        mark
    }

    pub(crate) fn leave(&mut self, mark: usize) {
        for frame in self.frames.drain(mark..) {
            self.active.remove(&frame.at);
        }
    }

    pub(crate) fn live(&mut self, at: NodeIdx, live: Option<Ty>) {
        if let Some(&i) = self.active.get(&at) {
            self.frames[i].live = live;
            self.frames[i].recording = live.is_some();
        }
    }

    /// Record a read. An unfinished omitted result is poison until it is
    /// inferred, except across a written-result boundary in the same body,
    /// where its live inference variable remains usable.
    pub(crate) fn read(&mut self, at: NodeIdx, position: usize) -> Read {
        let v = self.vertex(at, position);
        self.edge(v);
        let Some(&i) = self.active.get(&at) else {
            return Read::Inactive;
        };
        let frame = &self.frames[i];
        if self.frames.last().is_some_and(|f| f.boundary > i) {
            frame.live.map_or(Read::Poison, Read::Live)
        } else {
            self.poisoned = true;
            Read::Poison
        }
    }

    pub(crate) fn open(&self) -> bool {
        !self.frames.is_empty()
    }

    pub(crate) fn finish(&mut self) -> (Vec<NodeIdx>, u64) {
        debug_assert!(self.frames.is_empty());
        let graph = std::mem::take(self);
        let scc = hd_base::scc::sccs(graph.nodes.len(), |v, out| {
            out.extend_from_slice(&graph.edges[v as usize]);
        });
        let mut cycles = Vec::new();
        for c in 0..scc.len() {
            if scc.cyclic(c)
                && let Some(&v) = scc
                    .members(c)
                    .iter()
                    .min_by_key(|&&v| graph.nodes[v as usize].1)
            {
                cycles.push(graph.nodes[v as usize].0);
            }
        }
        (cycles, scc.work)
    }
}

#[cfg(feature = "testing")]
pub(crate) fn generated_work(width: u32, depth: u32, nested: bool) -> (usize, u64, u64) {
    let mut graph = Cycles::default();
    let n = width * depth;
    for i in 0..n {
        let at = NodeIdx::from_raw(i);
        let mark = graph.enter(at, i as usize, true, nested, Some(Ty::I32));
        if !nested {
            let next = i / width * width + (i + 1) % width;
            graph.read(NodeIdx::from_raw(next), next as usize);
            if i + width < n {
                graph.read(NodeIdx::from_raw(i + width), (i + width) as usize);
            }
            graph.leave(mark);
        }
    }
    if nested {
        graph.read(NodeIdx::from_raw(0), 0);
        graph.leave(0);
    }
    let (cycles, work) = graph.finish();
    let (_, empty_work) = graph.finish();
    (cycles.len(), work, empty_work)
}
