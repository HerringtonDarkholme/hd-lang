//! Pair cancellation (`docs/compiler/tir-v2.md`, "Pair cancellation"): a
//! `copy` of a part of a place and a later `Drop` of that place, or of a
//! place that holds it, with nothing in between that can tell, cancel. The
//! copy becomes a `move`, which takes the part's own count, and the drop
//! goes; a drop of a larger place becomes drops of the parts beside the
//! one the read took, as move's rule 3 splits a drop.
//!
//! This is the case move leaves (`moves`): its liveness is per local and
//! per temporary, so a part read of a base that is read again later
//! copies, as `n` in `.Square(n, k)`, where the decision tree reads `n`
//! before `k`, and the base's drop releases `n` afterwards. The plan's
//! other case, a copy into a temporary dropped at the end of its statement
//! while the place outlives it, is borrow's as built (`borrow`): it turns
//! every such copy into a `borrow` whose span holds no write, no call that
//! reaches the place and no suspension point, which is this pass's check
//! and more.
//!
//! A read `r` of part `p` of a local or a temporary that move's candidates
//! name (an owned counted local no reference, closure or cleanup suite
//! reaches, or an owned temporary a `copy` reads a part of) pairs with the
//! first later instruction of its block that touches `p` or all of the
//! base, when that is a `Drop` of `p` or of a place holding `p`, and:
//!
//! 1. **A straight line.** Every statement between them completes and runs
//!    no block: no branch, loop, match, jump or suspension point. So every
//!    path from the read reaches the drop.
//! 2. **Nothing reads or writes the part.** No instruction between them
//!    touches `p`, all of the base, or a borrowed value that lives on the
//!    base (it would see the part empty); other parts may be touched. For
//!    a temporary, nothing but other parts' moves and drops may touch it
//!    at all (the verifier's rule for a value taken in parts).
//! 3. **Laid out in place.** Each step of `p` is a part of its base in
//!    every instance ([`OwnCx::parts`]), or the drop stays whole and
//!    releases what the move left.
//!
//! Why it is sound: the base holds the part's count from the read to the
//! drop, and nothing in between can see the part or the count, so the
//! consumer may take that count at the read instead of one of its own; the
//! drop then has nothing left there to release.
//!
//! One backward walk over each block's statements, with the move pass's
//! and the borrow pass's edits as they stand (a deleted statement is gone,
//! a new `Drop` is in its place), keeps for each candidate base the nearest
//! later touch of all of it and of each of its first-level parts: dense
//! tables indexed by the move pass's candidate bits, stamped per straight
//! line. The edits join the pipeline's last rebuild.

use super::moves::{self, Root, in_place, root_of};
use super::splice::{self, for_blocks};
use super::{Loc, Operand, OwnCx, Tri, operands};
use crate::ir::{Body, NONE, ReadMode, Tag, is_suspension, place};

/// The nearest later touch seen so far: the straight line it is in, its
/// order in the walk (a larger number is nearer), and the instruction or
/// edit (`n + edit`).
#[derive(Clone, Copy)]
struct Slot {
    stamp: u32,
    time: u32,
    item: u32,
}

const EMPTY: Slot = Slot {
    stamp: NONE,
    time: 0,
    item: NONE,
};

/// Pair cancellation's dense tables, reused across bodies.
#[derive(Default)]
pub(crate) struct Tables {
    /// Per instruction: a statement an earlier pass deleted.
    dead: Vec<bool>,
    /// Each block's edits (by block instruction), by position.
    row_start: Vec<u32>,
    row: Vec<u32>,
    /// Per candidate base (the move pass's bit): its first-level parts'
    /// slots in `acc` (`off`, `width` of them), its whole slot, and the
    /// nearest touch that takes no part (a temporary's).
    off: Vec<u32>,
    width: Vec<u32>,
    acc: Vec<Slot>,
    whole: Vec<Slot>,
    other: Vec<Slot>,
    /// Per `Drop` item: the slot it replaced, restored if it goes.
    prev: Vec<Slot>,
    ops: Vec<Operand>,
    bits: Vec<u32>,
    stamp: u32,
    time: u32,
}

fn u32_of(i: usize) -> u32 {
    u32::try_from(i).expect("insts")
}

/// The first-level part a place's projection words `from..to` name: the
/// field of its first step (after a variant), or `None` for all of the
/// base or a step that is no part of it.
fn first_part(b: &Body, from: u32, to: u32) -> Option<u32> {
    if from >= to {
        return None;
    }
    let w = b.extra[from as usize];
    match w & place::PROJ_KIND {
        place::PROJ_FIELD => Some(w & place::PROJ_INDEX),
        place::PROJ_VARIANT if from + 1 < to => {
            let f = b.extra[from as usize + 1];
            (f & place::PROJ_KIND == place::PROJ_FIELD).then_some(f & place::PROJ_INDEX)
        }
        _ => None,
    }
}

/// One touch of a candidate base by an item.
#[derive(Clone, Copy)]
struct Touch {
    bit: u32,
    part: Option<u32>,
    /// A `move` read or a `Drop`: takes what it names.
    takes: bool,
}

impl Tables {
    fn valid(&self, s: Slot) -> bool {
        s.item != NONE && s.stamp == self.stamp
    }

    /// Records item `x`'s touch at the current time.
    fn record(&mut self, x: u32, t: Touch, is_drop: bool) {
        let slot = Slot {
            stamp: self.stamp,
            time: self.time,
            item: x,
        };
        let at = t.part.and_then(|k| {
            (k < self.width[t.bit as usize]).then(|| (self.off[t.bit as usize] + k) as usize)
        });
        let old = match (t.part, at) {
            (None, _) => std::mem::replace(&mut self.whole[t.bit as usize], slot),
            (Some(_), Some(at)) => std::mem::replace(&mut self.acc[at], slot),
            // A part no read names: nothing asks for it.
            (Some(_), None) => EMPTY,
        };
        if is_drop {
            self.prev[x as usize] = old;
        }
        if !t.takes {
            self.other[t.bit as usize] = slot;
        }
    }
}

/// Runs pair cancellation over `b` with the borrow and move passes' edits
/// in `edits`, adding its own.
pub(crate) fn run(
    b: &mut Body,
    cx: &dyn OwnCx,
    mt: &mut moves::Tables,
    t: &mut Tables,
    edits: &mut splice::Tables,
) {
    let bits = mt.words * 64;
    if bits == 0 {
        return;
    }
    let n = b.len();
    t.dead.clear();
    t.dead.resize(n, false);
    for &(_, x) in edits.removed() {
        t.dead[x as usize] = true;
    }
    // Each block's edits, by position, in recording order.
    let k = edits.len();
    t.row_start.clear();
    t.row_start.resize(n + 1, 0);
    for e in 0..k {
        let (blk, ..) = edits.get(u32_of(e));
        t.row_start[blk as usize + 1] += 1;
    }
    for i in 0..n {
        t.row_start[i + 1] += t.row_start[i];
    }
    t.row.clear();
    t.row.resize(k, 0);
    t.bits.clear();
    t.bits.extend_from_slice(&t.row_start[..n]);
    for e in 0..k {
        let (blk, ..) = edits.get(u32_of(e));
        t.row[t.bits[blk as usize] as usize] = u32_of(e);
        t.bits[blk as usize] += 1;
    }
    for i in 0..n {
        let (a, z) = (t.row_start[i] as usize, t.row_start[i + 1] as usize);
        if z - a > 1 {
            t.row[a..z].sort_by_key(|&e| (edits.get(e).1, e));
        }
    }
    // The first-level parts any place names of each candidate base.
    t.width.clear();
    t.width.resize(bits, 0);
    let widen = |t: &mut Tables, word: u32| {
        if let Some(bit) = candidate(mt, b, word) {
            let (_, from, to) = root_of(b, word);
            if let Some(f) = first_part(b, from, to) {
                let w = &mut t.width[bit as usize];
                *w = (*w).max(f + 1);
            }
        }
    };
    for i in 0..n {
        if is_place(b.tags[i]) {
            widen(t, b.data[i][0]);
        }
    }
    for e in 0..k {
        widen(t, edits.get(u32_of(e)).3[0]);
    }
    t.off.clear();
    let mut total = 0;
    for bit in 0..bits {
        t.off.push(total);
        total += t.width[bit];
    }
    t.acc.clear();
    t.acc.resize(total as usize, EMPTY);
    t.whole.clear();
    t.whole.resize(bits, EMPTY);
    t.other.clear();
    t.other.resize(bits, EMPTY);
    t.prev.clear();
    t.prev.resize(n + k, EMPTY);
    t.stamp = 0;
    t.time = 0;
    for blk in 0..n {
        if b.tags[blk] == Tag::Block {
            block(b, cx, mt, t, edits, u32_of(blk));
        }
    }
}

fn is_place(tag: Tag) -> bool {
    matches!(
        tag,
        Tag::Read | Tag::Ref | Tag::Store | Tag::Drop | Tag::IsUnique | Tag::Pin
    )
}

/// The candidate bit of the root of place `word`, if it has one. A local
/// a reference addresses lives in a cell, whose other holders may read
/// it: it is no candidate here, as no read of it moves.
fn candidate(mt: &moves::Tables, b: &Body, word: u32) -> Option<u32> {
    let x = match root_of(b, word).0 {
        Root::Local(l) if mt.addr[l as usize] => NONE,
        Root::Local(l) => mt.lnum[l as usize],
        Root::Value(v) => mt.vnum[v as usize],
        Root::Other => NONE,
    };
    (x != NONE).then_some(x)
}

/// Block `blk`'s statements and edits, backward.
fn block(
    b: &mut Body,
    cx: &dyn OwnCx,
    mt: &mut moves::Tables,
    t: &mut Tables,
    edits: &mut splice::Tables,
    blk: u32,
) {
    t.stamp += 1;
    let n = u32_of(b.len());
    let rec = b.data[blk as usize][0] as usize;
    let len = b.extra[rec] as usize;
    let (r0, r1) = (
        t.row_start[blk as usize] as usize,
        t.row_start[blk as usize + 1] as usize,
    );
    let mut y = r1;
    for p in (0..=len).rev() {
        if p < len {
            let m = b.extra[rec + 1 + p];
            if !t.dead[m as usize] {
                statement(b, cx, mt, t, edits, m);
            }
        }
        // The edits before position `p`, backward.
        while y > r0 && edits.get(t.row[y - 1]).1 as usize >= p {
            y -= 1;
            let e = t.row[y];
            if edits.get(e).1 as usize == p {
                edit(b, mt, t, edits, e, n);
            }
        }
    }
}

/// A new `Drop` edit `e`, as a touch of its base.
fn edit(b: &Body, mt: &moves::Tables, t: &mut Tables, edits: &splice::Tables, e: u32, n: u32) {
    let (_, _, tag, [word, _], _) = edits.get(e);
    if tag != Tag::Drop {
        t.stamp += 1;
        return;
    }
    t.time += 1;
    if let Some(bit) = candidate(mt, b, word) {
        let (_, from, to) = root_of(b, word);
        let touch = Touch {
            bit,
            part: first_part(b, from, to),
            takes: true,
        };
        t.record(n + e, touch, true);
    }
}

/// Statement `m`, backward: a straight line ends before it unless it
/// completes and runs no block; a `copy` of a part may pair with the
/// nearest later touch of its base; then its touches.
fn statement(
    b: &mut Body,
    cx: &dyn OwnCx,
    mt: &mut moves::Tables,
    t: &mut Tables,
    edits: &mut splice::Tables,
    m: u32,
) {
    let i = m as usize;
    let tag = b.tags[i];
    // A block in the list runs where the statement that owns it runs it,
    // which ends the straight line there.
    if tag == Tag::Block {
        return;
    }
    let mut runs = false;
    for_blocks(b, i, |_| runs = true);
    if runs
        || matches!(
            tag,
            Tag::Break | Tag::Continue | Tag::Return | Tag::ToArm | Tag::Unreachable
        )
        || b.ty[i] == hd_types::Ty::NEVER
        || is_suspension(tag)
    {
        t.stamp += 1;
        return;
    }
    t.time += 1;
    let [a, w] = b.data[i];
    if tag == Tag::Read && w == ReadMode::Copy as u32 {
        pair(b, cx, mt, t, edits, m);
    }
    // The touches: the place's root, then what its operands and a binding
    // local at its root live on, as all of a base.
    let [a, w] = [a, b.data[i][1]];
    let mut root_value = NONE;
    if is_place(tag) {
        let (root, from, to) = root_of(b, a);
        if let Root::Value(v) = root {
            root_value = v;
        }
        if let Some(bit) = candidate(mt, b, a) {
            let takes = tag == Tag::Drop || (tag == Tag::Read && w == ReadMode::Move as u32);
            let part = if in_place(b, from, to) {
                first_part(b, from, to)
            } else {
                None
            };
            t.record(m, Touch { bit, part, takes }, tag == Tag::Drop);
        }
        if let Root::Local(l) = root
            && mt.holds[l as usize] != NONE
        {
            t.bits.clear();
            let bits = &mut t.bits;
            mt.deps(b, mt.holds[l as usize], &mut |bit| bits.push(bit));
            whole(t, m);
        }
    }
    if tag == Tag::Closure {
        let rec = b.record(w);
        t.bits.clear();
        for c in rec[0] as usize..(rec[0] + rec[1]) as usize {
            let l = b.cap_local[c].idx();
            if mt.lnum[l] != NONE {
                t.bits.push(mt.lnum[l]);
            }
        }
        whole(t, m);
    }
    let mut ops = std::mem::take(&mut t.ops);
    operands(b, i, &mut ops);
    t.bits.clear();
    for op in &ops {
        if op.value == root_value && op.at == Loc::Extra(a + 2) {
            continue;
        }
        if mt.vnum[op.value as usize] != NONE {
            t.bits.push(mt.vnum[op.value as usize]);
        }
        let bits = &mut t.bits;
        mt.deps(b, op.value, &mut |bit| bits.push(bit));
    }
    t.ops = ops;
    whole(t, m);
}

/// Records item `m`'s touch of all of each base in `t.bits`.
fn whole(t: &mut Tables, m: u32) {
    for k in 0..t.bits.len() {
        let bit = t.bits[k];
        let touch = Touch {
            bit,
            part: None,
            takes: false,
        };
        t.record(m, touch, false);
    }
}

/// `copy` read `r`: when it reads a part of a candidate base and the
/// nearest later touch of that part is a `Drop` that covers it (see the
/// module), the read moves and the drop goes or splits.
fn pair(
    b: &mut Body,
    cx: &dyn OwnCx,
    mt: &mut moves::Tables,
    t: &mut Tables,
    edits: &mut splice::Tables,
    r: u32,
) {
    let word = b.data[r as usize][0];
    let Some(bit) = candidate(mt, b, word) else {
        return;
    };
    let (root, from, to) = root_of(b, word);
    if !in_place(b, from, to) || cx.counted(b.ty[r as usize]) == Tri::No {
        return;
    }
    let Some(key) = first_part(b, from, to) else {
        return;
    };
    let n = u32_of(b.len());
    let part = t.acc[(t.off[bit as usize] + key) as usize];
    let all = t.whole[bit as usize];
    let near = match (t.valid(part), t.valid(all)) {
        (true, true) if part.time > all.time => part,
        (true, false) => part,
        (_, true) => all,
        (false, false) => return,
    };
    // A temporary taken in parts may only lose more parts before the drop.
    let other = t.other[bit as usize];
    if matches!(root, Root::Value(_)) && t.valid(other) && other.time > near.time {
        return;
    }
    let d = near.item;
    let (dword, blk, pos, syn) = if d < n {
        if b.tags[d as usize] != Tag::Drop {
            return;
        }
        (
            b.data[d as usize][0],
            mt.blk[d as usize],
            mt.pos[d as usize],
            b.syn[d as usize],
        )
    } else {
        let (blk, pos, tag, [w, _], syn) = edits.get(d - n);
        if tag != Tag::Drop {
            return;
        }
        (w, blk, pos, syn)
    };
    let (droot, qfrom, qto) = root_of(b, dword);
    let qlen = qto - qfrom;
    // The drop's place holds the read's: the same root, a prefix of its
    // steps, ending at a whole step.
    if droot != root
        || qlen > to - from
        || b.extra[qfrom as usize..qto as usize] != b.extra[from as usize..(from + qlen) as usize]
        || (qlen > 0 && b.extra[qto as usize - 1] & place::PROJ_KIND == place::PROJ_VARIANT)
    {
        return;
    }
    let base = match root {
        Root::Local(l) => b.local_ty[l as usize],
        Root::Value(v) => b.ty[v as usize],
        Root::Other => return,
    };
    let exact = qlen == to - from;
    let split = moves::rest(b, cx, mt, base, (from, from + qlen, to));
    if exact && !split {
        // A part drop names a part laid out in place; a move of one that
        // is not would read a copy, which the drop must then release.
        return;
    }
    b.data[r as usize][1] = ReadMode::Move as u32;
    if !split {
        // The drop stays whole and releases what the move left.
        return;
    }
    if d < n {
        edits.remove(blk, d);
    } else {
        edits.cancel(d - n);
    }
    // The drop's slot goes back to what it replaced.
    let restored = t.prev[d as usize];
    if qlen == 0 {
        t.whole[bit as usize] = restored;
    } else {
        t.acc[(t.off[bit as usize] + key) as usize] = restored;
    }
    for k in 0..mt.drops.len() {
        let at = mt.drops[k];
        let e = edits.insert(blk, pos, Tag::Drop, [at, NONE], hd_types::Ty::VOID, syn);
        let item = n + e;
        if t.prev.len() <= item as usize {
            t.prev.resize(item as usize + 1, EMPTY);
        }
        let (_, f, z) = root_of(b, at);
        let Some(k) = first_part(b, f, z).filter(|&k| k < t.width[bit as usize]) else {
            continue;
        };
        let x = (t.off[bit as usize] + k) as usize;
        if !t.valid(t.acc[x]) || t.acc[x].time < near.time {
            t.prev[item as usize] = t.acc[x];
            t.acc[x] = Slot {
                stamp: t.stamp,
                time: near.time,
                item,
            };
        }
    }
}
