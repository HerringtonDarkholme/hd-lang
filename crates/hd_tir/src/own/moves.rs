//! Move (`docs/compiler/tir-v2.md`, "Move"): a `copy` read whose place no
//! path reads again becomes a `move`, which takes the place's own count
//! and leaves it empty, and the `Drop` that then has nothing to release
//! goes.
//!
//! 1. **Locals.** A `copy` read of a local `[$l]`, or of a field of one
//!    (`[$l .a .b]`), after which no path reads `$l` again before a store
//!    replaces all of it, moves. A store into a field path that is all
//!    of the local (each struct on the way has that one field) replaces
//!    all of it too. Its local is not addressed and lives in
//!    no cell (a `Ref` or a shared capture reaches it), no cleanup suite
//!    touches it, no `borrow` reads it, and no `Pin` names a place in it:
//!    a pinned collection's copy is a clone (the pin's eager copy, until phase 10 of `docs/future-work/POINTER_LOANS.md`), which a
//!    move would skip. A part whose type no instance copies has a
//!    liveness bit of its own (`ref.move.field`): its read moves when no
//!    path touches that part again, though another part of the local is
//!    still read (`eat(p.a)` then `p.b.len()`); in a local a `Ref`
//!    addresses, only where no live reference use overlaps the part, and the
//!    local's `Drop` stays whole, the backend zeroing the moved part in
//!    the cell. A non-copyable local with no counted leaf owns no count,
//!    and its reads move all the same: a copy of it is never chosen.
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
//!    it goes. A later whole-value read can instead move the value out of
//!    the cell and release the emptied cell. Not in a sub-body that holds a suspension point (an
//!    `Await`, as known-null's points), whose frame saves the cell's local
//!    at each point. A frame local's `Ref` never takes a count
//!    (`ref_word::FRAME`); its final whole read may move the slot payload
//!    when insertion put the local's drop immediately after it. Elsewhere,
//!    as in a branch, a second walk counts each use of what may point into
//!    the slot as a read of it, drops aside (`frame`), and the read moves
//!    when no such use follows it on any path.
//! 5. **Captures at last use.** A `Copy` capture of all of a candidate
//!    local, after which no path reads the local again, becomes a `Move`
//!    when the local's `Drop` follows the closure in its block: the
//!    environment takes the local's count, and the drop goes. Not a
//!    generic local function's, whose instantiations share their captures.
//!    The backend keeps a lent parameter's copy, which holds no count.
//!
//! "No path reads it again" is a backward liveness walk over the
//! structured control flow of each sub-body, with one bit per candidate
//! local and temporary, kept at block boundaries and loop heads in one
//! reused arena; a loop runs to a fixed point. A read decides on the
//! state after it. A store into all of a local ends its life, a cell's
//! included, so a local declared in a loop body moves at its last read
//! of each round: the next round's declaration makes it anew. A `Drop`
//! is no read: a drop of a moved place releases nothing, since the
//! backend zeroes what a move empties.
//!
//! Why it is sound: a moved place is read on no path afterwards, so
//! nothing sees it empty; a `Store` after the move refills it and releases
//! the zero it holds; every drop that stays releases what is left, which
//! the move zeroed out of it. A drop that goes is one that only the moved
//! path reaches, straight after the move.

use hd_base::SliceSort as _;
use hd_types::Ty;

use super::flow::{Backward, BitArena, Flow, backward};
use super::reason::LAST_USE;
use super::{
    Operand, OwnCx, Role, Tri, fill, operands, owned_value, splice, suite_touches, tracked,
};
use crate::ir::{
    Body, CaptureMode, NONE, PlaceView, Proj, ReadMode, Root, Tag, Topology, for_blocks,
    is_suspension, place, sub_flags, u32_of,
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
    /// Locals a `Ref` addresses, which live in a cell: last whole reads
    /// and dead field paths may move; only a `Ref` of the whole local
    /// takes its count.
    pub(super) addr: Vec<bool>,
    /// Addressed locals held in frame slots, whose references never take
    /// a cell count. A whole read immediately before their inserted drop
    /// may still move the slot's payload out.
    frame: Vec<bool>,
    /// Per local: the `borrow` read a binding local holds (the borrow
    /// pass's), or `NONE`; and whether the body owns it (its exit row).
    pub(super) holds: Vec<u32>,
    owns: Vec<bool>,
    /// Per sub-body: whether it holds a suspension point (known-null's
    /// points, `is_suspension`).
    susp_sub: Vec<bool>,
    /// The liveness walk's states, one bit per candidate.
    pub(super) back: Flow,
    /// Per instruction: [`MOVE`] or [`KEEP`] for a candidate read the walk
    /// reached, from the state after it.
    decide: Vec<u8>,
    /// Per capture (an index into `Body::cap_local`): the same for a
    /// `Copy` capture of a candidate local (rule 5).
    cap_decide: Vec<u8>,
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
    /// The uses of a frame local through a value or a local that may
    /// point into it, but its drops: (instruction, candidate bit), by
    /// instruction. Empty unless a frame local's last whole read has no
    /// drop right after it (rule 4).
    frame_uses: Vec<(u32, u32)>,
    /// Per local: whether [`frame_uses`] holds its uses, so its last whole
    /// read needs no drop right after it.
    frame_read: Vec<bool>,
    frame_scratch: super::frame::Tables,
    /// Per-field liveness (rule 1 for a part no instance copies): a bit
    /// per part of a local that such a `copy` read reads, as (local, the
    /// read's place word, bit), in rows by local (`pstart`); and per
    /// instruction, its read's part bit or `NONE`.
    pbits: Vec<(u32, u32, u32)>,
    pstart: Vec<u32>,
    pread: Vec<u32>,
    part_uses: Vec<(u32, u32)>,
    part_keep: Vec<bool>,
    /// Per instruction: whether its value leaves its sub-body as the
    /// sub-body's result (a default argument's `&mut []`).
    returned: Vec<bool>,
    /// Per instruction: whether it is a read that lends its local as a
    /// local reference ([`super::lending_read`]), which never moves.
    lending: Vec<bool>,
    /// Per instruction: whether it is a store into a field path of a local
    /// that is all of the local's value ([`fills_local`]).
    fills: Vec<bool>,
}

impl Tables {
    fn reset(&mut self, n: usize, locals: usize) {
        fill(&mut self.lnum, locals, NONE);
        fill(&mut self.vnum, n, NONE);
        fill(&mut self.keep_l, locals, false);
        fill(&mut self.keep_v, n, false);
        fill(&mut self.addr, locals, false);
        fill(&mut self.frame, locals, false);
        fill(&mut self.holds, locals, NONE);
        fill(&mut self.owns, locals, false);
        fill(&mut self.decide, n, UNSEEN);
        self.touch.clear();
        self.cap_decide.clear();
        self.stack.clear();
        self.frame_uses.clear();
        fill(&mut self.frame_read, locals, false);
        self.pbits.clear();
        fill(&mut self.pstart, locals + 1, 0);
        fill(&mut self.pread, n, NONE);
        self.part_uses.clear();
        self.part_keep.clear();
        fill(&mut self.returned, n, false);
        fill(&mut self.lending, n, false);
        fill(&mut self.fills, n, false);
    }

    /// Calls `f` with the part bit of each part of local `l` that the
    /// place at `word` overlaps (one is a prefix of the other).
    fn part_bits(&self, b: &Body, l: u32, word: u32, f: &mut impl FnMut(u32)) {
        let (lo, hi) = (self.pstart[l as usize], self.pstart[l as usize + 1]);
        for &(_, pw, bit) in &self.pbits[lo as usize..hi as usize] {
            if overlaps(b, word, pw) {
                f(bit);
            }
        }
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
                        self.part_bits(b, l, b.data[i][0], f);
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
    returned(b, t);
    for i in 0..b.len() {
        t.lending[i] = super::lending_read(b, cx, i);
        t.fills[i] = fills_local(b, cx, i);
    }
    part_uses(b, cx, t);
    touches(b, t);
    let mut back = std::mem::take(&mut t.back);
    backward(b, topo, &mut back, t);
    // A frame local's references are no reads of it here. Where its last
    // whole read has no drop right after it, the walk runs again with
    // their uses as reads of it.
    let apart = (0..b.len()).any(|i| frame_move_apart(b, topo, t, i));
    if apart {
        frame_uses(b, topo, cx, t);
    }
    // A root local (`ref.box.root`) is read by each use of what points
    // into its object or buffer, so no move of it comes before one.
    if root_reads(b, cx, t) || apart {
        fill(&mut t.decide, b.len(), UNSEEN);
        backward(b, topo, &mut back, t);
    }
    t.back = back;
    apply(b, topo, cx, t, edits);
}

/// Whether instruction `i` is a `Store` into a field path of a local
/// (`[$l .a .b]`) where each struct on the way, laid out in place, has
/// that one field: the store replaces all of the local, as a whole store
/// does (`p.name = t` for a `Point` of one `name`).
pub(crate) fn fills_local(b: &Body, cx: &dyn OwnCx, i: usize) -> bool {
    if b.tags[i] != Tag::Store {
        return false;
    }
    let p = b.view(b.data[i][0]);
    let Some(l) = p.local() else {
        return false;
    };
    if p.whole() {
        return false;
    }
    let mut ty = b.local_ty[l as usize];
    let mut parts = Vec::new();
    for step in p.steps(b) {
        let Proj::Field(k) = step else {
            return false;
        };
        if k != 0 || !cx.parts(ty, None, &mut parts) || parts.len() != 1 {
            return false;
        }
        ty = parts[0];
    }
    true
}

/// Marks each value its sub-body returns as its result: the root block's
/// tail, through blocks, scopes and coercions that keep it. A local
/// reference returned so (a default argument's `&mut []`) takes its
/// cell, which nothing in the body outlives.
fn returned(b: &Body, t: &mut Tables) {
    for &r in &b.sub_root {
        let mut v = r;
        while crate::ir::Ref(v).as_inst().is_some()
            && (v as usize) < b.len()
            && !t.returned[v as usize]
        {
            t.returned[v as usize] = true;
            let [a, w] = b.data[v as usize];
            v = match b.tags[v as usize] {
                Tag::Block => w,
                Tag::Scope | Tag::Coerce => a,
                _ => break,
            };
        }
    }
}

/// Whether read `i` is a whole read of a frame local that the walk found
/// last, with no drop of the local right after it.
fn frame_move_apart(b: &Body, topo: &Topology, t: &Tables, i: usize) -> bool {
    if t.decide[i] != MOVE || topo.blk[i] == NONE {
        return false;
    }
    let word = b.data[i][0];
    let Root::Local(l) = b.view(word).root else {
        return false;
    };
    t.frame[l as usize]
        && b.view(word).whole()
        && t.bit(b, word).and_then(|bit| next_drop(b, topo, t, i, bit)) != Some(u32_of(i) + 1)
}

/// The uses of each frame candidate through what may point into it
/// ([`super::frame::run`]), but its drops: a drop of a value that points
/// into a local reads nothing there.
fn frame_uses(b: &mut Body, topo: &Topology, cx: &dyn OwnCx, t: &mut Tables) {
    let mut out = std::mem::take(&mut t.frame_uses);
    out.clear();
    let (lnum, frame) = (&t.lnum, &t.frame);
    let mut ft = std::mem::take(&mut t.frame_scratch);
    let tags = b.tags.clone();
    super::frame::run(b, topo, cx, &mut ft, |i, l, in_frame| {
        let bit = lnum[l as usize];
        if in_frame && frame[l as usize] && bit != NONE && tags[i as usize] != Tag::Drop {
            out.push((i, bit));
        }
    });
    out.sort_stable();
    out.dedup();
    for l in 0..t.frame.len() {
        t.frame_read[l] = t.frame[l] && ft.is_frame(l);
    }
    t.frame_scratch = ft;
    t.frame_uses = out;
}

/// Adds the uses of each root local through what may point into it
/// ([`super::frame::roots`]) to [`Tables::frame_uses`], drops aside:
/// whether there was any.
fn root_reads(b: &Body, cx: &dyn OwnCx, t: &mut Tables) -> bool {
    let mut out = std::mem::take(&mut t.frame_uses);
    let before = out.len();
    let lnum = &t.lnum;
    super::frame::roots(b, cx, |i, l| {
        let bit = lnum[l as usize];
        if bit != NONE && b.tags[i as usize] != Tag::Drop {
            out.push((i, bit));
        }
    });
    let added = out.len() > before;
    out.sort_stable();
    out.dedup();
    t.frame_uses = out;
    added
}

/// The drop of the whole place of read `i`, candidate `bit`, right after
/// the read in its block: the next instruction that touches the place.
fn next_drop(b: &Body, topo: &Topology, t: &Tables, i: usize, bit: u32) -> Option<u32> {
    let root = b.view(b.data[i][0]).root;
    t.next_touch(bit, u32_of(i)).filter(|&d| {
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
    })
}

/// Numbers the candidates: the counted locals the body owns and the
/// non-copyable locals with no counted leaf, that no reference, shared
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
                    if w & crate::ir::ref_word::FRAME != 0 {
                        t.frame[l.idx()] = true;
                    }
                }
            }
            // A binding local holds a borrowed value: it owns no count.
            Tag::Store if b.loan_read(w).is_some() => {
                let p = b.view(a);
                if let Some(l) = p.local().filter(|_| p.whole()) {
                    t.keep_l[l as usize] = true;
                    t.holds[l as usize] = b.loan_read(w).unwrap_or(w);
                }
            }
            Tag::Scope if w != NONE => keep_suites(b, t, b.record(w)),
            Tag::Defer => keep_suites(b, t, &[a]),
            _ => {}
        }
    }
    for (l, m) in b.cap_local.iter().zip(&b.cap_mode) {
        if m.shares() {
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
        if t.susp_sub[s.idx()] && t.addr[l.idx()] && !between_points(b, topo, l.raw()) {
            t.keep_l[l.idx()] = true;
        }
    }
    let mut k = 0u32;
    for l in 0..b.local_ty.len() {
        let counted = t.addr[l] || cx.counted(b.local_ty[l]);
        // A non-copyable local with no counted leaf owns no count, yet a
        // read of it moves too (`ref.move.sites`): its move takes the
        // value, as a copy of it would.
        let uncounted = !counted && cx.copies(b.local_ty[l]) != Tri::Yes;
        if !t.keep_l[l] && ((t.owns[l] && counted) || uncounted) {
            t.lnum[l] = k;
            k += 1;
        }
    }
    parts_of_locals(b, cx, t, &mut k);
    for i in 0..n {
        let [a, w] = b.data[i];
        if b.tags[i] != Tag::Read || w != ReadMode::Copy as u32 {
            continue;
        }
        let p = b.view(a);
        let Some(v) = p.value().map(|v| v as usize) else {
            continue;
        };
        if t.vnum[v] == NONE
            && !t.keep_v[v]
            && p.in_place(b)
            && owned_value(b, v)
            && (tracked(b, cx, v) || cx.copies(b.ty[v]) == Tri::No)
        {
            t.vnum[v] = k;
            k += 1;
        }
    }
    t.back.arena.words = (k as usize).div_ceil(64);
    k > 0
}

/// Per-field liveness: numbers from `*k` each part of a candidate local
/// that a `copy` read of a type no instance copies reads (`[$l .a]`, a
/// field of a field, a payload field), so that the read moves when no path
/// touches that part again, though another part of the local is still
/// read (`ref.move.field`). A local a `Ref` addresses, which lives in
/// memory, has one only for a path of fields. Uses through references
/// keep the part live until the borrow ends ([`part_uses`]).
fn parts_of_locals(b: &Body, cx: &dyn OwnCx, t: &mut Tables, k: &mut u32) {
    let mut found = Vec::new();
    for i in 0..b.len() {
        let [a, w] = b.data[i];
        if b.tags[i] != Tag::Read || w != ReadMode::Copy as u32 {
            continue;
        }
        let p = b.view(a);
        let Some(l) = p.local() else {
            continue;
        };
        if t.lnum[l as usize] == NONE
            || p.whole()
            || !p.in_place(b)
            || cx.copies(b.ty[i]) == Tri::Yes
            || (t.addr[l as usize] && !p.steps(b).all(|s| matches!(s, Proj::Field(_))))
        {
            continue;
        }
        found.push((l, a, u32_of(i)));
    }
    found.sort_stable_by_key(|&(l, _, i)| (l, i));
    for (l, a, i) in found {
        let same = t
            .pbits
            .iter()
            .rev()
            .take_while(|&&(m, _, _)| m == l)
            .find(|&&(_, pw, _)| {
                let (x, y) = (b.view(pw), b.view(a));
                b.extra[x.from as usize..x.to as usize] == b.extra[y.from as usize..y.to as usize]
            });
        let bit = if let Some(&(_, _, bit)) = same {
            bit
        } else {
            t.pbits.push((l, a, *k));
            t.pstart[l as usize + 1] += 1;
            *k += 1;
            *k - 1
        };
        t.pread[i as usize] = bit;
    }
    for l in 0..t.pstart.len() - 1 {
        t.pstart[l + 1] += t.pstart[l];
    }
}

/// Index reference uses of addressed field candidates before the backward
/// liveness walk. Queries use bounded batches, so the number of fields is
/// not limited by the origin analysis's machine-word representation.
fn part_uses(b: &Body, cx: &dyn OwnCx, t: &mut Tables) {
    let parts: Vec<_> = t
        .pbits
        .iter()
        .copied()
        .filter(|&(l, _, _)| t.addr[l as usize])
        .collect();
    if parts.is_empty() {
        return;
    }
    fill(&mut t.part_keep, t.back.arena.words * 64, false);
    let mut ft = std::mem::take(&mut t.frame_scratch);
    for chunk in parts.chunks(64) {
        let targets: Vec<_> = chunk.iter().map(|&(l, word, _)| (l, word)).collect();
        let escaped = super::frame::part_uses(b, cx, &mut ft, &targets, |i, k| {
            t.part_uses.push((i, chunk[k].2));
        });
        for (k, &(_, _, bit)) in chunk.iter().enumerate() {
            if escaped & (1 << k) != 0 {
                t.part_keep[bit as usize] = true;
            }
        }
    }
    t.frame_scratch = ft;
    t.part_uses.sort_stable();
    t.part_uses.dedup();
}

/// Whether the places at words `a` and `c` of one root overlap: the steps
/// of one are a prefix of the other's.
fn overlaps(b: &Body, a: u32, c: u32) -> bool {
    let (x, y) = (b.view(a), b.view(c));
    let (x, y) = (
        &b.extra[x.from as usize..x.to as usize],
        &b.extra[y.from as usize..y.to as usize],
    );
    x.iter().zip(y).all(|(u, v)| u == v)
}

/// Whether the place at word `a` holds all of the place at word `c` of
/// the same root: its steps are a prefix of the other's.
fn covers(b: &Body, a: u32, c: u32) -> bool {
    let (x, y) = (b.view(a), b.view(c));
    x.to - x.from <= y.to - y.from && overlaps(b, a, c)
}

/// Whether local `l` lives only between suspension points: every place
/// rooted at it lies in one block whose statements hold no suspension
/// point, and the first of them in that block is a store of all of it,
/// so each run of the block makes it anew (the locals of a body the
/// inliner copied into a suspending one). Then no resume enters its
/// life, the frame saves no value of it, and its cell may move as in a
/// plain body.
fn between_points(b: &Body, topo: &Topology, l: u32) -> bool {
    if b.cap_local.iter().any(|c| c.raw() == l) {
        return false;
    }
    let mut touches = Vec::new();
    for i in 0..b.len() {
        if matches!(
            b.tags[i],
            Tag::Read | Tag::Store | Tag::Ref | Tag::Drop | Tag::IsUnique
        ) && b.view(b.data[i][0]).local() == Some(l)
        {
            if topo.blk[i] == NONE {
                return false;
            }
            touches.push(u32_of(i));
        }
    }
    let Some(&first) = touches.first() else {
        return false;
    };
    // The innermost block that holds every touch.
    let chains: Vec<Vec<u32>> = touches
        .iter()
        .map(|&i| topo.enclosing(i).collect())
        .collect();
    let Some(&lca) = chains[0]
        .iter()
        .find(|blk| chains.iter().all(|c| c.contains(blk)))
    else {
        return false;
    };
    if (0..u32_of(b.len()))
        .filter(|&p| is_suspension(b.tags[p as usize]))
        .any(|p| topo.enclosing(p).any(|blk| blk == lca))
    {
        return false;
    }
    // The statement of `lca` that holds each touch, and the first.
    let top = |i: u32| -> u32 {
        let mut x = i;
        while topo.blk[x as usize] != lca {
            x = topo.blk[x as usize];
        }
        x
    };
    let decl = touches
        .iter()
        .copied()
        .min_by_key(|&i| (topo.pos[top(i) as usize], i))
        .unwrap_or(first);
    let whole = b.tags[decl as usize] == Tag::Store
        && topo.blk[decl as usize] == lca
        && b.local_in_place(b.data[decl as usize][0])
            .is_some_and(|x| x.raw() == l)
        && b.view(b.data[decl as usize][0]).whole();
    whole
        && touches
            .iter()
            .all(|&i| i == decl || topo.pos[top(i) as usize] > topo.pos[decl as usize])
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
                Tag::Read | Tag::Store | Tag::Ref | Tag::Drop | Tag::IsUnique => {
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
            Tag::Read | Tag::Ref | Tag::IsUnique => {
                // A read through a binding local reads what its read lives on.
                self.place_deps(b, a, &mut |bit| s.set(live, bit, true));
                if let Some(bit) = self.bit(b, a) {
                    let p = b.view(a);
                    let addr = matches!(p.root, Root::Local(l) if self.addr[l as usize]);
                    let last = if s.get(live, bit) { KEEP } else { MOVE };
                    match b.tags[i] {
                        // A part with a bit of its own decides on it.
                        Tag::Read if w == ReadMode::Copy as u32 && self.pread[i] != NONE => {
                            let pbit = self.pread[i];
                            self.decide[i] = if s.get(live, pbit)
                                || self.part_keep.get(pbit as usize) == Some(&true)
                            {
                                KEEP
                            } else {
                                MOVE
                            };
                        }
                        Tag::Read
                            if w == ReadMode::Copy as u32
                                && p.in_place(b)
                                && !self.lending[i]
                                && (!addr || p.whole()) =>
                        {
                            self.decide[i] = last;
                        }
                        // A local reference never takes its local's count
                        // (`ref.count.local`): the local lives until the
                        // reference's last use instead (frame rule 4).
                        Tag::Ref
                            if addr
                                && p.whole()
                                && (w & crate::ir::ref_word::LOCAL == 0 || self.returned[i])
                                && !matches!(p.root, Root::Local(l) if self.frame[l as usize]) =>
                        {
                            self.decide[i] = if last == MOVE { TAKE } else { KEEP };
                        }
                        _ => {}
                    }
                    s.set(live, bit, true);
                    if let Root::Local(l) = p.root {
                        self.part_bits(b, l, a, &mut |pb| s.set(live, pb, true));
                    }
                }
            }
            Tag::Store => {
                let p = b.view(a);
                if let Some(l) = p.local()
                    && self.lnum[l as usize] != NONE
                {
                    // A store into all of the local replaces it, in a
                    // cell too: a store makes the cell a move emptied, and
                    // no reference into the old value lives past a write
                    // (`ref.excl.shared`), nor past a loop body's
                    // declaration (`ref.excl.declare`). Into a part, it
                    // keeps the rest.
                    let whole = p.whole() || self.fills[i] && !self.addr[l as usize];
                    s.set(live, self.lnum[l as usize], !whole);
                    // A part the store replaces is dead before it; one it
                    // writes into keeps the rest.
                    let (lo, hi) = (self.pstart[l as usize], self.pstart[l as usize + 1]);
                    for &(_, pw, pb) in &self.pbits[lo as usize..hi as usize] {
                        if covers(b, a, pw) {
                            s.set(live, pb, false);
                        } else if overlaps(b, a, pw) {
                            s.set(live, pb, true);
                        }
                    }
                }
            }
            Tag::Closure => {
                let rec = b.record(w);
                // A generic local function's instantiations share their
                // captures: those keep their copies.
                let generic = b
                    .sub_flags
                    .get(a as usize)
                    .is_some_and(|f| f & sub_flags::GENERIC != 0)
                    || b.is_instantiation(i);
                for c in rec[0] as usize..(rec[0] + rec[1]) as usize {
                    let l = b.cap_local[c].idx();
                    if self.lnum[l] != NONE {
                        // Rule 5: a copy of a whole local no path reads
                        // again moves into the environment.
                        let whole = self.pstart[l] == self.pstart[l + 1] && !self.addr[l];
                        if b.cap_mode.get(c) == Some(&CaptureMode::Copy) && whole && !generic {
                            if self.cap_decide.len() < b.cap_local.len() {
                                self.cap_decide.resize(b.cap_local.len(), UNSEEN);
                            }
                            let last = if s.get(live, self.lnum[l]) {
                                KEEP
                            } else {
                                MOVE
                            };
                            self.cap_decide[c] = self.cap_decide[c].max(last);
                        }
                        s.set(live, self.lnum[l], true);
                        let (lo, hi) = (self.pstart[l], self.pstart[l + 1]);
                        for &(_, _, pb) in &self.pbits[lo as usize..hi as usize] {
                            s.set(live, pb, true);
                        }
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

    /// A use of a frame local through what may point into it.
    fn reads(&mut self, _: &Body, x: u32, s: &mut BitArena, live: usize) {
        let k = self.frame_uses.partition_point(|u| u.0 < x);
        for &(_, bit) in self.frame_uses[k..].iter().take_while(|u| u.0 == x) {
            s.set(live, bit, true);
        }
        let k = self.part_uses.partition_point(|u| u.0 < x);
        for &(_, bit) in self.part_uses[k..].iter().take_while(|u| u.0 == x) {
            s.set(live, bit, true);
        }
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
            // Not in a suspending sub-body (invariant 23): a cell between
            // its points moves by a read, and a `Ref` keeps its count.
            if !t.susp_sub[topo.sub[i] as usize] {
                take(b, topo, t, edits, i);
            }
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
        let drop = next_drop(b, topo, t, i, bit);
        if from == to {
            // A frame slot empties at its last whole read: insertion put the
            // local's drop directly after its last use, the read itself, or
            // the walk read every use through a reference into it
            // ([`frame_uses`]).
            if matches!(root, Root::Local(l) if t.frame[l as usize] && !t.frame_read[l as usize])
                && drop != Some(x + 1)
            {
                continue;
            }
            // A tracked whole value moves only when its drop goes with
            // it. An uncounted non-copyable value needs no runtime drop.
            if drop.is_none() && matches!(root, Root::Value(v) if tracked(b, cx, v as usize)) {
                continue;
            }
            b.data[i][1] = ReadMode::Move as u32;
            if let Some(d) = drop {
                edits.remove(d);
            }
            continue;
        }
        // A copyable part with no counted leaf copies for free: moving it
        // would only split the base's drop into one drop per other counted
        // part.
        if !cx.counted(b.ty[i]) && cx.copies(b.ty[i]) == Tri::Yes {
            continue;
        }
        b.data[i][1] = ReadMode::Move as u32;
        // A local in memory keeps its whole `Drop`, which releases the
        // rest, the moved part being zero (the backend's move of a part
        // out of a cell).
        if matches!(root, Root::Local(l) if t.addr[l as usize]) {
            continue;
        }
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
                edits.insert(blk, pos, Tag::Drop, [at, LAST_USE], Ty::VOID, syn);
            }
        }
    }
    for v in 0..b.len() {
        if t.vnum[v] != NONE {
            sink(b, topo, cx, t, edits, u32_of(v));
        }
    }
    capture_moves(b, topo, t, edits);
}

/// Rule 5: each `Copy` capture the walk proved last becomes a `Move` when
/// the local's `Drop` follows the closure in its block, and that drop
/// goes: the environment takes the local's count.
fn capture_moves(b: &mut Body, topo: &Topology, t: &Tables, edits: &mut splice::Tables) {
    if !t.cap_decide.contains(&MOVE) {
        return;
    }
    for i in 0..b.len() {
        if b.tags[i] != Tag::Closure || topo.blk[i] == NONE {
            continue;
        }
        let rec = b.record(b.data[i][1]);
        for c in rec[0] as usize..(rec[0] + rec[1]) as usize {
            if t.cap_decide.get(c) != Some(&MOVE) {
                continue;
            }
            let l = b.cap_local[c].raw();
            let drop = t.next_touch(t.lnum[l as usize], u32_of(i)).filter(|&d| {
                let d = d as usize;
                b.tags[d] == Tag::Drop
                    && topo.blk[d] == topo.blk[i]
                    && b.data[d][0] == l | place::BARE_LOCAL
            });
            if let Some(d) = drop {
                b.cap_mode[c] = CaptureMode::Move;
                edits.remove(d);
            }
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
        edits.insert(
            blk,
            pos,
            Tag::Drop,
            [at, LAST_USE],
            Ty::VOID,
            b.syn[d as usize],
        );
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
            if x < start || u32_of(k) == field || !cx.counted(t.parts[k]) {
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
