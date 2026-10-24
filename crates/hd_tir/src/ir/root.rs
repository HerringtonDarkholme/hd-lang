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
//! A base read `%v = Read P`, with `P` rooted at a local or a top-level
//! binding, folds into its uses when all of these hold:
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
//!    so no other check runs before that one. When `P` is a local's own
//!    value or a field of it, with no element or dereference, any
//!    statement may stand between, a call included, if nothing between
//!    names the local and nothing can reach it: no reference to it and no
//!    closure capture (`m["k$n"].push(v)`, whose key calls a formatter
//!    between the map's read and its `MapEntry`);
//! 3. the local's type names no type parameter or associated type: an
//!    erased instance outlines an open read by its values
//!    (`crates/hd_mono/DESIGN.md` §13.5.1), and a local is none.
//!
//! When only rule 2's statements between fail, the `Store`s and `Ref`s
//! among the uses still become `P` followed by their steps, and the read
//! stays only for the reads among them: `xs[i] = f(i)` stores into
//! `[$xs [%i]]`, so the store names its place, as a change does.
//!
//! The reads run in instruction order, so a chain folds from its root:
//! `%r = Read [$0]`, `%h = Read [*%r]`, `%e = Read [%h [%i]]` and
//! `Read [%e .x]` become `Read [*$0 [%i] .x]`. A read that does not fold
//! keeps its place, which the passes resolve through its value as before.

use hd_types::Ty;

use super::{Body, IntrinsicOp, NONE, PlaceView, Root, Tag, Topology, place, u32_of};
use crate::own::{Loc, operands};

/// The tables of [`root_places`], reused across bodies.
#[derive(Default)]
pub struct RootTables {
    topo: Topology,
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

/// Whether place `p` goes into memory: an element or a dereference.
fn has_mem(b: &Body, p: PlaceView) -> bool {
    p.kinds(b) != (0, false)
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
    !b.view(b.data[s][0]).kinds(b).1
}

/// Whether local `l` keeps its value from read `v` to its last use `last`
/// whatever runs between: no instruction between but `v`'s own uses
/// (marked in `mark`) names it, and no closure captures it or reference
/// refers to it, which the checker marks as a capture.
fn unreached(b: &Body, l: u32, v: usize, last: usize, mark: &[u32]) -> bool {
    use super::local_flags::{CAPTURED, CAPTURED_ASSIGNED};
    let names = |x: usize| {
        matches!(b.tags[x], Tag::Read | Tag::Store | Tag::Ref)
            && b.view(b.data[x][0]).local() == Some(l)
    };
    b.local_flags[l as usize] & (CAPTURED | CAPTURED_ASSIGNED) == 0
        && !(v + 1..last).any(|x| mark[x] != u32_of(v) && names(x))
}

/// Folds every base read of a local's place into the places rooted at its
/// value (the module docs' rules), and deletes the reads it folds. `open`
/// says whether a type names a type parameter or an associated type.
pub fn root_places(b: &mut Body, open: &dyn Fn(Ty) -> bool, t: &mut RootTables) {
    let n = b.len();
    t.topo.build(b);
    t.gone.clear();
    t.gone.resize(n, false);
    t.mark.clear();
    t.mark.resize(n, NONE);
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
            && matches!(b.view(word).root, Root::Value(_) | Root::Const(_)))
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
        if b.tags[v] != Tag::Read
            || b.data[v][1] != super::ReadMode::Use as u32
            || t.topo.blk[v] == NONE
        {
            continue;
        }
        // A read through memory from another value (a list behind a
        // reference a call returned) folds into its stores and references
        // alone, which name their place.
        let w = b.data[v][0];
        let p = b.view(w);
        let valued = match p.root {
            Root::Local(_) | Root::Global(..) => false,
            Root::Value(_) | Root::Const(_) if has_mem(b, p) => true,
            _ => continue,
        };
        let (lo, hi) = (t.start[v] as usize, t.start[v + 1] as usize);
        // A top-level binding's type is never open.
        if lo == hi || matches!(p.root, Root::Local(l) if open(b.local_ty[l as usize])) {
            continue;
        }
        let blk = t.topo.blk[v];
        let mut last = t.topo.pos[v];
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
            let through = |b: &Body| has_mem(b, b.view(b.data[u as usize][0]));
            if !(root || entry)
                || t.topo.blk[u as usize] != blk
                || t.topo.pos[u as usize] <= t.topo.pos[v]
                || (root && b.tags[u as usize] != Tag::Read && !through(b))
            {
                ok = false;
                break;
            }
            last = last.max(t.topo.pos[u as usize]);
            t.mark[u as usize] = u32_of(v);
        }
        let mut quiet = ok && !valued;
        if ok {
            let elem = p.kinds(b).1;
            let members = b.record(b.data[blk as usize][0]);
            let between = &members[t.topo.pos[v] as usize + 1..last as usize];
            let calm = p.local().is_some_and(|l| {
                !has_mem(b, p) && unreached(b, l, v, members[last as usize] as usize, &t.mark)
            });
            for &s in between {
                let s = s as usize;
                let fine = t.gone[s]
                    || t.mark[s] == u32_of(v)
                    || calm
                    || if elem { unchecked(b, s) } else { inert(b, s) };
                if !fine {
                    quiet = false;
                    break;
                }
            }
        }
        if !ok {
            continue;
        }
        // Each use's place: `P` followed by the use's own steps. Where a
        // statement between may write, only a `Store`'s or a `Ref`'s: they
        // write or address the place as it is when they run, as a change
        // read again after its arguments does (`xs[i] = f(i)`), and the
        // read stays for the reads.
        for k in (lo..hi).filter(|&k| t.uses[k].1) {
            let u = t.uses[k].0 as usize;
            if !quiet && b.tags[u] == Tag::Read {
                kept = true;
                continue;
            }
            let q = b.view(b.data[u][0]);
            t.words.clear();
            match p.root {
                Root::Local(l) if w & place::BARE_LOCAL != 0 => {
                    t.words.extend([place::ROOT_LOCAL, l]);
                }
                _ => t
                    .words
                    .extend_from_slice(&b.extra[w as usize + 1..p.to as usize]),
            }
            t.words
                .extend_from_slice(&b.extra[q.from as usize..q.to as usize]);
            let at = u32_of(b.extra.len());
            b.extra.push(u32::try_from(t.words.len()).expect("place"));
            b.extra.extend_from_slice(&t.words);
            b.data[u][0] = at;
        }
        if !kept {
            t.gone[v] = true;
            t.edits.remove(u32_of(v));
        }
    }
    if t.gone.iter().any(|g| *g) {
        // A folded read has no use left, so the rebuild only drops it.
        crate::own::splice::apply(b, &t.topo, &mut t.edits).expect("a rebuild that only deletes");
    }
    t.edits.clear();
}
