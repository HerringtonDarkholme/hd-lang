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
//! The plan's rules 1 and 3, and the cancel blocks' releases:
//!
//! - **Rule 1, drops.** A `Drop` whose place's root is empty on every
//!   path that reaches it goes: a local moved, dropped or never stored, as
//!   the drops after moved locals in branches and loops that the move pass
//!   leaves (#677). An immediately following drop can instead move to
//!   the single normal arm that can still hold its count.
//! - **Rule 3, the exit set.** A local empty at every way out of its
//!   sub-body leaves the exit set: each `Return`, the end of its root
//!   block, and each suspension point, whose cancel block ends in a
//!   cancelled `Return`. A parameter and a capture keep
//!   their rows: the backend reads one outside the exit set as unowned.
//! - **Boxes.** A local in a heap cell (`Storage::Cell`) holds a box
//!   whose life is not its value's, as Swift's `alloc_box` and
//!   `destroy_value` around `load [take]`: a `move` of all of its value
//!   leaves the box held, so its `Drop` and exit row stay to end it. A
//!   `Drop` ends it, and a `Ref` that takes its count hands it to the
//!   reference. A slot or a static cell needs no end of its own: a move
//!   empties its value as any local's, and its address stays.
//! - **Cancel blocks.** Each suspension point's cancel block
//!   (`Body::cancel`, which `own::scopes` gives every point) starts with a
//!   `Release` of each owned value that may hold a count there, before
//!   the cleanup suites and the cancelled `Return`.
//!
//! The plan's rule 2 (a store into an empty place) is
//! `own::overwrites`; rule 4 (a move with no zeroing) is not built
//! (`docs/compiler/tir-v2.md`, "Known-null").
//!
//! An owned body has no cleanup suites: `own::scopes` copied each one to
//! the ways out of its scope, where the walk sees it as any code, so the
//! walk's suite rows stay empty.
//!
//! Why it is sound: a drop, an exit row or a cancel release goes only where
//! no path reaches it with a count in its place; any other state keeps
//! it, and a place that is empty there holds zero, which releases as
//! nothing. A rule 5 row stands only where every path that reaches it
//! stored into the local after the last `Drop` or taking `Ref` of it, and
//! each store leaves the backend's cell made.
//!
//! The walk reads the body as the earlier passes' pending edits leave it
//! ([`splice::Pending`]): a deleted statement is gone. One bit per tracked
//! local and value and per part place taken, over the shared forward walk
//! ([`forward`]); per-item flags in dense tables. Its deletions join the
//! pipeline's last rebuild.

use super::flow::{BitArena, Flow, Forward, Suites, forward};
use super::parts::{Slots, takes};
use super::splice;
use super::{
    Loc, Operand, OwnCx, Role, consumes_counts as consumes, fill, lending_coercion, lending_read,
    operands, owned_value, tracked,
};
use hd_base::SliceSort as _;
use std::ops::Range;

use crate::ir::count::{At, Info, Part};

use crate::ir::{
    Body, NONE, ReadMode, Ref, Root, Storage, Tag, Topology, is_suspension, local_flags, ref_word,
    u32_of,
};
use hd_base::NodeIdx;
use hd_types::Ty;

/// A whole-local drop after a branch and the normal arm that may need it.
/// A second arm makes the drop stay at the join.
struct ArmDrop {
    inst: u32,
    bit: u32,
    arm: Option<u32>,
    shared: bool,
}

/// A branch's candidates and whether more than one normal arm reached
/// their join.
struct ArmCleanup {
    drops: Range<usize>,
    first: Option<u32>,
    multiple: bool,
}

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
    /// by root bit, with items the instructions. A slot's
    /// bit follows the values' bits; a root whose taken parts cover every
    /// counted leaf is empty.
    slots: Slots,
    /// Per bit: a place a cleanup suite touches, which nothing empties.
    sticky: Vec<bool>,
    /// Per local: in the sub-body walked, it lives in a heap cell, a box
    /// that a move of its value leaves in place; only a `Drop` or a
    /// taking `Ref` ends it.
    boxed: Vec<bool>,
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
    /// Per instruction: a `Drop` the walk reached, and one it reached where
    /// its place may hold a count.
    seen: Vec<bool>,
    needed: Vec<bool>,
    /// The cleanup plan of each instruction with a branch, or `NONE`.
    arm_plan: Vec<u32>,
    arm_cleanup: Vec<ArmCleanup>,
    arm_drops: Vec<ArmDrop>,
    /// Per instruction: its suspension state's offset in `susp`, or `NONE`;
    /// the suspension points reached.
    susp_at: Vec<u32>,
    susp: Vec<u64>,
    points: Vec<u32>,
    /// Per exit row: whether it stays.
    keep: Vec<bool>,
    ops: Vec<Operand>,
    empty_variants: std::cell::OnceCell<super::parts::EmptyVariants>,
    /// Each local a `Ref` of its own storage or a shared capture puts in
    /// a cell: an arm never takes its drop.
    addressed: Vec<bool>,
}

/// Runs known-null over `b` with the earlier passes' deletions in `edits`,
/// adding its own there, fills each point's cancel block, and trims
/// `b.exit_drops`.
pub(crate) fn run(
    b: &mut Body,
    topo: &Topology,
    cx: &dyn OwnCx,
    t: &mut Tables,
    edits: &mut splice::Tables,
) {
    let n = b.len();
    t.empty_variants.take();
    fill(&mut t.susp_at, n, NONE);
    t.susp.clear();
    t.points.clear();
    t.pending.build(n, edits);
    t.arm_drops.clear();
    t.arm_cleanup.clear();
    if !number(b, cx, t) {
        return;
    }
    t.prepare_arm_drops(b, topo, cx);
    fill(&mut t.seen, n, false);
    fill(&mut t.needed, n, false);
    fill(&mut t.keep, b.exit_drops.len(), true);
    let mut flow = std::mem::take(&mut t.flow);
    flow.arena.free(0);
    let mut walk = Walk {
        t: &mut *t,
        topo,
        cx,
    };
    for s in 0..b.sub_root.len() {
        let root = b.sub_root[s];
        if root as usize >= n {
            continue;
        }
        // A local's storage is its sub-body's own row (`Body::storage`,
        // sorted by sub-body): a heap cell here may be a slot in another.
        let rows = b.storage.partition_point(|r| r.0.idx() < s)
            ..b.storage.partition_point(|r| r.0.idx() <= s);
        for &(_, l, kind) in &b.storage[rows.clone()] {
            walk.t.boxed[l.idx()] = kind == Storage::Cell;
        }
        let (t, arena) = (&mut *walk.t, &mut flow.arena);
        t.exit = arena.alloc(None);
        let st = arena.alloc(None);
        for l in 0..b.local_ty.len() {
            if t.entry[l] && t.lbit[l] != NONE {
                arena.set(st, t.lbit[l], true);
            }
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
        for &(_, l, _) in &b.storage[rows] {
            t.boxed[l.idx()] = false;
        }
    }
    t.flow = flow;
    // Rule 1: the drops no path reaches with a count.
    for x in 0..n {
        if t.seen[x] && !t.needed[x] {
            edits.remove(u32_of(x));
        }
    }
    // Rule 3: the exit rows of locals empty at every way out.
    let mut k = 0;
    let keep = &t.keep;
    b.exit_drops.retain(|_| {
        k += 1;
        keep[k - 1]
    });
    // Each point's cancel block: a `Release` of each value that may hold
    // a count there, in value order, all sharing one count record, before
    // the suites and the cancelled `Return` (`scopes`, which gives every
    // point its row). Rows and points both ascend: one merge, no lookup.
    let (mut record, mut row) = (NONE, 0);
    t.points.sort_stable();
    for &x in &t.points {
        while b.cancel[row].0 < x {
            row += 1;
        }
        let blk = b.cancel[row].1;
        let at = t.susp_at[x as usize] as usize;
        for bit in t.nl as usize..(t.nl + t.nv) as usize {
            if t.susp[at + bit / 64] & (1 << (bit % 64)) != 0 {
                if record == NONE {
                    let reason = super::reason::CANCELLED;
                    record = Info::new(At::Value, Part::Result, reason).write(b);
                }
                let v = t.vals[bit - t.nl as usize];
                edits.insert(blk, 0, Tag::Release, [v, record], Ty::VOID, NodeIdx::NONE);
            }
        }
    }
}

/// Move the cleanup prefix in order. Leave the first shared drop and
/// everything after it at the join, since moving later drops would change
/// their order. Hoisting must see the original drops before this splice.
pub(super) fn place_arm_drops(b: &Body, t: &Tables, edits: &mut splice::Tables) {
    for plan in &t.arm_cleanup {
        if !plan.multiple {
            continue;
        }
        for drop in &t.arm_drops[plan.drops.clone()] {
            let i = drop.inst as usize;
            if !t.needed[i] {
                continue;
            }
            let Some(arm) = drop.arm.filter(|_| !drop.shared) else {
                break;
            };
            let end = b.record(b.data[arm as usize][0]).len();
            edits.insert(arm, u32_of(end), Tag::Drop, b.data[i], b.ty[i], b.syn[i]);
            edits.remove(drop.inst);
        }
    }
}

/// Explicit arms that can reach the join through their block ends.
/// A jump to the branch or arm label could bypass the moved cleanup.
fn arm_blocks(b: &Body, branch: usize) -> Option<impl Iterator<Item = u32> + '_> {
    let tag = b.tags[branch];
    let (start, step, with_default) = match tag {
        Tag::If => (0, 1, false),
        Tag::Match => (1, 1, false),
        Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => (1, 2, true),
        _ => return None,
    };
    let row = b.record(b.data[branch][1]);
    let end = row.len().saturating_sub(usize::from(with_default));
    let arms = row.get(start..end)?.iter().step_by(step).copied();
    let default = with_default.then(|| row.last().copied()).flatten();
    Some(arms.chain(default))
}

impl Tables {
    fn prepare_arm_drops(&mut self, b: &Body, topo: &Topology, cx: &dyn OwnCx) {
        let n = b.len();
        fill(&mut self.arm_plan, n, NONE);
        // Each admitted drop costs one bit test per arm per visit. Charge
        // the arm/drop product to the body length, so a wide match followed
        // by a long cleanup does not introduce a quadratic scan.
        let mut budget = n;
        for i in 0..n {
            let parent = topo.blk[i];
            if parent == NONE || topo.label_of[i] != NONE || self.pending.dead[i] {
                continue;
            }
            let siblings = b.record(b.data[parent as usize][0]);
            let following = &siblings[topo.pos[i] as usize + 1..];
            if following
                .first()
                .is_none_or(|&x| b.tags[x as usize] != Tag::Drop)
            {
                continue;
            }
            let Some(arms) = arm_blocks(b, i) else {
                continue;
            };
            let mut count = 0;
            let explicit = arms.into_iter().all(|arm| {
                count += 1;
                let a = arm as usize;
                if b.tags.get(a) != Some(&Tag::Block)
                    || topo.owner[a] != u32_of(i)
                    || topo.label_of[a] != NONE
                {
                    return false;
                }
                let tail = b.data[a][1];
                // The tail must already own its result. A borrowed
                // tail can still need its owner during the transfer.
                Ref(tail).as_inst().is_none_or(|v| {
                    let v = v.0 as usize;
                    owned_value(b, v)
                        && (!cx.holds_ref(b.ty[v]) || cx.counted_refs_only(b.ty[v]))
                        && !lending_read(b, cx, v)
                        && !lending_coercion(b, cx, v)
                })
            });
            if !explicit || count < 2 || (b.tags[i] == Tag::If && count != 2) {
                continue;
            }
            let start = self.arm_drops.len();
            for &x in following {
                let d = x as usize;
                if budget < count || b.tags[d] != Tag::Drop {
                    break;
                }
                if self.pending.dead[d] {
                    continue;
                }
                let p = b.view(b.data[d][0]);
                let Some(l) = p.local().filter(|_| p.whole()) else {
                    break;
                };
                let bit = self.lbit[l as usize];
                if bit == NONE || self.addressed[l as usize] {
                    break;
                }
                self.arm_drops.push(ArmDrop {
                    inst: x,
                    bit,
                    arm: None,
                    shared: false,
                });
                budget -= count;
            }
            if start != self.arm_drops.len() {
                self.arm_plan[i] = u32_of(self.arm_cleanup.len());
                self.arm_cleanup.push(ArmCleanup {
                    drops: start..self.arm_drops.len(),
                    first: None,
                    multiple: false,
                });
            }
        }
    }

    fn observe_arm(&mut self, branch: u32, arm: u32, s: &BitArena, st: usize) {
        let Some(&plan) = self.arm_plan.get(branch as usize) else {
            return;
        };
        let Some(plan) = self.arm_cleanup.get_mut(plan as usize) else {
            return;
        };
        plan.multiple |= *plan.first.get_or_insert(arm) != arm;
        for drop in &mut self.arm_drops[plan.drops.clone()] {
            if !drop.shared && s.get(st, drop.bit) {
                let first = drop.arm.get_or_insert(arm);
                drop.shared = *first != arm;
            }
        }
    }
}

/// Numbers the tracked places: the locals in the exit set that no cleanup
/// suite touches, then the owned values and the `borrow` reads with a
/// `Drop`. Whether there is any.
fn number(b: &Body, cx: &dyn OwnCx, t: &mut Tables) -> bool {
    let n = b.len();
    let locals = b.local_ty.len();
    fill(&mut t.lbit, locals, NONE);
    fill(&mut t.vbit, n, NONE);
    fill(&mut t.pin_l, locals, false);
    fill(&mut t.pin_v, n, false);
    fill(&mut t.entry, locals, false);
    fill(&mut t.boxed, locals, false);
    fill(&mut t.dropped, n, false);
    t.locs.clear();
    t.vals.clear();
    t.sticky.clear();
    for i in 0..n {
        if b.tags[i] == Tag::Drop && !t.pending.dead[i] {
            mark_dropped(b, t, b.data[i][0]);
        }
    }
    for (l, flags) in b.local_flags.iter().enumerate() {
        if flags & local_flags::PARAM != 0 {
            t.entry[l] = true;
        }
    }
    for sub in 0..b.sub_params.len() {
        for &local in b.sub_param_locals(sub) {
            if let Some(x) = t.entry.get_mut(local as usize) {
                *x = true;
            }
        }
    }
    for (l, m) in b.cap_local.iter().zip(&b.cap_mode) {
        t.entry[l.idx()] = true;
        if m.shares() {
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
            t.dropped[v] && cx.counted(b.ty[v])
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
    fill(&mut t.addressed, locals, false);
    for i in 0..n {
        if b.tags[i] == Tag::Ref
            && !t.pending.dead[i]
            && let Some(l) = b.local_in_place(b.data[i][0])
        {
            t.addressed[l.idx()] = true;
        }
    }
    for (l, m) in b.cap_local.iter().zip(&b.cap_mode) {
        t.addressed[l.idx()] |= m.shares();
    }
    if k == 0 {
        return false;
    }
    let slots = number_parts(b, t);
    t.flow.arena.words = ((k + slots) as usize).div_ceil(64);
    true
}

/// Numbers the part slots: each distinct part place of a tracked root
/// that a `move` read or a `Drop` takes. Returns how many.
fn number_parts(b: &Body, t: &mut Tables) -> u32 {
    let mut slots = std::mem::take(&mut t.slots);
    let takes =
        takes(b, |i| !t.pending.dead[i]).filter_map(|(item, word)| match t.root_bit(b, word) {
            Some((bit, false)) if b.view(word).in_place(b) => Some((bit, u32_of(item), word)),
            _ => None,
        });
    let count = slots.build(b, b.len(), (t.nl + t.nv) as usize, takes);
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

impl Tables {
    /// Empties `bit`, unless a cleanup suite touches its place.
    fn clear(&self, s: &mut BitArena, st: usize, bit: u32) {
        if !self.sticky[bit as usize] {
            s.set(st, bit, false);
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
        // The cell exists even on a path that skips its payload's first
        // store. Its Drop/exit owns that allocation until it is released.
        if tag == Tag::Box {
            let bit = self.lbit[b.data[i][0] as usize];
            if bit != NONE {
                self.refill(bit, s, st);
            }
        }

        if tag == Tag::Drop {
            self.drop_item(b, cx, i, a, s, st);
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
                // A move out of a box leaves the box, which its drop ends.
                Tag::Read
                    if local
                        && whole
                        && w == ReadMode::Move as u32
                        && !self.boxed[self.locs[bit as usize] as usize] =>
                {
                    self.clear(s, st, bit);
                }
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
}

impl Forward for Walk<'_> {
    // An owned body has no cleanup suites (`scopes`).
    const SUITES: Suites = Suites::FromEnd { union: true };
    const ALL_ROUNDS: bool = true;

    fn stmt(&mut self, b: &Body, x: u32, s: &mut BitArena, st: usize) {
        self.t.uses(b, self.cx, x, s, st);
    }

    /// A point's value is not made on its cancel exit.
    fn cancel(&mut self, _: &Body, x: u32, s: &mut BitArena, st: usize) {
        let bit = self.t.vbit[x as usize];
        if bit != NONE {
            self.t.clear(s, st, bit);
        }
    }

    /// The operands' uses.
    fn enter(&mut self, b: &Body, x: u32, s: &mut BitArena, st: usize) {
        let t = &mut *self.t;
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

    fn case(&mut self, b: &Body, x: u32, blk: u32, s: &mut BitArena, st: usize) {
        if b.tags[x as usize] != Tag::SwitchTag {
            return;
        }
        if let Some(v) = self
            .t
            .empty_variants
            .get_or_init(|| super::parts::EmptyVariants::build(b, self.cx))
            .get(x, blk)
        {
            let bit = self.t.vbit[v as usize];
            if bit != NONE {
                self.t.clear(s, st, bit);
            }
        }
    }

    fn leave(&mut self, _: &Body, x: u32, s: &mut BitArena, st: usize) {
        self.t.defined(x, s, st);
    }

    /// Block `blk`'s tail, which its owner's value takes when the owner
    /// uses it.
    fn tail(&mut self, b: &Body, blk: u32, s: &mut BitArena, st: usize) {
        let topo = self.topo;
        let owner = if topo.is_root[blk as usize] {
            NONE
        } else {
            topo.owner[blk as usize]
        };
        // An ordinary call with result type never has no normal edge,
        // even though the shared walk visits its block's tail.
        if b.ty[blk as usize] != hd_types::Ty::NEVER {
            self.t.observe_arm(owner, blk, s, st);
        }
        let w = b.data[blk as usize][1];
        if w == NONE || Ref(w).as_inst().is_none() || w as usize >= b.len() {
            return;
        }
        let bit = self.t.vbit[w as usize];
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

    fn skip(&self, x: u32) -> bool {
        self.t.pending.dead[x as usize]
    }
}
