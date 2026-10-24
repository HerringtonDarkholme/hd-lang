//! The bitset states of the ownership passes' dataflow walks (moves,
//! known-null and the verifier), and the backward walk over the structured
//! control flow that moves and the verifier's borrow walk run on.

use super::{Operand, Role, fill, operands};
use crate::ir::{Body, NONE, Ref, Tag, Topology, u32_of};

/// Bitset states of `words` words each in one growable arena, by start
/// offset, allocated and freed last in, first out.
#[derive(Default)]
pub(crate) struct BitArena {
    pub words: usize,
    cells: Vec<u64>,
}

impl BitArena {
    /// Where the next state goes.
    pub fn top(&self) -> usize {
        self.cells.len()
    }

    /// A new state of all zeros, or a copy of state `from`.
    pub fn alloc(&mut self, from: Option<usize>) -> usize {
        let at = self.cells.len();
        match from {
            Some(f) => self.cells.extend_from_within(f..f + self.words),
            None => self.cells.resize(at + self.words, 0),
        }
        at
    }

    /// Frees state `at` and every state allocated after it.
    pub fn free(&mut self, at: usize) {
        self.cells.truncate(at);
    }

    pub fn copy(&mut self, to: usize, from: usize) {
        self.cells.copy_within(from..from + self.words, to);
    }

    /// Empties state `at`.
    pub fn clear(&mut self, at: usize) {
        self.cells[at..at + self.words].fill(0);
    }

    /// State `to` becomes state `from`, or empty when there is none.
    pub fn copy_or_clear(&mut self, to: usize, from: Option<u32>) {
        match from {
            Some(from) => self.copy(to, from as usize),
            None => self.clear(to),
        }
    }

    /// `to |= from`; whether `to` grew.
    pub fn union(&mut self, to: usize, from: usize) -> bool {
        let mut grew = false;
        for k in 0..self.words {
            let v = self.cells[from + k];
            let w = &mut self.cells[to + k];
            grew |= v & !*w != 0;
            *w |= v;
        }
        grew
    }

    /// Joins `from` into `to`: a copy when `to` holds no path yet.
    pub fn join(&mut self, to: usize, any: &mut bool, from: usize) {
        if *any {
            self.union(to, from);
        } else {
            self.copy(to, from);
            *any = true;
        }
    }

    pub fn set(&mut self, at: usize, bit: u32, on: bool) {
        let w = &mut self.cells[at + (bit / 64) as usize];
        if on {
            *w |= 1 << (bit % 64);
        } else {
            *w &= !(1 << (bit % 64));
        }
    }

    pub fn get(&self, at: usize, bit: u32) -> bool {
        self.cells[at + (bit / 64) as usize] & (1 << (bit % 64)) != 0
    }

    /// State `at`'s words.
    pub fn row(&self, at: usize) -> &[u64] {
        &self.cells[at..at + self.words]
    }
}

/// A backward walk's states: the arena, and the states a jump to each open
/// target carries.
#[derive(Default)]
pub(crate) struct Back {
    pub arena: BitArena,
    /// Per label, the offsets of the states a `Break` and a `Continue` to
    /// it carry while its target is walked, or `NONE`.
    jumps: Vec<[u32; 2]>,
    /// The open matches' arm states: the first arm's offset and the count.
    arms: Vec<(u32, u32)>,
    ops: Vec<Operand>,
}

/// A backward analysis over the structured control flow ([`backward`]):
/// one bit per fact in state `live` of `s`, joins are unions ("on any
/// path"). The analysis gives each instruction's transfer; the walk gives
/// the paths.
pub(crate) trait Backward {
    /// Instruction `x` over `live`, what holds after it, before the walk
    /// enters its blocks: what it defines and writes.
    fn stmt(&mut self, b: &Body, x: u32, s: &mut BitArena, live: usize);
    /// A use of value `v`: an operand of an instruction, which it reads
    /// before it runs any block, or a block's tail at the block's end.
    fn uses(&mut self, b: &Body, v: u32, s: &mut BitArena, live: usize);
    /// What else `x` reads, after its operands' uses.
    fn reads(&mut self, _: &Body, _x: u32, _s: &mut BitArena, _live: usize) {}
}

/// Walks each sub-body of `b` backward, from nothing live at its end, with
/// the states in `s`.
pub(crate) fn backward<A: Backward>(b: &Body, topo: &Topology, s: &mut Back, a: &mut A) {
    fill(&mut s.jumps, b.label_inst.len(), [NONE; 2]);
    s.arms.clear();
    s.arena.free(0);
    let mut walk = Walk { b, topo, s, a };
    for &root in b.sub_root.iter().filter(|&&r| (r as usize) < b.len()) {
        let live = walk.s.arena.alloc(None);
        walk.block(root, live);
        walk.s.arena.free(live);
    }
}

struct Walk<'a, A> {
    b: &'a Body,
    topo: &'a Topology,
    s: &'a mut Back,
    a: &'a mut A,
}

impl<A: Backward> Walk<'_, A> {
    /// Block `blk` backward: state `live`, what holds after the block,
    /// becomes what holds at its start.
    fn block(&mut self, blk: u32, live: usize) {
        let b = self.b;
        if b.tags.get(blk as usize) != Some(&Tag::Block) {
            return;
        }
        // A `Break` to a labeled block continues after it.
        let first = self.s.arena.top();
        let saved = self.target(blk, live, NONE);
        // The tail is taken at the block's end.
        let tail = b.data[blk as usize][1];
        if tail != NONE && Ref(tail).as_inst().is_some() {
            self.a.uses(b, tail, &mut self.s.arena, live);
        }
        for &x in b.record(b.data[blk as usize][0]).iter().rev() {
            // A block in the list runs where the instruction that owns it
            // runs it.
            if b.tags[x as usize] != Tag::Block {
                self.inst(x, live);
            }
        }
        self.restore(blk, saved, first);
    }

    /// Points a `Break` to `x`'s label, if it has one, at a copy of
    /// `live`, and a `Continue` at state `cont`; the label's states before.
    fn target(&mut self, x: u32, live: usize, cont: u32) -> [u32; 2] {
        let label = self.topo.label_of[x as usize] as usize;
        if label >= self.s.jumps.len() {
            return [NONE; 2];
        }
        let at = u32_of(self.s.arena.alloc(Some(live)));
        std::mem::replace(&mut self.s.jumps[label], [at, cont])
    }

    /// Gives `x`'s label back its states `old` and frees the states from
    /// `first` on.
    fn restore(&mut self, x: u32, old: [u32; 2], first: usize) {
        let label = self.topo.label_of[x as usize] as usize;
        if label < self.s.jumps.len() {
            self.s.jumps[label] = old;
        }
        self.s.arena.free(first);
    }

    /// Instruction `x` backward over state `live`.
    fn inst(&mut self, x: u32, live: usize) {
        let b = self.b;
        self.a.stmt(b, x, &mut self.s.arena, live);
        let [p, w] = b.data[x as usize];
        match b.tags[x as usize] {
            Tag::If => {
                let r = b.record(w);
                self.any([0, 1].map(|k| r.get(k).copied().unwrap_or(NONE)), live);
            }
            // The right operand may not run.
            Tag::And | Tag::Or => self.any([w, NONE], live),
            Tag::With => self.block(w, live),
            Tag::RefDyn | Tag::Scope => self.block(p, live),
            Tag::Loop => self.repeat(x, live),
            Tag::Match => self.decide(x, live),
            Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => {
                let r = b.record(w);
                let (cases, default) = r.split_at(r.len().saturating_sub(1));
                let blks = cases.iter().skip(1).step_by(2).chain(default);
                self.any(blks.copied(), live);
            }
            Tag::Guard => {
                self.any(b.record(w).iter().copied(), live);
                self.block(p, live);
            }
            Tag::ToArm => {
                let s = &mut *self.s;
                let words = u32_of(s.arena.words);
                let arm = s.arms.last().filter(|a| p < a.1).map(|a| a.0 + p * words);
                // A path of the decision tree enters an arm.
                s.arena.copy_or_clear(live, arm);
                return;
            }
            tag @ (Tag::Break | Tag::Continue) => {
                let k = usize::from(tag == Tag::Continue);
                let at = self.s.jumps.get(p as usize).map(|j| j[k]);
                let at = at.filter(|&at| at != NONE);
                self.s.arena.copy_or_clear(live, at);
            }
            Tag::Return | Tag::Unreachable => self.s.arena.clear(live),
            _ => {}
        }
        // A `Drop` keeps nothing live, and a block's tail is used at its
        // end.
        let mut ops = std::mem::take(&mut self.s.ops);
        operands(b, x as usize, &mut ops);
        for op in &ops {
            if op.role != Role::Dropped && op.role != Role::Tail {
                self.a.uses(b, op.value, &mut self.s.arena, live);
            }
        }
        self.s.ops = ops;
        self.a.reads(b, x, &mut self.s.arena, live);
    }

    /// The paths through blocks `blks`, each from `live`, joined into
    /// `live`. A `NONE` block is a path that runs nothing.
    fn any(&mut self, blks: impl IntoIterator<Item = u32>, live: usize) {
        let acc = self.s.arena.alloc(None);
        for blk in blks {
            let at = self.s.arena.alloc(Some(live));
            self.block(blk, at);
            self.s.arena.union(acc, at);
            self.s.arena.free(at);
        }
        self.s.arena.copy(live, acc);
        self.s.arena.free(acc);
    }

    /// Loop `x` backward: its head, from which the body runs, to a fixed
    /// point; the body's end and a `Continue` go back to the head, a
    /// `Break` to after the loop.
    fn repeat(&mut self, x: u32, live: usize) {
        let first = self.s.arena.top();
        let head = self.s.arena.alloc(None);
        // The walk of the body leaves the head as it is.
        let saved = self.target(x, live, u32_of(head));
        // Each round but the last grows the head, by a bit at least.
        for _ in 0..self.s.arena.words * 64 + 2 {
            let end = self.s.arena.alloc(Some(head));
            self.block(self.b.data[x as usize][0], end);
            let grew = self.s.arena.union(head, end);
            self.s.arena.free(end);
            if !grew {
                break;
            }
        }
        self.s.arena.copy(live, head);
        self.restore(x, saved, first);
    }

    /// Match `x` backward: each arm from after the match, then the decision
    /// tree, whose every path ends in a `ToArm` or `Unreachable`.
    fn decide(&mut self, x: u32, live: usize) {
        let r = self.b.record(self.b.data[x as usize][1]);
        let Some((&decision, arms)) = r.split_first() else {
            return;
        };
        let first = self.s.arena.top();
        let saved = self.target(x, live, NONE);
        let arms_at = self.s.arena.top();
        for _ in arms {
            self.s.arena.alloc(Some(live));
        }
        for (k, &arm) in arms.iter().enumerate() {
            self.block(arm, arms_at + k * self.s.arena.words);
        }
        self.s.arms.push((u32_of(arms_at), u32_of(arms.len())));
        let tree = self.s.arena.alloc(None);
        self.block(decision, tree);
        self.s.arms.pop();
        self.s.arena.copy(live, tree);
        self.restore(x, saved, first);
    }
}
