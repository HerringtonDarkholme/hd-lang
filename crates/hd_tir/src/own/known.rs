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
//! The plan's rules 2, 4 and 5 (a store into an empty place, a move with
//! no zeroing, a cell made before use) are not built here
//! (`docs/compiler/tir-v2.md`, stage 2's commit 10).
//!
//! A place a cleanup suite touches is never empty to the walk (its local
//! keeps every drop and its exit row, its value stays in every later
//! unwind row): a suite runs at its scope's exits, which the walk does
//! not follow.
//!
//! Why it is sound: a drop, an exit row or an unwind row goes only where
//! no path reaches it with a count in its place; any other state keeps
//! it, and a place that is empty there holds zero, which releases as
//! nothing.
//!
//! The walk reads the body as the earlier passes' pending edits leave it
//! ([`splice::Pending`]): a deleted statement is gone and a new `Drop`
//! stands in its place. One bit per tracked local and value and per part
//! place taken, states in one arena, allocated and freed last in, first
//! out; per-item flags in dense tables. Its deletions join the pipeline's
//! last rebuild.

use hd_types::Ty;

use super::moves::{Root, in_place, root_of};
use super::splice::{self, for_blocks};
use super::{Loc, Operand, OwnCx, Role, Tri, consumes, operands, tracked};

use crate::ir::{
    Body, CaptureMode, NONE, ReadMode, Ref, Tag, is_suspension, local_flags, place, ref_word,
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
    /// The parts a `move` read or a `Drop` takes of a tracked place: per
    /// item, its slot (a distinct part place of one root), or `NONE`; per
    /// root bit, its slots, in compressed rows, with a place word of each.
    /// A slot's bit follows the values' bits; a root whose taken parts
    /// cover every counted leaf is empty.
    slot_of: Vec<u32>,
    slot_start: Vec<u32>,
    slot_word: Vec<u32>,
    takes: Vec<(u32, u32)>,
    paths: Vec<(u32, u32)>,
    tys: Vec<Ty>,
    parts: Vec<Ty>,
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
    /// Each block's owner, and whether it is a sub-body's root.
    owner: Vec<u32>,
    is_root: Vec<bool>,
    label_of: Vec<u32>,
    /// Per label: the arena offset of the state its `Break`s (or
    /// `Continue`s) carry, and whether one reached it.
    brk: Vec<(u32, bool)>,
    cont: Vec<(u32, bool)>,
    /// The open matches: their arms' first state, count, and first flag in
    /// `hit`.
    arms: Vec<(u32, u32, u32)>,
    hit: Vec<bool>,
    arena: Vec<u64>,
    words: usize,
    /// The sub-body's exit state, while it is walked.
    exit: usize,
    /// Per item (instructions, then edits): a `Drop` the walk reached, and
    /// one it reached where its place may hold a count.
    seen: Vec<bool>,
    needed: Vec<bool>,
    /// Each statement's block, as the walk reached it.
    blk_of: Vec<u32>,
    /// Per instruction: its suspension state's offset in `susp`, or `NONE`;
    /// the suspension points reached.
    susp_at: Vec<u32>,
    susp: Vec<u64>,
    points: Vec<u32>,
    /// Per exit row: whether it stays.
    keep: Vec<bool>,
    stack: Vec<u32>,
    ops: Vec<Operand>,
}

fn fill<T: Clone>(v: &mut Vec<T>, n: usize, x: T) {
    v.clear();
    v.resize(n, x);
}

fn u32_of(i: usize) -> u32 {
    u32::try_from(i).expect("insts")
}

/// Runs known-null over `b` with the earlier passes' edits in `edits`,
/// adding its deletions there, and fills `b.unwind_drops` and trims
/// `b.exit_drops`.
pub(crate) fn run(b: &mut Body, cx: &dyn OwnCx, t: &mut Tables, edits: &mut splice::Tables) {
    let n = b.len();
    b.unwind_drops.clear();
    t.pending.build(n, edits);
    if !number(b, cx, t, edits) {
        return;
    }
    let k = edits.len();
    fill(&mut t.seen, n + k, false);
    fill(&mut t.needed, n + k, false);
    fill(&mut t.blk_of, n, NONE);
    fill(&mut t.susp_at, n, NONE);
    fill(&mut t.brk, b.label_inst.len(), (NONE, false));
    fill(&mut t.cont, b.label_inst.len(), (NONE, false));
    fill(&mut t.keep, b.exit_drops.len(), true);
    t.susp.clear();
    t.points.clear();
    t.arms.clear();
    t.hit.clear();
    t.arena.clear();
    for s in 0..b.sub_root.len() {
        let root = b.sub_root[s];
        if root as usize >= n {
            continue;
        }
        t.exit = t.alloc(None);
        let st = t.alloc(None);
        for l in 0..b.local_ty.len() {
            if t.entry[l] && t.lbit[l] != NONE {
                t.set(st, t.lbit[l]);
            }
        }
        if t.block(b, cx, edits, root, st) {
            t.tail(b, cx, root, st);
            t.union(t.exit, st);
        }
        for (k, &(sub, l)) in b.exit_drops.iter().enumerate() {
            let bit = t.lbit[l.idx()];
            if sub.idx() == s && bit != NONE && !t.entry[l.idx()] && !t.get(t.exit, bit) {
                t.keep[k] = false;
            }
        }
        t.free(t.exit);
    }
    // Rule 1: the drops no path reaches with a count.
    for x in 0..n {
        if t.seen[x] && !t.needed[x] {
            edits.remove(t.blk_of[x], u32_of(x));
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
    fill(&mut t.owner, n, NONE);
    fill(&mut t.is_root, n, false);
    fill(&mut t.label_of, n, NONE);
    t.locs.clear();
    t.vals.clear();
    t.sticky.clear();
    for i in 0..n {
        let [a, w] = b.data[i];
        match b.tags[i] {
            Tag::Scope if w != NONE => {
                t.stack.clear();
                t.stack.extend_from_slice(b.record(w));
                pin_suites(b, t);
            }
            Tag::Defer => {
                t.stack.clear();
                t.stack.push(a);
                pin_suites(b, t);
            }
            Tag::Drop if !t.pending.dead[i] => mark_dropped(b, t, a),
            _ => {}
        }
        let defer = b.tags[i] == Tag::Defer;
        let owner = &mut t.owner;
        for_blocks(b, i, |x| {
            if !(defer && owner[x as usize] != NONE) {
                owner[x as usize] = u32_of(i);
            }
        });
    }
    for e in 0..edits.len() {
        let (_, _, tag, [word, _], _) = edits.get(u32_of(e));
        if tag == Tag::Drop && !edits.is_cancelled(u32_of(e)) {
            mark_dropped(b, t, word);
        }
    }
    for &r in &b.sub_root {
        if let Some(x) = t.is_root.get_mut(r as usize) {
            *x = true;
        }
    }
    for (l, &x) in b.label_inst.iter().enumerate() {
        if let Some(slot) = t.label_of.get_mut(x as usize) {
            *slot = u32_of(l);
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
        let borrow = b.tags[v] == Tag::Read && b.data[v][1] == ReadMode::Borrow as u32;
        let owned = if borrow {
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
    if k == 0 {
        return false;
    }
    let slots = number_parts(b, t, edits);
    t.words = ((k + slots) as usize).div_ceil(64);
    true
}

/// Numbers the part slots: each distinct part place of a tracked root
/// that a `move` read or a `Drop` takes. Returns how many.
fn number_parts(b: &Body, t: &mut Tables, edits: &splice::Tables) -> u32 {
    let n = b.len();
    let k = edits.len();
    fill(&mut t.slot_of, n + k, NONE);
    t.takes.clear();
    let part = |t: &Tables, word: u32| -> Option<u32> {
        let (_, from, to) = root_of(b, word);
        match t.root_bit(b, word) {
            Some((bit, false)) if in_place(b, from, to) => Some(bit),
            _ => None,
        }
    };
    for i in 0..n {
        let [a, w] = b.data[i];
        let takes = match b.tags[i] {
            Tag::Read => w == ReadMode::Move as u32,
            Tag::Drop => !t.pending.dead[i],
            _ => false,
        };
        if takes && let Some(bit) = part(t, a) {
            t.takes.push((bit, u32_of(i)));
        }
    }
    for e in 0..k {
        let (_, _, tag, [word, _], _) = edits.get(u32_of(e));
        if tag == Tag::Drop
            && !edits.is_cancelled(u32_of(e))
            && let Some(bit) = part(t, word)
        {
            t.takes.push((bit, u32_of(n + e)));
        }
    }
    let word_of = |item: u32| -> u32 {
        if (item as usize) < n {
            b.data[item as usize][0]
        } else {
            edits.get(item - u32_of(n)).3[0]
        }
    };
    let proj = |item: u32| {
        let (_, from, to) = root_of(b, word_of(item));
        &b.extra[from as usize..to as usize]
    };
    t.takes
        .sort_unstable_by(|p, q| p.0.cmp(&q.0).then_with(|| proj(p.1).cmp(proj(q.1))));
    let roots = (t.nl + t.nv) as usize;
    fill(&mut t.slot_start, roots + 1, 0);
    t.slot_word.clear();
    for x in 0..t.takes.len() {
        let (bit, item) = t.takes[x];
        let fresh = x == 0 || t.takes[x - 1].0 != bit || proj(t.takes[x - 1].1) != proj(item);
        if fresh {
            t.slot_word.push(word_of(item));
            t.slot_start[bit as usize + 1] += 1;
        }
        t.slot_of[item as usize] = u32_of(t.slot_word.len() - 1);
    }
    for r in 0..roots {
        t.slot_start[r + 1] += t.slot_start[r];
    }
    u32_of(t.slot_word.len())
}

/// Marks the value a `Drop` of all of place `word` drops, if it is one.
fn mark_dropped(b: &Body, t: &mut Tables, word: u32) {
    if let (Root::Value(v), from, to) = root_of(b, word)
        && from == to
    {
        t.dropped[v as usize] = true;
    }
}

/// Marks every local and value the blocks on `t.stack` touch, with every
/// block they run: a cleanup suite runs at its scope's exits, which the
/// walk does not follow.
fn pin_suites(b: &Body, t: &mut Tables) {
    let mut ops = std::mem::take(&mut t.ops);
    while let Some(x) = t.stack.pop() {
        let i = x as usize;
        if i >= b.len() {
            continue;
        }
        if b.tags[i] == Tag::Block {
            t.stack.extend_from_slice(b.record(b.data[i][0]));
        }
        let stack = &mut t.stack;
        for_blocks(b, i, |blk| stack.push(blk));
        match b.tags[i] {
            Tag::Read | Tag::Store | Tag::Ref | Tag::Drop | Tag::IsUnique | Tag::Pin => {
                if let (Root::Local(l), _, _) = root_of(b, b.data[i][0]) {
                    t.pin_l[l as usize] = true;
                }
            }
            Tag::Closure => {
                let rec = b.record(b.data[i][1]);
                for c in rec[0] as usize..(rec[0] + rec[1]) as usize {
                    t.pin_l[b.cap_local[c].idx()] = true;
                }
            }
            _ => {}
        }
        operands(b, i, &mut ops);
        for op in &ops {
            t.pin_v[op.value as usize] = true;
        }
        t.pin_v[i] = true;
    }
    t.ops = ops;
}

impl Tables {
    /// A new state of all zeros, or a copy of state `from`.
    fn alloc(&mut self, from: Option<usize>) -> usize {
        let at = self.arena.len();
        match from {
            Some(f) => self.arena.extend_from_within(f..f + self.words),
            None => self.arena.resize(at + self.words, 0),
        }
        at
    }

    /// Frees state `at` and every state allocated after it.
    fn free(&mut self, at: usize) {
        self.arena.truncate(at);
    }

    fn copy(&mut self, to: usize, from: usize) {
        self.arena.copy_within(from..from + self.words, to);
    }

    /// `to |= from`; whether `to` grew.
    fn union(&mut self, to: usize, from: usize) -> bool {
        let mut grew = false;
        for k in 0..self.words {
            let v = self.arena[from + k];
            let w = &mut self.arena[to + k];
            grew |= v & !*w != 0;
            *w |= v;
        }
        grew
    }

    /// Joins `from` into `to`: a copy when `to` holds no path yet.
    fn join(&mut self, to: usize, any: &mut bool, from: usize) {
        if *any {
            self.union(to, from);
        } else {
            self.copy(to, from);
            *any = true;
        }
    }

    fn set(&mut self, at: usize, bit: u32) {
        self.arena[at + (bit / 64) as usize] |= 1 << (bit % 64);
    }

    /// Empties `bit`, unless a cleanup suite touches its place.
    fn clear(&mut self, at: usize, bit: u32) {
        if !self.sticky[bit as usize] {
            self.arena[at + (bit / 64) as usize] &= !(1 << (bit % 64));
        }
    }

    fn get(&self, at: usize, bit: u32) -> bool {
        self.arena[at + (bit / 64) as usize] & (1 << (bit % 64)) != 0
    }

    /// The bit of the root of place `word`, and whether the place is all
    /// of it.
    fn root_bit(&self, b: &Body, word: u32) -> Option<(u32, bool)> {
        let (root, from, to) = root_of(b, word);
        let bit = match root {
            Root::Local(l) => self.lbit[l as usize],
            Root::Value(v) => self.vbit[v as usize],
            Root::Other => NONE,
        };
        (bit != NONE).then_some((bit, from == to))
    }

    /// A `Drop` item (an instruction, or `n + edit`) of place `word`: it
    /// is needed where its root may hold a count; it empties all of it, or
    /// takes a part.
    fn drop_item(&mut self, b: &Body, cx: &dyn OwnCx, item: usize, word: u32, st: usize) {
        let Some((bit, whole)) = self.root_bit(b, word) else {
            return;
        };
        self.seen[item] = true;
        if self.get(st, bit) || self.sticky[bit as usize] {
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
        let slot = self.slot_of[item];
        if slot == NONE {
            return;
        }
        self.set(st, self.nl + self.nv + slot);
        let base = if bit < self.nl {
            b.local_ty[self.locs[bit as usize] as usize]
        } else {
            b.ty[self.vals[(bit - self.nl) as usize] as usize]
        };
        self.paths.clear();
        let first = self.nl + self.nv;
        for s in self.slot_start[bit as usize]..self.slot_start[bit as usize + 1] {
            if self.get(st, first + s) {
                let (_, from, to) = root_of(b, self.slot_word[s as usize]);
                self.paths.push((from, to));
            }
        }
        self.tys.clear();
        let end = self.paths.len();
        if !self.uncovered(b, cx, base, (0, end), 0) {
            self.clear(st, bit);
        }
    }

    /// The root with `bit` holds all of its value again: no part taken.
    fn refill(&mut self, bit: u32, st: usize) {
        let first = self.nl + self.nv;
        for s in self.slot_start[bit as usize]..self.slot_start[bit as usize + 1] {
            let k = first + s;
            self.arena[st + (k / 64) as usize] &= !(1 << (k % 64));
        }
        self.set(st, bit);
    }

    /// Whether a value of type `ty` holds a counted leaf that none of the
    /// part places `self.paths[lo..hi]` covers, at projection step
    /// `depth`: the verifier's rule (a value is one variant on a path),
    /// over scratch stacks instead of allocations.
    fn uncovered(
        &mut self,
        b: &Body,
        cx: &dyn OwnCx,
        ty: Ty,
        (lo, hi): (usize, usize),
        depth: u32,
    ) -> bool {
        if lo == hi {
            return cx.counted(ty) != Tri::No;
        }
        let word = |p: (u32, u32), d: u32| -> Option<u32> {
            (p.0 + d < p.1).then(|| b.extra[(p.0 + d) as usize])
        };
        if self.paths[lo..hi].iter().any(|&p| p.1 - p.0 == depth) {
            return false;
        }
        for x in lo..hi {
            // One group per variant (or none), at its first path.
            let w = word(self.paths[x], depth).unwrap_or(0);
            let variant = match w & place::PROJ_KIND {
                place::PROJ_VARIANT => Some(w & place::PROJ_INDEX),
                place::PROJ_FIELD => None,
                _ => return true,
            };
            let key = |p: (u32, u32)| {
                let w = word(p, depth).unwrap_or(0);
                match (variant, w & place::PROJ_KIND) {
                    (Some(v), place::PROJ_VARIANT) => w & place::PROJ_INDEX == v,
                    (None, place::PROJ_FIELD) => true,
                    _ => false,
                }
            };
            if self.paths[lo..x].iter().any(|&p| key(p)) {
                continue;
            }
            let step = depth + u32::from(variant.is_some());
            if self.paths[x..hi]
                .iter()
                .any(|&p| key(p) && p.1 - p.0 == step)
            {
                continue;
            }
            if !cx.parts(ty, variant, &mut self.parts) {
                return true;
            }
            let t0 = self.tys.len();
            self.tys.extend_from_slice(&self.parts);
            let t1 = self.tys.len();
            for j in t0..t1 {
                let ft = self.tys[j];
                if cx.counted(ft) == Tri::No {
                    continue;
                }
                let field = place::PROJ_FIELD | u32_of(j - t0);
                let s0 = self.paths.len();
                for y in x..hi {
                    let p = self.paths[y];
                    if key(p) && word(p, step) == Some(field) {
                        self.paths.push(p);
                    }
                }
                let s1 = self.paths.len();
                let open = s0 == s1 || self.uncovered(b, cx, ft, (s0, s1), step + 1);
                self.paths.truncate(s0);
                if open {
                    self.tys.truncate(t0);
                    return true;
                }
            }
            self.tys.truncate(t0);
        }
        false
    }

    /// Walks block `blk` forward over state `st`; false when no path falls
    /// out of its end.
    fn block(
        &mut self,
        b: &Body,
        cx: &dyn OwnCx,
        edits: &splice::Tables,
        blk: u32,
        st: usize,
    ) -> bool {
        if b.tags.get(blk as usize) != Some(&Tag::Block) {
            return true;
        }
        let n = b.len();
        let label = self.label_of[blk as usize];
        let saved = (label != NONE).then(|| self.brk[label as usize]);
        let slot = self.arena.len();
        if label != NONE {
            let at = self.alloc(None);
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
            self.blk_of[m as usize] = blk;
            falls = self.inst(b, cx, edits, m, st);
        }
        if let Some(old) = saved {
            let (at, reached) = self.brk[label as usize];
            if reached {
                self.join(st, &mut falls, at as usize);
            }
            self.brk[label as usize] = old;
            self.free(slot);
        }
        falls
    }

    /// Walks block `blk` and takes its tail.
    fn valued(
        &mut self,
        b: &Body,
        cx: &dyn OwnCx,
        edits: &splice::Tables,
        blk: u32,
        st: usize,
    ) -> bool {
        let falls = self.block(b, cx, edits, blk, st);
        if falls && b.tags.get(blk as usize) == Some(&Tag::Block) {
            self.tail(b, cx, blk, st);
        }
        falls
    }

    /// Block `blk`'s tail, which its owner's value takes when the owner
    /// uses it.
    fn tail(&mut self, b: &Body, cx: &dyn OwnCx, blk: u32, st: usize) {
        let w = b.data[blk as usize][1];
        if w == NONE || Ref(w).as_inst().is_none() || w as usize >= b.len() {
            return;
        }
        let bit = self.vbit[w as usize];
        if bit == NONE {
            return;
        }
        let owner = if self.is_root[blk as usize] {
            NONE
        } else {
            self.owner[blk as usize]
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
                let other = self.alloc(Some(st));
                let t = then.is_none_or(|blk| self.valued(b, cx, edits, blk, other));
                let e = els.is_none_or(|blk| self.valued(b, cx, edits, blk, st));
                let falls = match (t, e) {
                    (true, true) => {
                        self.union(st, other);
                        true
                    }
                    (true, false) => {
                        self.copy(st, other);
                        true
                    }
                    (false, e) => e,
                };
                self.free(other);
                if falls {
                    self.defined(x, st);
                }
                falls
            }
            Tag::And | Tag::Or => {
                self.uses_only(b, cx, x, st);
                let right = self.alloc(Some(st));
                if self.valued(b, cx, edits, w, right) {
                    self.union(st, right);
                }
                self.free(right);
                self.defined(x, st);
                true
            }
            Tag::With => {
                self.uses_only(b, cx, x, st);
                let falls = self.valued(b, cx, edits, w, st);
                if falls {
                    self.defined(x, st);
                }
                falls
            }
            Tag::RefDyn => {
                let falls = self.valued(b, cx, edits, a, st);
                if falls {
                    self.defined(x, st);
                }
                falls
            }
            Tag::Loop => {
                let falls = self.repeat(b, cx, edits, x, st);
                if falls {
                    self.defined(x, st);
                }
                falls
            }
            Tag::Match => {
                self.uses_only(b, cx, x, st);
                let falls = self.decide(b, cx, edits, x, st);
                if falls {
                    self.defined(x, st);
                }
                falls
            }
            Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => {
                self.uses_only(b, cx, x, st);
                let inp = self.alloc(Some(st));
                let s = self.alloc(None);
                let r = b.record(w);
                let (cases, default) = r.split_at(r.len().saturating_sub(1));
                let mut any = false;
                for &blk in cases.iter().skip(1).step_by(2).chain(default) {
                    if b.tags.get(blk as usize) != Some(&Tag::Block) {
                        continue;
                    }
                    self.copy(s, inp);
                    if self.valued(b, cx, edits, blk, s) {
                        self.join(st, &mut any, s);
                    }
                }
                self.free(inp);
                if any {
                    self.defined(x, st);
                }
                any
            }
            Tag::Guard => {
                if !self.valued(b, cx, edits, a, st) {
                    return false;
                }
                let inp = self.alloc(Some(st));
                let s = self.alloc(None);
                let mut any = false;
                for &blk in b.record(w) {
                    self.copy(s, inp);
                    if self.block(b, cx, edits, blk, s) {
                        self.join(st, &mut any, s);
                    }
                }
                self.free(inp);
                any
            }
            Tag::Scope => {
                if !self.valued(b, cx, edits, a, st) {
                    return false;
                }
                if w != NONE {
                    let inp = self.alloc(Some(st));
                    let s = self.alloc(None);
                    for &suite in b.record(w) {
                        self.copy(s, inp);
                        if self.block(b, cx, edits, suite, s) {
                            self.union(st, s);
                        }
                    }
                    self.free(inp);
                }
                self.defined(x, st);
                true
            }
            Tag::ToArm => {
                if let Some(&(first, count, h)) = self.arms.last()
                    && a < count
                {
                    let at = first as usize + a as usize * self.words;
                    let mut reached = self.hit[(h + a) as usize];
                    self.join(at, &mut reached, st);
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
                    self.join(at as usize, &mut r, st);
                    self.brk[a as usize].1 = true;
                }
                false
            }
            Tag::Continue => {
                if let Some(&(at, reached)) = self.cont.get(a as usize)
                    && at != NONE
                {
                    let mut r = reached;
                    self.join(at as usize, &mut r, st);
                    self.cont[a as usize].1 = true;
                }
                false
            }
            Tag::Return => {
                self.uses(b, cx, x, st);
                self.union(self.exit, st);
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
        // A `move` of all of a value consumes it.
        let moved = match tag {
            Tag::Read if w == ReadMode::Move as u32 => match root_of(b, a) {
                (Root::Value(v), from, to) if from == to => v,
                _ => NONE,
            },
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
            let local = matches!(root_of(b, a).0, Root::Local(_));
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
                self.susp.resize(self.susp.len() + self.words, 0);
                self.susp_at[i] = at;
                self.points.push(x);
            }
            for k in 0..self.words {
                self.susp[at as usize + k] |= self.arena[st + k];
            }
            self.union(self.exit, st);
        }
        self.defined(x, st);
    }

    /// Loop `x` forward: its body from the head until the head stops
    /// growing; what follows it is what its `Break`s carry (and its else
    /// block, from the head).
    fn repeat(
        &mut self,
        b: &Body,
        cx: &dyn OwnCx,
        edits: &splice::Tables,
        x: u32,
        st: usize,
    ) -> bool {
        let [body, els] = b.data[x as usize];
        let label = self.label_of[x as usize];
        let head = self.alloc(Some(st));
        let brk = self.alloc(None);
        let cont = self.alloc(None);
        let end = self.alloc(None);
        let saved = (label != NONE).then(|| (self.brk[label as usize], self.cont[label as usize]));
        if label != NONE {
            self.brk[label as usize] = (u32_of(brk), false);
        }
        for _ in 0..self.words * 64 + 2 {
            if label != NONE {
                self.cont[label as usize] = (u32_of(cont), false);
            }
            self.copy(end, head);
            let mut grew = false;
            if self.block(b, cx, edits, body, end) {
                grew |= self.union(head, end);
            }
            if label != NONE && self.cont[label as usize].1 {
                grew |= self.union(head, cont);
            }
            if !grew {
                break;
            }
        }
        let mut falls = false;
        if label != NONE && self.brk[label as usize].1 {
            self.join(st, &mut falls, brk);
        }
        if els != NONE && b.tags.get(els as usize) == Some(&Tag::Block) {
            self.copy(end, head);
            if self.block(b, cx, edits, els, end) {
                self.join(st, &mut falls, end);
            }
        }
        if let Some((bs, cs)) = saved {
            self.brk[label as usize] = bs;
            self.cont[label as usize] = cs;
        }
        self.free(head);
        falls
    }

    /// Match `x` forward: its decision tree from `st`, each arm from what
    /// its `ToArm`s carry.
    fn decide(
        &mut self,
        b: &Body,
        cx: &dyn OwnCx,
        edits: &splice::Tables,
        x: u32,
        st: usize,
    ) -> bool {
        let r = b.record(b.data[x as usize][1]);
        let Some((&decision, arms)) = r.split_first() else {
            return true;
        };
        let label = self.label_of[x as usize];
        let saved = (label != NONE).then(|| self.brk[label as usize]);
        let first = self.arena.len();
        if label != NONE {
            let at = self.alloc(None);
            self.brk[label as usize] = (u32_of(at), false);
        }
        let arms_at = self.arena.len();
        for _ in arms {
            self.alloc(None);
        }
        let h = u32_of(self.hit.len());
        self.hit.resize(self.hit.len() + arms.len(), false);
        self.arms.push((u32_of(arms_at), u32_of(arms.len()), h));
        let _ = self.block(b, cx, edits, decision, st);
        self.arms.pop();
        let mut falls = false;
        for (k, &arm) in arms.iter().enumerate() {
            if !self.hit[h as usize + k] {
                continue;
            }
            let at = arms_at + k * self.words;
            if self.valued(b, cx, edits, arm, at) {
                self.join(st, &mut falls, at);
            }
        }
        self.hit.truncate(h as usize);
        if let Some(old) = saved {
            let (at, reached) = self.brk[label as usize];
            if reached {
                self.join(st, &mut falls, at as usize);
            }
            self.brk[label as usize] = old;
        }
        self.free(first);
        falls
    }
}
