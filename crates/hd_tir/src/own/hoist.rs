//! Hoisting (`docs/compiler/tir-v2.md`, "Hoisting"): an `IsUnique` of a
//! collection place `p` inside a loop moves to just before the loop,
//! guarded by the loop's entry test, so the loop makes the buffer its own
//! once instead of on every change; and the same proof marks the loop's
//! **stable list places**, whose header and buffer nothing in the loop
//! replaces, in `Body::stable_lists`, which the backend reads to keep a
//! buffer's address and length in locals for the loop.
//!
//! An `IsUnique` hoists when:
//!
//! 1. it runs on every iteration that is entered: it is a statement of the
//!    loop's iteration block, the arm of the **entry test** (the loop
//!    body's first `If`, one arm of which only breaks out of the loop) that
//!    goes on, with no `If` or `Match` around it, or of the iteration block
//!    of an inner loop with an entry test that stands there in turn, at any
//!    depth, so it runs whenever that inner loop is entered; and the
//!    statements before the entry test are pure, so the backend can run
//!    them once more before the loop as its guard;
//! 2. the loop never copies `p` or a place holding it, takes no `Ref` of a
//!    place that overlaps it, writes no place that holds it, pins nothing
//!    and holds no suspension point;
//! 3. no call in the loop may write, unless `p`'s root is a local that is
//!    never addressed or captured, whose every stored value is fresh, and
//!    `p` is that whole local, and not a parameter's. Any other
//!    `p` (behind a reference, a field, a parameter's or a shared header)
//!    may also be reached through another reference or list, so nothing
//!    in the loop may write through a dereference or into a list's buffer
//!    unless its place is `p` or below it, or change, copy or pin another
//!    collection.
//!
//! `p` is the place the `IsUnique`'s header read reads (`%h = Read
//! [*$0]` makes `IsUnique [%h]` one of `[*$0]`), rooted at a local that is
//! never addressed or captured, with one leading dereference at most and
//! fields; its header is read nowhere in the body with a use that keeps
//! that very header (a store, an owned argument, a block's value, a
//! coercion, a pin; a copy or a snapshot outside the loop has a header of
//! its own, which the hoisted check separates from `p`'s), no
//! `borrow` of it lives across the loop's entry, and its type names no
//! type parameter. When the loop hoists `p`, every other `IsUnique` of `p`
//! in it, at any depth, goes too; one behind a dereference stays, marked
//! `COVERED`, for the instances where a reference view's getter makes the
//! header fresh, and the backend skips it where no view is reachable.
//! Loops are taken outermost first, so an outer loop's hoist covers the
//! inner loops' `IsUnique`s.
//!
//! A stable list place is a place of that form, read by an element read,
//! an element store or a `ListLen` in the loop, that passes rules 2 and 3
//! and has no `IsUnique` left in the loop and no push.
//!
//! Why it is sound: a buffer made unique before the loop stays unique,
//! since nothing in the loop can make a second owner of it (rule 2: no
//! copy, snapshot or reference of the header, which is held nowhere
//! else) and nothing outside the body can reach the place while the loop
//! runs (rule 3). An empty loop runs no `IsUnique`, and uniqueness is never
//! observable but through its cost. One taken from an inner loop costs a
//! copy the loop did not need only when the buffer is shared and no
//! iteration enters that inner loop: one copy per entry at most, against
//! a check per entry of the inner loop.
//!
//! Data layout: dense tables per instruction and local, reused; the uses
//! of every value in compressed rows; one scan to gather a loop's
//! candidates and one to check them, both over the loop's instruction
//! range, a loop's instructions being the range from its first to itself;
//! the edits join the other passes' for the one rebuild.

use hd_base::SliceSort as _;
use hd_types::Ty;

use super::splice;
use super::{Operand, OwnCx, Role, Tri, fill, operands};
use crate::ir::{
    Body, Callee, DEREF, ELEM, IntrinsicOp, NONE, PlaceView, Proj, ReadMode, Ref, Root, Tag,
    Topology, VARIANT, is_suspension, local_flags, place, u32_of, unique_word,
};

/// The longest chain of reads a place is resolved through.
const DEPTH: usize = 8;

/// The most candidates one loop checks.
const MAX: usize = 64;

/// A loop's entry test (`docs/compiler/tir-v2.md`, "Hoisting"): the first
/// `If` of the loop's body block one arm of which only breaks out of the
/// loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryTest {
    /// The `If`.
    pub test: u32,
    /// The arm that goes on: the iteration block.
    pub iteration: u32,
    /// Whether the loop leaves when the condition holds (the `If`'s first
    /// arm breaks).
    pub leaves_on_true: bool,
    /// How many statements of the body block stand before the `If`.
    pub prefix: usize,
}

/// The entry test of `Loop` `l`, if it has one: its [`loop_test`], with
/// an arm that goes on.
#[must_use]
pub fn entry_test(b: &Body, l: u32) -> Option<EntryTest> {
    let (test, prefix, [then, els]) = loop_test(b, l, false)?;
    let arms = b.record(b.data[test as usize][1]);
    let iteration = arms.get(usize::from(then)).copied().unwrap_or(NONE);
    (then != els && iteration != NONE).then_some(EntryTest {
        test,
        iteration,
        leaves_on_true: then,
        prefix,
    })
}

/// The statements of `Loop` `l`'s body block, without the blocks among
/// them.
pub fn loop_statements(b: &Body, l: u32) -> impl Iterator<Item = u32> + '_ {
    let body = b.data[l as usize][0];
    b.record(b.data[body as usize][0])
        .iter()
        .copied()
        .filter(|&x| b.tags[x as usize] != Tag::Block)
}

/// The test of `Loop` `l`: the first `If` among its statements, how many
/// statements stand before it, and whether each of its arms (then, else)
/// only breaks out of the loop. A break with a value counts when `valued`.
#[must_use]
pub fn loop_test(b: &Body, l: u32, valued: bool) -> Option<(u32, usize, [bool; 2])> {
    if b.tags.get(l as usize) != Some(&Tag::Loop) {
        return None;
    }
    let (prefix, test) = loop_statements(b, l)
        .enumerate()
        .find(|&(_, x)| b.tags[x as usize] == Tag::If)?;
    let arms = b.record(b.data[test as usize][1]);
    let breaks = |k: usize| {
        let Some(&blk) = arms.get(k).filter(|&&a| a != NONE) else {
            return false;
        };
        if b.tags.get(blk as usize) != Some(&Tag::Block) {
            return false;
        }
        let mut items = b
            .record(b.data[blk as usize][0])
            .iter()
            .filter(|&&x| b.tags[x as usize] != Tag::Block);
        let (Some(&x), None) = (items.next(), items.next()) else {
            return false;
        };
        let [label, value] = b.data[x as usize];
        b.tags[x as usize] == Tag::Break
            && (valued || value == NONE)
            && b.label_inst.get(label as usize) == Some(&l)
    };
    Some((test, prefix, [breaks(0), breaks(1)]))
}

/// One place a loop may hoist or keep: a local root and its steps (`len`
/// codes of `Tables::csteps` from `start`).
#[derive(Clone, Copy, Debug, Default)]
struct Cand {
    local: u32,
    start: u32,
    len: u32,
    /// From an `IsUnique` of the iteration block (rule 1).
    hoist: bool,
    /// Rule 3's case for calls: not the whole of a fresh, private local.
    external: bool,
    /// Rules 2 and 3 hold so far.
    ok: bool,
    /// A `ListPush` changes it.
    pushes: bool,
    /// An element read or store, or a `ListLen`, reads it.
    used: bool,
    /// An `IsUnique` of it stands in the loop.
    unique: bool,
}

/// The hoisting pass's dense tables, reused across bodies.
#[derive(Default)]
pub(crate) struct Tables {
    pending: splice::Pending,
    /// The loops, outermost first.
    loops: Vec<(u32, u32)>,
    /// Per local: a `Ref` roots at it; a closure captures it; a parameter;
    /// every value stored into all of it is fresh; a read of it keeps its
    /// value (rule 2's copies, anywhere in the body).
    addressed: Vec<bool>,
    captured: Vec<bool>,
    param: Vec<bool>,
    fresh: Vec<bool>,
    escapes: Vec<bool>,
    /// Per value: its uses (user and role), in compressed rows, and its
    /// last use.
    use_start: Vec<u32>,
    use_at: Vec<(u32, Role)>,
    use_fill: Vec<u32>,
    last_use: Vec<u32>,
    /// The `borrow` reads of a local's own place (no element step), by
    /// local: (local, read).
    borrows: Vec<(u32, u32)>,
    /// Per instruction: an `IsUnique` a hoist removed or covered.
    gone: Vec<bool>,
    cands: Vec<Cand>,
    csteps: Vec<u32>,
    /// The `IsUnique`s of each candidate in the loop: (candidate, inst).
    hits: Vec<(u32, u32)>,
    steps: Vec<u32>,
    raw: Vec<u32>,
    ops: Vec<Operand>,
}

/// Resolves the place at `word` through the reads its value roots come
/// from (`%h = Read [$xs]` makes `[%h [%i]]` the place `[$xs [%i]]`): its
/// local root, or `NONE`, with every step into `out`. Also whether the
/// place's own root is a value and none of its own steps goes into memory:
/// a place inside a temporary, which no write of it reaches.
pub(crate) fn resolve(b: &Body, word: u32, out: &mut Vec<u32>, raw: &mut Vec<u32>) -> (u32, bool) {
    out.clear();
    raw.clear();
    let mut views = [b.view(word); DEPTH];
    let mut depth = 0;
    let mut word = word;
    let mut temp = false;
    let local = loop {
        let p = b.view(word);
        if depth < DEPTH {
            views[depth] = p;
        }
        depth += 1;
        // Hoisting tracks only a local's and a value's steps.
        if depth == 1 && matches!(p.root, Root::Local(_) | Root::Value(_)) {
            let memory = p.push_steps(b, raw) != (0, false);
            temp = p.value().is_some() && !memory;
        }
        match p.root {
            Root::Local(l) => break l,
            Root::Value(v) if depth < DEPTH && b.tags[v as usize] == Tag::Read => {
                word = b.data[v as usize][0];
            }
            _ => break NONE,
        }
    };
    if depth > DEPTH || local == NONE {
        return (NONE, temp);
    }
    for p in views[..depth].iter().rev() {
        p.push_steps(b, out);
    }
    (local, temp)
}

/// Whether `a` is a prefix of `c` (or equal).
pub(crate) fn prefix(a: &[u32], c: &[u32]) -> bool {
    a.len() <= c.len() && a.iter().zip(c).all(|(x, y)| x == y)
}

/// Whether steps `s` are a hoistable place's: one leading dereference at
/// most, then fields.
pub(crate) fn hoistable(s: &[u32]) -> bool {
    s.iter()
        .enumerate()
        .all(|(k, &c)| (c == DEREF && k == 0) || c < VARIANT)
}

/// A value-producing call: may it reach memory outside its arguments? A
/// call that writes nothing may still return a copy of a header that
/// `p` holds, a second owner of its buffer, unless its result is uncounted.
pub(crate) fn writes(b: &Body, cx: &dyn OwnCx, x: usize) -> bool {
    match b.tags[x] {
        Tag::Call => {
            Callee::from_words(b.record(b.data[x][0])).is_none_or(|c| cx.may_write(&c))
                || cx.counted(b.ty[x]) != Tri::No
        }
        Tag::CallValue | Tag::CallDyn | Tag::CallHost | Tag::DefaultCall => true,
        _ => false,
    }
}

/// An intrinsic that changes, copies, pins or snapshots its collection,
/// or calls std code that may.
fn changes(op: Option<IntrinsicOp>) -> bool {
    !matches!(
        op,
        Some(
            IntrinsicOp::ListLen
                | IntrinsicOp::MapLen
                | IntrinsicOp::MapGet
                | IntrinsicOp::MapEntry
                | IntrinsicOp::MapSlots
                | IntrinsicOp::MapSlotNext
                | IntrinsicOp::MapSlotEntry
                | IntrinsicOp::StrIndex
                | IntrinsicOp::StrConcat
                | IntrinsicOp::StrEq
        )
    )
}

/// Whether statement `x` of an entry test's prefix may run once more
/// before its loop, as the guard of what hoists there (rule 1): a copy of
/// a value with no counted leaf, a `borrow` and its kept `Drop`, a
/// primitive, a scalar coercion, a length, or `and` and `or` of such.
fn pure(b: &Body, cx: &dyn OwnCx, x: u32, budget: &mut usize) -> bool {
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    let i = x as usize;
    let [a, w] = b.data[i];
    let scalar = cx.counted(b.ty[i]) == Tri::No;
    match b.tags[i] {
        Tag::Read => {
            (w == ReadMode::Copy as u32 && scalar)
                || (w == ReadMode::Borrow as u32 && cx.counted(b.ty[i]) != Tri::Maybe)
        }
        // A borrow's kept drop, which releases what an instance's view
        // made fresh.
        Tag::Drop => unique_header(b, a).is_some_and(|v| b.is_borrow(v)),
        Tag::Prim => true,
        Tag::Coerce => scalar,
        Tag::Intrinsic => matches!(
            IntrinsicOp::from_u32(a),
            Some(IntrinsicOp::ListLen | IntrinsicOp::MapLen)
        ),
        Tag::And | Tag::Or => {
            let blk = w as usize;
            b.tags.get(blk) == Some(&Tag::Block)
                && b.record(b.data[blk][0])
                    .iter()
                    .filter(|&&m| b.tags[m as usize] != Tag::Block)
                    .all(|&m| pure(b, cx, m, budget))
        }
        _ => false,
    }
}

impl Tables {
    fn cand_steps(&self, c: &Cand) -> &[u32] {
        &self.csteps[c.start as usize..(c.start + c.len) as usize]
    }

    /// The uses of value `v`.
    fn uses(&self, v: u32) -> &[(u32, Role)] {
        let v = v as usize;
        &self.use_at[self.use_start[v] as usize..self.use_start[v + 1] as usize]
    }

    /// Whether a read's value is kept by a use (rule 2's copy): stored,
    /// passed owned or as an argument, a block's value, coerced, or copied
    /// or snapshot by an intrinsic.
    fn kept(&self, b: &Body, r: u32) -> bool {
        self.uses(r).iter().any(|&(u, role)| keeps(b, u, role))
    }
}

/// Whether a use by `user` in role `role` keeps the value it uses, so a
/// read of a header used so makes another holder of it (rule 2).
pub(crate) fn keeps(b: &Body, user: u32, role: Role) -> bool {
    !matches!(role, Role::Lend | Role::Dropped)
        || b.tags[user as usize] == Tag::Coerce
        || (b.tags[user as usize] == Tag::Intrinsic
            && matches!(
                IntrinsicOp::from_u32(b.data[user as usize][0]),
                Some(IntrinsicOp::SnapBegin | IntrinsicOp::SnapEnd | IntrinsicOp::Item)
            ))
}

/// Whether a use by `user` in role `role` makes another holder of the
/// very header it uses (a store, an owned argument, a block's value, a
/// coercion), or hands it to std code: then the header may be
/// reached from outside the loop. A copy or a snapshot makes a header of
/// its own, which matters only inside the loop ([`keeps`]).
fn aliases(b: &Body, user: u32, role: Role) -> bool {
    keeps(b, user, role)
        && !(b.tags[user as usize] == Tag::Intrinsic
            && matches!(
                IntrinsicOp::from_u32(b.data[user as usize][0]),
                Some(IntrinsicOp::SnapBegin | IntrinsicOp::SnapEnd)
            ))
}

/// Runs hoisting over `b`, as the pending edits leave it, adding its own.
pub(crate) fn run(
    b: &mut Body,
    topo: &Topology,
    cx: &dyn OwnCx,
    t: &mut Tables,
    edits: &mut splice::Tables,
) {
    b.stable_lists.clear();
    let n = b.len();
    if !b.tags.contains(&Tag::Loop) {
        return;
    }
    t.pending.build(n, edits);
    t.loops.clear();
    for i in (0..n).filter(|&i| b.tags[i] == Tag::Loop) {
        t.loops.push((topo.first[i], u32_of(i)));
    }
    // Outermost first: an outer loop starts no later and ends later.
    t.loops
        .sort_stable_by_key(|&(f, l)| (f, std::cmp::Reverse(l)));
    facts(b, t);
    fill(&mut t.gone, n, false);
    for k in 0..t.loops.len() {
        let l = t.loops[k].1;
        hoist_loop(b, topo, cx, t, edits, l);
    }
    b.stable_lists.sort_stable();
    b.stable_lists.dedup();
}

/// The per-local facts and the uses of every value.
fn facts(b: &Body, t: &mut Tables) {
    let n = b.len();
    let locals = b.local_ty.len();
    fill(&mut t.addressed, locals, false);
    fill(&mut t.captured, locals, false);
    fill(&mut t.param, locals, false);
    fill(&mut t.fresh, locals, true);
    fill(&mut t.escapes, locals, false);
    for (l, flags) in b.local_flags.iter().enumerate() {
        t.param[l] = flags & local_flags::PARAM != 0;
        t.captured[l] = flags & (local_flags::CAPTURED | local_flags::CAPTURED_ASSIGNED) != 0;
    }
    for r in &b.sub_params {
        for k in 0..r.len {
            if let Some(x) = t
                .param
                .get_mut(b.extra[(r.start + 1 + k) as usize] as usize)
            {
                *x = true;
            }
        }
    }
    for l in &b.cap_local {
        t.captured[l.idx()] = true;
    }
    // The uses of each value, in compressed rows.
    fill(&mut t.use_start, n + 1, 0);
    fill(&mut t.last_use, n, 0);
    let mut ops = std::mem::take(&mut t.ops);
    for i in 0..n {
        if t.pending.dead[i] {
            continue;
        }
        operands(b, i, &mut ops);
        for op in &ops {
            t.use_start[op.value as usize + 1] += 1;
            t.last_use[op.value as usize] = u32_of(i);
        }
    }
    for v in 0..n {
        t.use_start[v + 1] += t.use_start[v];
    }
    t.use_at.clear();
    t.use_at.resize(t.use_start[n] as usize, (NONE, Role::Lend));
    t.use_fill.clear();
    t.use_fill.extend_from_slice(&t.use_start[..n]);
    for i in 0..n {
        if t.pending.dead[i] {
            continue;
        }
        operands(b, i, &mut ops);
        for op in &ops {
            let at = &mut t.use_fill[op.value as usize];
            t.use_at[*at as usize] = (u32_of(i), op.role);
            *at += 1;
        }
    }
    t.ops = ops;
    // Per local: references, fresh stores, reads that keep the value, and
    // borrows of its own place.
    t.borrows.clear();
    let (mut steps, mut raw) = (std::mem::take(&mut t.steps), std::mem::take(&mut t.raw));
    for i in 0..n {
        if t.pending.dead[i] {
            continue;
        }
        let [a, w] = b.data[i];
        match b.tags[i] {
            Tag::Ref => {
                if let Some(l) = b.local_in_place(a) {
                    t.addressed[l.idx()] = true;
                }
            }
            Tag::Store => {
                let p = b.view(a);
                if let Some(l) = p.local().filter(|_| p.whole()) {
                    let fresh = Ref(w).as_inst().is_none_or(|_| {
                        matches!(
                            b.tags.get(w as usize),
                            Some(
                                Tag::Call
                                    | Tag::CallValue
                                    | Tag::CallHost
                                    | Tag::DefaultCall
                                    | Tag::NewList
                                    | Tag::NewMap
                            )
                        ) || (b.tags.get(w as usize) == Some(&Tag::Intrinsic)
                            && matches!(
                                IntrinsicOp::from_u32(b.data[w as usize][0]),
                                Some(IntrinsicOp::SnapBegin)
                            ))
                    });
                    t.fresh[l as usize] &= fresh;
                }
            }
            Tag::Read if w != ReadMode::Move as u32 => {
                let (l, _) = resolve(b, a, &mut steps, &mut raw);
                if l == NONE || steps.contains(&ELEM) {
                    continue;
                }
                if t.uses(u32_of(i))
                    .iter()
                    .any(|&(u, role)| aliases(b, u, role))
                {
                    t.escapes[l as usize] = true;
                }
                if w == ReadMode::Borrow as u32 {
                    t.borrows.push((l, u32_of(i)));
                }
            }
            _ => {}
        }
    }
    t.steps = steps;
    t.raw = raw;
}

/// Adds a candidate for the place resolved into `t.steps` at local `l`,
/// or marks the one there; returns its index.
fn candidate(t: &mut Tables, l: u32, hoist: bool) -> Option<usize> {
    let found = t.cands.iter().position(|c| {
        c.local == l && t.csteps[c.start as usize..(c.start + c.len) as usize] == t.steps[..]
    });
    if let Some(k) = found {
        t.cands[k].hoist |= hoist;
        return Some(k);
    }
    if t.cands.len() == MAX {
        return None;
    }
    let start = u32_of(t.csteps.len());
    t.csteps.extend_from_slice(&t.steps);
    let external = !t.steps.is_empty() || t.param[l as usize] || !t.fresh[l as usize];
    t.cands.push(Cand {
        local: l,
        start,
        len: u32_of(t.steps.len()),
        hoist,
        external,
        ok: true,
        ..Cand::default()
    });
    Some(t.cands.len() - 1)
}

/// The place an `IsUnique`, an element access or a `ListLen` makes a
/// candidate of: the header its place or operand reads, resolved, when it
/// has the hoistable form at a usable local and its type is a counted
/// collection's in every instance. Into `t.steps`; its local.
fn header_place(b: &Body, cx: &dyn OwnCx, t: &mut Tables, header: u32) -> Option<u32> {
    let (ty, word) = match Ref(header).as_inst() {
        Some(h) if b.tags[h.0 as usize] == Tag::Read => {
            (b.ty[h.0 as usize], b.data[h.0 as usize][0])
        }
        _ => return None,
    };
    if cx.counted(ty) != Tri::Yes || b.data[header as usize][1] == ReadMode::Move as u32 {
        return None;
    }
    let (l, _) = resolve(b, word, &mut t.steps, &mut t.raw);
    if l == NONE || !hoistable(&t.steps) {
        return None;
    }
    let li = l as usize;
    if t.addressed[li] || t.captured[li] || t.escapes[li] {
        return None;
    }
    Some(l)
}

/// The list place a place rooted at a local names, as [`header_place`]
/// checks a header read's (the places the checker roots at their local,
/// `crate::ir::root_places`): with `elem`, the list whose element the
/// place's first element step reads (`[*$0 [%i] .x]` reads one of
/// `[*$0]`); without, all of the place (an `IsUnique`'s). Into `t.steps`;
/// its local.
fn rooted_place(b: &Body, t: &mut Tables, word: u32, elem: bool) -> Option<u32> {
    let p = b.view(word);
    let l = p.local()?;
    let to = if elem {
        (p.from..p.to).find(|&x| b.extra[x as usize] & place::PROJ_KIND == place::PROJ_ELEM)?
    } else {
        p.to
    };
    t.steps.clear();
    if (PlaceView { to, ..p }).push_steps(b, &mut t.steps).1 {
        return None;
    }
    if !hoistable(&t.steps) {
        return None;
    }
    let li = l as usize;
    if t.addressed[li] || t.captured[li] || t.escapes[li] {
        return None;
    }
    Some(l)
}

/// The header value an `IsUnique` at `word` reads: a bare local's place
/// is its own header (none here: the forms come from reads), a value's is
/// the value.
fn unique_header(b: &Body, word: u32) -> Option<u32> {
    let p = b.view(word);
    p.value().filter(|_| p.whole())
}

fn hoist_loop(
    b: &mut Body,
    topo: &Topology,
    cx: &dyn OwnCx,
    t: &mut Tables,
    edits: &mut splice::Tables,
    l: u32,
) {
    let lo = topo.first[l as usize];
    t.cands.clear();
    t.csteps.clear();
    t.hits.clear();
    // Rule 1: the `IsUnique`s of the iteration block, behind a pure
    // prefix.
    if let Some(e) = entry_test(b, l) {
        let mut budget = b.len();
        let prefix_pure = loop_statements(b, l)
            .take(e.prefix)
            .all(|x| pure(b, cx, x, &mut budget));
        // The iteration block's statements, and those of the iteration
        // block of each loop that stands among them, at any depth: an
        // `IsUnique` there runs whenever that inner loop is entered.
        let mut blocks = if prefix_pure {
            vec![e.iteration]
        } else {
            Vec::new()
        };
        while let Some(blk) = blocks.pop() {
            for k in 0..b.record(b.data[blk as usize][0]).len() {
                let x = b.record(b.data[blk as usize][0])[k];
                if b.tags[x as usize] == Tag::Loop
                    && !t.pending.dead[x as usize]
                    && let Some(inner) = entry_test(b, x)
                {
                    blocks.push(inner.iteration);
                    continue;
                }
                if b.tags[x as usize] != Tag::IsUnique
                    || t.gone[x as usize]
                    || t.pending.dead[x as usize]
                {
                    continue;
                }
                let word = b.data[x as usize][0];
                let local = match unique_header(b, word) {
                    Some(h) => header_place(b, cx, t, h),
                    None => rooted_place(b, t, word, false),
                };
                if let Some(local) = local {
                    candidate(t, local, true);
                }
            }
        }
    }
    // The stable list candidates: the headers element accesses and
    // `ListLen` read.
    for x in lo..l {
        let i = x as usize;
        if t.pending.dead[i] {
            continue;
        }
        let header = match b.tags[i] {
            Tag::Read | Tag::Store => match b.view(b.data[i][0]) {
                p if p.value().is_some() => p
                    .value()
                    .filter(|_| matches!(p.first(b), Some(Proj::Elem(_)))),
                // An element of a list a place rooted at a local names.
                p if p.local().is_some() => {
                    if let Some(local) = rooted_place(b, t, b.data[i][0], true) {
                        candidate(t, local, false);
                    }
                    None
                }
                _ => None,
            },
            Tag::Intrinsic if IntrinsicOp::from_u32(b.data[i][0]) == Some(IntrinsicOp::ListLen) => {
                b.record(b.data[i][1]).first().copied()
            }
            _ => None,
        };
        if let Some(h) = header
            && let Some(local) = header_place(b, cx, t, h)
        {
            candidate(t, local, false);
        }
    }
    if t.cands.is_empty() {
        return;
    }
    // No `borrow` of a candidate's place lives across the loop's entry,
    // where its `IsUnique` will stand.
    for k in 0..t.cands.len() {
        let c = t.cands[k];
        let from = t.borrows.partition_point(|&(x, _)| x < c.local);
        for &(x, r) in &t.borrows[from..] {
            if x != c.local {
                break;
            }
            if r < lo && t.last_use[r as usize] >= lo {
                t.cands[k].ok = false;
            }
        }
    }
    // Rules 2 and 3 over the loop's instructions and the edits pending in
    // its blocks.
    for x in lo..l {
        let i = x as usize;
        if t.pending.dead[i] {
            continue;
        }
        let [a, w] = b.data[i];
        check(b, cx, t, x, b.tags[i], [a, w]);
        if b.tags[i] == Tag::Block {
            for k in 0..t.pending.of(x).len() {
                let e = t.pending.of(x)[k];
                let (_, _, tag, data, _) = edits.get(e);
                check(b, cx, t, NONE, tag, data);
            }
        }
    }
    let at = (topo.blk[l as usize], topo.pos[l as usize]);
    for k in 0..t.cands.len() {
        let c = t.cands[k];
        if !c.ok {
            continue;
        }
        let deref = t.cand_steps(&c).first() == Some(&DEREF);
        let stable = c.used && !c.pushes && (c.hoist || !c.unique);
        if !c.hoist && !stable {
            continue;
        }
        let word = place_word(b, t, &c);
        if c.hoist {
            edits.insert(
                at.0,
                at.1,
                Tag::IsUnique,
                [word, unique_word::HOISTED],
                Ty::VOID,
                b.syn[l as usize],
            );
            for h in 0..t.hits.len() {
                let (kk, x) = t.hits[h];
                if kk as usize != k {
                    continue;
                }
                t.gone[x as usize] = true;
                if deref {
                    b.data[x as usize][1] = unique_word::COVERED;
                } else {
                    edits.remove(x);
                }
            }
        }
        if stable {
            b.stable_lists.push((l, word));
        }
    }
}

/// A place record for candidate `c`.
fn place_word(b: &mut Body, t: &Tables, c: &Cand) -> u32 {
    let steps = t.cand_steps(c);
    if steps.is_empty() {
        return c.local | place::BARE_LOCAL;
    }
    let at = u32_of(b.extra.len());
    b.extra.push(2 + c.len);
    b.extra.extend([place::ROOT_LOCAL, c.local]);
    for &s in steps {
        b.extra.push(if s == DEREF {
            place::PROJ_DEREF
        } else {
            place::PROJ_FIELD | s
        });
    }
    at
}

/// Checks one instruction of the loop (`x`, or `NONE` for a pending edit)
/// against every candidate (rules 2 and 3).
fn check(b: &Body, cx: &dyn OwnCx, t: &mut Tables, x: u32, tag: Tag, [a, w]: [u32; 2]) {
    if is_suspension(tag) || tag == Tag::Pin {
        for c in &mut t.cands {
            c.ok = false;
        }
        return;
    }
    if x != NONE && writes(b, cx, x as usize) {
        for c in &mut t.cands {
            c.ok &= !c.external;
        }
    }
    let mut steps = std::mem::take(&mut t.steps);
    let mut raw = std::mem::take(&mut t.raw);
    match tag {
        Tag::Read | Tag::Store | Tag::Drop | Tag::Ref | Tag::IsUnique => {
            let (l, temp) = resolve(b, a, &mut steps, &mut raw);
            let memory = raw.iter().any(|&s| s == ELEM || s == DEREF);
            for k in 0..t.cands.len() {
                let c = t.cands[k];
                let mine = l == c.local;
                let (covers, below, next_elem) = {
                    let cs = t.cand_steps(&c);
                    (
                        mine && prefix(&steps, cs),
                        mine && steps.len() > cs.len() && prefix(cs, &steps),
                        steps.get(cs.len()) == Some(&ELEM),
                    )
                };
                let exact = covers && steps.len() == c.len as usize;
                let ok = match tag {
                    Tag::Read if w == ReadMode::Move as u32 => temp || !covers,
                    // A read of the header, or of a place holding it, whose
                    // value a use keeps; for a place another reference or
                    // list may reach, such a copy of any collection through
                    // memory.
                    Tag::Read => {
                        x == NONE
                            || !t.kept(b, x)
                            || (!covers
                                && (mine
                                    || !c.external
                                    || !memory
                                    || !cx.holds_coll(b.ty[x as usize])))
                    }
                    Tag::Store | Tag::Drop => temp || (!covers && (mine || !c.external || !memory)),
                    Tag::Ref => temp || !(covers || below),
                    // A removed or covered `IsUnique` an outer loop hoisted
                    // stands for nothing here.
                    _ if x != NONE && t.gone[x as usize] => true,
                    _ => {
                        if exact && x != NONE {
                            t.hits.push((u32_of(k), x));
                            t.cands[k].unique = true;
                        }
                        mine || !c.external
                    }
                };
                if !ok {
                    t.cands[k].ok = false;
                }
                if below && next_elem && matches!(tag, Tag::Read | Tag::Store) {
                    t.cands[k].used = true;
                }
            }
        }
        Tag::Intrinsic => {
            let op = IntrinsicOp::from_u32(a);
            let coll = b.record(w).first().copied().unwrap_or(NONE);
            let (l, _) = match Ref(coll).as_inst() {
                Some(h) if b.tags[h.0 as usize] == Tag::Read => {
                    resolve(b, b.data[h.0 as usize][0], &mut steps, &mut raw)
                }
                _ => {
                    steps.clear();
                    (NONE, false)
                }
            };
            for c in &mut t.cands {
                let cs = &t.csteps[c.start as usize..(c.start + c.len) as usize];
                let mine = l == c.local && prefix(cs, &steps);
                let exact = mine && steps.len() == cs.len();
                if exact && op == Some(IntrinsicOp::ListPush) {
                    c.pushes = true;
                }
                if exact && op == Some(IntrinsicOp::ListLen) {
                    c.used = true;
                }
                if c.external && !mine && changes(op) {
                    c.ok = false;
                }
            }
        }
        _ => {}
    }
    t.steps = steps;
    t.raw = raw;
}
