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
//!    of an inner loop that stands there in turn, at any depth, so it runs
//!    whenever that inner loop is entered; and the statements before the
//!    entry test are pure, so the backend can run them once more before
//!    the loop as its guard. A loop with no entry test (`while true`)
//!    runs its body block whenever it is entered: that block is its
//!    iteration block, and the hoisted check runs unguarded;
//! 2. the loop never copies `p` or a place holding it, takes no `Ref` of a
//!    place that overlaps it, writes no place that holds it, pins nothing
//!    and holds no suspension point. A local `&mut` reference to one
//!    element of `p` that the loop uses up (`for x in &mut xs`) is an
//!    address in the buffer, not a second owner of the header, so it is
//!    allowed;
//! 3. no call in the loop may write, unless `p`'s root is a local that is
//!    never addressed or captured, whose every stored value is fresh (a
//!    call's or a new collection's, or moved out of a local whose own
//!    values are fresh and that nothing else reaches), and `p` is that
//!    whole local, and not a parameter's. Any other
//!    `p` (behind a reference, a field, a parameter's or a shared header)
//!    may also be reached through another reference or list, so nothing
//!    in the loop may write through a dereference or into a list's buffer
//!    unless its place is `p` or below it (through a loop local whose
//!    every store is such an element reference counts as below), or
//!    change, copy or pin another collection.
//!
//! `p` is the place the `IsUnique`'s header read reads (`%h = Read
//! [*$0]` makes `IsUnique [%h]` one of `[*$0]`), rooted at a local that is
//! never addressed or captured, with one leading dereference at most and
//! fields; its header is read nowhere in the body with a use that keeps
//! that very header (a store, an owned argument, a block's value, a
//! coercion, a pin; a copy outside the loop has a header of its own,
//! which the hoisted check separates from `p`'s), no
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
//! copy or reference of the header, which is held nowhere
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
use super::{Operand, OwnCx, Role, fill, operands};
use crate::ir::{
    Body, Callee, DEREF, ELEM, IntrinsicOp, NONE, PlaceView, Proj, ReadMode, Ref, Root, Tag,
    Topology, VARIANT, is_suspension, local_flags, place, ref_word, u32_of, unique_word,
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
    /// A `ListPush` or `ListReserve` changes it.
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
    /// every value stored into all of it is fresh; a read of it keeps a
    /// value that may share a buffer or reach a place (rule 2's copies,
    /// anywhere in the body).
    addressed: Vec<bool>,
    captured: Vec<bool>,
    param: Vec<bool>,
    fresh: Vec<bool>,
    escapes: Vec<bool>,
    /// Per local: a read through it keeps a value that holds a reference,
    /// which may reach any place of the local.
    reaches: Vec<bool>,
    /// The places of the other such kept reads, each a second owner of
    /// what that place holds: (local, start, len) into `kept_steps`.
    kept_places: Vec<(u32, u32, u32)>,
    kept_steps: Vec<u32>,
    /// The stores of a whole local moved from another local: (to, from).
    moves: Vec<(u32, u32)>,
    /// Per local: its stores of all of it.
    stores: Vec<u32>,
    /// The loop's element reference locals: (local, candidate), a local
    /// whose every store is a local `&mut` reference to an element of the
    /// candidate that the loop uses up ([`elem_ref`]).
    elems: Vec<(u32, u32)>,
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

impl Tables {
    /// Whether a kept read may hold the buffer of the place in `steps` at
    /// local `l`, or reach it: one of a value holding a reference read
    /// through `l`, or of a place that holds that place (a prefix of
    /// `steps`) or lies in it. A kept copy of a part beside it (a `usize`
    /// or a string field) is a second owner of that part only.
    fn kept_through(&self, l: u32) -> bool {
        self.reaches[l as usize]
            || self.kept_places.iter().any(|&(k, start, len)| {
                let e = &self.kept_steps[start as usize..(start + len) as usize];
                k == l && (prefix(e, &self.steps) || prefix(&self.steps, e))
            })
    }
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
                || cx.counted(b.ty[x])
        }
        Tag::CallValue | Tag::CallDyn | Tag::CallHost | Tag::DefaultCall => true,
        _ => false,
    }
}

/// An intrinsic that changes, copies or pins its collection,
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
    let scalar = !cx.counted(b.ty[i]);
    match b.tags[i] {
        Tag::Read => {
            (w == ReadMode::Copy as u32 && scalar)
                || (w == ReadMode::Borrow as u32 && !cx.thunk_made(b.ty[i]))
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
    /// passed owned or as an argument, a block's value, coerced, or
    /// copied by an intrinsic.
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
            && IntrinsicOp::from_u32(b.data[user as usize][0]) == Some(IntrinsicOp::Item))
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
    let fresh = fresh_locals(b);
    let mut marked = Vec::new();
    for i in 0..n {
        if b.tags[i] == Tag::IsUnique && b.data[i][1] == 0 && fresh_unique(b, &fresh, i) {
            b.data[i][1] = unique_word::FRESH;
            marked.push(i);
        }
    }
    for i in 0..n {
        // Not in a loop, whose checks hoisting may move.
        if b.tags[i] == Tag::IsUnique
            && b.data[i][1] == 0
            && (topo.loop_of[i] == NONE || param_place(b, b.data[i][0], &mut Vec::new()).is_some())
            && repeats(b, topo, i)
        {
            b.data[i][1] = unique_word::REPEAT;
            marked.push(i);
        }
    }
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
    facts(b, cx, t);
    fill(&mut t.gone, n, false);
    // A fresh buffer's checks run nowhere: none hoists.
    for i in marked {
        t.gone[i] = true;
    }
    for k in 0..t.loops.len() {
        let l = t.loops[k].1;
        hoist_loop(b, topo, cx, t, edits, l);
    }
    b.stable_lists.sort_stable();
    b.stable_lists.dedup();
}

/// Whether `IsUnique` `i`, of all of a local, repeats an earlier one of
/// the same local in its block with nothing between that can share the
/// buffer (step 15b): the statements between are reads, stores, operators,
/// constructions, drops and intrinsics other than a std call's, none of
/// them naming all of the local (no whole read, store or drop of it: an
/// element or field access is fine), so no copy, call or block between
/// makes a second owner of the buffer the first check left unique.
pub(crate) fn repeats(b: &Body, topo: &Topology, i: usize) -> bool {
    let p = b.view(b.data[i][0]);
    let (Some(l), true) = (p.local(), p.whole()) else {
        return repeats_through(b, topo, i);
    };
    let blk = topo.blk[i];
    if blk == NONE {
        return false;
    }
    let list = b.record(b.data[blk as usize][0]);
    let pos = topo.pos[i] as usize;
    for k in (0..pos).rev() {
        let m = list[k] as usize;
        let q = b.view(b.data[m][0]);
        let names_all = q.whole() && q.local() == Some(l);
        match b.tags[m] {
            Tag::IsUnique if names_all => return true,
            Tag::Read | Tag::Store | Tag::Drop | Tag::IsUnique if !names_all => {}
            Tag::Prim | Tag::NewStruct | Tag::NewTuple | Tag::NewVariant | Tag::Coerce => {}
            Tag::Intrinsic if IntrinsicOp::from_u32(b.data[m][0]) != Some(IntrinsicOp::Item) => {}
            _ => return false,
        }
    }
    false
}

/// The steps of a place behind a parameter reference, `[*$p .f .g]`: one
/// leading dereference of a parameter, then fields. Into `out`; the
/// parameter. Its target was there before the body ran, so it is none of
/// the body's own locals.
pub(crate) fn param_place(b: &Body, word: u32, out: &mut Vec<u32>) -> Option<u32> {
    let p = b.view(word);
    let l = p.local()?;
    if b.local_flags
        .get(l as usize)
        .is_none_or(|f| f & local_flags::PARAM == 0)
    {
        return None;
    }
    out.clear();
    p.push_steps(b, out);
    (out.first() == Some(&DEREF) && out[1..].iter().all(|&s| s < VARIANT)).then_some(l)
}

/// [`repeats`] for a place behind a parameter reference ([`param_place`],
/// step 15c): an earlier `IsUnique` of the same place in the block, and
/// between them only operators, constructions, intrinsics other than a std
/// call's, `IsUnique`s (which only make a buffer unique), drops of values
/// and of places that do not hold the buffer, `borrow` reads of the place
/// whose every use is between them and keeps nothing, and reads and stores
/// that cannot reach its header: under the parameter, of another field or
/// below the place with no second dereference; of a local's or a
/// temporary's own storage. A read or store through any other dereference,
/// a pointer or a global may alias the place, so it stops the search, as
/// does a call, a block, a copy or move of the place or of what holds it,
/// and a store over either. No second owner of the buffer the first check
/// left unique can appear.
fn repeats_through(b: &Body, topo: &Topology, i: usize) -> bool {
    let mut ts = Vec::new();
    let Some(pl) = param_place(b, b.data[i][0], &mut ts) else {
        return false;
    };
    let blk = topo.blk[i];
    if blk == NONE {
        return false;
    }
    let list = b.record(b.data[blk as usize][0]);
    let pos = topo.pos[i] as usize;
    let (mut qs, mut raw, mut lent) = (Vec::new(), Vec::new(), Vec::new());
    let mut start = None;
    for k in (0..pos).rev() {
        let m = list[k] as usize;
        let tag = b.tags[m];
        match tag {
            Tag::Prim | Tag::NewStruct | Tag::NewTuple | Tag::NewVariant | Tag::Coerce => continue,
            Tag::Intrinsic if IntrinsicOp::from_u32(b.data[m][0]) != Some(IntrinsicOp::Item) => {
                continue;
            }
            Tag::Read | Tag::Store | Tag::Drop | Tag::IsUnique => {}
            _ => return false,
        }
        let (l, temp) = resolve(b, b.data[m][0], &mut qs, &mut raw);
        let ours = l == pl;
        let holds = ours && prefix(&qs, &ts);
        if tag == Tag::IsUnique {
            if holds && qs.len() == ts.len() {
                start = Some(k);
                break;
            }
            continue;
        }
        if tag == Tag::Drop {
            if holds && b.view(b.data[m][0]).local().is_some() {
                return false;
            }
            continue;
        }
        if tag == Tag::Read && b.data[m][1] == ReadMode::Borrow as u32 {
            if holds {
                lent.push(u32_of(m));
            }
            continue;
        }
        // A copy, a move or a store.
        let apart = if ours {
            !holds && !qs[1..].contains(&DEREF)
        } else if l == NONE {
            temp || matches!(b.view(b.data[m][0]).root, Root::Const(_))
        } else {
            !qs.contains(&DEREF)
        };
        if !apart {
            return false;
        }
    }
    let Some(start) = start else {
        return false;
    };
    if lent.is_empty() {
        return true;
    }
    // Each borrow of the place is used only between the checks, by uses
    // that keep nothing.
    let between = &list[start + 1..pos];
    let mut ops = Vec::new();
    for x in 0..b.len() {
        operands(b, x, &mut ops);
        for op in &ops {
            if lent.contains(&op.value)
                && (!between.contains(&u32_of(x)) || keeps(b, u32_of(x), op.role))
            {
                return false;
            }
        }
    }
    true
}

/// Whether `IsUnique` `i` is of all of a local [`fresh_locals`] marks.
pub(crate) fn fresh_unique(b: &Body, fresh: &[bool], i: usize) -> bool {
    let p = b.view(b.data[i][0]);
    p.whole() && p.local().is_some_and(|l| fresh[l as usize])
}

/// The **fresh-buffer locals** of `b` (step 15a), whose buffer nothing
/// else can hold, so it is always unique: a local that is no parameter,
/// never addressed or captured, stored at least once, every whole store of
/// which stores a new collection (`NewList`, `NewMap`) that is empty or
/// has an element that is no constant (a literal of constants is static
/// data, shared), and whose every
/// read lets nothing share its header. A read either moves all of it out,
/// or is used only by a collection intrinsic that lends it (`ListPush`,
/// `ListReserve`, `MapSet`, `MapGet`, a length, an entry walk), an
/// operator, a `Drop`, or as the root of a read or a store (an element or a field). A
/// copy of all of it, a call argument (the callee may keep it), a store of
/// it, a `Ref` into it, or any other use keeps its checks.
pub(crate) fn fresh_locals(b: &Body) -> Vec<bool> {
    let n = b.len();
    let locals = b.local_ty.len();
    let mut ok = vec![false; locals];
    if !b.tags.contains(&Tag::IsUnique) {
        return ok;
    }
    for (l, flags) in b.local_flags.iter().enumerate() {
        ok[l] = flags
            & (local_flags::PARAM | local_flags::CAPTURED | local_flags::CAPTURED_ASSIGNED)
            == 0;
    }
    for r in b.sub_params.iter().skip(1) {
        if r.len > 0 {
            for &x in b.record(r.start) {
                if let Some(o) = ok.get_mut(x as usize) {
                    *o = false;
                }
            }
        }
    }
    for &c in &b.cap_local {
        if let Some(o) = ok.get_mut(c.idx()) {
            *o = false;
        }
    }
    let mut stored = vec![false; locals];
    // Per instruction: the local it reads all of without moving, or `NONE`.
    let mut read_of = vec![NONE; n];
    for (i, read) in read_of.iter_mut().enumerate() {
        let p = b.view(b.data[i][0]);
        let Some(l) = (match b.tags[i] {
            Tag::Read | Tag::Store | Tag::Ref => p.local(),
            _ => None,
        }) else {
            continue;
        };
        let li = l as usize;
        match b.tags[i] {
            Tag::Store if p.whole() => {
                stored[li] = true;
                let v = b.data[i][1] as usize;
                // A nonempty literal of constants is static data the
                // backend shares (`static_scalar_list`), not a fresh buffer.
                ok[li] &= Ref(b.data[i][1]).as_inst().is_some()
                    && v < n
                    && matches!(b.tags[v], Tag::NewList | Tag::NewMap)
                    && {
                        let items = b.record(b.data[v][1]);
                        items.is_empty() || items.iter().any(|&x| Ref(x).as_inst().is_some())
                    };
            }
            Tag::Ref => ok[li] = false,
            Tag::Read if p.whole() => {
                if b.data[i][1] == ReadMode::Copy as u32 {
                    ok[li] = false;
                } else if b.data[i][1] != ReadMode::Move as u32 {
                    *read = l;
                }
            }
            _ => {}
        }
    }
    let mut ops = Vec::new();
    for i in 0..n {
        operands(b, i, &mut ops);
        for op in &ops {
            let v = op.value as usize;
            if v >= n || read_of[v] == NONE {
                continue;
            }
            let lends = match b.tags[i] {
                Tag::Intrinsic => {
                    op.role == Role::Lend
                        && matches!(
                            IntrinsicOp::from_u32(b.data[i][0]),
                            Some(
                                IntrinsicOp::ListLen
                                    | IntrinsicOp::ListPush
                                    | IntrinsicOp::ListReserve
                                    | IntrinsicOp::MapLen
                                    | IntrinsicOp::MapGet
                                    | IntrinsicOp::MapRemove
                                    | IntrinsicOp::MapSet
                                    | IntrinsicOp::MapSlots
                                    | IntrinsicOp::MapSlotNext
                                    | IntrinsicOp::MapEntry
                                    | IntrinsicOp::MapSlotEntry
                                    | IntrinsicOp::MapSlotEntryMut
                            )
                        )
                }
                Tag::Prim | Tag::Drop => true,
                tag => op.role == Role::Lend && matches!(tag, Tag::Read | Tag::Store),
            };
            if !lends {
                ok[read_of[v] as usize] = false;
            }
        }
        // A read or store rooted at such a read's value reaches an element
        // or a field of it; one of all of it (`Read copy [%h]`) copies
        // the header, and a reference into it may outlive the read.
        if matches!(
            b.tags[i],
            Tag::Read | Tag::Store | Tag::Ref | Tag::IsUnique | Tag::Drop
        ) {
            let p = b.view(b.data[i][0]);
            if let Root::Value(v) = p.root
                && (v as usize) < n
                && read_of[v as usize] != NONE
                && (matches!(b.tags[i], Tag::Ref | Tag::IsUnique)
                    || (p.whole() && b.tags[i] != Tag::Drop))
            {
                ok[read_of[v as usize] as usize] = false;
            }
        }
    }
    for (o, s) in ok.iter_mut().zip(&stored) {
        *o &= *s;
    }
    ok
}

/// Whether a kept value of `ty` may share a buffer or reach a place: it
/// is counted, holds a list or a map, or holds a reference.
fn shares(cx: &dyn OwnCx, ty: Ty) -> bool {
    cx.counted(ty) || cx.holds_coll(ty) || cx.holds_ref(ty)
}

/// The per-local facts and the uses of every value.
fn facts(b: &Body, cx: &dyn OwnCx, t: &mut Tables) {
    let n = b.len();
    let locals = b.local_ty.len();
    fill(&mut t.addressed, locals, false);
    fill(&mut t.captured, locals, false);
    fill(&mut t.param, locals, false);
    fill(&mut t.fresh, locals, true);
    fill(&mut t.escapes, locals, false);
    fill(&mut t.reaches, locals, false);
    t.kept_places.clear();
    t.kept_steps.clear();
    fill(&mut t.stores, locals, 0);
    t.moves.clear();
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
                    t.stores[l as usize] += 1;
                    // Through coercions that keep the value.
                    let mut v = w;
                    while Ref(v).as_inst().is_some() && b.same_value_coercion(v as usize) {
                        v = b.data[v as usize][0];
                    }
                    // A copy read is one more count of the buffer, which a
                    // change elsewhere copies first. A borrow read makes
                    // the local a binding local (`borrow.rs`): its place
                    // changes nowhere while the local lives, the loop
                    // included.
                    let fresh = Ref(v).as_inst().is_none_or(|_| {
                        matches!(
                            b.tags.get(v as usize),
                            Some(
                                Tag::Call
                                    | Tag::CallValue
                                    | Tag::CallHost
                                    | Tag::DefaultCall
                                    | Tag::NewList
                                    | Tag::NewMap
                            )
                        ) || (b.tags.get(v as usize) == Some(&Tag::Read)
                            && matches!(
                                b.data[v as usize][1],
                                m if m == ReadMode::Copy as u32 || m == ReadMode::Borrow as u32
                            ))
                    });
                    // A move out of another local is fresh when that
                    // local's own value is, settled below.
                    let moved = Ref(v)
                        .as_inst()
                        .filter(|_| b.data[v as usize][1] == ReadMode::Move as u32)
                        .and_then(|_| b.local_read(v as usize));
                    match moved {
                        Some(k) if !fresh => t.moves.push((l, u32_of(k.idx()))),
                        _ => t.fresh[l as usize] &= fresh,
                    }
                }
            }
            Tag::Read if w != ReadMode::Move as u32 => {
                let (l, _) = resolve(b, a, &mut steps, &mut raw);
                if l == NONE || steps.contains(&ELEM) {
                    continue;
                }
                // A kept value with no count, collection or reference
                // (a `usize` field) neither shares a buffer nor reaches
                // the local's storage: it lets nothing escape.
                if t.uses(u32_of(i)).iter().any(|&(u, role)| keeps(b, u, role))
                    && shares(cx, b.ty[i])
                {
                    t.escapes[l as usize] = true;
                    // Past a second dereference the place may be any
                    // place of the local again.
                    if cx.holds_ref(b.ty[i]) || steps.iter().skip(1).any(|&s| s == DEREF) {
                        t.reaches[l as usize] = true;
                    } else {
                        let start = u32_of(t.kept_steps.len());
                        t.kept_steps.extend_from_slice(&steps);
                        t.kept_places.push((l, start, u32_of(steps.len())));
                    }
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
    // A local moved from another is fresh when that one is fresh and no
    // reference, capture or kept read reaches its value: the move hands
    // over the only holder. Down to the greatest fixed point.
    let mut changed = true;
    while changed {
        changed = false;
        for &(l, k) in &t.moves {
            let (l, k) = (l as usize, k as usize);
            if t.fresh[l] && !(t.fresh[k] && !t.addressed[k] && !t.captured[k] && !t.escapes[k]) {
                t.fresh[l] = false;
                changed = true;
            }
        }
    }
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
    if !cx.counted(ty) || cx.thunk_made(ty) || b.data[header as usize][1] == ReadMode::Move as u32 {
        return None;
    }
    let (l, _) = resolve(b, word, &mut t.steps, &mut t.raw);
    if l == NONE || !hoistable(&t.steps) {
        return None;
    }
    let li = l as usize;
    if t.addressed[li] || t.captured[li] || t.kept_through(l) {
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
    if t.addressed[li] || t.captured[li] || t.kept_through(l) {
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
    // prefix; a loop with no entry test runs its body block whenever it
    // is entered.
    let mut blocks = match entry_test(b, l) {
        Some(e) => {
            let mut budget = b.len();
            let prefix_pure = loop_statements(b, l)
                .take(e.prefix)
                .all(|x| pure(b, cx, x, &mut budget));
            if prefix_pure {
                vec![e.iteration]
            } else {
                Vec::new()
            }
        }
        None => vec![b.data[l as usize][0]],
    };
    // The iteration block's statements, and those of the iteration block
    // of each loop that stands among them, at any depth: an `IsUnique`
    // there runs whenever that inner loop is entered.
    while let Some(blk) = blocks.pop() {
        for k in 0..b.record(b.data[blk as usize][0]).len() {
            let x = b.record(b.data[blk as usize][0])[k];
            if b.tags[x as usize] == Tag::Loop && !t.pending.dead[x as usize] {
                blocks.push(entry_test(b, x).map_or(b.data[x as usize][0], |e| e.iteration));
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
    elem_locals(b, t, lo, l);
    // Rules 2 and 3 over the loop's instructions and the edits pending in
    // its blocks.
    for x in lo..l {
        let i = x as usize;
        if t.pending.dead[i] {
            continue;
        }
        let [a, w] = b.data[i];
        check(b, cx, t, (x, l), b.tags[i], [a, w]);
        if b.tags[i] == Tag::Block {
            for k in 0..t.pending.of(x).len() {
                let e = t.pending.of(x)[k];
                let (_, _, tag, data, _) = edits.get(e);
                check(b, cx, t, (NONE, l), tag, data);
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

/// Whether `Ref` `x` (data word `w`) of an element of a candidate is a
/// local `&mut` reference that loop `lp` uses up: an address in the
/// buffer the hoisted check made unique, which makes no second owner of
/// the header (call-scoped element references, model E). A shared one
/// stays: the backend's short loans skip the check that serves it.
fn elem_ref(t: &Tables, x: u32, w: u32, lp: u32) -> bool {
    let local_mut = ref_word::MUT | ref_word::LOCAL;
    x != NONE && w & local_mut == local_mut && t.last_use[x as usize] < lp
}

/// Fills `t.elems` for the loop `lo..l`: the locals stored only inside
/// it, each store a local `&mut` reference to an element of one
/// candidate, or a read of all of one ([`elem_ref`]). A write through
/// such a local writes the candidate's buffer, below its place (rule 3).
fn elem_locals(b: &Body, t: &mut Tables, lo: u32, l: u32) {
    t.elems.clear();
    let mut steps = std::mem::take(&mut t.steps);
    let mut raw = std::mem::take(&mut t.raw);
    // (local, candidate, stores seen), until a store disagrees.
    let mut seen: Vec<(u32, u32, u32)> = Vec::new();
    for x in lo..l {
        let i = x as usize;
        if t.pending.dead[i] || b.tags[i] != Tag::Store {
            continue;
        }
        let p = b.view(b.data[i][0]);
        let Some(y) = p.local().filter(|_| p.whole()) else {
            continue;
        };
        let mut v = b.data[i][1];
        if Ref(v).as_inst().is_some() && b.tags[v as usize] == Tag::Read {
            let r = b.view(b.data[v as usize][0]);
            v = r.value().filter(|_| r.whole()).unwrap_or(NONE);
        }
        let cand = (v != NONE && Ref(v).as_inst().is_some() && b.tags[v as usize] == Tag::Ref)
            .then(|| {
                let (rl, _) = resolve(b, b.data[v as usize][0], &mut steps, &mut raw);
                t.cands.iter().position(|c| {
                    let cs = t.cand_steps(c);
                    rl == c.local
                        && steps.len() == cs.len() + 1
                        && prefix(cs, &steps)
                        && steps[cs.len()] == ELEM
                        && elem_ref(t, v, b.data[v as usize][1], l)
                })
            })
            .flatten()
            .map_or(NONE, u32_of);
        match seen.iter_mut().find(|e| e.0 == y) {
            Some(e) if e.1 == cand => e.2 += 1,
            Some(e) => e.1 = NONE,
            None => seen.push((y, cand, 1)),
        }
    }
    for (y, k, n) in seen {
        let yi = y as usize;
        if k != NONE
            && n == t.stores[yi]
            && !t.param[yi]
            && !t.addressed[yi]
            && !t.captured[yi]
            && !t.escapes[yi]
        {
            t.elems.push((y, k));
        }
    }
    t.steps = steps;
    t.raw = raw;
}

/// Checks one instruction of loop `lp` (`x`, or `NONE` for a pending edit)
/// against every candidate (rules 2 and 3).
fn check(
    b: &Body,
    cx: &dyn OwnCx,
    t: &mut Tables,
    (x, lp): (u32, u32),
    tag: Tag,
    [a, w]: [u32; 2],
) {
    if is_suspension(tag) {
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
                // Through an element reference local: into the buffer.
                let through = steps.first() == Some(&DEREF) && t.elems.contains(&(l, u32_of(k)));
                let (covers, below, next_elem) = {
                    let cs = t.cand_steps(&c);
                    (
                        mine && prefix(&steps, cs),
                        mine && steps.len() > cs.len() && prefix(cs, &steps),
                        steps.get(cs.len()) == Some(&ELEM),
                    )
                };
                let exact = covers && steps.len() == c.len as usize;
                let mine = mine || through;
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
                    Tag::Ref => {
                        temp || !(covers || below) || (below && next_elem && elem_ref(t, x, w, lp))
                    }
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
                if exact && matches!(op, Some(IntrinsicOp::ListPush | IntrinsicOp::ListReserve)) {
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
