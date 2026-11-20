//! Backward generated/keep dataflow. Structural control builds edges once;
//! loop convergence schedules predecessors instead of re-entering children.

use hd_base::{HashMap, HashSet};
use std::collections::VecDeque;

use super::{Backward, Flow, Role, charge, operands};
use crate::ir::{Body, NONE, Ref, Tag, Topology};

#[derive(Clone, Copy)]
enum Action {
    None,
    /// A block's start, which observes and changes nothing.
    Enter(u32),
    Stmt(u32),
    Finish(u32),
    Tail(u32),
}

enum Task {
    Block(u32, Option<u32>),
    Inst(u32, Option<u32>),
    Finish(u32),
}

struct Graph {
    index: HashMap<u32, usize>,
    actions: Vec<Action>,
    next: Vec<Vec<usize>>,
    catalog: Vec<usize>,
}

impl Graph {
    fn node(&mut self, b: &Body, x: u32) -> usize {
        if let Some(&node) = self.index.get(&x) {
            return node;
        }
        let node = self.actions.len();
        let block = b.tags.get(x as usize) == Some(&Tag::Block);
        self.actions.extend(if block {
            [Action::Enter(x), Action::None]
        } else {
            [Action::Finish(x), Action::Stmt(x)]
        });
        self.next.extend([Vec::new(), Vec::new()]);
        if !block {
            self.next[node].push(node + 1);
        }
        self.index.insert(x, node);
        node
    }

    fn new(b: &Body, roots: &[u32]) -> Self {
        let mut graph = Self {
            index: HashMap::default(),
            actions: Vec::new(),
            next: Vec::new(),
            catalog: Vec::new(),
        };
        let mut tasks: Vec<_> = roots.iter().rev().map(|&r| Task::Block(r, None)).collect();
        let mut visited = HashSet::default();
        while let Some(task) = tasks.pop() {
            charge(1);
            match task {
                Task::Finish(x) => {
                    let node = graph.node(b, x);
                    graph.catalog.push(node);
                }
                Task::Block(blk, arms) => {
                    let i = blk as usize;
                    if b.tags.get(i) != Some(&Tag::Block) || !visited.insert(blk) {
                        continue;
                    }
                    let node = graph.node(b, blk);
                    graph.catalog.push(node + 1);
                    tasks.push(Task::Finish(blk));
                    let tail = b.data[i][1];
                    let mut end = node + 1;
                    if tail != NONE && Ref(tail).as_inst().is_some() {
                        end = graph.actions.len();
                        graph.actions.push(Action::Tail(tail));
                        graph.next.push(vec![node + 1]);
                        graph.catalog.push(end);
                    }
                    for &x in b.record(b.data[i][0]).iter().rev() {
                        if b.tags[x as usize] != Tag::Block {
                            let inst = graph.node(b, x);
                            graph.next[inst + 1].push(end);
                            end = inst;
                        }
                    }
                    graph.next[node].push(end);
                    for &x in b.record(b.data[i][0]) {
                        if b.tags[x as usize] != Tag::Block {
                            tasks.push(Task::Inst(x, arms));
                        }
                    }
                }
                Task::Inst(x, arms) => {
                    let i = x as usize;
                    let node = graph.node(b, x);
                    let [p, w] = b.data[i];
                    graph.catalog.push(node + 1);
                    tasks.push(Task::Finish(x));
                    match b.tags[i] {
                        Tag::If => {
                            let r = b.record(w);
                            let children = [0, 1].map(|k| r.get(k).copied().unwrap_or(NONE));
                            graph.branches(b, x, &children);
                            for child in children.into_iter().rev() {
                                tasks.push(Task::Block(child, arms));
                            }
                        }
                        Tag::And | Tag::Or => {
                            graph.branches(b, x, &[w, NONE]);
                            tasks.push(Task::Block(w, arms));
                        }
                        Tag::With | Tag::RefDyn | Tag::Scope => {
                            let child = if b.tags[i] == Tag::With { w } else { p };
                            graph.branches(b, x, &[child]);
                            tasks.push(Task::Block(child, arms));
                        }
                        Tag::Loop => {
                            graph.next[node].clear();
                            if b.tags.get(p as usize) == Some(&Tag::Block) {
                                let child = graph.node(b, p);
                                graph.next[node].push(child);
                                graph.next[child + 1].push(child);
                            }
                            tasks.push(Task::Block(p, arms));
                        }
                        Tag::Match => {
                            let r = b.record(w);
                            if let Some((&decision, children)) = r.split_first() {
                                graph.next[node].clear();
                                if b.tags.get(decision as usize) == Some(&Tag::Block) {
                                    let decision = graph.node(b, decision);
                                    graph.next[node].push(decision);
                                }
                                for &child in children {
                                    let child = graph.node(b, child);
                                    graph.next[child + 1].push(node + 1);
                                }
                                tasks.push(Task::Block(decision, Some(x)));
                                for &child in children.iter().rev() {
                                    tasks.push(Task::Block(child, arms));
                                }
                            }
                        }
                        Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => {
                            let children: Vec<_> = super::cases(b, w).collect();
                            graph.branches(b, x, &children);
                            for child in children.into_iter().rev() {
                                tasks.push(Task::Block(child, arms));
                            }
                        }
                        Tag::Guard => {
                            graph.branches(b, x, b.record(w));
                            let children = std::mem::take(&mut graph.next[node]);
                            if b.tags.get(p as usize) == Some(&Tag::Block) {
                                let child = graph.node(b, p);
                                graph.next[node].push(child);
                                graph.next[child + 1].extend(children);
                            } else {
                                graph.next[node] = children;
                            }
                            tasks.push(Task::Block(p, arms));
                            for &child in b.record(w).iter().rev() {
                                tasks.push(Task::Block(child, arms));
                            }
                        }
                        Tag::Break | Tag::Continue => {
                            graph.next[node + 1].clear();
                            if let Some(&target) = b.label_inst.get(p as usize) {
                                if b.tags[i] == Tag::Continue {
                                    if b.tags.get(target as usize) == Some(&Tag::Loop) {
                                        let body = b.data[target as usize][0];
                                        if b.tags.get(body as usize) == Some(&Tag::Block) {
                                            let to = graph.node(b, body);
                                            graph.next[node + 1].push(to);
                                        }
                                    }
                                } else if (target as usize) < b.len() {
                                    let to = graph.node(b, target);
                                    graph.next[node + 1].push(to + 1);
                                }
                            }
                        }
                        Tag::Return | Tag::Unreachable => graph.next[node + 1].clear(),
                        Tag::ToArm => {
                            graph.actions[node] = Action::None;
                            graph.next[node + 1].clear();
                            if let Some(owner) = arms {
                                let r = b.record(b.data[owner as usize][1]);
                                if let Some(&arm) = r.get(p as usize + 1) {
                                    let to = graph.node(b, arm);
                                    graph.next[node + 1].push(to);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        graph
    }

    fn branches(&mut self, b: &Body, x: u32, children: &[u32]) {
        let node = self.node(b, x);
        self.next[node].clear();
        for &child in children {
            if b.tags.get(child as usize) == Some(&Tag::Block) {
                let child = self.node(b, child);
                self.next[node].push(child);
                self.next[child + 1].push(node + 1);
            } else {
                self.next[node].push(node + 1);
            }
        }
    }

    fn apply<A: Backward>(&self, node: usize, b: &Body, s: &mut Flow, a: &mut A, at: usize) {
        charge(1);
        match self.actions[node] {
            Action::None => {}
            Action::Enter(blk) => a.enter(b, blk, &mut s.arena, at),
            Action::Stmt(x) => a.stmt(b, x, &mut s.arena, at),
            Action::Tail(v) => a.uses(b, v, &mut s.arena, at),
            Action::Finish(x) => {
                a.ran(b, x, &mut s.arena, at);
                let mut ops = std::mem::take(&mut s.ops);
                operands(b, x as usize, &mut ops);
                for op in &ops {
                    if op.role != Role::Dropped && op.role != Role::Tail {
                        a.uses(b, op.value, &mut s.arena, at);
                    }
                }
                s.ops = ops;
                a.reads(b, x, &mut s.arena, at);
            }
        }
    }
}

/// Straight-line actions coalesce only when adjacent in the lexical backward
/// order. This keeps scope callbacks balanced while storing bitsets at control
/// boundaries, rather than at every instruction.
struct Blocks {
    actions: Vec<Vec<usize>>,
    next: Vec<Vec<usize>>,
}

impl Blocks {
    fn new(graph: &Graph) -> Self {
        let mut incoming = vec![0usize; graph.actions.len()];
        for next in &graph.next {
            for &to in next {
                incoming[to] += 1;
            }
        }
        let mut actions: Vec<Vec<usize>> = Vec::new();
        let mut group = vec![usize::MAX; graph.actions.len()];
        for &node in &graph.catalog {
            charge(1);
            let merge = actions
                .last()
                .and_then(|last| last.last())
                .is_some_and(|&previous| graph.next[node] == [previous] && incoming[previous] == 1);
            if !merge {
                actions.push(Vec::new());
            }
            let current = actions.len() - 1;
            group[node] = current;
            actions[current].push(node);
        }
        // A malformed jump can name a target outside these sub-body roots.
        // Such a target has no transfer in this walk, just as an unopened
        // label in the structured walker carried an empty state.
        for slot in &mut group {
            if *slot == usize::MAX {
                *slot = actions.len();
                actions.push(Vec::new());
            }
        }
        let next = actions
            .iter()
            .map(|nodes| {
                nodes.first().map_or_else(Vec::new, |&last| {
                    graph.next[last].iter().map(|&to| group[to]).collect()
                })
            })
            .collect();
        Self { actions, next }
    }
}

pub(super) fn run<A: Backward>(b: &Body, _topo: &Topology, s: &mut Flow, a: &mut A, roots: &[u32]) {
    s.reset(b);
    s.arena.free(0);
    let graph = Graph::new(b, roots);
    let blocks = Blocks::new(&graph);
    let words = s.arena.words;
    let count = blocks.actions.len();
    let mut generated = vec![0u64; count * words];
    let mut keep = generated.clone();
    let scratch = s.arena.alloc(None);
    // Each callback's transfer is generated/keep: its observations are disabled
    // during these probes, preserving lexical scope state on both passes.
    for (ones, rows) in [(false, &mut generated), (true, &mut keep)] {
        a.recording(false);
        for (node, actions) in blocks.actions.iter().enumerate() {
            charge(2 * words);
            s.arena
                .row_mut(scratch)
                .fill(if ones { u64::MAX } else { 0 });
            for &action in actions {
                graph.apply(action, b, s, a, scratch);
            }
            rows[node * words..(node + 1) * words].copy_from_slice(s.arena.row(scratch));
        }
    }
    let mut predecessors = vec![Vec::new(); count];
    for (node, next) in blocks.next.iter().enumerate() {
        for &to in next {
            predecessors[to].push(node);
        }
    }
    let mut postorder = Vec::with_capacity(count);
    let mut seen = vec![false; count];
    for root in 0..count {
        let mut stack = vec![(root, false)];
        while let Some((node, exit)) = stack.pop() {
            if exit {
                postorder.push(node);
                continue;
            }
            if std::mem::replace(&mut seen[node], true) {
                continue;
            }
            stack.push((node, true));
            for &to in &blocks.next[node] {
                if !seen[to] {
                    stack.push((to, false));
                }
            }
        }
    }
    let mut queue = VecDeque::from(postorder);
    let mut queued = vec![true; count];
    let mut live = vec![0u64; count * words];
    while let Some(node) = queue.pop_front() {
        queued[node] = false;
        let mut changed = false;
        for word in 0..words {
            charge(1 + blocks.next[node].len());
            let out = blocks.next[node]
                .iter()
                .fold(0, |out, &to| out | live[to * words + word]);
            let at = node * words + word;
            let value = generated[at] | (out & keep[at]);
            changed |= live[at] != value;
            live[at] = value;
        }
        if changed {
            for &before in &predecessors[node] {
                if !std::mem::replace(&mut queued[before], true) {
                    queue.push_back(before);
                }
            }
        }
    }
    a.recording(true);
    for (node, actions) in blocks.actions.iter().enumerate() {
        s.arena.clear(scratch);
        for &to in &blocks.next[node] {
            charge(words);
            for (dst, src) in s
                .arena
                .row_mut(scratch)
                .iter_mut()
                .zip(&live[to * words..(to + 1) * words])
            {
                *dst |= *src;
            }
        }
        for &action in actions {
            graph.apply(action, b, s, a, scratch);
        }
    }
    s.arena.free(0);
}
