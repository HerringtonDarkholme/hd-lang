//! Make-unique diamonds (#2369): a std list's `if !is_unique(P) { P =
//! f(P, ..) }`, with `f`'s result a block no other holder has, is the
//! operation a builtin `IsUnique [P]` is, and hoists as one (rule 1).
//! The diamond is known by its shape and its callees' facts
//! (`OwnCx::tests_unique`, `OwnCx::made_block`, `OwnCx::keeps`), never
//! by a name. Inlined `self` locals name their receiver's place
//! ([`aliases`]), so `[*$self .block]` is the place `[*$xs .block]`.
//!
//! A hoisted diamond moves before its loop under a copy of the loop's
//! entry test, as a hoisted `IsUnique` runs ([`place`]), and every other
//! diamond of its place in the loop goes. Sound as the `IsUnique` hoist
//! is: the copy `f` makes is unique and nothing in the loop makes a second
//! holder of it (rule 2) or reaches `P` from outside (rule 3). `f` keeps
//! nothing of its argument, so the diamond's own read of `P` is no second
//! holder after it. The move runs `f` before the loop's first iteration's
//! earlier statements: a copy before a bounds panic, the builtin's order.
//!
//! Cost: one shape test per `If` and one alias test per store, each O(1)
//! but for its place's resolution (`DEPTH` reads); a move is one
//! renumbering of the body, once per body with any moves.

use hd_base::SliceSort as _;
use hd_types::Ty;

use super::{DEPTH, Tables, UNIQUE, hoistable, resolve};
use crate::ir::{Body, Callee, DEREF, NONE, PrimOp, ReadMode, Ref, Tag, Topology, u32_of};
use crate::own::{OwnCx, operands};

/// A local no alias was seen for yet, or one that names no other.
const UNSEEN: u32 = NONE;
const NO: u32 = NONE - 1;

/// Fills `t.alias` and `t.plumb`: each local whose every store gives it
/// what one other local names, through coercions that keep the value,
/// each link used once. A copy of a reference local, `$s = $xs` (an
/// inlined `self`), names that local: its `[*$s ..]` is `[*$xs ..]`, when
/// `$xs` is a parameter no store changes. A local reference, `$s = &$xs`,
/// names its local: `[*$s ..]` is `[$xs ..]`, when every other place of
/// `$s` goes through it. The links are plumbing: they copy no header and
/// take no reference the loop could keep ([`super::check`] skips them).
pub(super) fn aliases(b: &Body, t: &mut Tables) {
    let n = b.len();
    let locals = b.local_ty.len();
    super::fill(&mut t.alias, locals, (UNSEEN, false));
    super::fill(&mut t.plumb, n, false);
    super::fill(&mut t.stores, locals, 0);
    // Per alias local: its stores' first link (the store itself).
    let mut links: Vec<(u32, u32)> = Vec::new();
    let one_use = |t: &Tables, v: u32| t.uses(v).len() == 1;
    for i in 0..n {
        if t.pending.dead[i] {
            continue;
        }
        match b.tags[i] {
            Tag::Ref => {
                if let Some(l) = b.local_in_place(b.data[i][0]) {
                    // A local whose address is taken names itself.
                    t.alias[l.idx()].0 = NO;
                }
            }
            Tag::Store => {
                let p = b.view(b.data[i][0]);
                let Some(l) = p.local().filter(|_| p.whole()) else {
                    continue;
                };
                let li = l as usize;
                t.stores[li] += 1;
                let mut v = b.data[i][1];
                while Ref(v).as_inst().is_some()
                    && b.same_value_coercion(v as usize)
                    && one_use(t, v)
                {
                    v = b.data[v as usize][0];
                }
                let src = (Ref(v).as_inst().is_some() && one_use(t, v))
                    .then(|| {
                        let q = b.view(b.data[v as usize][0]);
                        let k = q.local().filter(|_| q.whole())?;
                        match b.tags[v as usize] {
                            Tag::Read if b.data[v as usize][1] != ReadMode::Move as u32 => {
                                Some(match t.alias[k as usize] {
                                    (r, d) if r < NO => (r, d),
                                    _ => (k, false),
                                })
                            }
                            Tag::Ref if super::spent_ref(b.data[v as usize][1]) => Some((k, true)),
                            _ => None,
                        }
                    })
                    .flatten();
                t.alias[li] = match (t.alias[li], src) {
                    (_, None) | ((NO, _), _) => (NO, false),
                    ((UNSEEN, _), Some(s)) => s,
                    (a, Some(s)) if a == s => a,
                    _ => (NO, false),
                };
                links.push((l, u32_of(i)));
            }
            _ => {}
        }
    }
    // What an alias names must hold still: a reference copy's parameter
    // is never stored; a local reference's other places all dereference.
    for l in 0..locals {
        let (r, d) = t.alias[l];
        let fixed = t.param[l]
            || b.local_flags[l]
                & (crate::ir::local_flags::CAPTURED | crate::ir::local_flags::CAPTURED_ASSIGNED)
                != 0
            || (r < NO && !d && (!t.param[r as usize] || t.stores[r as usize] != 0));
        if r >= NO || fixed {
            t.alias[l] = (NO, false);
        }
    }
    for i in 0..n {
        if t.pending.dead[i] || b.tags[i] == Tag::Store {
            continue;
        }
        let (oa, ob) = b.tags[i].operands();
        for (slot, op) in [oa, ob].into_iter().enumerate() {
            if op != crate::ir::Op::Place {
                continue;
            }
            let p = b.view(b.data[i][slot]);
            if let Some(l) = p.local()
                && t.alias[l as usize].1
                && !matches!(p.steps(b).next(), Some(crate::ir::Proj::Deref))
            {
                t.alias[l as usize] = (NO, false);
            }
        }
    }
    for (l, s) in links {
        if t.alias[l as usize].0 >= NO {
            continue;
        }
        t.plumb[s as usize] = true;
        let mut v = b.data[s as usize][1];
        while Ref(v).as_inst().is_some() {
            t.plumb[v as usize] = true;
            if !b.same_value_coercion(v as usize) {
                break;
            }
            v = b.data[v as usize][0];
        }
    }
}

impl Tables {
    /// [`resolve`], with an alias local's place replaced by the place it
    /// names ([`aliases`]): into `t.steps`; its local.
    pub(super) fn place(&mut self, b: &Body, word: u32) -> (u32, bool) {
        let (l, temp) = resolve(b, word, &mut self.steps, &mut self.raw);
        (self.unalias(l), temp)
    }

    /// Local `l`'s place in `t.steps`, as the local an alias names.
    pub(super) fn unalias(&mut self, l: u32) -> u32 {
        unalias(&self.alias, l, &mut self.steps)
    }
}

/// Local `l`'s place with steps `steps`, as the place of the local an
/// alias names ([`aliases`]): that local, `steps` changed in place.
pub(super) fn unalias(alias: &[(u32, bool)], l: u32, steps: &mut Vec<u32>) -> u32 {
    match alias.get(l as usize) {
        Some(&(r, false)) if r < NO => r,
        Some(&(r, true)) if r < NO && steps.first() == Some(&DEREF) => {
            steps.remove(0);
            r
        }
        _ => l,
    }
}

/// The diamond `If` `x` is: its first instruction and its place's local,
/// the place's steps in `t.steps`. Its test is `Not` of a uniqueness test
/// of a read of the place; its one arm reads the place, passes that read
/// to a call whose result is a block no other holder has and which keeps
/// none of it, maybe drops the read, and stores the result into the
/// place; nothing else stands between its first instruction and `x`.
pub(super) fn shape(b: &Body, cx: &dyn OwnCx, t: &mut Tables, x: u32) -> Option<(u32, u32)> {
    let [cond, arms] = b.data[x as usize];
    let &[then, NONE] = b.record(arms) else {
        return None;
    };
    let ci = Ref(cond).as_inst()?.0 as usize;
    if b.tags[ci] != Tag::Prim || b.data[ci][0] != PrimOp::Not as u32 {
        return None;
    }
    let &[test] = b.record(b.data[ci][1]) else {
        return None;
    };
    let ti = Ref(test).as_inst()?.0 as usize;
    if b.tags[ti] != Tag::Call
        || !Callee::from_words(b.record(b.data[ti][0])).is_some_and(|c| cx.tests_unique(&c))
    {
        return None;
    }
    let &[mut first] = b.call_args(ti) else {
        return None;
    };
    let mut links: usize = 3;
    while Ref(first).as_inst().is_some() && b.same_value_coercion(first as usize) {
        first = b.data[first as usize][0];
        links += 1;
    }
    if Ref(first).as_inst().is_none()
        || b.tags[first as usize] != Tag::Read
        || b.data[first as usize][1] == ReadMode::Move as u32
    {
        return None;
    }
    let (l, _) = t.place(b, b.data[first as usize][0]);
    if l == NONE || !hoistable(&t.steps) || t.steps.len() > DEPTH {
        return None;
    }
    let want = t.steps.clone();
    let same = |t: &mut Tables, word: u32| t.place(b, word).0 == l && t.steps == want;
    // The arm: read, call, drops of the read, store.
    let members = b.record(b.data[then as usize][0]);
    let (mut read, mut call, mut store) = (NONE, NONE, NONE);
    for &m in members {
        let mi = m as usize;
        if t.pending.dead[mi] {
            continue;
        }
        let [a, w] = b.data[mi];
        match b.tags[mi] {
            Tag::Read if read == NONE && w == ReadMode::Copy as u32 && same(t, a) => read = m,
            Tag::Call if call == NONE && read != NONE => {
                let callee = Callee::from_words(b.record(a))?;
                let args = b.call_args(mi);
                let (&arg, rest) = args.split_first()?;
                if arg != read
                    || rest.iter().any(|&r| Ref(r).as_inst().is_some())
                    || cx.made_block(&callee) < UNIQUE
                    || cx.keeps(&callee, 0, args.len())
                {
                    return None;
                }
                call = m;
            }
            Tag::Drop if read != NONE && b.view(a).value() == Some(read) && b.view(a).whole() => {}
            Tag::Store if store == NONE && call != NONE && w == call && same(t, a) => store = m,
            _ => return None,
        }
    }
    if store == NONE {
        return None;
    }
    // Nothing else between: the test's links, the arm and its block, `x`.
    let arm = members
        .iter()
        .filter(|&&m| !t.pending.dead[m as usize])
        .count();
    let live = (first..=x).filter(|&y| !t.pending.dead[y as usize]).count();
    (live == links + arm + 2).then_some((first, l))
}

/// What the move of a diamond needs after the splice: the diamond `If`,
/// the loop it leaves, its first instruction and its place's word.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Move {
    pub iff: u32,
    pub first: u32,
    pub lp: u32,
    pub word: u32,
}

/// Moves each diamond of `moves` before its loop, under a copy of the loop's entry test,
/// its place reads and store rewritten to its place's own word; every
/// other diamond of `gone` is deleted. A move whose diamond the splice
/// changed, whose values a statement outside it uses, which reads a value
/// from outside it, or whose loop's entry test is not a flat pure prefix
/// of local reads and operators, is left as it is. One renumbering.
///
/// # Errors
/// A malformed renumbering.
pub(crate) fn place(
    b: &mut Body,
    topo: &Topology,
    moves: &[Move],
    gone: &[(u32, u32)],
    new_of: &mut Vec<u32>,
) -> Result<bool, String> {
    let n = b.len();
    // Per instruction: out of the order where it stands (moved or
    // deleted); out of its block's record.
    let mut dead = vec![false; n];
    let mut out = vec![false; n];
    let mut touched = Vec::new();
    let mut ops = Vec::new();
    let inside = |lo: u32, hi: u32, v: u32| (lo..=hi).contains(&v);
    let closed = |b: &Body, ops: &mut Vec<_>, lo: u32, hi: u32, lp: u32| {
        // Every operand from inside; no use from outside but in `lo..=hi`.
        (lo..=hi).all(|x| {
            operands(b, x as usize, ops);
            ops.iter()
                .all(|o: &crate::own::Operand| inside(lo, hi, o.value))
        }) && (topo.first[lp as usize]..=lp)
            .filter(|&x| !inside(lo, hi, x))
            .all(|x| {
                operands(b, x as usize, ops);
                ops.iter().all(|o| !inside(lo, hi, o.value))
            })
    };
    let mut held = Vec::new();
    // A diamond the splice changed is no longer one: its shape test ran
    // before, so the range must still hold only its own instructions.
    for m in moves {
        let (iff, first, lp) = (m.iff, m.first, m.lp);
        if iff == NONE || first == NONE || lp == NONE {
            continue;
        }
        let Some(e) = super::entry_test(b, topo, lp) else {
            continue;
        };
        let prefix: Vec<u32> = super::loop_statements(b, lp).take(e.prefix).collect();
        let flat = prefix.iter().all(|&x| match b.tags[x as usize] {
            Tag::Read => b.view(b.data[x as usize][0]).local().is_some(),
            Tag::Prim => true,
            _ => false,
        });
        if !flat || !closed(b, &mut ops, first, iff, lp) {
            continue;
        }
        held.push((*m, iff, first, lp, e, prefix));
    }
    for &(x, first) in gone {
        if x == NONE || first == NONE {
            continue;
        }
        let lp = topo.loop_of[x as usize];
        if lp != NONE && closed(b, &mut ops, first, x, lp) {
            dead[first as usize..=x as usize].fill(true);
            let blk = topo.blk[x as usize];
            for &y in b.record(b.data[blk as usize][0]) {
                out[y as usize] |= inside(first, x, y);
            }
            touched.push(blk);
        }
    }
    if held.is_empty() && !dead.contains(&true) {
        return Ok(false);
    }
    // The guards' new instructions, numbered from `n` in the old order.
    let mut extra_order: Vec<(u32, Vec<u32>)> = Vec::new();
    let mut blocks: Vec<(u32, u32, Vec<u32>)> = Vec::new();
    for (m, iff, first, lp, e, prefix) in held {
        let syn = b.syn[lp as usize];
        let anchor = b.anchors.get(lp as usize).copied();
        let push = |b: &mut Body, tag: Tag, data: [u32; 2], ty: Ty| {
            b.tags.push(tag);
            b.data.push(data);
            b.ty.push(ty);
            b.syn.push(syn);
            if let Some(a) = anchor {
                b.anchors.push(a);
            }
            u32_of(b.len() - 1)
        };
        let mut copies: Vec<(u32, u32)> = Vec::new();
        let mut order = Vec::new();
        for &x in &prefix {
            let [a, w] = b.data[x as usize];
            let data = if b.tags[x as usize] == Tag::Prim {
                let vals: Vec<u32> = b
                    .record(w)
                    .iter()
                    .map(|&v| copies.iter().find(|c| c.0 == v).map_or(v, |c| c.1))
                    .collect();
                [a, b.push_record(&vals)]
            } else {
                [a, w]
            };
            let y = push(b, b.tags[x as usize], data, b.ty[x as usize]);
            copies.push((x, y));
            order.push(y);
        }
        let test = b.data[e.test as usize][0];
        let mut cond = copies.iter().find(|c| c.0 == test).map_or(test, |c| c.1);
        if e.leaves_on_true {
            let rec = b.push_record(&[cond]);
            cond = push(b, Tag::Prim, [PrimOp::Not as u32, rec], Ty::BOOL);
            order.push(cond);
        }
        // The diamond's place reads and store name its place.
        let ib = topo.blk[iff as usize];
        let members: Vec<u32> = b
            .record(b.data[ib as usize][0])
            .iter()
            .copied()
            .filter(|&x| inside(first, iff, x))
            .collect();
        for x in first..=iff {
            let i = x as usize;
            let reads = b.tags[i] == Tag::Read && b.view(b.data[i][0]).local().is_some();
            if reads || b.tags[i] == Tag::Store {
                b.data[i][0] = m.word;
            }
            dead[i] = true;
        }
        for &y in &members {
            out[y as usize] = true;
        }
        let rec = b.push_record(&members);
        let blk = push(b, Tag::Block, [rec, NONE], Ty::VOID);
        let arms = b.push_record(&[blk, NONE]);
        let guard = push(b, Tag::If, [cond, arms], Ty::VOID);
        order.extend((first..=iff).chain([blk, guard]));
        let ob = topo.blk[lp as usize];
        blocks.push((ob, topo.pos[lp as usize], order.clone()));
        touched.extend([ob, ib]);
        extra_order.push((topo.first[lp as usize], order));
    }
    // Block records: a guard's statements before its loop; the moved and
    // deleted members out of their blocks.
    touched.sort_stable();
    touched.dedup();
    for blk in touched {
        let mut words = Vec::new();
        for (p, &m) in b.record(b.data[blk as usize][0]).iter().enumerate() {
            for (ob, pos, order) in &blocks {
                if *ob == blk && *pos == u32_of(p) {
                    // Only the guard's top statements: prefix copies, the
                    // negation and the `If`.
                    words.extend(
                        order
                            .iter()
                            .copied()
                            .filter(|&y| y as usize >= n && b.tags[y as usize] != Tag::Block),
                    );
                }
            }
            if !out[m as usize] {
                words.push(m);
            }
        }
        b.data[blk as usize][0] = b.push_record(&words);
    }
    // The new order: each guard right before its loop's first instruction.
    extra_order.sort_stable_by_key(|e| e.0);
    let mut order = Vec::with_capacity(b.len());
    let mut k = 0;
    for x in 0..u32_of(n) {
        while k < extra_order.len() && extra_order[k].0 == x {
            order.extend_from_slice(&extra_order[k].1);
            k += 1;
        }
        if !dead[x as usize] {
            order.push(x);
        }
    }
    crate::own::splice::reorder(b, &order, new_of)?;
    Ok(true)
}
