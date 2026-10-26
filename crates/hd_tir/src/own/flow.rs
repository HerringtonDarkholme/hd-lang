//! The bitset states of the ownership passes' dataflow walks (moves,
//! known-null and the verifier), and the walks over the structured control
//! flow they run on: backward for moves and the verifier's borrow walk,
//! forward for known-null and the verifier's read-after-move and
//! one-consumer walks.

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

    pub fn row_mut(&mut self, at: usize) -> &mut [u64] {
        &mut self.cells[at..at + self.words]
    }
}

/// Bit `bit` of a state's words.
pub(crate) fn get_bit(row: &[u64], bit: usize) -> bool {
    row[bit / 64] & (1 << (bit % 64)) != 0
}

pub(crate) fn set_bit(row: &mut [u64], bit: usize, on: bool) {
    if on {
        row[bit / 64] |= 1 << (bit % 64);
    } else {
        row[bit / 64] &= !(1 << (bit % 64));
    }
}

/// The states a `Break` and a `Continue` to a label carry.
type Jumps = [(u32, bool); 2];

/// A walk's states: the arena, and the states a jump to each open target
/// carries.
#[derive(Default)]
pub(crate) struct Flow {
    pub arena: BitArena,
    /// Per label, while its target is walked, the offsets of the states a
    /// `Break` and a `Continue` to it carry, or `NONE`, each with whether
    /// a forward walk's jump reached it yet.
    jumps: Vec<Jumps>,
    /// The open matches' arm states: the first arm's offset and the count;
    /// forward, whether a path reached each arm, innermost last.
    arms: Vec<(u32, u32)>,
    hit: Vec<bool>,
    /// Forward with [`Suites::FromExits`]: the open scopes' exit states,
    /// innermost last, and per label how many were open when its target's
    /// walk began.
    scopes: Vec<u32>,
    depth: Vec<u32>,
    ops: Vec<Operand>,
}

impl Flow {
    fn reset(&mut self, b: &Body) {
        fill(&mut self.jumps, b.label_inst.len(), [(NONE, false); 2]);
        fill(&mut self.depth, b.label_inst.len(), 0);
        self.arms.clear();
        self.hit.clear();
        self.scopes.clear();
    }

    /// Points a `Break` to `x`'s label, if it has one, at a new state, a
    /// copy of `from` or empty, and a `Continue` at state `cont`; the
    /// label's states before.
    fn open(&mut self, topo: &Topology, x: u32, from: Option<usize>, cont: u32) -> Jumps {
        let label = topo.label_of[x as usize] as usize;
        if label >= self.jumps.len() {
            return [(NONE, false); 2];
        }
        self.depth[label] = u32_of(self.scopes.len());
        let at = u32_of(self.arena.alloc(from));
        std::mem::replace(&mut self.jumps[label], [(at, false), (cont, false)])
    }

    /// Gives `x`'s label back its states `old` and frees the states from
    /// `first` on.
    fn restore(&mut self, topo: &Topology, x: u32, old: Jumps, first: usize) {
        let label = topo.label_of[x as usize] as usize;
        if label < self.jumps.len() {
            self.jumps[label] = old;
        }
        self.arena.free(first);
    }
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
    /// `x` over `live` once the walk has been through the blocks it runs,
    /// before its operands' uses: what holds before it runs them.
    fn ran(&mut self, _: &Body, _x: u32, _s: &mut BitArena, _live: usize) {}
}

/// Walks each sub-body of `b` backward, from nothing live at its end, with
/// the states in `s`.
pub(crate) fn backward<A: Backward>(b: &Body, topo: &Topology, s: &mut Flow, a: &mut A) {
    s.reset(b);
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
    s: &'a mut Flow,
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
        let saved = self.s.open(self.topo, blk, Some(live), NONE);
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
        self.s.restore(self.topo, blk, saved, first);
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
                self.any(cases(b, w), live);
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
                let at = self.s.jumps.get(p as usize).map(|j| j[k].0);
                let at = at.filter(|&at| at != NONE);
                self.s.arena.copy_or_clear(live, at);
            }
            Tag::Return | Tag::Unreachable => self.s.arena.clear(live),
            _ => {}
        }
        self.a.ran(b, x, &mut self.s.arena, live);
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
        let saved = self.s.open(self.topo, x, Some(live), u32_of(head));
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
        self.s.restore(self.topo, x, saved, first);
    }

    /// Match `x` backward: each arm from after the match, then the decision
    /// tree, whose every path ends in a `ToArm` or `Unreachable`.
    fn decide(&mut self, x: u32, live: usize) {
        let r = self.b.record(self.b.data[x as usize][1]);
        let Some((&decision, arms)) = r.split_first() else {
            return;
        };
        let first = self.s.arena.top();
        let saved = self.s.open(self.topo, x, Some(live), NONE);
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
        self.s.restore(self.topo, x, saved, first);
    }
}

/// The blocks a switch with record `w` may run: each case's, then the
/// default's.
fn cases(b: &Body, w: u32) -> impl Iterator<Item = u32> + '_ {
    let r = b.record(w);
    let (cases, default) = r.split_at(r.len().saturating_sub(1));
    cases.iter().skip(1).step_by(2).chain(default).copied()
}

/// Where a forward walk starts the cleanup suites of a `Scope`.
pub(crate) enum Suites {
    /// From the body's end, where a path reaches it. The scope's value is
    /// the union of the body's end and every suite's end (`union`), or the
    /// last suite's end that a path falls out of.
    FromEnd { union: bool },
    /// From the union of the states at the body's exits: its end and each
    /// jump that leaves it. The scope's value is the union of the body's
    /// end and every suite's end.
    FromExits,
}

/// A forward analysis over the structured control flow ([`forward`]):
/// one bit per fact in state `st` of `s`, joins are unions ("on any
/// path"). The analysis gives each instruction's transfer; the walk gives
/// the paths.
pub(crate) trait Forward {
    const SUITES: Suites;
    /// What a loop's `Break`s carry joins over every round, not only the
    /// last round's.
    const ALL_ROUNDS: bool = false;
    /// Instruction `x` over `st`, unless it runs blocks: a jump before it
    /// leaves.
    fn stmt(&mut self, b: &Body, x: u32, s: &mut BitArena, st: usize);
    /// The operands of `x`, which runs blocks, before it runs them.
    fn enter(&mut self, b: &Body, x: u32, s: &mut BitArena, st: usize);
    /// `x`'s own value, where a path falls out of the blocks it runs.
    fn leave(&mut self, _: &Body, _x: u32, _s: &mut BitArena, _st: usize) {}
    /// Block `blk`'s tail at its end, where the instruction that runs it
    /// has a value.
    fn tail(&mut self, _: &Body, _blk: u32, _s: &mut BitArena, _st: usize) {}
    /// `x` goes round loop `lp` again: a `Continue`, or the end of the
    /// loop's body (`x == lp`).
    fn back(&mut self, _: &Body, _x: u32, _lp: u32, _s: &mut BitArena, _st: usize) {}
    /// `Return` `x` leaves the sub-body.
    fn exit(&mut self, _: &Body, _x: u32, _s: &mut BitArena, _st: usize) {}
    /// Member `p` of block `blk` runs next, or the block ends (`p` is its
    /// length); `y` is the analysis's own place in the block, from 0.
    /// Whether to call it again in this block.
    fn before(
        &mut self,
        _: &Body,
        _blk: u32,
        _p: usize,
        _y: &mut usize,
        _s: &mut BitArena,
        _st: usize,
    ) -> bool {
        false
    }
    /// Whether member `x` of a block is gone and the walk skips it.
    fn skip(&self, _x: u32) -> bool {
        false
    }
    /// The walk enters a scope's cleanup suites (`true`), or leaves them.
    fn suites(&mut self, _inside: bool) {}
}

/// Walks block `blk` of `b` forward from state `st` of `s`; false when no
/// path falls out of its end.
pub(crate) fn forward<A: Forward>(
    b: &Body,
    topo: &Topology,
    s: &mut Flow,
    a: &mut A,
    blk: u32,
    st: usize,
) -> bool {
    s.reset(b);
    Ahead { b, topo, s, a }.block(blk, st)
}

struct Ahead<'a, A> {
    b: &'a Body,
    topo: &'a Topology,
    s: &'a mut Flow,
    a: &'a mut A,
}

impl<A: Forward> Ahead<'_, A> {
    /// Block `blk` forward over `st`; false when no path falls out of it.
    fn block(&mut self, blk: u32, st: usize) -> bool {
        let b = self.b;
        if b.tags.get(blk as usize) != Some(&Tag::Block) {
            return true;
        }
        let first = self.s.arena.top();
        let saved = self.s.open(self.topo, blk, None, NONE);
        // A block in the list runs where the instruction that owns it runs
        // it.
        let members = b.record(b.data[blk as usize][0]);
        let (mut y, mut more) = (0, true);
        let mut falls = members.iter().enumerate().all(|(p, &x)| {
            more = more && self.a.before(b, blk, p, &mut y, &mut self.s.arena, st);
            b.tags[x as usize] == Tag::Block || self.a.skip(x) || self.inst(x, st)
        });
        if falls && more {
            self.a
                .before(b, blk, members.len(), &mut y, &mut self.s.arena, st);
        }
        self.close(blk, saved, first, st, &mut falls);
        falls
    }

    /// Walks block `blk` and takes its tail.
    fn valued(&mut self, blk: u32, st: usize) -> bool {
        let falls = self.block(blk, st);
        if falls && self.b.tags.get(blk as usize) == Some(&Tag::Block) {
            self.a.tail(self.b, blk, &mut self.s.arena, st);
        }
        falls
    }

    /// Closes `x`'s label: what its `Break`s carry continues after it.
    fn close(&mut self, x: u32, old: Jumps, first: usize, st: usize, falls: &mut bool) {
        let label = self.topo.label_of[x as usize] as usize;
        if let Some(&[(at, true), _]) = self.s.jumps.get(label) {
            self.s.arena.join(st, falls, at as usize);
        }
        self.s.restore(self.topo, x, old, first);
    }

    /// Instruction `x` forward over `st`; false when it never completes.
    fn inst(&mut self, x: u32, st: usize) -> bool {
        let b = self.b;
        let [p, w] = b.data[x as usize];
        let falls = match b.tags[x as usize] {
            Tag::If => {
                let r = b.record(w);
                let blks = [0, 1].map(|k| r.get(k).copied().unwrap_or(NONE));
                self.enter(x, st).any(blks, st, Self::valued)
            }
            // The right operand may not run.
            Tag::And | Tag::Or => self.enter(x, st).any([NONE, w], st, Self::valued),
            Tag::With => self.enter(x, st).valued(w, st),
            Tag::RefDyn => self.enter(x, st).valued(p, st),
            Tag::Loop => self.enter(x, st).repeat(x, st),
            Tag::Match => self.enter(x, st).decide(x, st),
            Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => {
                let blks = cases(b, w).filter(|&c| b.tags.get(c as usize) == Some(&Tag::Block));
                self.enter(x, st).any(blks, st, Self::valued)
            }
            Tag::Scope => self.enter(x, st).scope(x, st),
            Tag::Guard => {
                let blks = b.record(w).iter().copied();
                return self.enter(x, st).valued(p, st) && self.any(blks, st, Self::block);
            }
            _ => return self.stmt(x, st),
        };
        if falls {
            self.a.leave(b, x, &mut self.s.arena, st);
        }
        falls
    }

    /// The analysis's operand uses of `x`, which runs blocks, before them.
    fn enter(&mut self, x: u32, st: usize) -> &mut Self {
        self.a.enter(self.b, x, &mut self.s.arena, st);
        self
    }

    /// Statement `x` over `st`, and where it goes; false when it never
    /// completes.
    fn stmt(&mut self, x: u32, st: usize) -> bool {
        let (b, s) = (self.b, &mut *self.s);
        self.a.stmt(b, x, &mut s.arena, st);
        let p = b.data[x as usize][0] as usize;
        let tag = b.tags[x as usize];
        match tag {
            Tag::ToArm => {
                if let Some(&(first, count)) = s.arms.last()
                    && p < count as usize
                {
                    let h = s.hit.len() - count as usize + p;
                    s.arena
                        .join(first as usize + p * s.arena.words, &mut s.hit[h], st);
                }
            }
            Tag::Break | Tag::Continue | Tag::Return => {
                // A jump leaves the scopes opened since its target's walk
                // began; a `Return` leaves all of them.
                let from = s.depth.get(p).filter(|_| tag != Tag::Return);
                for &at in s.scopes.iter().skip(from.copied().unwrap_or(0) as usize) {
                    s.arena.union(at as usize, st);
                }
                if tag == Tag::Return {
                    self.a.exit(b, x, &mut s.arena, st);
                    return false;
                }
                if tag == Tag::Continue {
                    let lp = b.label_inst.get(p).copied().unwrap_or(NONE);
                    self.a.back(b, x, lp, &mut s.arena, st);
                }
                let k = usize::from(tag == Tag::Continue);
                if let Some(j) = s.jumps.get_mut(p)
                    && j[k].0 != NONE
                {
                    s.arena.join(j[k].0 as usize, &mut j[k].1, st);
                }
            }
            Tag::Unreachable => {}
            _ => return true,
        }
        false
    }

    /// The paths through blocks `blks`, each from `st`, joined into `st`;
    /// whether a path falls through. A `NONE` block is a path that runs
    /// nothing.
    fn any(
        &mut self,
        blks: impl IntoIterator<Item = u32>,
        st: usize,
        walk: impl Fn(&mut Self, u32, usize) -> bool,
    ) -> bool {
        let from = self.s.arena.alloc(Some(st));
        let at = self.s.arena.alloc(None);
        let mut falls = false;
        for blk in blks {
            self.s.arena.copy(at, from);
            if walk(self, blk, at) {
                self.s.arena.join(st, &mut falls, at);
            }
        }
        self.s.arena.free(from);
        falls
    }

    /// Loop `x` forward: its body from the head until the head stops
    /// growing; what follows it is what its `Break`s carry.
    fn repeat(&mut self, x: u32, st: usize) -> bool {
        let first = self.s.arena.top();
        let head = self.s.arena.alloc(Some(st));
        let cont = self.s.arena.alloc(None);
        let end = self.s.arena.alloc(None);
        let saved = self.s.open(self.topo, x, None, u32_of(cont));
        let label = self.topo.label_of[x as usize] as usize;
        // Each round but the last grows the head, by a bit at least.
        for _ in 0..self.s.arena.words * 64 + 2 {
            // What the jumps carry is the last round's, or every round's
            // `Break`s with [`Forward::ALL_ROUNDS`].
            if let Some(j) = self.s.jumps.get_mut(label) {
                j[0].1 &= A::ALL_ROUNDS;
                j[1].1 = false;
            }
            self.s.arena.copy(end, head);
            let mut grew = false;
            if self.block(self.b.data[x as usize][0], end) {
                self.a.back(self.b, x, x, &mut self.s.arena, end);
                grew = self.s.arena.union(head, end);
            }
            if let Some(&[_, (_, true)]) = self.s.jumps.get(label) {
                grew |= self.s.arena.union(head, cont);
            }
            if !grew {
                break;
            }
        }
        let mut falls = false;
        self.close(x, saved, first, st, &mut falls);
        falls
    }

    /// Match `x` forward: its decision tree from `st`, whose every path
    /// ends in a `ToArm` or `Unreachable`, then each arm a path reaches,
    /// from what its `ToArm`s carry.
    fn decide(&mut self, x: u32, st: usize) -> bool {
        let b = self.b;
        let r = b.record(b.data[x as usize][1]);
        let Some((&decision, arms)) = r.split_first() else {
            return true;
        };
        let first = self.s.arena.top();
        let saved = self.s.open(self.topo, x, None, NONE);
        let arms_at = self.s.arena.top();
        for _ in arms {
            self.s.arena.alloc(None);
        }
        let h = self.s.hit.len();
        self.s.hit.resize(h + arms.len(), false);
        self.s.arms.push((u32_of(arms_at), u32_of(arms.len())));
        self.block(decision, st);
        self.s.arms.pop();
        let mut falls = false;
        for (k, &arm) in arms.iter().enumerate() {
            let at = arms_at + k * self.s.arena.words;
            if self.s.hit[h + k] && self.valued(arm, at) {
                self.s.arena.join(st, &mut falls, at);
            }
        }
        self.s.hit.truncate(h);
        self.close(x, saved, first, st, &mut falls);
        falls
    }

    /// Scope `x`: its body, then each cleanup suite from where
    /// [`Forward::SUITES`] starts it.
    fn scope(&mut self, x: u32, st: usize) -> bool {
        let b = self.b;
        let [body, suites] = b.data[x as usize];
        let exits = matches!(A::SUITES, Suites::FromExits);
        let from = self.s.arena.alloc(None);
        if exits {
            self.s.scopes.push(u32_of(from));
        }
        let falls = self.valued(body, st);
        if exits {
            self.s.scopes.pop();
        }
        if falls {
            self.s.arena.union(from, st);
        }
        if (falls || exits) && suites != NONE {
            let at = self.s.arena.alloc(None);
            self.a.suites(true);
            for &suite in b.record(suites) {
                self.s.arena.copy(at, from);
                if self.block(suite, at) && falls {
                    if matches!(A::SUITES, Suites::FromEnd { union: false }) {
                        self.s.arena.copy(st, at);
                    } else {
                        self.s.arena.union(st, at);
                    }
                }
            }
            self.a.suites(false);
        }
        self.s.arena.free(from);
        falls
    }
}
