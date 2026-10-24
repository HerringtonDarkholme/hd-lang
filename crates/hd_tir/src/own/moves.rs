//! Move (`docs/compiler/tir-v2.md`, "Move"): a `copy` read whose place no
//! path reads again becomes a `move`, which takes the place's own count
//! and leaves it empty, and the `Drop` that then has nothing to release
//! goes.
//!
//! 1. **Locals.** A `copy` read of a local `[$l]`, or of a field of one
//!    (`[$l .a .b]`), after which no path reads `$l` again before a store
//!    replaces all of it, moves. Its local is not addressed and lives in
//!    no cell (a `Ref` or a shared capture reaches it), no cleanup suite
//!    touches it, no `borrow` reads it, and no `Pin` names a place in it:
//!    a pinned collection's copy is a clone (`ref.elem.copy`), which a
//!    move would skip.
//! 2. **Temporaries.** A `copy` of a whole temporary `[%v]` at its last
//!    use moves when the temporary's `Drop` follows it in its block: the
//!    copy is the temporary's count, and the drop goes.
//! 3. **Partial moves.** A `copy` of a field or payload field of a
//!    temporary or a local (`[%v as .Ok .0]`, `[$l .a]`) at the last use
//!    of the base moves, when the part holds a counted leaf (a scalar part
//!    copies for free). When the base's `Drop` follows it in its block,
//!    that drop becomes the drop of the base's one other counted field, or
//!    nothing, provided the base is laid out in place in every instance
//!    ([`OwnCx::parts`]); otherwise, or when more than one counted field
//!    stays beside the path, the drop stays whole and releases what the
//!    move left, the moved part being zero: one call of the base's glue,
//!    where a drop per field would be a release site each. A temporary
//!    whose last uses
//!    are in the `If`, switch or match right before its `Drop` has that
//!    drop sunk into the statement's paths ([`sink`]): each path drops
//!    what its last move left, and a whole copy at a path's end moves.
//! 4. **References at last access.** A `Ref` of a whole local that lives
//!    in a cell, after which no path reaches the local, takes the local's
//!    count of the cell (`ref_word::TAKES`); the local's `Drop` right after
//!    it goes. Not in a sub-body that holds a suspension point (an
//!    `Await`, as known-null's points), whose frame saves the cell's local
//!    at each point.
//!
//! "No path reads it again" is a backward liveness walk over the
//! structured control flow of each sub-body, with one bit per candidate
//! local and temporary, kept at block boundaries and loop heads in one
//! reused arena; a loop runs to a fixed point. A read decides on the
//! state after it. A `Drop` is no read: a drop of a moved place releases
//! nothing, since the backend zeroes what a move empties.
//!
//! Why it is sound: a moved place is read on no path afterwards, so
//! nothing sees it empty; a `Store` after the move refills it and releases
//! the zero it holds; every drop that stays releases what is left, which
//! the move zeroed out of it. A drop that goes is one that only the moved
//! path reaches, straight after the move.

use hd_types::Ty;

use super::flow::{Back, Backward, BitArena, backward};
use super::{Operand, OwnCx, Role, Tri, fill, operands, splice, suite_touches, tracked};
use crate::ir::{
    Body, CaptureMode, NONE, PlaceView, ReadMode, Root, Tag, Topology, for_blocks, is_suspension,
    place, u32_of,
};

/// A decision of [`Tables::decide`].
const UNSEEN: u8 = 0;
const MOVE: u8 = 1;
const KEEP: u8 = 2;
/// A `Ref` of a whole local at its last access, which takes the local's
/// count of its cell.
const TAKE: u8 = 3;

/// The move pass's dense tables, reused across bodies.
#[derive(Default)]
pub(crate) struct Tables {
    /// Each local's and each value's bit among the candidates, or `NONE`.
    pub(super) lnum: Vec<u32>,
    pub(super) vnum: Vec<u32>,
    /// Locals and values that stay out: locals a closure shares, binding
    /// locals, which own nothing, and the places a cleanup suite touches.
    keep_l: Vec<bool>,
    keep_v: Vec<bool>,
    /// Locals a `Ref` addresses, which live in a cell: no read of one
    /// moves, and only a `Ref` of the whole local takes its count.
    pub(super) addr: Vec<bool>,
    /// Per local: the `borrow` read a binding local holds (the borrow
    /// pass's), or `NONE`; and whether the body owns it (its exit row).
    pub(super) holds: Vec<u32>,
    owns: Vec<bool>,
    /// Per sub-body: whether it holds a suspension point (known-null's
    /// points, `is_suspension`).
    susp_sub: Vec<bool>,
    /// The liveness walk's states, one bit per candidate.
    pub(super) back: Back,
    /// Per instruction: [`MOVE`] or [`KEEP`] for a candidate read the walk
    /// reached, from the state after it.
    decide: Vec<u8>,
    /// The instructions that touch each candidate (any read, store, drop,
    /// reference or capture of it), in compressed rows by bit, in index
    /// order.
    start: Vec<u32>,
    fill: Vec<u32>,
    touch: Vec<u32>,
    ops: Vec<Operand>,
    stack: Vec<u32>,
    parts: Vec<Ty>,
    pub(super) drops: Vec<u32>,
    /// A sunk drop's pending edits: drops to insert as (block, position,
    /// place), statements to delete, reads that become moves.
    sink_ins: Vec<(u32, u32, u32)>,
    sink_rm: Vec<u32>,
    sink_mv: Vec<u32>,
}

impl Tables {
    fn reset(&mut self, n: usize, locals: usize) {
        fill(&mut self.lnum, locals, NONE);
        fill(&mut self.vnum, n, NONE);
        fill(&mut self.keep_l, locals, false);
        fill(&mut self.keep_v, n, false);
        fill(&mut self.addr, locals, false);
        fill(&mut self.holds, locals, NONE);
        fill(&mut self.owns, locals, false);
        fill(&mut self.decide, n, UNSEEN);
        self.touch.clear();
        self.stack.clear();
    }

    /// Calls `f` with the bit of each candidate a use of value `v` keeps
    /// live besides `v` itself: a `borrow` read lives on its place, so a
    /// use of it uses the place's root too, and what that root's value
    /// lives on in turn (a borrowed value, a binding local's read).
    pub(super) fn deps(&self, b: &Body, v: u32, f: &mut impl FnMut(u32)) {
        let (lnum, vnum, holds) = (&self.lnum, &self.vnum, &self.holds);
        let mut x = v;
        for _ in 0..16 {
            let i = x as usize;
            if !b.is_borrow(x) {
                return;
            }
            match b.view(b.data[i][0]).root {
                Root::Local(l) => {
                    let li = l as usize;
                    if lnum[li] != NONE {
                        f(lnum[li]);
                    }
                    if holds[li] == NONE {
                        return;
                    }
                    x = holds[li];
                }
                Root::Value(u) => {
                    if vnum[u as usize] != NONE {
                        f(vnum[u as usize]);
                    }
                    x = u;
                }
                _ => return,
            }
        }
    }

    /// [`Self::deps`] of what the place at `word` reads through its root:
    /// the read a binding local at the root holds.
    fn place_deps(&self, b: &Body, word: u32, f: &mut impl FnMut(u32)) {
        if let Some(l) = b.view(word).local()
            && self.holds[l as usize] != NONE
        {
            self.deps(b, self.holds[l as usize], f);
        }
    }

    /// The candidate bit of the root of place `word`, if it has one.
    fn bit(&self, b: &Body, word: u32) -> Option<u32> {
        let x = match b.view(word).root {
            Root::Local(l) => self.lnum.get(l as usize).copied()?,
            Root::Value(v) => self.vnum.get(v as usize).copied()?,
            _ => NONE,
        };
        (x != NONE).then_some(x)
    }

    /// Whether an instruction in `lo..=hi` touches candidate `bit`.
    fn touched(&self, bit: u32, lo: u32, hi: u32) -> bool {
        let row =
            &self.touch[self.start[bit as usize] as usize..self.start[bit as usize + 1] as usize];
        let k = row.partition_point(|&t| t < lo);
        row.get(k).is_some_and(|&t| t <= hi)
    }

    /// The first instruction after `x` that touches candidate `bit`.
    fn next_touch(&self, bit: u32, x: u32) -> Option<u32> {
        let row =
            &self.touch[self.start[bit as usize] as usize..self.start[bit as usize + 1] as usize];
        let k = row.partition_point(|&t| t <= x);
        row.get(k).copied()
    }
}

/// Runs the move pass over every sub-body of `b`.
/// The edits go to `edits` beside the borrow pass's, which still stand in
/// the body: it reads a deleted `Drop` of a borrowed value or of a binding
/// local as a statement that touches no candidate.
pub(crate) fn run(
    b: &mut Body,
    topo: &Topology,
    cx: &dyn OwnCx,
    t: &mut Tables,
    edits: &mut splice::Tables,
) {
    let n = b.len();
    t.reset(n, b.local_ty.len());
    if !candidates(b, topo, cx, t) {
        return;
    }
    touches(b, t);
    let mut back = std::mem::take(&mut t.back);
    backward(b, topo, &mut back, t);
    t.back = back;
    apply(b, topo, cx, t, edits);
}

/// Numbers the candidates: the counted locals no reference, shared
/// capture or cleanup suite reaches and that hold no borrowed value, and
/// the tracked values a `copy` read reads a part of, or all of, in place.
/// Whether there is any.
fn candidates(b: &Body, topo: &Topology, cx: &dyn OwnCx, t: &mut Tables) -> bool {
    let n = b.len();
    for i in 0..n {
        let [a, w] = b.data[i];
        match b.tags[i] {
            Tag::Ref => {
                if let Some(l) = b.local_in_place(a) {
                    t.addr[l.idx()] = true;
                }
            }
            // A binding local holds a borrowed value: it owns no count.
            Tag::Store if b.is_borrow(w) => {
                let p = b.view(a);
                if let Some(l) = p.local().filter(|_| p.whole()) {
                    t.keep_l[l as usize] = true;
                    t.holds[l as usize] = w;
                }
            }
            // A pinned collection's copy is a clone (`ref.elem.copy`), so
            // a read of its local copies even at its last use.
            Tag::Pin => {
                if let Some(l) = b.view(a).local() {
                    t.keep_l[l as usize] = true;
                }
            }
            Tag::Scope if w != NONE => keep_suites(b, t, b.record(w)),
            Tag::Defer => keep_suites(b, t, &[a]),
            _ => {}
        }
    }
    for (l, m) in b.cap_local.iter().zip(&b.cap_mode) {
        if *m == CaptureMode::Shared {
            t.keep_l[l.idx()] = true;
        }
    }
    // A local the body owns is in the exit set; the others (a binding
    // local, a lent parameter, the borrow pass's) hold no count to move.
    for &(_, l) in &b.exit_drops {
        t.owns[l.idx()] = true;
    }
    // A suspending body's frame saves a cell's local at each point, and a
    // resume enters blocks in the middle: its cells keep their counts. A
    // sub-body suspends when it holds a suspension point, the instructions
    // known-null gives unwind sets (`is_suspension`); an owned local's
    // sub-body is its exit row's.
    suspending_subs(b, topo, t);
    for &(s, l) in &b.exit_drops {
        if t.susp_sub[s.idx()] && t.addr[l.idx()] {
            t.keep_l[l.idx()] = true;
        }
    }
    let mut k = 0u32;
    for l in 0..b.local_ty.len() {
        let counted = t.addr[l] || cx.counted(b.local_ty[l]) != Tri::No;
        if !t.keep_l[l] && t.owns[l] && counted {
            t.lnum[l] = k;
            k += 1;
        }
    }
    for i in 0..n {
        let [a, w] = b.data[i];
        if b.tags[i] != Tag::Read || w != ReadMode::Copy as u32 {
            continue;
        }
        let p = b.view(a);
        let Some(v) = p.value().map(|v| v as usize) else {
            continue;
        };
        if t.vnum[v] == NONE && !t.keep_v[v] && p.in_place(b) && tracked(b, cx, v) {
            t.vnum[v] = k;
            k += 1;
        }
    }
    t.back.arena.words = (k as usize).div_ceil(64);
    k > 0
}

/// Marks each sub-body that holds a suspension point: from each point,
/// up through the blocks that hold it to a sub-body's root (a closure's
/// root is listed in its parent's block too, so the climb stops at the
/// first root).
///
/// The climb follows only `blk`, yet reaches the root from a point inside
/// an `if`, `match` or loop body: every child block is listed in the block
/// around the instruction that runs it, as the builder lists each
/// instruction made while a block is open and the inliner lists the
/// copy's root. Issue #828; own/a-reference-after-a-suspension-in-* guard it.
fn suspending_subs(b: &Body, topo: &Topology, t: &mut Tables) {
    fill(&mut t.susp_sub, b.sub_root.len(), false);
    for i in (0..u32_of(b.len())).filter(|&i| is_suspension(b.tags[i as usize])) {
        let x = topo.enclosing(i).last().unwrap_or(i) as usize;
        if topo.is_root[x] {
            t.susp_sub[topo.sub[x] as usize] = true;
        }
    }
}

/// Keeps out every local and value the cleanup suites `roots` touch
/// ([`suite_touches`]).
fn keep_suites(b: &Body, t: &mut Tables, roots: &[u32]) {
    let (keep_l, keep_v) = (&mut t.keep_l, &mut t.keep_v);
    let (local, value) = (
        |l: u32| keep_l[l as usize] = true,
        |v: u32| keep_v[v as usize] = true,
    );
    suite_touches(b, roots, &mut t.stack, &mut t.ops, false, local, value);
}

/// The instructions that touch each candidate, by bit, in index order.
fn touches(b: &Body, t: &mut Tables) {
    let n = b.len();
    let bits = t.back.arena.words * 64;
    fill(&mut t.start, bits + 1, 0);
    let mut ops = std::mem::take(&mut t.ops);
    for pass in 0..2 {
        if pass == 1 {
            for k in 0..bits {
                t.start[k + 1] += t.start[k];
            }
            t.touch.resize(t.start[bits] as usize, 0);
            t.fill.clone_from(&t.start);
        }
        for i in 0..n {
            let hit = |t: &mut Tables, bit: u32| {
                if pass == 0 {
                    t.start[bit as usize + 1] += 1;
                } else {
                    t.touch[t.fill[bit as usize] as usize] = u32_of(i);
                    t.fill[bit as usize] += 1;
                }
            };
            match b.tags[i] {
                Tag::Read | Tag::Store | Tag::Ref | Tag::Drop | Tag::IsUnique | Tag::Pin => {
                    if let Some(l) = b.view(b.data[i][0]).local()
                        && t.lnum[l as usize] != NONE
                    {
                        hit(t, t.lnum[l as usize]);
                    }
                    if b.tags[i] != Tag::Drop {
                        let mut bits = std::mem::take(&mut t.stack);
                        bits.clear();
                        t.place_deps(b, b.data[i][0], &mut |bit| bits.push(bit));
                        for &bit in &bits {
                            hit(t, bit);
                        }
                        t.stack = bits;
                    }
                }
                Tag::Closure => {
                    let rec = b.record(b.data[i][1]);
                    for c in rec[0] as usize..(rec[0] + rec[1]) as usize {
                        let l = b.cap_local[c].idx();
                        if t.lnum[l] != NONE {
                            hit(t, t.lnum[l]);
                        }
                    }
                }
                _ => {}
            }
            operands(b, i, &mut ops);
            for op in &ops {
                let bit = t.vnum[op.value as usize];
                if bit != NONE {
                    hit(t, bit);
                }
                if op.role != Role::Dropped {
                    let mut bits = std::mem::take(&mut t.stack);
                    bits.clear();
                    t.deps(b, op.value, &mut |bit| bits.push(bit));
                    for &bit in &bits {
                        hit(t, bit);
                    }
                    t.stack = bits;
                }
            }
        }
    }
    t.ops = ops;
}

/// The liveness walk ([`backward`]): a read decides on what is live
/// after it.
impl Backward for Tables {
    fn stmt(&mut self, b: &Body, x: u32, s: &mut BitArena, live: usize) {
        let i = x as usize;
        let [a, w] = b.data[i];
        // Its own value is made here: dead before.
        if self.vnum[i] != NONE {
            s.set(live, self.vnum[i], false);
        }
        match b.tags[i] {
            Tag::Read | Tag::Ref | Tag::IsUnique | Tag::Pin => {
                // A read through a binding local reads what its read lives on.
                self.place_deps(b, a, &mut |bit| s.set(live, bit, true));
                if let Some(bit) = self.bit(b, a) {
                    let p = b.view(a);
                    let addr = matches!(p.root, Root::Local(l) if self.addr[l as usize]);
                    let last = if s.get(live, bit) { KEEP } else { MOVE };
                    match b.tags[i] {
                        Tag::Read if w == ReadMode::Copy as u32 && !addr && p.in_place(b) => {
                            self.decide[i] = last;
                        }
                        Tag::Ref if addr && p.whole() => {
                            self.decide[i] = if last == MOVE { TAKE } else { KEEP };
                        }
                        _ => {}
                    }
                    s.set(live, bit, true);
                }
            }
            Tag::Store => {
                let p = b.view(a);
                if let Some(l) = p.local()
                    && self.lnum[l as usize] != NONE
                {
                    // A store into all of the local replaces it; into a
                    // part, it keeps the rest. A store into a cell writes
                    // through the cell the local holds.
                    let keep = !p.whole() || self.addr[l as usize];
                    s.set(live, self.lnum[l as usize], keep);
                }
            }
            Tag::Closure => {
                let rec = b.record(w);
                for c in rec[0] as usize..(rec[0] + rec[1]) as usize {
                    let l = b.cap_local[c].idx();
                    if self.lnum[l] != NONE {
                        s.set(live, self.lnum[l], true);
                    }
                }
            }
            _ => {}
        }
    }

    /// A use of value `v`: it and what it lives on ([`Tables::deps`]).
    fn uses(&mut self, b: &Body, v: u32, s: &mut BitArena, live: usize) {
        if let Some(&bit) = self.vnum.get(v as usize)
            && bit != NONE
        {
            s.set(live, bit, true);
        }
        self.deps(b, v, &mut |bit| s.set(live, bit, true));
    }
}

/// Turns each read the walk proved last into a `move`, and deletes or
/// splits the `Drop` of its place that follows it in its block.
fn apply(
    b: &mut Body,
    topo: &Topology,
    cx: &dyn OwnCx,
    t: &mut Tables,
    edits: &mut splice::Tables,
) {
    for i in 0..b.len() {
        if t.decide[i] == TAKE && topo.blk[i] != NONE {
            take(b, topo, t, edits, i);
            continue;
        }
        if t.decide[i] != MOVE || topo.blk[i] == NONE {
            continue;
        }
        let x = u32_of(i);
        let word = b.data[i][0];
        let PlaceView { root, from, to } = b.view(word);
        let Some(bit) = t.bit(b, word) else {
            continue;
        };
        // The drop of the whole place right after the read, in its block.
        let drop = t.next_touch(bit, x).filter(|&d| {
            let d = d as usize;
            if b.tags[d] != Tag::Drop || topo.blk[d] != topo.blk[i] {
                return false;
            }
            let dw = b.data[d][0];
            match root {
                Root::Local(l) => dw == l | place::BARE_LOCAL,
                Root::Value(v) => drops_whole(b, u32_of(d), v),
                _ => false,
            }
        });
        if from == to {
            // A whole value moves only when its drop goes with it: no
            // other drop of it may follow on the moved path.
            if drop.is_none() && matches!(root, Root::Value(_)) {
                continue;
            }
            b.data[i][1] = ReadMode::Move as u32;
            if let Some(d) = drop {
                edits.remove(d);
            }
            continue;
        }
        // A part with no counted leaf copies for free: moving it would only
        // split the base's drop into one drop per other counted part.
        if cx.counted(b.ty[i]) == Tri::No {
            continue;
        }
        b.data[i][1] = ReadMode::Move as u32;
        let Some(d) = drop else {
            continue;
        };
        let base = match root {
            Root::Local(l) => b.local_ty[l as usize],
            Root::Value(v) => b.ty[v as usize],
            _ => continue,
        };
        if rest(b, cx, t, base, (from, from, to)) {
            let (blk, pos, syn) = (
                topo.blk[d as usize],
                topo.pos[d as usize],
                b.syn[d as usize],
            );
            edits.remove(d);
            for k in 0..t.drops.len() {
                let at = t.drops[k];
                edits.insert(blk, pos, Tag::Drop, [at, NONE], Ty::VOID, syn);
            }
        }
    }
    for v in 0..b.len() {
        if t.vnum[v] != NONE {
            sink(b, topo, cx, t, edits, u32_of(v));
        }
    }
}

/// Whether instruction `d` is a `Drop` of all of value `v`.
fn drops_whole(b: &Body, d: u32, v: u32) -> bool {
    let p = b.view(b.data[d as usize][0]);
    b.tags[d as usize] == Tag::Drop && p.whole() && p.root == Root::Value(v)
}

/// Rule 3 across branches: temporary `v`'s `Drop` right after the `If`,
/// switch or match that holds its last uses goes into that statement's
/// paths, where move's reads took parts of it: at the end of each branch
/// that completes, and before each `ToArm` of a match's decision tree, a
/// drop of what the path's last move left (nothing when it took every
/// counted field), or of all of `v` where no move took a part. A whole
/// copy at the path's end then moves too. A match's arms touch no part of
/// `v` but its drops before their jumps, which go. Done only when it adds
/// no more drops than it deletes, and never through a `Guard`, a loop or
/// a nested match, whose paths leave the decision tree's shape.
fn sink(
    b: &mut Body,
    topo: &Topology,
    cx: &dyn OwnCx,
    t: &mut Tables,
    edits: &mut splice::Tables,
    v: u32,
) {
    let bit = t.vnum[v as usize];
    let (lo, hi) = (
        t.start[bit as usize] as usize,
        t.start[bit as usize + 1] as usize,
    );
    if hi - lo < 2 {
        return;
    }
    let d = t.touch[hi - 1];
    if !drops_whole(b, d, v) || topo.blk[d as usize] == NONE {
        return;
    }
    // The statement before the drop, past other statements' drops.
    let (h, pd) = (topo.blk[d as usize], topo.pos[d as usize]);
    let members = b.record(b.data[h as usize][0]);
    let Some(&x) = members[..pd as usize]
        .iter()
        .rev()
        .find(|&&m| b.tags[m as usize] != Tag::Drop)
    else {
        return;
    };
    let prev = t.touch[hi - 2];
    if prev < topo.first[x as usize] || prev > x || b.ty[x as usize] == Ty::NEVER {
        return;
    }
    t.sink_ins.clear();
    t.sink_rm.clear();
    t.sink_mv.clear();
    let [_, w] = b.data[x as usize];
    let ok = match b.tags[x as usize] {
        Tag::If => {
            let r = b.record(w);
            r.len() == 2 && {
                let (then, els) = (r[0], r[1]);
                sink_block(b, topo, cx, t, v, then) && sink_block(b, topo, cx, t, v, els)
            }
        }
        Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => {
            let r = b.record(w).to_vec();
            let (cases, default) = r.split_at(r.len().saturating_sub(1));
            cases
                .iter()
                .skip(1)
                .step_by(2)
                .chain(default)
                .all(|&blk| sink_block(b, topo, cx, t, v, blk))
        }
        Tag::Match => {
            let r = b.record(w).to_vec();
            let Some((&decision, arms)) = r.split_first() else {
                return;
            };
            sink_block(b, topo, cx, t, v, decision)
                && arms.iter().all(|&arm| {
                    let (a0, a1) = (topo.first[arm as usize] as usize, arm as usize);
                    t.touch[lo..hi]
                        .iter()
                        .filter(|&&u| (a0..=a1).contains(&(u as usize)))
                        .all(|&u| {
                            let whole = drops_whole(b, u, v);
                            if whole {
                                t.sink_rm.push(u);
                            }
                            whole
                        })
                })
        }
        _ => false,
    };
    if !ok || t.sink_ins.len() > 1 + t.sink_rm.len() {
        return;
    }
    edits.remove(d);
    for &u in &t.sink_rm {
        edits.remove(u);
    }
    for &(blk, pos, at) in &t.sink_ins {
        edits.insert(blk, pos, Tag::Drop, [at, NONE], Ty::VOID, b.syn[d as usize]);
    }
    for &m in &t.sink_mv {
        b.data[m as usize][1] = ReadMode::Move as u32;
    }
}

/// The drops of temporary `v` at the end of block `blk`'s paths, into
/// `t.sink_*`: false when the block's shape is not one [`sink`] handles.
fn sink_block(
    b: &mut Body,
    topo: &Topology,
    cx: &dyn OwnCx,
    t: &mut Tables,
    v: u32,
    blk: u32,
) -> bool {
    if b.tags.get(blk as usize) != Some(&Tag::Block) || b.data[blk as usize][1] == v {
        return false;
    }
    let bit = t.vnum[v as usize];
    let rec = b.data[blk as usize][0] as usize;
    let len = b.extra[rec];
    let mut end = len;
    // The block's last statement but drops: a jump, whose path has its own
    // drops before it; a match decision's `ToArm`, before which the path's
    // drops go; or a statement that runs blocks and never completes (a
    // decision tree's switch or guard), whose paths the drops must go into.
    let mut never = None;
    if let Some(p) = (0..len)
        .rev()
        .find(|&p| b.tags[b.extra[rec + 1 + p as usize] as usize] != Tag::Drop)
    {
        let m = b.extra[rec + 1 + p as usize];
        match b.tags[m as usize] {
            Tag::Return | Tag::Break | Tag::Continue | Tag::Unreachable => return true,
            Tag::ToArm => end = p,
            _ if b.ty[m as usize] == Ty::NEVER => {
                let mut runs = false;
                for_blocks(b, m as usize, |_| runs = true);
                if runs {
                    never = Some(m);
                }
            }
            _ => {}
        }
    }
    let whole = |b: &mut Body, t: &mut Tables| {
        let at = u32_of(b.extra.len());
        b.extra.extend([2, place::ROOT_VALUE, v]);
        t.sink_ins.push((blk, end, at));
    };
    for p in (0..end).rev() {
        let m = b.extra[rec + 1 + p as usize];
        if !t.touched(bit, topo.first[m as usize], m) {
            continue;
        }
        // The block's end is never reached: only the statement that ends
        // every path can hold the drops.
        if never.is_some_and(|n| n != m) {
            return false;
        }
        let [a, w] = b.data[m as usize];
        match b.tags[m as usize] {
            Tag::Read if b.view(a).root == Root::Value(v) => {
                let PlaceView { from, to, .. } = b.view(a);
                if from == to {
                    // A whole copy at the path's end moves, as a copy with
                    // its drop right after it does.
                    if t.decide[m as usize] == MOVE {
                        t.sink_mv.push(m);
                    } else {
                        whole(b, t);
                    }
                } else if w == ReadMode::Move as u32 {
                    let base = b.ty[v as usize];
                    if rest(b, cx, t, base, (from, from, to)) {
                        for k in 0..t.drops.len() {
                            t.sink_ins.push((blk, end, t.drops[k]));
                        }
                    } else {
                        whole(b, t);
                    }
                } else {
                    whole(b, t);
                }
                return true;
            }
            Tag::If => {
                let r = b.record(w).to_vec();
                if r.len() != 2 {
                    whole(b, t);
                    return true;
                }
                return sink_block(b, topo, cx, t, v, r[0]) && sink_block(b, topo, cx, t, v, r[1]);
            }
            Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => {
                let r = b.record(w).to_vec();
                let (cases, default) = r.split_at(r.len().saturating_sub(1));
                return cases
                    .iter()
                    .skip(1)
                    .step_by(2)
                    .chain(default)
                    .all(|&c| sink_block(b, topo, cx, t, v, c));
            }
            Tag::Guard
            | Tag::Match
            | Tag::Loop
            | Tag::Scope
            | Tag::With
            | Tag::And
            | Tag::Or
            | Tag::RefDyn
            | Tag::Drop
            | Tag::Break
            | Tag::Continue
            | Tag::Return
            | Tag::ToArm => return false,
            _ => {
                whole(b, t);
                return true;
            }
        }
    }
    if never.is_some() {
        return false;
    }
    whole(b, t);
    true
}

/// Rule 4: `Ref` `i` of a whole local at its last access takes the
/// local's count of its cell, and the local's `Drop` right after it in its
/// block goes.
fn take(b: &mut Body, topo: &Topology, t: &Tables, edits: &mut splice::Tables, i: usize) {
    let Some(l) = b.view(b.data[i][0]).local() else {
        return;
    };
    b.data[i][1] |= crate::ir::ref_word::TAKES;
    if let Some(d) = t.next_touch(t.lnum[l as usize], u32_of(i))
        && b.tags[d as usize] == Tag::Drop
        && topo.blk[d as usize] == topo.blk[i]
        && b.data[d as usize][0] == l | place::BARE_LOCAL
    {
        edits.remove(d);
    }
}

/// The drops of what a base of type `base` holds besides the part at the
/// projection words `from..to` of its place, into `t.drops` as new place
/// records: each counted field beside the path at each step from the
/// step at word `start` on (`from` for all of them). False when a step's
/// type is not laid out in place in every instance ([`OwnCx::parts`]).
pub(super) fn rest(
    b: &mut Body,
    cx: &dyn OwnCx,
    t: &mut Tables,
    base: Ty,
    (from, start, to): (u32, u32, u32),
) -> bool {
    t.drops.clear();
    let head = from - 2;
    let mut ty = base;
    let mut x = from;
    while x < to {
        let w = b.extra[x as usize];
        let (variant, field, step_end) = if w & place::PROJ_KIND == place::PROJ_VARIANT {
            let Some(&f) = b.extra.get(x as usize + 1).filter(|_| x + 1 < to) else {
                return false;
            };
            (Some(w & place::PROJ_INDEX), f & place::PROJ_INDEX, x + 2)
        } else {
            (None, w & place::PROJ_INDEX, x + 1)
        };
        if !cx.parts(ty, variant, &mut t.parts) {
            return false;
        }
        let Some(&next) = t.parts.get(field as usize) else {
            return false;
        };
        for k in 0..t.parts.len() {
            if x < start || u32_of(k) == field || cx.counted(t.parts[k]) == Tri::No {
                continue;
            }
            // `[root path-so-far (as v) .k]`
            let at = u32_of(b.extra.len());
            let len = (x - head) + u32::from(variant.is_some()) + 1;
            b.extra.push(len);
            b.extra.extend_from_within(head as usize..x as usize);
            if let Some(v) = variant {
                b.extra.push(place::PROJ_VARIANT | v);
            }
            b.extra.push(place::PROJ_FIELD | u32_of(k));
            t.drops.push(at);
        }
        ty = next;
        x = step_end;
    }
    // More than one part beside the path: one drop of the whole base
    // releases them in one call of its glue, the moved part being zero,
    // where a drop per part is a release site each.
    if t.drops.len() > 1 {
        b.extra.truncate(t.drops[0] as usize);
        t.drops.clear();
        return false;
    }
    true
}
