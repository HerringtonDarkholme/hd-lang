//! Case of a known constructor after inlining (#2367), on TIR, before the
//! ownership passes. An inlined `next()` or any inlined function that
//! returns an enum leaves `Match %s` on a `Scope` whose every way out is
//! a `NewVariant`: each `return .Some(v)` and `.None` builds the value
//! the match takes apart at once. The backend then keeps a (tag, payload)
//! pair and a dispatch block that V8 threads and Cranelift does not.
//!
//! This pass sends each way out straight to its arm, GHC's
//! case-of-known-constructor over a join (case-of-case), or LLVM's jump
//! threading of a switch on a phi of constants. For a match whose
//! scrutinee is a join of known constructors (a `NewVariant`, or a
//! `Scope` without suites or an `If` whose every exit is one), used by
//! nothing but the match's decision:
//!
//! - each constructor's decision is read off the `SwitchTag` case its
//!   variant selects, which must be a leaf: payload reads `[%s as .V .k]`,
//!   `Store`s of them (or of constants) into the arm's bindings, `ToArm`;
//! - the exit becomes those `Store`s of the constructor's own operands
//!   and a `Break` to a landing block whose continuation is the arm;
//! - the match becomes nested landing blocks, one per arm some exit
//!   reaches, innermost first, as a Wasm `br_table` lowers to blocks:
//!   `Block Lm [Block L1 [Block L0 [producer]; arm 0; Break Lm]; arm 1]`.
//!
//! The match's arms move whole (a `Scope` runs each), the decision and
//! the constructors go, and an arm no exit reaches goes with them. A
//! constructor field no leaf binds must be a value that copies, so
//! nothing is left to drop. Anything else keeps the match: a scrutinee
//! used elsewhere (it escapes, so it stays built), a decision that tests
//! a payload, a guard, or an exit that is not a known constructor.
//!
//! Cost: linear in the body. One use count and one `Break` bucket pass,
//! then per match its decision's leaves and its producer's exits, each
//! instruction visited by one match at most (a scrutinee has one use, so
//! producers never share an exit); one rebuild of the touched block
//! records and one renumbering walk ([`hd_tir::own::reorder`]).

use hd_base::NodeIdx;
use hd_tir::ir::{Body, NONE, PlaceRoot, Proj, Ref, Tag, Topology, Word, place, slots, u32_of};
use hd_types::Ty;

#[cfg(test)]
mod tests;

#[cfg(test)]
thread_local! {
    /// Work units of [`run`] on this thread, for the counted test.
    static WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn charge(n: usize) {
    #[cfg(test)]
    WORK.with(|w| w.set(w.get() + n));
    #[cfg(not(test))]
    let _ = n;
}

/// Where a constructor or a join reaches the match: as the value of a
/// `Break`, as a block's tail, or as the scrutinee itself.
#[derive(Clone, Copy)]
enum Site {
    Break(u32),
    Tail(u32),
    Root,
}

/// What a leaf stores into a binding: payload field `k` or a word as is.
#[derive(Clone, Copy)]
enum Src {
    Field(u32),
    Word(u32),
}

/// A `SwitchTag` case with no test left: its stores and its arm.
struct Leaf {
    arm: u32,
    stores: Vec<(u32, Src, NodeIdx)>,
    /// The fields it reads, as a bit set over the first 64.
    fields: u64,
    reads: u32,
}

/// One match to rewrite.
struct Plan {
    m: usize,
    /// Each exit: the constructor, where it reaches the match, its arm.
    exits: Vec<(u32, Site, usize)>,
    /// The joins between them and the match, with where each reaches it.
    joins: Vec<(u32, Site)>,
    /// The blocks of the joins, whose values never come.
    join_blocks: Vec<u32>,
    leaves: Vec<Leaf>,
}

/// The block edits of all plans, applied once by instruction identity, so
/// plans in one another's arms or producers compose.
struct Edits {
    before: hd_base::HashMap<u32, Vec<u32>>,
    append: hd_base::HashMap<u32, Vec<u32>>,
    removed: Vec<bool>,
    /// New blocks and their members, before the edits apply.
    fresh: Vec<(u32, Vec<u32>)>,
    /// Old blocks whose records change.
    touched: Vec<u32>,
}

/// Runs the pass over `b`; whether it changed anything. `copies` says a
/// type's values copy, so a field no arm binds needs no drop.
///
/// # Errors
/// A malformed body the renumbering refuses.
pub fn run(b: &mut Body, copies: &dyn Fn(Ty) -> bool) -> Result<bool, String> {
    let n = b.len();
    charge(n);
    if !b.tags.contains(&Tag::Match) {
        return Ok(false);
    }
    let Some(walk) = evaluation_order(b) else {
        return Ok(false);
    };
    if walk.len() != n {
        // ruled out: every instruction is listed in one block of the
        // tree from the root (invariant 2); a body that is not keeps its
        // match.
        return Ok(false);
    }
    let topo = Topology::new(b);
    let mut uses = vec![0u32; n];
    for i in 0..n {
        slots(b, i, |_, w, word| {
            charge(1);
            if matches!(word, Word::Value(_) | Word::Provider | Word::ProviderRef)
                && w & Ref::CONST_BIT == 0
                && (w as usize) < n
            {
                uses[w as usize] += 1;
            }
        });
    }
    // The `Break`s to each block: a counting sort by target.
    let mut from = vec![0u32; n + 1];
    let target = |b: &Body, i: usize| -> Option<usize> {
        if b.tags[i] != Tag::Break {
            return None;
        }
        let t = *b.label_inst.get(b.data[i][0] as usize)? as usize;
        (t < n).then_some(t)
    };
    for i in 0..n {
        if let Some(t) = target(b, i) {
            from[t + 1] += 1;
        }
    }
    for t in 0..n {
        from[t + 1] += from[t];
    }
    let mut next = from.clone();
    let mut breaks = vec![0u32; from[n] as usize];
    for i in 0..n {
        if let Some(t) = target(b, i) {
            breaks[next[t] as usize] = u32_of(i);
            next[t] += 1;
        }
    }
    let plans: Vec<Plan> = (0..n)
        .filter(|&m| b.tags[m] == Tag::Match)
        .filter_map(|m| plan(b, &topo, &uses, (&breaks, &from), copies, m))
        .collect();
    if plans.is_empty() {
        return Ok(false);
    }
    let mut e = Edits {
        before: hd_base::HashMap::default(),
        append: hd_base::HashMap::default(),
        removed: vec![false; n],
        fresh: Vec::new(),
        touched: Vec::new(),
    };
    let empty = b.push_record(&[]);
    for p in &plans {
        rewrite(b, &topo, &mut e, p, empty);
    }
    apply(b, &e);
    let order = evaluation_order(b).ok_or("known_ctor: a block left the tree")?;
    hd_tir::own::reorder(b, &order)?;
    compact_labels(b);
    Ok(true)
}

/// Drops the labels of deleted blocks (a dead arm's, the decision's) and
/// renumbers the rest, so every label row names a block (invariant 5).
fn compact_labels(b: &mut Body) {
    let mut new_of = vec![NONE; b.label_inst.len()];
    let mut kept = Vec::with_capacity(b.label_inst.len());
    for (l, &blk) in b.label_inst.iter().enumerate() {
        if blk != NONE {
            new_of[l] = u32_of(kept.len());
            kept.push(blk);
        }
    }
    if kept.len() == b.label_inst.len() {
        return;
    }
    for i in 0..b.len() {
        charge(1);
        if matches!(b.tags[i], Tag::Break | Tag::Continue) {
            b.data[i][0] = new_of[b.data[i][0] as usize];
        }
    }
    b.label_inst = kept;
}

/// The instructions in evaluation order from the body's root: each
/// block's members, a listed block's own members before it, the root
/// last. `None` when a block lists one twice or the root is missing.
fn evaluation_order(b: &Body) -> Option<Vec<u32>> {
    let root = *b.sub_root.first()? as usize;
    let n = b.len();
    let mut seen = vec![false; n];
    let mut order = Vec::with_capacity(n);
    // (block, next member position): an explicit stack, no recursion.
    let mut stack = vec![(root, 0usize)];
    seen[root] = true;
    while let Some(&mut (blk, ref mut at)) = stack.last_mut() {
        charge(1);
        let members = b.record(b.data[blk][0]);
        if let Some(&x) = members.get(*at) {
            *at += 1;
            let x = x as usize;
            if x >= n || seen[x] {
                return None;
            }
            seen[x] = true;
            if b.tags[x] == Tag::Block {
                stack.push((x, 0));
            } else {
                order.push(u32_of(x));
            }
        } else {
            stack.pop();
            order.push(u32_of(blk));
        }
    }
    Some(order)
}

fn value(w: u32, n: usize) -> Option<usize> {
    (w & Ref::CONST_BIT == 0 && (w as usize) < n).then_some(w as usize)
}

/// The plan for match `m`, or `None` where it keeps its dispatch.
fn plan(
    b: &Body,
    topo: &Topology,
    uses: &[u32],
    (breaks, from): (&[u32], &[u32]),
    copies: &dyn Fn(Ty) -> bool,
    m: usize,
) -> Option<Plan> {
    let n = b.len();
    let s = value(b.data[m][0], n)?;
    let blocks = b.record(b.data[m][1]);
    let (&dec, arms) = blocks.split_first()?;
    let parent = topo.blk[m];
    // The decision and the arms are listed beside the match.
    if blocks
        .iter()
        .any(|&x| x == NONE || topo.blk[x as usize] != parent)
    {
        return None;
    }
    // Every arm's type is the match's, or `never`.
    let t = b.ty[m];
    if arms
        .iter()
        .any(|&a| b.ty[a as usize] != t && b.ty[a as usize] != Ty::NEVER)
    {
        return None;
    }
    // The decision: its case blocks, then one `SwitchTag` on `s`.
    let members = b.record(b.data[dec as usize][0]);
    let (&switch, cases) = members.split_last()?;
    let switch = switch as usize;
    if b.tags[switch] != Tag::SwitchTag
        || b.data[switch][0] != u32_of(s)
        || cases.iter().any(|&c| b.tags[c as usize] != Tag::Block)
    {
        return None;
    }
    let rec = b.record(b.data[switch][1]);
    let (&default, pairs) = rec.split_last()?;
    // Each case's leaf, read once: a non-leaf case is `None`.
    let mut case_of: Vec<(u32, usize)> = Vec::new();
    let mut leaves: Vec<Leaf> = Vec::new();
    let mut reads = 0u32;
    let mut leaf_at = |blk: u32, leaves: &mut Vec<Leaf>| -> Option<usize> {
        let l = leaf(b, uses, s, blk as usize, arms.len())?;
        reads += l.reads;
        leaves.push(l);
        Some(leaves.len() - 1)
    };
    for case in pairs.chunks_exact(2) {
        charge(1);
        let k = leaf_at(case[1], &mut leaves)?;
        case_of.push((case[0], k));
    }
    let default_leaf = if default == NONE {
        None
    } else {
        Some(leaf_at(default, &mut leaves)?)
    };
    // `s` is used by the match, its switch and the leaves' reads: no more.
    if uses[s] != 2 + reads {
        return None;
    }
    // The producer: a join of known constructors.
    let mut exits = Vec::new();
    let mut joins = Vec::new();
    let mut join_blocks = Vec::new();
    let mut work = vec![(u32_of(s), Site::Root)];
    let blocks_of = |blk: u32, work: &mut Vec<(u32, Site)>| -> Option<()> {
        let tail = b.data[blk as usize][1];
        if tail != NONE {
            work.push((u32::try_from(value(tail, n)?).ok()?, Site::Tail(blk)));
        }
        for &k in &breaks[from[blk as usize] as usize..from[blk as usize + 1] as usize] {
            work.push((value(b.data[k as usize][1], n).map(u32_of)?, Site::Break(k)));
        }
        Some(())
    };
    while let Some((v, site)) = work.pop() {
        charge(1);
        let x = v as usize;
        if uses[x] != 1 && !matches!(site, Site::Root) {
            return None;
        }
        match b.tags[x] {
            Tag::NewVariant => {
                let variant = b.data[x][0];
                let k = case_of
                    .iter()
                    .find(|&&(c, _)| c == variant)
                    .map(|&(_, k)| k)
                    .or(default_leaf)?;
                let fields = b.record(b.data[x][1]);
                let l = &leaves[k];
                // Every field the leaf reads is there; every other one
                // copies, so dropping the variant drops nothing.
                if fields.len() < 64 && l.fields >> fields.len() != 0 {
                    return None;
                }
                for (j, &f) in fields.iter().enumerate() {
                    let read = j < 64 && l.fields & (1 << j) != 0;
                    let plain =
                        f & Ref::CONST_BIT != 0 || value(f, n).is_some_and(|f| copies(b.ty[f]));
                    if read && f == NONE || !read && f != NONE && !plain {
                        return None;
                    }
                }
                exits.push((v, site, k));
            }
            Tag::Scope if b.record(b.data[x][1]).is_empty() => {
                let blk = b.data[x][0];
                joins.push((v, site));
                join_blocks.push(blk);
                blocks_of(blk, &mut work)?;
            }
            Tag::If => {
                let arms = b.record(b.data[x][1]);
                if arms.len() != 2 || arms.contains(&NONE) {
                    return None;
                }
                joins.push((v, site));
                for &a in arms {
                    join_blocks.push(a);
                    blocks_of(a, &mut work)?;
                }
            }
            _ => return None,
        }
    }
    // The root join or constructor is listed beside the match, and only
    // the match's own blocks stand between them: nothing runs between the
    // producer and the decision that the landings would reorder.
    if topo.blk[s] != parent {
        return None;
    }
    let listed = b.record(b.data[parent as usize][0]);
    let (from, to) = (topo.pos[s] as usize + 1, topo.pos[m] as usize);
    if from > to
        || listed[from..to]
            .iter()
            .any(|&x| topo.owner[x as usize] != u32_of(m))
    {
        return None;
    }
    Some(Plan {
        m,
        exits,
        joins,
        join_blocks,
        leaves,
    })
}

/// Case block `blk` of a switch on `s` as a leaf: payload reads of `s`,
/// each stored once into a bare local, stores of constants, then `ToArm`.
fn leaf(b: &Body, uses: &[u32], s: usize, blk: usize, arms: usize) -> Option<Leaf> {
    let members = b.record(b.data[blk][0]);
    let (&last, body) = members.split_last()?;
    let last = last as usize;
    if b.tags[last] != Tag::ToArm || b.data[last][0] as usize >= arms {
        return None;
    }
    let mut field_of: Vec<(u32, u32)> = Vec::new();
    let mut out = Leaf {
        arm: b.data[last][0],
        stores: Vec::new(),
        fields: 0,
        reads: 0,
    };
    for &x in body {
        charge(1);
        let x = x as usize;
        match b.tags[x] {
            Tag::Read => {
                let p = b.decode_place(b.data[x][0])?;
                let (PlaceRoot::Value(r), [Proj::Variant(_), Proj::Field(k)]) =
                    (p.root, p.proj.as_slice())
                else {
                    return None;
                };
                if r.0 != u32_of(s) || *k >= 64 || out.fields & (1 << k) != 0 || uses[x] != 1 {
                    return None;
                }
                out.fields |= 1 << k;
                out.reads += 1;
                field_of.push((u32_of(x), *k));
            }
            Tag::Store if b.data[x][0] & place::BARE_LOCAL != 0 => {
                let w = b.data[x][1];
                let src = match field_of.iter().find(|&&(r, _)| r == w) {
                    Some(&(_, k)) => Src::Field(k),
                    None if w & Ref::CONST_BIT != 0 => Src::Word(w),
                    None => return None,
                };
                out.stores.push((b.data[x][0], src, b.syn[x]));
            }
            _ => return None,
        }
    }
    // Every read feeds a store (its one use is in this leaf).
    let fed = out
        .stores
        .iter()
        .filter(|s| matches!(s.1, Src::Field(_)))
        .count();
    (fed == field_of.len()).then_some(out)
}

/// Appends a new instruction and returns it.
fn push(b: &mut Body, tag: Tag, data: [u32; 2], ty: Ty, syn: NodeIdx) -> u32 {
    let i = u32_of(b.len());
    b.tags.push(tag);
    b.data.push(data);
    b.ty.push(ty);
    b.syn.push(syn);
    if !b.anchors.is_empty() {
        b.anchors.push(NONE);
    }
    i
}

fn label(b: &mut Body, blk: u32) -> u32 {
    b.label_inst.push(blk);
    u32_of(b.label_inst.len() - 1)
}

fn rewrite(b: &mut Body, topo: &Topology, e: &mut Edits, p: &Plan, empty: u32) {
    let m = p.m;
    let syn = b.syn[m];
    let t = b.ty[m];
    let blocks = b.record(b.data[m][1]).to_vec();
    let (dec, arms) = (blocks[0], &blocks[1..]);
    let s = b.data[m][0];
    // The arms some exit reaches, in their order: one landing each.
    let mut reached = vec![false; arms.len()];
    for &(_, _, k) in &p.exits {
        reached[p.leaves[k].arm as usize] = true;
    }
    let order: Vec<usize> = (0..arms.len()).filter(|&a| reached[a]).collect();
    // Landing blocks and the outer block are pushed now, their members
    // recorded at the end; a `Break` names a label, so labels come first.
    let mut landing = vec![NONE; arms.len()];
    let mut labels = vec![NONE; arms.len()];
    for &a in &order {
        landing[a] = push(b, Tag::Block, [NONE, NONE], Ty::VOID, syn);
        labels[a] = label(b, landing[a]);
    }
    let outer = push(b, Tag::Block, [NONE, NONE], t, syn);
    let outer_label = label(b, outer);
    // Each exit: the leaf's stores of the constructor's operands, then a
    // jump to its arm's landing.
    let mut root_members = Vec::new();
    for &(v, site, k) in &p.exits {
        let leaf = &p.leaves[k];
        let fields = b.record(b.data[v as usize][1]).to_vec();
        let mut new = Vec::new();
        for &(at, src, ssyn) in &leaf.stores {
            let w = match src {
                Src::Field(j) => fields[j as usize],
                Src::Word(w) => w,
            };
            new.push(push(b, Tag::Store, [at, w], Ty::VOID, ssyn));
        }
        let to = labels[leaf.arm as usize];
        e.removed[v as usize] = true;
        e.touched.push(topo.blk[v as usize]);
        match site {
            Site::Break(i) => {
                e.before.entry(i).or_default().extend(new);
                b.data[i as usize] = [to, NONE];
                e.touched.push(topo.blk[i as usize]);
            }
            Site::Tail(blk) => {
                new.push(push(
                    b,
                    Tag::Break,
                    [to, NONE],
                    Ty::NEVER,
                    b.syn[v as usize],
                ));
                e.append.entry(blk).or_default().extend(new);
                b.data[blk as usize][1] = NONE;
                b.ty[blk as usize] = Ty::NEVER;
                e.touched.push(blk);
            }
            Site::Root => {
                new.push(push(b, Tag::Break, [to, NONE], Ty::NEVER, syn));
                root_members.extend(new);
                e.touched.push(topo.blk[v as usize]);
            }
        }
    }
    // The joins never finish now: a `Break` that carried one goes, and a
    // tail that was one leaves its block with none.
    for &(j, site) in &p.joins {
        b.ty[j as usize] = Ty::NEVER;
        match site {
            Site::Break(i) => {
                e.removed[i as usize] = true;
                e.touched.push(topo.blk[i as usize]);
            }
            Site::Tail(blk) => {
                b.data[blk as usize][1] = NONE;
                b.ty[blk as usize] = Ty::NEVER;
                e.touched.push(blk);
            }
            Site::Root => {
                // The root join moves into the innermost landing, with the
                // blocks it runs, which are listed beside it.
                let parent = topo.blk[m];
                for &x in b.record(b.data[parent as usize][0]) {
                    if x != j && topo.owner[x as usize] == j {
                        root_members.push(x);
                    }
                }
                root_members.push(j);
            }
        }
    }
    for &blk in &p.join_blocks {
        b.ty[blk as usize] = Ty::NEVER;
    }
    // The match's parent loses the producer's root, the decision and the
    // arms, and lists the outer block before the match, which runs it.
    let parent = topo.blk[m];
    for &x in &root_members {
        e.removed[x as usize] |= topo.blk[x as usize] == parent;
    }
    e.removed[s as usize] = true;
    e.removed[dec as usize] = true;
    for &a in arms {
        e.removed[a as usize] = true;
    }
    e.before.entry(u32_of(m)).or_default().push(outer);
    e.touched.push(parent);
    // A removed root member that is moved must still be listed in its
    // landing: `fresh` lists are taken as given, not filtered.
    let mut inner = root_members;
    for (j, &a) in order.iter().enumerate() {
        let run = push(b, Tag::Scope, [landing[a], empty], Ty::VOID, syn);
        let arm = arms[a];
        let ty = b.ty[arm as usize];
        let took = push(b, Tag::Scope, [arm, empty], ty, syn);
        e.fresh.push((landing[a], std::mem::take(&mut inner)));
        let mut members = vec![landing[a], run, arm, took];
        let last = j + 1 == order.len();
        // Each arm that finishes leaves the outer block with its value, as
        // an inlined body's `return`s do; the last one of a `void` match
        // falls through.
        if ty != Ty::NEVER && !(last && t == Ty::VOID) {
            let carried = if t == Ty::VOID { NONE } else { took };
            members.push(push(b, Tag::Break, [outer_label, carried], Ty::NEVER, syn));
        }
        if last {
            e.fresh.push((outer, members));
        } else {
            inner = members;
        }
    }
    // The match runs the outer block in its place.
    b.tags[m] = Tag::Scope;
    b.data[m] = [outer, empty];
}

/// Writes each touched or new block's record: the old members with the
/// stores before them and the jumps after them, less the removed ones;
/// a new block's members as given, with the same insertions.
fn apply(b: &mut Body, e: &Edits) {
    let mut touched = e.touched.clone();
    hd_base::SliceSort::sort_stable(&mut touched[..]);
    touched.dedup();
    let expand = |members: &[u32], fresh: bool, out: &mut Vec<u32>| {
        for &x in members {
            charge(1);
            if let Some(pre) = e.before.get(&x) {
                out.extend_from_slice(pre);
            }
            if fresh || !e.removed.get(x as usize).copied().unwrap_or(false) {
                out.push(x);
            }
        }
    };
    let mut lists: Vec<(u32, Vec<u32>)> = Vec::new();
    for &blk in &touched {
        let mut out = Vec::new();
        expand(b.record(b.data[blk as usize][0]), false, &mut out);
        if let Some(tail) = e.append.get(&blk) {
            out.extend_from_slice(tail);
        }
        lists.push((blk, out));
    }
    for (blk, members) in &e.fresh {
        let mut out = Vec::new();
        expand(members, true, &mut out);
        lists.push((*blk, out));
    }
    for (blk, out) in lists {
        let rec = b.push_record(&out);
        b.data[blk as usize][0] = rec;
    }
}
