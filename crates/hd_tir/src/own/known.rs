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
//! place taken, over the shared forward walk ([`forward`]); per-item flags
//! in dense tables. Its deletions join the pipeline's last rebuild.

use super::flow::{BitArena, Flow, Forward, Suites, forward};
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
    flow: Flow,
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
    fill(&mut t.keep, b.exit_drops.len(), true);
    t.susp.clear();
    t.points.clear();
    let mut flow = std::mem::take(&mut t.flow);
    flow.arena.free(0);
    let mut walk = Walk {
        t: &mut *t,
        topo,
        cx,
        edits,
    };
    for s in 0..b.sub_root.len() {
        let root = b.sub_root[s];
        if root as usize >= n {
            continue;
        }
        let (t, arena) = (&mut *walk.t, &mut flow.arena);
        t.exit = arena.alloc(None);
        let st = arena.alloc(None);
        for l in 0..b.local_ty.len() {
            if t.entry[l] && t.lbit[l] != NONE {
                arena.set(st, t.lbit[l], true);
            }
        }
        // An addressed local holds no value where its sub-body starts:
        // the backend makes an entry's cell in the prologue itself.
        for c in 0..u32_of(t.clocs.len()) {
            arena.set(st, t.cfirst + c, true);
        }
        if forward(b, topo, &mut flow, &mut walk, root, st) {
            walk.tail(b, root, &mut flow.arena, st);
            flow.arena.union(walk.t.exit, st);
        }
        let (t, arena) = (&mut *walk.t, &mut flow.arena);
        for (k, &(sub, l)) in b.exit_drops.iter().enumerate() {
            let bit = t.lbit[l.idx()];
            if sub.idx() == s && bit != NONE && !t.entry[l.idx()] && !arena.get(t.exit, bit) {
                t.keep[k] = false;
            }
        }
        arena.free(t.exit);
    }
    t.flow = flow;
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
    t.flow.arena.words = ((k + slots + nc) as usize).div_ceil(64);
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
    fn clear(&self, s: &mut BitArena, st: usize, bit: u32) {
        if !self.sticky[bit as usize] {
            s.set(st, bit, false);
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
    fn cell_visit(&mut self, x: u32, l: u32, s: &BitArena, st: usize) {
        let c = self.cbit.get(l as usize).copied().unwrap_or(NONE);
        if c != NONE && self.suites == 0 {
            let full = !s.get(st, c);
            self.cells.push((x, l, full));
        }
    }

    /// Rule 5: addressed local `l` may hold no value (`empty`), or holds
    /// its value, from here.
    fn cell_mark(&self, l: u32, s: &mut BitArena, st: usize, empty: bool) {
        let c = self.cbit.get(l as usize).copied().unwrap_or(NONE);
        if c != NONE {
            s.set(st, c, empty);
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
    fn drop_item(
        &mut self,
        b: &Body,
        cx: &dyn OwnCx,
        item: usize,
        word: u32,
        s: &mut BitArena,
        st: usize,
    ) {
        if let Some(l) = b.view(word).local() {
            self.cell_mark(l, s, st, true);
        }
        let Some((bit, whole)) = self.root_bit(b, word) else {
            return;
        };
        self.seen[item] = true;
        if s.get(st, bit) || self.sticky[bit as usize] {
            self.needed[item] = true;
        }
        if whole {
            self.clear(s, st, bit);
        } else {
            self.take(b, cx, item, bit, s, st);
        }
    }

    /// Item `item` takes a part of the place whose root has `bit`: the
    /// root is empty once the parts taken on the path cover every counted
    /// leaf.
    fn take(
        &mut self,
        b: &Body,
        cx: &dyn OwnCx,
        item: usize,
        bit: u32,
        s: &mut BitArena,
        st: usize,
    ) {
        let slot = self.slots.of[item];
        if slot == NONE {
            return;
        }
        let first = self.nl + self.nv;
        s.set(st, first + slot, true);
        let base = if bit < self.nl {
            b.local_ty[self.locs[bit as usize] as usize]
        } else {
            b.ty[self.vals[(bit - self.nl) as usize] as usize]
        };
        let arena = &*s;
        if !self
            .slots
            .rest(b, cx, bit, base, |k, _| arena.get(st, first + k))
        {
            self.clear(s, st, bit);
        }
    }

    /// The root with `bit` holds all of its value again: no part taken.
    fn refill(&self, bit: u32, s: &mut BitArena, st: usize) {
        let first = self.nl + self.nv;
        for k in self.slots.of_key(bit) {
            s.set(st, first + k, false);
        }
        s.set(st, bit, true);
    }

    /// The uses of statement `x`'s operands, its effect on the locals, and
    /// its own value. A suspension point records what may hold a count
    /// there, before its own value.
    fn uses(&mut self, b: &Body, cx: &dyn OwnCx, x: u32, s: &mut BitArena, st: usize) {
        let i = x as usize;
        let [a, w] = b.data[i];
        let tag = b.tags[i];
        if tag == Tag::Drop {
            self.drop_item(b, cx, i, a, s, st);
        }
        // Rule 5: a read of an addressed local's storage, and what it
        // leaves there.
        if !self.clocs.is_empty() {
            match tag {
                Tag::Read | Tag::Store | Tag::Ref | Tag::IsUnique | Tag::Pin => {
                    if let Some(l) = b.view(a).local() {
                        self.cell_visit(x, l, s, st);
                        match tag {
                            Tag::Store if b.local_in_place(a).is_some() => {
                                self.cell_mark(l, s, st, false);
                            }
                            Tag::Read if w == ReadMode::Move as u32 => {
                                self.cell_mark(l, s, st, true);
                            }
                            Tag::Ref if w & ref_word::TAKES != 0 => self.cell_mark(l, s, st, true),
                            _ => {}
                        }
                    }
                }
                Tag::Closure => {
                    let r = b.record(w);
                    for c in r[0]..r[0] + r[1] {
                        self.cell_visit(x, b.cap_local[c as usize].raw(), s, st);
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
                self.clear(s, st, bit);
            }
        }
        self.ops = ops;
        // A `move` read of a part takes it; a local's own changes.
        if matches!(tag, Tag::Read | Tag::Store | Tag::Ref)
            && let Some((bit, whole)) = self.root_bit(b, a)
        {
            let local = b.view(a).local().is_some();
            match tag {
                Tag::Read if !whole && w == ReadMode::Move as u32 => {
                    self.take(b, cx, i, bit, s, st);
                }
                // A store through the local, into what it refers to, leaves
                // the local as it was.
                Tag::Store if local && b.local_in_place(a).is_some() => self.refill(bit, s, st),
                Tag::Read if local && whole && w == ReadMode::Move as u32 => self.clear(s, st, bit),
                Tag::Ref if local && whole && w & ref_word::TAKES != 0 => self.clear(s, st, bit),
                _ => {}
            }
        }
        if is_suspension(tag) {
            let mut at = self.susp_at[i];
            if at == NONE {
                at = u32_of(self.susp.len());
                self.susp.resize(self.susp.len() + s.words, 0);
                self.susp_at[i] = at;
                self.points.push(x);
            }
            for (k, w) in self.susp[at as usize..].iter_mut().zip(s.row(st)) {
                *k |= w;
            }
            s.union(self.exit, st);
        }
        self.defined(x, s, st);
    }

    /// Instruction `x`'s own value: all of it, no part taken.
    fn defined(&self, x: u32, s: &mut BitArena, st: usize) {
        let bit = self.vbit[x as usize];
        if bit != NONE {
            self.refill(bit, s, st);
        }
    }
}

/// Known-null's transfer, over the forward walk of one sub-body.
struct Walk<'a> {
    t: &'a mut Tables,
    topo: &'a Topology,
    cx: &'a dyn OwnCx,
    edits: &'a splice::Tables,
}

impl Forward for Walk<'_> {
    const SUITES: Suites = Suites::FromEnd { union: true };
    const ALL_ROUNDS: bool = true;

    fn stmt(&mut self, b: &Body, x: u32, s: &mut BitArena, st: usize) {
        self.t.uses(b, self.cx, x, s, st);
    }

    /// The operands' uses, and rule 5's visit of each addressed local at
    /// a `Loop`.
    fn enter(&mut self, b: &Body, x: u32, s: &mut BitArena, st: usize) {
        let t = &mut *self.t;
        if b.tags[x as usize] == Tag::Loop {
            for c in 0..t.clocs.len() {
                t.cell_visit(x, t.clocs[c], s, st);
            }
        }
        let mut ops = std::mem::take(&mut t.ops);
        operands(b, x as usize, &mut ops);
        for op in &ops {
            let bit = t.vbit[op.value as usize];
            if bit != NONE && consumes(b, self.cx, x as usize, op, NONE) {
                t.clear(s, st, bit);
            }
        }
        t.ops = ops;
    }

    fn leave(&mut self, _: &Body, x: u32, s: &mut BitArena, st: usize) {
        self.t.defined(x, s, st);
    }

    /// Block `blk`'s tail, which its owner's value takes when the owner
    /// uses it.
    fn tail(&mut self, b: &Body, blk: u32, s: &mut BitArena, st: usize) {
        let w = b.data[blk as usize][1];
        if w == NONE || Ref(w).as_inst().is_none() || w as usize >= b.len() {
            return;
        }
        let bit = self.t.vbit[w as usize];
        let topo = self.topo;
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
        if bit != NONE && consumes(b, self.cx, blk as usize, &op, owner) {
            self.t.clear(s, st, bit);
        }
    }

    fn exit(&mut self, _: &Body, _: u32, s: &mut BitArena, st: usize) {
        s.union(self.t.exit, st);
    }

    /// The pending `Drop`s that stand before member `p`; whether any
    /// stand after it.
    #[inline]
    fn before(
        &mut self,
        b: &Body,
        blk: u32,
        p: usize,
        y: &mut usize,
        s: &mut BitArena,
        st: usize,
    ) -> bool {
        while let Some(&e) = self.t.pending.of(blk).get(*y) {
            let (_, pos, tag, [word, _], _) = self.edits.get(e);
            if pos as usize > p {
                return true;
            }
            if tag == Tag::Drop {
                self.t
                    .drop_item(b, self.cx, b.len() + e as usize, word, s, st);
            }
            *y += 1;
        }
        false
    }

    fn skip(&self, x: u32) -> bool {
        self.t.pending.dead[x as usize]
    }

    fn suites(&mut self, inside: bool) {
        if inside {
            self.t.suites += 1;
        } else {
            self.t.suites -= 1;
        }
    }
}
