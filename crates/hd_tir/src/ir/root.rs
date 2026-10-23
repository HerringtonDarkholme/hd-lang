//! Places rooted at the local (`docs/compiler/tir-v2.md`, "Places", and
//! stage 2 commit 12a): the checker's TIR names each place from the local it starts at, `bodies[i].x` through
//! `&mut List[Body]` as one `Read [*$0 [%5] .x]`, `xs[i] = v` as
//! `Store [$0 [%1]] v`, the payload of a matched local's value as
//! `Read [$0 as .1 .0]`. The checker builds each place one step at a time,
//! a read of the base and then a projection rooted at that read's value
//! (`%h = Read [$0]`, `Read [%h [%i]]`), since its own walkers (the
//! writable check, `mark_write_path`) step back through those values while
//! it checks. [`root_places`] then folds every such chain into one place
//! when the body is finished.
//!
//! A base read `%v = Read P`, with `P` rooted at a local, folds into its
//! uses when all of these hold:
//!
//! 1. every use of `%v` is the root of a place of a `Read`, `Store` or
//!    `Ref` (`[%v .x]`, `[*%v]`, `[%v [%i]]`), so nothing but those places
//!    needs its value: each becomes `P` followed by its own steps, and the
//!    read goes. A `MapEntry` of the map `%v` may use it too, and keeps
//!    the read: `m[k]` reads `[$m [%e]]`, at the entry `%e` that
//!    `MapEntry [%v k]` found;
//! 2. the uses are statements of `%v`'s own block, and every statement
//!    between `%v` and its last use is inert: it writes no place and calls
//!    nothing (a read, a primitive, a construction, a coercion, a length),
//!    so the local holds the same value at every use as at `%v`. When `P`
//!    has an element step, which checks its index, only other uses of `%v`
//!    and reads that check nothing (an index local's) may stand between,
//!    so no other check runs before that one;
//! 3. the local's type names no type parameter or associated type: an
//!    erased instance outlines an open read by its values
//!    (`crates/hd_mono/DESIGN.md` §13.5.1), and a local is none.
//!
//! The reads run in instruction order, so a chain folds from its root:
//! `%r = Read [$0]`, `%h = Read [*%r]`, `%e = Read [%h [%i]]` and
//! `Read [%e .x]` become `Read [*$0 [%i] .x]`. A read that does not fold
//! keeps its place, which the passes resolve through its value as before.

use hd_types::Ty;

use super::{Body, IntrinsicOp, NONE, Tag, place};
use crate::own::{Loc, operands};

/// The tables of [`root_places`], reused across bodies.
#[derive(Default)]
pub struct RootTables {
    blk: Vec<u32>,
    pos: Vec<u32>,
    /// The uses of each value, in compressed rows by value: the
    /// instruction, and whether the use is the root of its place.
    start: Vec<u32>,
    fill: Vec<u32>,
    uses: Vec<(u32, bool)>,
    /// Per instruction: a base read folded into its uses, gone.
    gone: Vec<bool>,
    /// Per instruction: a use of the read being folded.
    mark: Vec<u32>,
    ops: Vec<crate::own::Operand>,
    words: Vec<u32>,
    edits: crate::own::splice::Tables,
}

fn u32_of(i: usize) -> u32 {
    u32::try_from(i).expect("insts")
}

/// The root local of place word `word` and the `extra` range of its
/// projection words; `None` for a place rooted elsewhere.
fn local_root(b: &Body, word: u32) -> Option<(u32, u32, u32)> {
    if word & place::BARE_LOCAL != 0 {
        return Some((word & !place::BARE_LOCAL, 0, 0));
    }
    let at = word as usize;
    let n = *b.extra.get(at)?;
    match b.extra.get(at + 1..at + 3)? {
        [place::ROOT_LOCAL, l] => Some((*l, word + 3, word + 1 + n)),
        _ => None,
    }
}

/// Whether the projection words `from..to` hold a step of one of the
/// kinds `kinds` (`place::PROJ_*`).
fn has_step(b: &Body, from: u32, to: u32, kinds: &[u32]) -> bool {
    let mut x = from;
    while x < to {
        let kind = b.extra[x as usize] & place::PROJ_KIND;
        if kinds.contains(&kind) {
            return true;
        }
        x += if kind == place::PROJ_ELEM { 2 } else { 1 };
    }
    false
}

/// Whether the projection words `from..to` hold an element step.
fn has_elem(b: &Body, from: u32, to: u32) -> bool {
    has_step(b, from, to, &[place::PROJ_ELEM])
}

/// Whether the projection words `from..to` go into memory: an element or
/// a dereference.
fn has_mem(b: &Body, from: u32, to: u32) -> bool {
    has_step(b, from, to, &[place::PROJ_ELEM, place::PROJ_DEREF])
}

/// Whether statement `i` leaves every place as it was and calls nothing:
/// a read, a primitive, a coercion, a construction, a type test, a length
/// or a lookup.
fn inert(b: &Body, i: usize) -> bool {
    match b.tags[i] {
        Tag::Read
        | Tag::Prim
        | Tag::Coerce
        | Tag::NewStruct
        | Tag::NewTuple
        | Tag::NewVariant
        | Tag::NewList
        | Tag::NewMap
        | Tag::Is
        | Tag::ItemRef => true,
        Tag::Intrinsic => matches!(
            IntrinsicOp::from_u32(b.data[i][0]),
            Some(
                IntrinsicOp::ListLen
                    | IntrinsicOp::MapLen
                    | IntrinsicOp::MapGet
                    | IntrinsicOp::MapEntry
                    | IntrinsicOp::StrIndex
                    | IntrinsicOp::StrConcat
                    | IntrinsicOp::StrEq
            )
        ),
        _ => false,
    }
}

/// Whether statement `s` is inert and checks nothing that could stop the
/// program: a read of a place with no element step, such as an index
/// local's.
fn unchecked(b: &Body, s: usize) -> bool {
    if b.tags[s] != Tag::Read {
        return false;
    }
    let w = b.data[s][0];
    if w & place::BARE_LOCAL != 0 {
        return true;
    }
    let n = b.extra[w as usize];
    let first = match b.extra.get(w as usize + 1) {
        Some(&place::ROOT_GLOBAL) => w + 4,
        _ => w + 3,
    };
    !has_elem(b, first, w + 1 + n)
}

/// Folds every base read of a local's place into the places rooted at its
/// value (the module docs' rules), and deletes the reads it folds. `open`
/// says whether a type names a type parameter or an associated type.
pub fn root_places(b: &mut Body, open: &dyn Fn(Ty) -> bool, t: &mut RootTables) {
    let n = b.len();
    t.blk.clear();
    t.blk.resize(n, NONE);
    t.pos.clear();
    t.pos.resize(n, 0);
    t.gone.clear();
    t.gone.resize(n, false);
    t.mark.clear();
    t.mark.resize(n, NONE);
    for i in 0..n {
        if b.tags[i] == Tag::Block {
            for (p, &m) in b.record(b.data[i][0]).iter().enumerate() {
                if (m as usize) < n {
                    t.blk[m as usize] = u32_of(i);
                    t.pos[m as usize] = u32_of(p);
                }
            }
        }
    }
    // Every use of each value, and whether it is its place's root.
    t.start.clear();
    t.start.resize(n + 1, 0);
    let mut ops = std::mem::take(&mut t.ops);
    for i in 0..n {
        operands(b, i, &mut ops);
        for op in &ops {
            t.start[op.value as usize + 1] += 1;
        }
    }
    for v in 0..n {
        t.start[v + 1] += t.start[v];
    }
    t.uses.clear();
    t.uses.resize(t.start[n] as usize, (NONE, false));
    t.fill.clone_from(&t.start);
    for i in 0..n {
        operands(b, i, &mut ops);
        let word = b.data[i][0];
        // The place's root value sits at the record's third word.
        let root_at = (matches!(b.tags[i], Tag::Read | Tag::Store | Tag::Ref)
            && word & place::BARE_LOCAL == 0
            && b.extra.get(word as usize + 1) == Some(&place::ROOT_VALUE))
        .then(|| word + 2);
        for op in &ops {
            let v = op.value as usize;
            let root = root_at.is_some_and(|x| op.at == Loc::Extra(x));
            t.uses[t.fill[v] as usize] = (u32_of(i), root);
            t.fill[v] += 1;
        }
    }
    t.ops = ops;
    for v in 0..n {
        if b.tags[v] != Tag::Read || b.data[v][1] != super::ReadMode::Use as u32 || t.blk[v] == NONE
        {
            continue;
        }
        let Some((l, from, to)) = local_root(b, b.data[v][0]) else {
            continue;
        };
        let (lo, hi) = (t.start[v] as usize, t.start[v + 1] as usize);
        if lo == hi || open(b.local_ty[l as usize]) {
            continue;
        }
        let blk = t.blk[v];
        let mut last = t.pos[v];
        let (mut ok, mut kept) = (true, false);
        for &(u, root) in &t.uses[lo..hi] {
            let entry = !root
                && b.tags[u as usize] == Tag::Intrinsic
                && IntrinsicOp::from_u32(b.data[u as usize][0]) == Some(IntrinsicOp::MapEntry);
            kept |= entry;
            // A `Store` or a `Ref` rooted at a value writes or addresses
            // memory the value names (a referent, a buffer): one that names
            // the value's own parts would change or address a copy, which
            // the local does not hold.
            let through = |b: &Body| {
                let w = b.data[u as usize][0];
                let n = b.extra[w as usize];
                has_mem(b, w + 3, w + 1 + n)
            };
            if !(root || entry)
                || t.blk[u as usize] != blk
                || t.pos[u as usize] <= t.pos[v]
                || (root && b.tags[u as usize] != Tag::Read && !through(b))
            {
                ok = false;
                break;
            }
            last = last.max(t.pos[u as usize]);
            t.mark[u as usize] = u32_of(v);
        }
        if ok {
            let elem = has_elem(b, from, to);
            let members = b.record(b.data[blk as usize][0]);
            for &s in &members[t.pos[v] as usize + 1..last as usize] {
                let s = s as usize;
                let fine = t.gone[s]
                    || t.mark[s] == u32_of(v)
                    || if elem { unchecked(b, s) } else { inert(b, s) };
                if !fine {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            continue;
        }
        // Each use's place: `P` followed by the use's own steps.
        for k in (lo..hi).filter(|&k| t.uses[k].1) {
            let u = t.uses[k].0 as usize;
            let word = b.data[u][0];
            let len = b.extra[word as usize];
            t.words.clear();
            t.words.extend([place::ROOT_LOCAL, l]);
            t.words
                .extend_from_slice(&b.extra[from as usize..to as usize]);
            t.words
                .extend_from_slice(&b.extra[word as usize + 3..(word + 1 + len) as usize]);
            let at = u32_of(b.extra.len());
            b.extra.push(u32::try_from(t.words.len()).expect("place"));
            b.extra.extend_from_slice(&t.words);
            b.data[u][0] = at;
        }
        if !kept {
            t.gone[v] = true;
            t.edits.remove(blk, u32_of(v));
        }
    }
    if t.gone.iter().any(|g| *g) {
        // A folded read has no use left, so the rebuild only drops it.
        crate::own::splice::apply(b, &mut t.edits).expect("a rebuild that only deletes");
    }
    t.edits.clear();
}
