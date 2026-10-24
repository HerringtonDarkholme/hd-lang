//! Known-null (`docs/compiler/tir-v2.md`, "Known-null"): a forward walk
//! over the structured control flow that tracks, per owned local and per
//! owned value, whether it may hold a count. A place is empty before its
//! first store, after a `move` of all of it, after a `Drop` of all of it,
//! after the use that consumes it, and once the parts a `move` or a `Drop`
//! took of it on the path cover every counted leaf ([`OwnCx::parts`]); it
//! may hold a count after a store or its definition. States join by union
//! at the ends of branches and arms, and a loop's head runs to a fixed
//! point.
//!
//! The plan's rules 1 and 3, and the unwind set:
//!
//! - **Rule 1, drops.** A `Drop` whose place's root is empty on every
//!   path that reaches it goes: a local moved, dropped or never stored, as
//!   the drops after moved locals in branches and loops that the move pass
//!   leaves (#677).
//! - **Rule 3, the exit set.** A local empty at every way out of its
//!   sub-body leaves the exit set: each `Return`, the end of its root
//!   block, and each suspension point, whose cancellation leaves through
//!   the exit, which releases the exit set. A parameter and a capture keep
//!   their rows: the backend reads one outside the exit set as unowned.
//! - **The unwind set.** Each suspension point gets a row per owned value
//!   that may hold a count there (`Body::unwind_drops`): its cancellation
//!   is a way out that no TIR jump spells, so the backend releases those
//!   values there, and the locals at the exit.
//!
//! - **Rule 5, cells made before use.** An addressed local (a `Ref` of
//!   its own storage, or a shared capture, puts it in a cell) gets a second
//!   bit, set where it may hold no value: before its first store in its
//!   sub-body, after a `Drop` of it, a `move` read of it and a `Ref` that
//!   takes its count; a store into its own storage clears it. Each
//!   instruction that reads the local's storage (a `Read`, `Store`, `Ref`,
//!   `IsUnique` or `Pin` of a place rooted at it, a `Closure` that captures
//!   it) and each `Loop` (where the backend reads stable places before the
//!   loop) gets a row in `Body::full_cells` when the bit is clear on every
//!   path that reaches it: there the backend reads the cell with no null
//!   test (#678). A use inside a cleanup suite gets none, since a suite
//!   also runs at jumps the walk does not follow there.
//!
//! The plan's rules 2 and 4 (a store into an empty place, a move with no
//! zeroing) are not built here (`docs/compiler/tir-v2.md`, stage 2's
//! commit 10).
//!
//! A place a cleanup suite touches is never empty to the walk (its local
//! keeps every drop and its exit row, its value stays in every later
//! unwind row): a suite runs at its scope's exits, which the walk does
//! not follow.
//!
//! Why it is sound: a drop, an exit row or an unwind row goes only where
//! no path reaches it with a count in its place; any other state keeps
//! it, and a place that is empty there holds zero, which releases as
//! nothing. A rule 5 row stands only where every path that reaches it
//! stored into the local after the last `Drop`, move or taking `Ref` of
//! it, and each store leaves the backend's cell made.
//!
//! The walk reads the body as the earlier passes' pending edits leave it
//! ([`splice::Pending`]): a deleted statement is gone and a new `Drop`
//! stands in its place. One bit per tracked local and value and per part
//! place taken, states in one arena, allocated and freed last in, first
//! out; per-item flags in dense tables. Its deletions join the pipeline's
//! last rebuild.

use super::flow::BitArena;
use super::parts::{Slots, takes};
use super::splice;
use super::{Loc, Operand, OwnCx, Role, Tri, consumes, fill, operands, suite_touches, tracked};

use crate::ir::{
    Body, CaptureMode, NONE, ReadMode, Ref, Root, Tag, Topology, is_suspension, local_flags,
    ref_word, u32_of,
};

/// Known-null's dense tables, reused across bodies.
#[derive(Default)]
pub(crate) struct Tables {
    pending: splice::Pending,
    /// Each local's and each value's bit, or `NONE`; the local of each
    /// local bit, and the value of each value bit, from `nl` on.
    lbit: Vec<u32>,
    vbit: Vec<u32>,
    locs: Vec<u32>,
    vals: Vec<u32>,
    nl: u32,
    nv: u32,
    /// The parts a `move` read or a `Drop` takes of a tracked place, keyed
    /// by root bit, with items the instructions, then the edits. A slot's
    /// bit follows the values' bits; a root whose taken parts cover every
    /// counted leaf is empty.
    slots: Slots,
    /// Per bit: a place a cleanup suite touches, which nothing empties.
    sticky: Vec<bool>,
    /// Locals and values a cleanup suite touches.
    pin_l: Vec<bool>,
    pin_v: Vec<bool>,
    /// Locals that hold a value where their sub-body starts: parameters
    /// and captures.
    entry: Vec<bool>,
    /// Per value: a `borrow` read with a `Drop` (a kept drop, which an
    /// instance may make release).
    dropped: Vec<bool>,
    /// Per label: the arena offset of the state its `Break`s (or
    /// `Continue`s) carry, and whether one reached it.
    brk: Vec<(u32, bool)>,
    cont: Vec<(u32, bool)>,
    /// The open matches: their arms' first state, count, and first flag in
    /// `hit`.
    arms: Vec<(u32, u32, u32)>,
    hit: Vec<bool>,
    arena: BitArena,
    /// The sub-body's exit state, while it is walked.
    exit: usize,
    /// Per item (instructions, then edits): a `Drop` the walk reached, and
    /// one it reached where its place may hold a count.
    seen: Vec<bool>,
    needed: Vec<bool>,
    /// Per instruction: its suspension state's offset in `susp`, or `NONE`;
    /// the suspension points reached.
    susp_at: Vec<u32>,
    susp: Vec<u64>,
    points: Vec<u32>,
    /// Per exit row: whether it stays.
    keep: Vec<bool>,
    stack: Vec<u32>,
    ops: Vec<Operand>,
    /// Rule 5: each addressed local's bit, or `NONE`, set where the local
    /// may hold no value, after the parts' bits from `cfirst`; the local
    /// of each.
    cbit: Vec<u32>,
    cfirst: u32,
    clocs: Vec<u32>,
    /// How many cleanup suites the walk is inside: no rule 5 row there.
    suites: u32,
    /// Rule 5's visits: an instruction, an addressed local, and whether
    /// the local certainly held its value there.
    cells: Vec<(u32, u32, bool)>,
}

/// Runs known-null over `b` with the earlier passes' edits in `edits`,
/// adding its deletions there, and fills `b.unwind_drops` and
/// `b.full_cells` and trims `b.exit_drops`.
pub(crate) fn run(
    b: &mut Body,
    topo: &Topology,
    cx: &dyn OwnCx,
    t: &mut Tables,
    edits: &mut splice::Tables,
) {
    let n = b.len();
    b.unwind_drops.clear();
    b.full_cells.clear();
    t.cells.clear();
    t.suites = 0;
    t.pending.build(n, edits);
    if !number(b, cx, t, edits) {
        return;
    }
    let k = edits.len();
    fill(&mut t.seen, n + k, false);
    fill(&mut t.needed, n + k, false);
    fill(&mut t.susp_at, n, NONE);
    fill(&mut t.brk, b.label_inst.len(), (NONE, false));
    fill(&mut t.cont, b.label_inst.len(), (NONE, false));
    fill(&mut t.keep, b.exit_drops.len(), true);
    t.susp.clear();
    t.points.clear();
    t.arms.clear();
    t.hit.clear();
    t.arena.free(0);
    for s in 0..b.sub_root.len() {
        let root = b.sub_root[s];
        if root as usize >= n {
            continue;
        }
        t.exit = t.arena.alloc(None);
        let st = t.arena.alloc(None);
        for l in 0..b.local_ty.len() {
            if t.entry[l] && t.lbit[l] != NONE {
                t.arena.set(st, t.lbit[l], true);
            }
        }
        // An addressed local holds no value where its sub-body starts:
        // the backend makes an entry's cell in the prologue itself.
        for c in 0..u32_of(t.clocs.len()) {
            t.arena.set(st, t.cfirst + c, true);
        }
        if t.block(b, topo, cx, edits, root, st) {
            t.tail(b, topo, cx, root, st);
            t.arena.union(t.exit, st);
        }
        for (k, &(sub, l)) in b.exit_drops.iter().enumerate() {
            let bit = t.lbit[l.idx()];
            if sub.idx() == s && bit != NONE && !t.entry[l.idx()] && !t.arena.get(t.exit, bit) {
                t.keep[k] = false;
            }
        }
        t.arena.free(t.exit);
    }
    // Rule 1: the drops no path reaches with a count.
    for x in 0..n {
        if t.seen[x] && !t.needed[x] {
            edits.remove(u32_of(x));
        }
    }
    for e in 0..k {
        if t.seen[n + e] && !t.needed[n + e] {
            edits.cancel(u32_of(e));
        }
    }
    // Rule 3: the exit rows of locals empty at every way out.
    let mut k = 0;
    let keep = &t.keep;
    b.exit_drops.retain(|_| {
        k += 1;
        keep[k - 1]
    });
    // Rule 5: the reads that found the local full on every visit.
    t.cells.sort_unstable();
    let mut x = 0;
    while x < t.cells.len() {
        let (at, l, _) = t.cells[x];
        let mut full = true;
        while x < t.cells.len() && t.cells[x].0 == at && t.cells[x].1 == l {
            full &= t.cells[x].2;
            x += 1;
        }
        if full {
            b.full_cells.push((at, l));
        }
    }
    // The unwind set: each suspension point's values that may hold a
    // count.
    t.points.sort_unstable();
    for &x in &t.points {
        let at = t.susp_at[x as usize] as usize;
        for bit in t.nl as usize..(t.nl + t.nv) as usize {
            if t.susp[at + bit / 64] & (1 << (bit % 64)) != 0 {
                b.unwind_drops.push((x, t.vals[bit - t.nl as usize]));
            }
        }
    }
}

/// Numbers the tracked places: the locals in the exit set that no cleanup
/// suite touches, then the owned values and the `borrow` reads with a
/// `Drop`. Whether there is any.
fn number(b: &Body, cx: &dyn OwnCx, t: &mut Tables, edits: &splice::Tables) -> bool {
    let n = b.len();
    let locals = b.local_ty.len();
    fill(&mut t.lbit, locals, NONE);
    fill(&mut t.vbit, n, NONE);
    fill(&mut t.pin_l, locals, false);
    fill(&mut t.pin_v, n, false);
    fill(&mut t.entry, locals, false);
    fill(&mut t.dropped, n, false);
    t.locs.clear();
    t.vals.clear();
    t.sticky.clear();
    for i in 0..n {
        let [a, w] = b.data[i];
        match b.tags[i] {
            Tag::Scope if w != NONE => pin_suites(b, t, b.record(w)),
            Tag::Defer => pin_suites(b, t, &[a]),
            Tag::Drop if !t.pending.dead[i] => mark_dropped(b, t, a),
            _ => {}
        }
    }
    for e in 0..edits.len() {
        let (_, _, tag, [word, _], _) = edits.get(u32_of(e));
        if tag == Tag::Drop && !edits.is_cancelled(u32_of(e)) {
            mark_dropped(b, t, word);
        }
    }
    for (l, flags) in b.local_flags.iter().enumerate() {
        if flags & local_flags::PARAM != 0 {
            t.entry[l] = true;
        }
    }
    for r in &b.sub_params {
        for k in 0..r.len {
            if let Some(x) = t
                .entry
                .get_mut(b.extra[(r.start + 1 + k) as usize] as usize)
            {
                *x = true;
            }
        }
    }
    for (l, m) in b.cap_local.iter().zip(&b.cap_mode) {
        t.entry[l.idx()] = true;
        if *m == CaptureMode::Shared {
            t.pin_l[l.idx()] = true;
        }
    }
    let mut k = 0u32;
    for &(_, l) in &b.exit_drops {
        let l = l.idx();
        if t.lbit[l] == NONE && !t.pin_l[l] {
            t.lbit[l] = k;
            t.locs.push(u32_of(l));
            t.sticky.push(false);
            k += 1;
        }
    }
    t.nl = k;
    for v in 0..n {
        let owned = if b.is_borrow(u32_of(v)) {
            t.dropped[v] && cx.counted(b.ty[v]) != Tri::No
        } else {
            tracked(b, cx, v)
        };
        if owned {
            t.vbit[v] = k;
            t.vals.push(u32_of(v));
            t.sticky.push(t.pin_v[v]);
            k += 1;
        }
    }
    t.nv = k - t.nl;
    // Rule 5's addressed locals.
    fill(&mut t.cbit, locals, NONE);
    t.clocs.clear();
    for i in 0..n {
        if b.tags[i] == Tag::Ref
            && !t.pending.dead[i]
            && let Some(l) = b.local_in_place(b.data[i][0])
        {
            t.mark_cell(l.raw());
        }
    }
    for (l, m) in b.cap_local.iter().zip(&b.cap_mode) {
        if *m == CaptureMode::Shared {
            t.mark_cell(l.raw());
        }
    }
    let nc = u32_of(t.clocs.len());
    if k == 0 && nc == 0 {
        return false;
    }
    let slots = number_parts(b, t, edits);
    t.cfirst = k + slots;
    for c in &mut t.cbit {
        if *c != NONE {
            *c += t.cfirst;
        }
    }
    t.arena.words = ((k + slots + nc) as usize).div_ceil(64);
    true
}

/// Numbers the part slots: each distinct part place of a tracked root
/// that a `move` read or a `Drop` takes. Returns how many.
fn number_parts(b: &Body, t: &mut Tables, edits: &splice::Tables) -> u32 {
    let (n, k) = (b.len(), edits.len());
    let mut slots = std::mem::take(&mut t.slots);
    let insts = takes(b, |i| !t.pending.dead[i]);
    let added = (0..k).filter_map(|e| {
        let (_, _, tag, [word, _], _) = edits.get(u32_of(e));
        (tag == Tag::Drop && !edits.is_cancelled(u32_of(e))).then_some((n + e, word))
    });
    let takes = insts
        .chain(added)
        .filter_map(|(item, word)| match t.root_bit(b, word) {
            Some((bit, false)) if b.view(word).in_place(b) => Some((bit, u32_of(item), word)),
            _ => None,
        });
    let count = slots.build(b, n + k, (t.nl + t.nv) as usize, takes);
    t.slots = slots;
    count
}

/// Marks the value a `Drop` of all of place `word` drops, if it is one.
fn mark_dropped(b: &Body, t: &mut Tables, word: u32) {
    let p = b.view(word);
    if let Some(v) = p.value().filter(|_| p.whole()) {
        t.dropped[v as usize] = true;
    }
}

/// Pins every local and value the cleanup suites `roots` touch, and each
/// value they compute ([`suite_touches`]).
fn pin_suites(b: &Body, t: &mut Tables, roots: &[u32]) {
    let (pin_l, pin_v) = (&mut t.pin_l, &mut t.pin_v);
    let (local, value) = (
        |l: u32| pin_l[l as usize] = true,
        |v: u32| pin_v[v as usize] = true,
    );
    suite_touches(b, roots, &mut t.stack, &mut t.ops, true, local, value);
}

impl Tables {
    /// Empties `bit`, unless a cleanup suite touches its place.
    fn clear(&mut self, at: usize, bit: u32) {
        if !self.sticky[bit as usize] {
            self.arena.set(at, bit, false);
        }
    }

    /// Rule 5: local `l` is addressed, and gets a bit.
    fn mark_cell(&mut self, l: u32) {
        if let Some(c) = self.cbit.get_mut(l as usize)
            && *c == NONE
        {
            *c = u32_of(self.clocs.len());
            self.clocs.push(l);
        }
    }

    /// Rule 5: a visit of instruction `x`, which reads addressed local
    /// `l`'s storage or, as a `Loop`, runs before such reads.
    fn cell_visit(&mut self, x: u32, l: u32, st: usize) {
        let c = self.cbit.get(l as usize).copied().unwrap_or(NONE);
        if c != NONE && self.suites == 0 {
            let full = !self.arena.get(st, c);
            self.cells.push((x, l, full));
        }
    }

    /// Rule 5: addressed local `l` may hold no value (`empty`), or holds
    /// its value, from here.
    fn cell_mark(&mut self, l: u32, st: usize, empty: bool) {
        let c = self.cbit.get(l as usize).copied().unwrap_or(NONE);
        if c != NONE {
            self.arena.set(st, c, empty);
        }
    }

    /// The bit of the root of place `word`, and whether the place is all
    /// of it.
    fn root_bit(&self, b: &Body, word: u32) -> Option<(u32, bool)> {
        let p = b.view(word);
        let bit = match p.root {
            Root::Local(l) => self.lbit[l as usize],
            Root::Value(v) => self.vbit[v as usize],
            _ => NONE,
        };
        (bit != NONE).then_some((bit, p.whole()))
    }

    /// A `Drop` item (an instruction, or `n + edit`) of place `word`: it
    /// is needed where its root may hold a count; it empties all of it, or
    /// takes a part.
    fn drop_item(&mut self, b: &Body, cx: &dyn OwnCx, item: usize, word: u32, st: usize) {
        if let Some(l) = b.view(word).local() {
            self.cell_mark(l, st, true);
        }
        let Some((bit, whole)) = self.root_bit(b, word) else {
            return;
        };
        self.seen[item] = true;
        if self.arena.get(st, bit) || self.sticky[bit as usize] {
            self.needed[item] = true;
        }
        if whole {
            self.clear(st, bit);
        } else {
            self.take(b, cx, item, bit, st);
        }
    }

    /// Item `item` takes a part of the place whose root has `bit`: the
    /// root is empty once the parts taken on the path cover every counted
    /// leaf.
    fn take(&mut self, b: &Body, cx: &dyn OwnCx, item: usize, bit: u32, st: usize) {
        let slot = self.slots.of[item];
        if slot == NONE {
            return;
        }
        let first = self.nl + self.nv;
        self.arena.set(st, first + slot, true);
        let base = if bit < self.nl {
            b.local_ty[self.locs[bit as usize] as usize]
        } else {
            b.ty[self.vals[(bit - self.nl) as usize] as usize]
        };
        let arena = &self.arena;
        if !self
            .slots
            .rest(b, cx, bit, base, |s, _| arena.get(st, first + s))
        {
            self.clear(st, bit);
        }
    }

    /// The root with `bit` holds all of its value again: no part taken.
    fn refill(&mut self, bit: u32, st: usize) {
        let first = self.nl + self.nv;
        for s in self.slots.of_key(bit) {
            self.arena.set(st, first + s, false);
        }
        self.arena.set(st, bit, true);
    }

    /// Walks block `blk` forward over state `st`; false when no path falls
    /// out of its end.
    fn block(
        &mut self,
        b: &Body,
        topo: &Topology,
        cx: &dyn OwnCx,
        edits: &splice::Tables,
        blk: u32,
        st: usize,
    ) -> bool {
        if b.tags.get(blk as usize) != Some(&Tag::Block) {
            return true;
        }
        let n = b.len();
        let label = topo.label_of[blk as usize];
        let saved = (label != NONE).then(|| self.brk[label as usize]);
        let slot = self.arena.top();
        if label != NONE {
            let at = self.arena.alloc(None);
            self.brk[label as usize] = (u32_of(at), false);
        }
        let rec = b.data[blk as usize][0] as usize;
        let len = b.extra[rec] as usize;
        let mut y = 0;
        let mut falls = true;
        for p in 0..=len {
            while falls && y < self.pending.of(blk).len() {
                let e = self.pending.of(blk)[y];
                let (_, pos, tag, [word, _], _) = edits.get(e);
                if pos as usize > p {
                    break;
                }
                if tag == Tag::Drop {
                    self.drop_item(b, cx, n + e as usize, word, st);
                }
                y += 1;
            }
            if !falls || p == len {
                break;
            }
            let m = b.extra[rec + 1 + p];
            // A block in the list runs where the instruction that owns it
            // runs it.
            if self.pending.dead[m as usize] || b.tags[m as usize] == Tag::Block {
                continue;
            }
            falls = self.inst(b, topo, cx, edits, m, st);
        }
        if let Some(old) = saved {
            let (at, reached) = self.brk[label as usize];
            if reached {
                self.arena.join(st, &mut falls, at as usize);
            }
            self.brk[label as usize] = old;
            self.arena.free(slot);
        }
        falls
    }

    /// Walks block `blk` and takes its tail.
    fn valued(
        &mut self,
        b: &Body,
        topo: &Topology,
        cx: &dyn OwnCx,
        edits: &splice::Tables,
        blk: u32,
        st: usize,
    ) -> bool {
        let falls = self.block(b, topo, cx, edits, blk, st);
        if falls && b.tags.get(blk as usize) == Some(&Tag::Block) {
            self.tail(b, topo, cx, blk, st);
        }
        falls
    }

    /// Block `blk`'s tail, which its owner's value takes when the owner
    /// uses it.
    fn tail(&mut self, b: &Body, topo: &Topology, cx: &dyn OwnCx, blk: u32, st: usize) {
        let w = b.data[blk as usize][1];
        if w == NONE || Ref(w).as_inst().is_none() || w as usize >= b.len() {
            return;
        }
        let bit = self.vbit[w as usize];
        if bit == NONE {
            return;
        }
        let owner = if topo.is_root[blk as usize] {
            NONE
        } else {
            topo.owner[blk as usize]
        };
        let op = Operand {
            value: w,
            role: Role::Tail,
            at: Loc::Data(1),
        };
        if consumes(b, cx, blk as usize, &op, owner) {
            self.clear(st, bit);
        }
    }

    /// The operand uses of control instruction `x`, before its blocks.
    fn uses_only(&mut self, b: &Body, cx: &dyn OwnCx, x: u32, st: usize) {
        let mut ops = std::mem::take(&mut self.ops);
        operands(b, x as usize, &mut ops);
        for op in &ops {
            let bit = self.vbit[op.value as usize];
            if bit != NONE && consumes(b, cx, x as usize, op, NONE) {
                self.clear(st, bit);
            }
        }
        self.ops = ops;
    }

    /// Control instruction `x`'s own value, after its blocks.
    fn defined(&mut self, x: u32, st: usize) {
        let bit = self.vbit[x as usize];
        if bit != NONE {
            self.refill(bit, st);
        }
    }

    /// Statement `x` forward over `st`; false when it never completes.
    fn inst(
        &mut self,
        b: &Body,
        topo: &Topology,
        cx: &dyn OwnCx,
        edits: &splice::Tables,
        x: u32,
        st: usize,
    ) -> bool {
        let [a, w] = b.data[x as usize];
        match b.tags[x as usize] {
            Tag::If => {
                self.uses_only(b, cx, x, st);
                let r = b.record(w);
                let (then, els) = (r.first().copied(), r.get(1).copied());
                let other = self.arena.alloc(Some(st));
                let t = then.is_none_or(|blk| self.valued(b, topo, cx, edits, blk, other));
                let e = els.is_none_or(|blk| self.valued(b, topo, cx, edits, blk, st));
                let falls = match (t, e) {
                    (true, true) => {
                        self.arena.union(st, other);
                        true
                    }
                    (true, false) => {
                        self.arena.copy(st, other);
                        true
                    }
                    (false, e) => e,
                };
                self.arena.free(other);
                if falls {
                    self.defined(x, st);
                }
                falls
            }
            Tag::And | Tag::Or => {
                self.uses_only(b, cx, x, st);
                let right = self.arena.alloc(Some(st));
                if self.valued(b, topo, cx, edits, w, right) {
                    self.arena.union(st, right);
                }
                self.arena.free(right);
                self.defined(x, st);
                true
            }
            Tag::With => {
                self.uses_only(b, cx, x, st);
                let falls = self.valued(b, topo, cx, edits, w, st);
                if falls {
                    self.defined(x, st);
                }
                falls
            }
            Tag::RefDyn => {
                let falls = self.valued(b, topo, cx, edits, a, st);
                if falls {
                    self.defined(x, st);
                }
                falls
            }
            Tag::Loop => {
                for c in 0..self.clocs.len() {
                    self.cell_visit(x, self.clocs[c], st);
                }
                let falls = self.repeat(b, topo, cx, edits, x, st);
                if falls {
                    self.defined(x, st);
                }
                falls
            }
            Tag::Match => {
                self.uses_only(b, cx, x, st);
                let falls = self.decide(b, topo, cx, edits, x, st);
                if falls {
                    self.defined(x, st);
                }
                falls
            }
            Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => {
                self.uses_only(b, cx, x, st);
                let inp = self.arena.alloc(Some(st));
                let s = self.arena.alloc(None);
                let r = b.record(w);
                let (cases, default) = r.split_at(r.len().saturating_sub(1));
                let mut any = false;
                for &blk in cases.iter().skip(1).step_by(2).chain(default) {
                    if b.tags.get(blk as usize) != Some(&Tag::Block) {
                        continue;
                    }
                    self.arena.copy(s, inp);
                    if self.valued(b, topo, cx, edits, blk, s) {
                        self.arena.join(st, &mut any, s);
                    }
                }
                self.arena.free(inp);
                if any {
                    self.defined(x, st);
                }
                any
            }
            Tag::Guard => {
                if !self.valued(b, topo, cx, edits, a, st) {
                    return false;
                }
                let inp = self.arena.alloc(Some(st));
                let s = self.arena.alloc(None);
                let mut any = false;
                for &blk in b.record(w) {
                    self.arena.copy(s, inp);
                    if self.block(b, topo, cx, edits, blk, s) {
                        self.arena.join(st, &mut any, s);
                    }
                }
                self.arena.free(inp);
                any
            }
            Tag::Scope => {
                if !self.valued(b, topo, cx, edits, a, st) {
                    return false;
                }
                if w != NONE {
                    let inp = self.arena.alloc(Some(st));
                    let s = self.arena.alloc(None);
                    self.suites += 1;
                    for &suite in b.record(w) {
                        self.arena.copy(s, inp);
                        if self.block(b, topo, cx, edits, suite, s) {
                            self.arena.union(st, s);
                        }
                    }
                    self.suites -= 1;
                    self.arena.free(inp);
                }
                self.defined(x, st);
                true
            }
            Tag::ToArm => {
                if let Some(&(first, count, h)) = self.arms.last()
                    && a < count
                {
                    let at = first as usize + a as usize * self.arena.words;
                    let mut reached = self.hit[(h + a) as usize];
                    self.arena.join(at, &mut reached, st);
                    self.hit[(h + a) as usize] = true;
                }
                false
            }
            Tag::Break => {
                self.uses(b, cx, x, st);
                if let Some(&(at, reached)) = self.brk.get(a as usize)
                    && at != NONE
                {
                    let mut r = reached;
                    self.arena.join(at as usize, &mut r, st);
                    self.brk[a as usize].1 = true;
                }
                false
            }
            Tag::Continue => {
                if let Some(&(at, reached)) = self.cont.get(a as usize)
                    && at != NONE
                {
                    let mut r = reached;
                    self.arena.join(at as usize, &mut r, st);
                    self.cont[a as usize].1 = true;
                }
                false
            }
            Tag::Return => {
                self.uses(b, cx, x, st);
                self.arena.union(self.exit, st);
                false
            }
            Tag::Unreachable => false,
            _ => {
                self.uses(b, cx, x, st);
                true
            }
        }
    }

    /// The uses of statement `x`'s operands, its effect on the locals, and
    /// its own value. A suspension point records what may hold a count
    /// there, before its own value.
    fn uses(&mut self, b: &Body, cx: &dyn OwnCx, x: u32, st: usize) {
        let i = x as usize;
        let [a, w] = b.data[i];
        let tag = b.tags[i];
        if tag == Tag::Drop {
            self.drop_item(b, cx, i, a, st);
        }
        // Rule 5: a read of an addressed local's storage, and what it
        // leaves there.
        if !self.clocs.is_empty() {
            match tag {
                Tag::Read | Tag::Store | Tag::Ref | Tag::IsUnique | Tag::Pin => {
                    if let Some(l) = b.view(a).local() {
                        self.cell_visit(x, l, st);
                        match tag {
                            Tag::Store if b.local_in_place(a).is_some() => {
                                self.cell_mark(l, st, false);
                            }
                            Tag::Read if w == ReadMode::Move as u32 => self.cell_mark(l, st, true),
                            Tag::Ref if w & ref_word::TAKES != 0 => self.cell_mark(l, st, true),
                            _ => {}
                        }
                    }
                }
                Tag::Closure => {
                    let r = b.record(w);
                    for c in r[0]..r[0] + r[1] {
                        self.cell_visit(x, b.cap_local[c as usize].raw(), st);
                    }
                }
                _ => {}
            }
        }
        // A `move` of all of a value consumes it.
        let moved = match tag {
            Tag::Read if w == ReadMode::Move as u32 => {
                let p = b.view(a);
                p.value().filter(|_| p.whole()).unwrap_or(NONE)
            }
            _ => NONE,
        };
        let mut ops = std::mem::take(&mut self.ops);
        operands(b, i, &mut ops);
        for op in &ops {
            let bit = self.vbit[op.value as usize];
            if bit == NONE {
                continue;
            }
            let gone = match op.role {
                Role::Dropped | Role::Tail => false,
                _ if op.value == moved && op.at == Loc::Extra(a + 2) => true,
                _ => consumes(b, cx, i, op, NONE),
            };
            if gone {
                self.clear(st, bit);
            }
        }
        self.ops = ops;
        // A `move` read of a part takes it; a local's own changes.
        if matches!(tag, Tag::Read | Tag::Store | Tag::Ref)
            && let Some((bit, whole)) = self.root_bit(b, a)
        {
            let local = b.view(a).local().is_some();
            match tag {
                Tag::Read if !whole && w == ReadMode::Move as u32 => self.take(b, cx, i, bit, st),
                // A store through the local, into what it refers to, leaves
                // the local as it was.
                Tag::Store if local && b.local_in_place(a).is_some() => self.refill(bit, st),
                Tag::Read if local && whole && w == ReadMode::Move as u32 => self.clear(st, bit),
                Tag::Ref if local && whole && w & ref_word::TAKES != 0 => self.clear(st, bit),
                _ => {}
            }
        }
        if is_suspension(tag) {
            let mut at = self.susp_at[i];
            if at == NONE {
                at = u32_of(self.susp.len());
                self.susp.resize(self.susp.len() + self.arena.words, 0);
                self.susp_at[i] = at;
                self.points.push(x);
            }
            for (s, w) in self.susp[at as usize..].iter_mut().zip(self.arena.row(st)) {
                *s |= w;
            }
            self.arena.union(self.exit, st);
        }
        self.defined(x, st);
    }

    /// Loop `x` forward: its body from the head until the head stops
    /// growing; what follows it is what its `Break`s carry.
    fn repeat(
        &mut self,
        b: &Body,
        topo: &Topology,
        cx: &dyn OwnCx,
        edits: &splice::Tables,
        x: u32,
        st: usize,
    ) -> bool {
        let body = b.data[x as usize][0];
        let label = topo.label_of[x as usize];
        let head = self.arena.alloc(Some(st));
        let brk = self.arena.alloc(None);
        let cont = self.arena.alloc(None);
        let end = self.arena.alloc(None);
        let saved = (label != NONE).then(|| (self.brk[label as usize], self.cont[label as usize]));
        if label != NONE {
            self.brk[label as usize] = (u32_of(brk), false);
        }
        for _ in 0..self.arena.words * 64 + 2 {
            if label != NONE {
                self.cont[label as usize] = (u32_of(cont), false);
            }
            self.arena.copy(end, head);
            let mut grew = false;
            if self.block(b, topo, cx, edits, body, end) {
                grew |= self.arena.union(head, end);
            }
            if label != NONE && self.cont[label as usize].1 {
                grew |= self.arena.union(head, cont);
            }
            if !grew {
                break;
            }
        }
        let mut falls = false;
        if label != NONE && self.brk[label as usize].1 {
            self.arena.join(st, &mut falls, brk);
        }
        if let Some((bs, cs)) = saved {
            self.brk[label as usize] = bs;
            self.cont[label as usize] = cs;
        }
        self.arena.free(head);
        falls
    }

    /// Match `x` forward: its decision tree from `st`, each arm from what
    /// its `ToArm`s carry.
    fn decide(
        &mut self,
        b: &Body,
        topo: &Topology,
        cx: &dyn OwnCx,
        edits: &splice::Tables,
        x: u32,
        st: usize,
    ) -> bool {
        let r = b.record(b.data[x as usize][1]);
        let Some((&decision, arms)) = r.split_first() else {
            return true;
        };
        let label = topo.label_of[x as usize];
        let saved = (label != NONE).then(|| self.brk[label as usize]);
        let first = self.arena.top();
        if label != NONE {
            let at = self.arena.alloc(None);
            self.brk[label as usize] = (u32_of(at), false);
        }
        let arms_at = self.arena.top();
        for _ in arms {
            self.arena.alloc(None);
        }
        let h = u32_of(self.hit.len());
        self.hit.resize(self.hit.len() + arms.len(), false);
        self.arms.push((u32_of(arms_at), u32_of(arms.len()), h));
        let _ = self.block(b, topo, cx, edits, decision, st);
        self.arms.pop();
        let mut falls = false;
        for (k, &arm) in arms.iter().enumerate() {
            if !self.hit[h as usize + k] {
                continue;
            }
            let at = arms_at + k * self.arena.words;
            if self.valued(b, topo, cx, edits, arm, at) {
                self.arena.join(st, &mut falls, at);
            }
        }
        self.hit.truncate(h as usize);
        if let Some(old) = saved {
            let (at, reached) = self.brk[label as usize];
            if reached {
                self.arena.join(st, &mut falls, at as usize);
            }
            self.brk[label as usize] = old;
        }
        self.arena.free(first);
        falls
    }
}
