//! Loads read once while nothing between can write them (#2343). Runs in
//! release, after [`super::loop_loads`].
//!
//! A load through a reference a parameter holds, `[*$p .f]`, or that
//! reference read on the way of a longer place, `*$p .state` in
//! `[*$p .state * .next]`, gives the value an earlier load of the same
//! place gave when that load dominates it and nothing on the way may write
//! the place. The group's first load is read once before it; a longer
//! place restarts at that read's value (`[*%r .next]`, so a write through
//! it keeps the value, invariant 23), and a whole load reads a local the
//! read fills. The parameter is one the body never stores, so every copy
//! of it (an inlined `self` each) names the same reference.
//!
//! The method is redundant-load elimination over dominator scopes, as
//! LLVM's `EarlyCSE` keeps available loads per dominator-tree scope, with
//! Cranelift's alias analysis (`cranelift/codegen/src/alias_analysis.rs`)
//! for kills: a load stays available until a write of its alias class.
//! Cranelift's classes are coarse (heap, table, vmctx, other) and a call
//! kills every one; here places give the classes, and the language rules
//! and the callee's write summary say what a write can reach:
//!
//! - **Frozen**: a field of the referent of a `&T` parameter. While the
//!   reference is live, which it is for the whole call, no place
//!   overlapping its referent is written (`ref.excl.shared`), and no `Ptr`
//!   write, store, `copy` or `fill` std makes writes a byte of it
//!   (`module.rt.ptr.write.shared`, `module.rt.memory.store.shared`). An
//!   atomic call still may, so a call kills the class unless its callee
//!   writes nothing a caller reaches (`hd_tir::own::Summary::writes`,
//!   [`Cx::call_writes`]); so do a count's release and a drop, which may
//!   run a drop hook. A store that drops nothing, a raw write and a retain
//!   do not.
//! - **Apart**: a field of the referent of any other parameter reference,
//!   killed by any write but a store that drops nothing into another
//!   field of the same referent: two fields of one value never share a
//!   byte. LLVM gets the same from struct-path TBAA; here the place says it.
//!
//! A place through a reference read from memory has no class: an earlier
//! read of that reference may have seen another value than a later one.
//!
//! Instructions come in evaluation order, a loop after its body, so the
//! instructions between two loads in index order cover every path from
//! the first to the second that stays in the loops around the first; for
//! a second load in a loop the first is outside of, the range runs to that
//! loop's end.
//!
//! Cost: one pass for the kill counts and the candidates, each definition
//! a key follows computed once ([`super::places`]), then per candidate a
//! hash lookup, an amortized O(1) pop of its key's stack of dominating
//! leaders, a walk of the loops around it, and binary searches over its
//! root's keyed stores: O(n × log n) plus the places' lengths.

use super::places::{Defs, Key};
use super::{Cx, Lists, Sets, Topology, scans};
use hd_tir::ir::{Body, Callee, NONE, Nest, Place, PlaceRoot, Proj, ReadMode, Tag, local_flags};
use hd_tir::ir::{place, u32_of};
use hd_types::Ty;

/// What a candidate reads: a whole load, or the reference before the
/// dereference at projection `j` of its place.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    Whole,
    Prefix(usize),
}

/// The alias class of a key ([`self`]).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Frozen,
    Apart(Option<u32>),
}

/// A load group: its first load, the value's type and the loads that
/// reuse it.
struct Group {
    leader: u32,
    part: Part,
    key: Key,
    ty: Ty,
    members: Vec<(u32, Part)>,
}

/// Reads each redundant load of `b` from the local its group's first load
/// fills.
///
/// # Errors
/// The splice of the new reads failing, a malformed body.
pub(super) fn run(b: &mut Body, cx: &dyn Cx) -> Result<(), String> {
    let topo = Topology::new(b);
    let nest = Nest::new(b, &topo);
    let sets = Sets::new(b);
    scans(2 * b.len());
    let n = b.len();
    let mut awaits = vec![false; b.sub_root.len()];
    for i in 0..n {
        if matches!(
            b.tags[i],
            Tag::Await | Tag::AwaitValue | Tag::AwaitAll | Tag::AwaitRace
        ) {
            awaits[topo.sub[i] as usize] = true;
        }
    }
    let stored = |sub: u32, local: usize| {
        b.storage
            .binary_search_by(|&(s, x, _)| (s.idx(), x.idx()).cmp(&(sub as usize, local)))
            .is_ok()
    };
    let defs = Defs::new(b, &topo, &nest, &sets, &stored);
    // `any[i]` and `frozen[i]`: the writes before `i` that kill an apart
    // or deeper key, and a frozen one. `keyed`: the stores of an uncounted
    // value to a field of a referent, by root local and by root and field.
    let mut any = vec![0u32; n + 1];
    let mut frozen = vec![0u32; n + 1];
    let mut keyed: hd_base::HashMap<(u32, Option<u32>), Vec<u32>> = hd_base::HashMap::default();
    let mut keyed_root: hd_base::HashMap<u32, Vec<u32>> = hd_base::HashMap::default();
    let mut loads: Vec<(u32, Part, Key, Class, Option<Ty>)> = Vec::new();
    for i in 0..n {
        let x = u32_of(i);
        let sub = topo.sub[i];
        let cell = |local: usize| stored(sub, local);
        let lists = Lists {
            b,
            topo: &topo,
            nest: &nest,
            cx,
            cell: &cell,
            sets: &sets,
        };
        // A store that drops nothing: of an uncounted value, or into a
        // `Ptr` place (`module.rt.ptr.write.over`).
        let plain_store = b.tags[i] == Tag::Store
            && (!cx.tracked(stored_ty(b, i))
                || b.decode_place(b.data[i][0])
                    .is_some_and(|p| matches!(p.root, PlaceRoot::Ptr(_))));
        // A count's header word is no place's byte.
        let writes = b.tags[i] != Tag::Retain && lists.writes_memory(i);
        any[i + 1] = any[i] + u32::from(writes);
        frozen[i + 1] = frozen[i] + u32::from(writes && !plain_store && !raw_write(b, i, cx));
        if writes
            && plain_store
            && let Some(s) = b.decode_place(b.data[i][0]).and_then(|p| key(&defs, &p, x))
            && let Some(class) = class_of(b, cx, &defs, s)
            && let Class::Apart(field) = class
        {
            let root = defs.local(s);
            keyed.entry((root, field)).or_default().push(x);
            keyed_root.entry(root).or_default().push(x);
        }
        if awaits[sub as usize] {
            continue;
        }
        if let Some((part, place, ty)) = candidate(b, cx, i)
            && let Some(key) = key(&defs, &place, x)
            && let Some(class) = class_of(b, cx, &defs, key)
            && fixed(b, &sets, &cell, defs.local(key) as usize)
        {
            loads.push((x, part, key, class, ty));
        }
    }
    // Each load joins the group of the nearest earlier load of its key
    // that dominates it, when nothing between kills the key: per key a
    // stack of group leaders, each dominating the one above it.
    let mut groups: Vec<Group> = Vec::new();
    let mut open: hd_base::HashMap<Key, Vec<usize>> = hd_base::HashMap::default();
    for (x, part, key, class, ty) in loads {
        let sub = topo.sub[x as usize];
        let stack = open.entry(key).or_default();
        while let Some(&g) = stack.last() {
            let l = groups[g].leader;
            if topo.sub[l as usize] == sub && nest.dominates(&topo, l, x) {
                break;
            }
            stack.pop();
        }
        if let Some(&g) = stack.last() {
            let l = groups[g].leader;
            let hi = range_end(&topo, l, x);
            let count = |v: &[u32]| v[hi as usize + 1] - v[l as usize];
            let within = |v: Option<&Vec<u32>>| {
                v.map_or(0, |v| {
                    u32_of(v.partition_point(|&s| s <= hi) - v.partition_point(|&s| s < l))
                })
            };
            let killed = match class {
                Class::Frozen => count(&frozen) != 0,
                // A store to another field of the same referent is apart.
                Class::Apart(field) => {
                    let root = defs.local(key);
                    let apart = field.map_or(0, |f| {
                        within(keyed_root.get(&root))
                            - within(keyed.get(&(root, Some(f))))
                            - within(keyed.get(&(root, None)))
                    });
                    count(&any) != apart
                }
            };
            if !killed {
                groups[g].members.push((x, part));
                continue;
            }
            stack.pop();
        }
        let Some(ty) = ty else {
            continue;
        };
        open.entry(key).or_default().push(groups.len());
        groups.push(Group {
            leader: x,
            part,
            key,
            ty,
            members: Vec::new(),
        });
    }
    groups.retain(|g| !g.members.is_empty() && topo.blk[g.leader as usize] != NONE);
    if groups.is_empty() {
        return Ok(());
    }
    let places: Vec<(u32, Vec<Proj>)> = (groups.iter())
        .map(|g| (defs.local(g.key), defs.proj(g.key)))
        .collect();
    let mut inserts = Vec::new();
    for (g, (root, proj)) in groups.into_iter().zip(places) {
        let at = g.leader as usize;
        let mode = if cx.tracked(g.ty) {
            ReadMode::Borrow
        } else {
            ReadMode::Copy
        } as u32;
        let words = Place {
            root: PlaceRoot::Local(hd_base::LocalId::from_raw(root)),
            proj,
        }
        .words()
        .ok_or("a reused load's place is a bare local")?;
        let word = push_record(b, &words);
        // Insertion `k` is value `n + k` (`insert_batch`).
        let read = u32_of(n + inserts.len());
        let syn = b.syn[at];
        let mut new = vec![(Tag::Read, [word, mode], g.ty)];
        // A whole load reads a local the group's read fills; a longer place
        // restarts at the read's value, a write through which keeps that
        // value (invariant 23).
        let mut local = NONE;
        let wholes = std::iter::once(g.part).chain(g.members.iter().map(|m| m.1));
        if wholes.into_iter().any(|p| p == Part::Whole) {
            local = u32_of(b.local_ty.len());
            b.local_ty.push(g.ty);
            b.local_name.push(cx.load_name());
            b.local_syn.push(b.local_syn[root as usize]);
            b.local_flags
                .push(local_flags::ASSIGNED | local_flags::READ);
            new.push((Tag::Store, [local | place::BARE_LOCAL, read], Ty::VOID));
        }
        for (tag, data, ty) in new {
            inserts.push(hd_tir::own::BatchInsert {
                block: topo.blk[at],
                pos: topo.pos[at],
                tag,
                data,
                ty,
                syn,
            });
        }
        for (x, part) in std::iter::once((g.leader, g.part)).chain(g.members) {
            let x = x as usize;
            match part {
                Part::Whole => b.data[x] = [local | place::BARE_LOCAL, b.data[x][1]],
                Part::Prefix(j) => {
                    let rest = suffix_words(b, x, j).ok_or("a reused load's place is malformed")?;
                    let mut w = vec![place::ROOT_VALUE, read, place::PROJ_DEREF];
                    w.extend(rest);
                    b.data[x][0] = push_record(b, &w);
                }
            }
        }
    }
    hd_tir::own::insert_batch(b, &topo, &inserts).map(|_| ())
}

/// The candidate load of instruction `i`: a copy or borrow `Read` through
/// exactly one dereference, or the reference a place with a second
/// dereference reads before it. Its place is rooted at a local, with
/// fields and dereferences only up to the load. With it, the type of what
/// it reads, when it is one a group may name ([`part_ty`]).
///
/// Cost: O(the place's steps), twice.
fn candidate(b: &Body, cx: &dyn Cx, i: usize) -> Option<(Part, Place, Option<Ty>)> {
    if !matches!(b.tags[i], Tag::Read | Tag::Store | Tag::Ref)
        || b.data[i][0] & place::BARE_LOCAL != 0
    {
        return None;
    }
    let p = b.decode_place(b.data[i][0])?;
    let PlaceRoot::Local(root) = p.root else {
        return None;
    };
    let derefs: Vec<usize> = (p.proj.iter().enumerate())
        .filter(|(_, s)| **s == Proj::Deref)
        .map(|(k, _)| k)
        .take(2)
        .collect();
    let (part, end) = match derefs.as_slice() {
        [_, j] => (Part::Prefix(*j), *j),
        // A counted value's borrow would hold its place in a local
        // (invariant 23), so only an uncounted copy is a whole load.
        [_] if b.tags[i] == Tag::Read
            && b.data[i][1] == ReadMode::Copy as u32
            && !cx.tracked(b.ty[i]) =>
        {
            (Part::Whole, p.proj.len())
        }
        _ => return None,
    };
    let proj = p.proj[..end].to_vec();
    if !proj
        .iter()
        .all(|s| matches!(s, Proj::Field(_) | Proj::Deref))
    {
        return None;
    }
    let ty = match part {
        Part::Whole => Some(b.ty[i]),
        Part::Prefix(_) => part_ty(cx, b.local_ty[root.idx()], &proj),
    };
    Some((part, Place { root: p.root, proj }, ty))
}

/// Whether local `k` holds one value through the whole body: a parameter
/// the body never stores, in no cell and never addressed. A key's root
/// reached through copies is then the same reference at every load, and
/// the group's read may name it wherever the first load stands.
fn fixed(b: &Body, sets: &Sets, cell: &dyn Fn(usize) -> bool, k: usize) -> bool {
    b.local_flags[k] & local_flags::PARAM != 0
        && b.local_flags[k] & local_flags::CAPTURED_ASSIGNED == 0
        && sets.stores[k].is_empty()
        && !sets.referenced[k]
        && !cell(k)
}

/// The alias class of `key`, a place through the reference its root local
/// holds and fields: `[*$r .f ..]`. A key through a reference read from
/// memory has none: an earlier read of that reference may have seen
/// another value than a later one.
fn class_of(b: &Body, cx: &dyn Cx, defs: &Defs, key: Key) -> Option<Class> {
    if defs.derefs(key) != 1 {
        return None;
    }
    let path = defs.proj(key);
    if path.first() != Some(&Proj::Deref) {
        return None;
    }
    if cx.shared_ref(b.local_ty[defs.local(key) as usize]) {
        return Some(Class::Frozen);
    }
    Some(Class::Apart(match path.get(1) {
        Some(Proj::Field(f)) => Some(*f),
        _ => None,
    }))
}

/// The key of place `p` that instruction `at` names, when no memory read
/// on its way: then a key with a [`fixed`] root names one place for the
/// whole body.
fn key(defs: &Defs, p: &Place, at: u32) -> Option<Key> {
    defs.place(p, at)
        .filter(|d| d.mem_at == NONE)
        .map(|d| d.key)
}

/// The last instruction whose writes may reach load `x` from load `l`:
/// the one before `x`, or the end of the outermost loop around `x` that
/// `l` is outside of.
fn range_end(topo: &Topology, l: u32, x: u32) -> u32 {
    let mut hi = x - 1;
    let mut lp = topo.loop_of[x as usize];
    while lp != NONE && !(topo.first[lp as usize] <= l && l < lp) {
        hi = lp;
        lp = topo.loop_of[lp as usize];
    }
    hi
}

/// The type of the value `Store` `i` stores.
fn stored_ty(b: &Body, i: usize) -> Ty {
    let value = hd_tir::ir::Ref(b.data[i][1]);
    value.as_inst().map_or_else(
        || {
            b.consts
                .get((value.0 & !hd_tir::ir::Ref::CONST_BIT) as usize)
                .map_or(Ty::POISON, |v| v.0)
        },
        |v| b.ty[v.0 as usize],
    )
}

/// Whether instruction `i` writes raw memory only: a `Ptr` place store,
/// or a call to a memory store, `copy` or `fill` intrinsic. Std makes
/// none of them to a live shared referent's bytes
/// (`module.rt.ptr.write.shared`, `module.rt.memory.store.shared`).
fn raw_write(b: &Body, i: usize, cx: &dyn Cx) -> bool {
    match b.tags[i] {
        Tag::Call => Callee::from_words(b.record(b.data[i][0]))
            .and_then(|c| cx.intrinsic(&c))
            .is_some_and(|n| RAW_WRITES.contains(&&*n)),
        _ => false,
    }
}

/// The intrinsics that write raw heap bytes and run nothing else.
const RAW_WRITES: &[&str] = &[
    "ptr_write",
    "mem_store_u8",
    "mem_store_u16",
    "mem_store_u32",
    "mem_store_u64",
    "mem_copy",
    "mem_fill",
];

/// The type of the reference a prefix candidate reads: its local's type
/// `ty` through `steps`, fields and dereferences ([`candidate`]). A
/// dereference through a fat reference reaches a trait value, which no
/// later field, dereference or thin reference check gets past.
fn part_ty(cx: &dyn Cx, mut ty: Ty, steps: &[Proj]) -> Option<Ty> {
    for step in steps {
        ty = match step {
            Proj::Field(k) => cx.field(ty, *k)?,
            _ => cx.ref_target(ty)?.0,
        };
    }
    // The loaded reference itself, which must be a thin one.
    matches!(cx.ref_target(ty), Some((_, false))).then_some(ty)
}

/// The projection words of instruction `x`'s place after its prefix's
/// dereference at step `j`, element payloads kept.
fn suffix_words(b: &Body, x: usize, j: usize) -> Option<Vec<u32>> {
    record(b, x)?.get(2 + j + 1..).map(<[u32]>::to_vec)
}

/// Instruction `i`'s place record after its length word: root kind, root
/// local, projections.
fn record(b: &Body, i: usize) -> Option<&[u32]> {
    let at = b.data[i][0] as usize;
    let len = *b.extra.get(at)? as usize;
    let rec = b.extra.get(at + 1..at + 1 + len)?;
    (rec.first() == Some(&place::ROOT_LOCAL)).then_some(rec)
}

/// Appends place record `words` and gives its word.
fn push_record(b: &mut Body, words: &[u32]) -> u32 {
    let word = u32_of(b.extra.len());
    b.extra.push(u32_of(words.len()));
    b.extra.extend_from_slice(words);
    word
}
