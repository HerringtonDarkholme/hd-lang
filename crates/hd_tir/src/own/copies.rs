//! Header copies, insertion's rule 7 (`docs/compiler/tir-v2.md`, stage 2
//! commit 13a): where a list or map value needs a header of its own, a
//! `ValueCopy` of it, so that a change made in place through one holder
//! never reaches another (`types.coll.value`). Insertion makes them from
//! the facts it already has about where a change goes; the checker places
//! none.
//!
//! A list or map value is the address of a header over a buffer
//! (`crates/hd_wasm_linear/DESIGN.md` §23.7). A change in place makes the
//! buffer the header's own first (`IsUnique`) and then changes it through
//! the header, so every holder of that header sees it; a holder of a copy,
//! a new header over the same buffer, does not. The rule keeps a header
//! that may change in place held by one place:
//!
//! - **Changing locals.** A local is changing when a change reaches its
//!   header in place: rules 5 and 6 put an `IsUnique` or a `Pin` on a
//!   place rooted at it, through no dereference, or on a value read from
//!   such a place; or a `&mut` reference takes its own storage. It is
//!   **aliased** when the change can happen while a callee runs: through
//!   such a reference, or inside a closure that captures it.
//! - **Fresh values** are ones an instruction just made, which no place
//!   holds: a literal, a construction, a call's result, a reference, a
//!   copy, a snapshot (the checker's `is_fresh`, and `SnapBegin`).
//! - **Fixed sources.** A value comes from a fixed source when it is a
//!   read of a place rooted at a local that is not changing, through no
//!   dereference, or a part of a fresh value.
//!
//! A value of a type that may hold a list or a map
//! ([`OwnCx::header_copy`]) that is not fresh gets a `ValueCopy` for:
//!
//! 1. **A store into a whole local** that is changing, or of a value whose
//!    source is not fixed (a changing local, a global, a dereference, a
//!    block's or a decision's value). A map value read for a change
//!    (`MapIndexMut`), held in a local for a `&mut self` call, is the value
//!    the change must reach, and is never copied. Into a local that is not
//!    changing, two more cases need no copy: **a header that moves**, a
//!    read of a local of the body's own at its last access, outside any
//!    loop, when that local holds a header no other place holds (no
//!    parameter, captured, referenced or shared by a binding, every value
//!    stored into it fresh or copied); and **a binding that shares
//!    safely**, stored once, captured and addressed by nothing, read only
//!    in its store's block after the store and by no call's argument (a
//!    callee may keep it in its result), when nothing that may change
//!    the source in place stands between the store and the last use of
//!    what it reads (a change of the source local, or for a source the
//!    body does not own or a closure or reference reaches, a call or any
//!    change). A read of such a binding then stands for its source
//!    (`link`). `SnapEnd`'s first operand reads nothing, so the source a
//!    `for` loop keeps for it shares the list.
//! 2. **The function's result** (a `Return`, the function's root tail),
//!    unless it is a read of a local of its own: no parameter, captured by
//!    nothing (the checker's flags), with every value stored into it
//!    fresh, copied or moved in, so no other place holds its header.
//! 3. **A store through a dereference** into a field or the referent, and
//!    **the source of a struct update** (`CopyStruct`).
//! 4. **A value argument** whose source may change while the callee runs:
//!    a global, a dereference, an aliased local, or a local a later
//!    argument changes. Not an in-place argument ([`Pass::InPlace`]), which
//!    is meant to change, nor a list or map receiver, passed as is.
//! 5. **A changing parameter**, at its sub-body's entry, but a `mut self`
//!    view of a collection ([`OwnCx::in_place_view`]): it is read, copied
//!    and stored back.
//! 6. **A local a closure captures by value** that may share its header: a
//!    parameter without an entry copy, or a local some value was stored
//!    into with no copy (a binding of a fixed or safely shared source, a
//!    loop's element);
//!    read, copied and stored back before the closure is made. One that an
//!    enclosing closure captures was copied when that closure was made.
//!
//! **Sources.** A read's source is the local its place is rooted at
//! (through the values it reads parts of), a global, a dereference, or a
//! part of a fresh value. A map's `MapIndex`, `MapGet` and `MapSlotEntry`
//! read what the map's table holds, so their source is the map's own; a
//! `MapIndexMut` is the map's value that the change must reach
//! (`Src::Handle`).
//!
//! **Placement.** A copy stands right before its use, after every other
//! edit at that position (`splice::Tables::insert_late`), as the checker's
//! did. The value's own use ends at the copy, so its `Drop` stands between
//! the copy and the use. The use consumes the copy, or lends it and a
//! `Drop` follows the use's statement. An argument's copy stands right
//! after the argument, so a later argument's change cannot reach it, and a
//! jump between the copy and the call drops it. The copies of 5 and 6 are
//! `Read copy`, `ValueCopy`, `Drop`, `Store` back.
//!
//! **Not here yet.** The emitter's store glue copies a value stored into a
//! part of a place (an element, a field, a payload, a construction's
//! member, `slot_src`, §23.17), so those stores need no rule
//! here until commit 14 moves them. The changing and aliased facts hold for
//! the whole body: a local that a `&mut` reference or a capturing closure
//! reaches anywhere is aliased at every argument read, as the checker's
//! former copies took it; commit 13b narrows that if parity needs it.
//!
//! Why it is sound: a changing local holds a header no other place holds,
//! since every value stored into it is fresh or copied (1) and so is a
//! changing parameter's (5); a read of it that another place keeps is
//! copied (1, 2, 4, 6) or the emitter's glue copies it, but for a result
//! that is its own local, which ends with the call, an argument that
//! nothing can change while the callee runs, and a binding read only
//! before anything can change it. A header that moves out of a dying local
//! that held it alone has one holder again. A fixed source's header may
//! be shared, but only with places no change reaches in place; a place
//! that keeps it beyond the body (a result, a capture) copies it (2, 6).
//!
//! Data layout: per instruction, whether it is fresh and where its value
//! comes from, filled in one walk in instruction order (operands come
//! first); per local, whether it is changing, aliased or shares; the
//! change sites as sorted `(local, instruction)` rows and the captures as
//! sorted `(sub-body, local)` rows, searched by bisection. The decisions
//! are marks on insertion's use table, which rule 2 reads, and the
//! read-copy-store edits of 5 and 6. No hash maps.

use hd_base::NodeIdx;
use hd_types::Ty;

use super::insert;
use super::{HeaderCopy, Loc, OwnCx, Pass, Role, splice};
use crate::ir::{
    Body, BodyKind, Callee, CaptureMode, IntrinsicOp, NONE, ReadMode, Ref, Tag, local_flags, place,
    ref_word,
};

/// Where a value's header comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Src {
    /// Made by an instruction: no place holds it.
    Fresh,
    /// A part of a fresh value, which no place holds either.
    Part,
    /// A read of a place rooted at local `l`, through no dereference, by
    /// read `at` (the first read of the chain the value comes from).
    Local { l: u32, at: u32 },
    /// A read of a global.
    Global,
    /// A read through a dereference: a place this body does not own.
    Deref,
    /// A map value read for a change (`MapIndexMut`), which the change
    /// must reach.
    Handle,
    /// Any other value: a block's or a decision's, a suspension's.
    Other,
}

/// The tables of the header copies, reused across bodies.
#[derive(Default)]
pub(crate) struct Tables {
    /// Per instruction: where its value comes from.
    pub(crate) src: Vec<Src>,
    /// Per local: changing, and aliased.
    changing: Vec<bool>,
    aliased: Vec<bool>,
    /// Per local: whether every value stored into it is fresh, a copy, or
    /// a header that left a dying local (1): it holds a header no other
    /// place holds.
    owns: Vec<bool>,
    /// Per local: the source a binding shares safely (1), which a read of
    /// it stands for; `Src::Fresh` for none.
    link: Vec<Src>,
    /// Per local: some binding shares its header (1).
    linked: Vec<bool>,
    /// Per local: whether a `Ref` takes its storage or a place in its
    /// list, and how many stores write it.
    addressed: Vec<bool>,
    stores: Vec<u32>,
    /// The local accesses, `(local, instruction)`, sorted: reads, stores,
    /// references.
    acc: Vec<(u32, u32)>,
    /// Per instruction: the first instruction of the statement span it
    /// ends (a statement's blocks precede it).
    first: Vec<u32>,
    /// Instructions before each index that may change a header a body
    /// does not own: calls and changes (prefix counts, `n + 1` rows).
    writes: Vec<u32>,
    /// A scratch stack of values.
    stack: Vec<u32>,
    /// Per local: a parameter that gets an entry copy.
    entry_copied: Vec<bool>,
    /// Per local: a parameter of a closure (sub-body params).
    sub_param: Vec<bool>,
    /// The change sites, `(local, instruction)`, sorted.
    sites: Vec<(u32, u32)>,
    /// The captures, `(sub-body, local)`, sorted.
    caps: Vec<(u32, u32)>,
}

impl Tables {
    fn reset(&mut self, n: usize, locals: usize) {
        fill(&mut self.src, n, Src::Other);
        fill(&mut self.changing, locals, false);
        fill(&mut self.aliased, locals, false);
        fill(&mut self.owns, locals, true);
        fill(&mut self.link, locals, Src::Fresh);
        fill(&mut self.linked, locals, false);
        fill(&mut self.addressed, locals, false);
        fill(&mut self.stores, locals, 0);
        fill(&mut self.first, n, 0);
        fill(&mut self.writes, n + 1, 0);
        self.acc.clear();
        fill(&mut self.entry_copied, locals, false);
        fill(&mut self.sub_param, locals, false);
        self.sites.clear();
        self.caps.clear();
    }
}

fn fill<T: Clone>(v: &mut Vec<T>, n: usize, x: T) {
    v.clear();
    v.resize(n, x);
}

fn u32_of(i: usize) -> u32 {
    u32::try_from(i).expect("insts")
}

/// Whether instruction `i`'s value is fresh: one it just made, which no
/// place holds.
pub(crate) fn is_fresh(b: &Body, i: usize) -> bool {
    match b.tags[i] {
        Tag::NewList
        | Tag::NewMap
        | Tag::NewStruct
        | Tag::CopyStruct
        | Tag::NewVariant
        | Tag::NewTuple
        | Tag::Closure
        | Tag::Interp
        | Tag::Prim
        | Tag::Call
        | Tag::CallDyn
        | Tag::DefaultCall
        | Tag::CallValue
        | Tag::Ref
        | Tag::RefDyn
        | Tag::RefIdentity => true,
        Tag::Intrinsic => matches!(
            IntrinsicOp::from_u32(b.data[i][0]),
            Some(IntrinsicOp::ValueCopy | IntrinsicOp::Item | IntrinsicOp::SnapBegin)
        ),
        _ => false,
    }
}

/// Fills `src` for every instruction of `b`, in order.
pub(crate) fn sources(b: &Body, src: &mut Vec<Src>) {
    let n = b.len();
    fill(src, n, Src::Other);
    let of = |src: &[Src], v: u32| match Ref(v).as_inst() {
        Some(x) if (x.0 as usize) < n => src[x.0 as usize],
        _ => Src::Fresh,
    };
    for i in 0..n {
        let s = match b.tags[i] {
            _ if is_fresh(b, i) => Src::Fresh,
            Tag::Coerce => of(src, b.data[i][0]),
            Tag::Intrinsic => match IntrinsicOp::from_u32(b.data[i][0]) {
                Some(IntrinsicOp::MapIndexMut) => Src::Handle,
                // What the map's table holds: a value, or a slot's entry.
                Some(IntrinsicOp::MapIndex | IntrinsicOp::MapGet | IntrinsicOp::MapSlotEntry) => {
                    match b.record(b.data[i][1]).first() {
                        Some(&m) => of(src, m),
                        None => Src::Other,
                    }
                }
                _ => Src::Other,
            },
            Tag::Read => read_src(b, i, src),
            _ => Src::Other,
        };
        src[i] = s;
    }
}

/// Where read `i` reads from.
fn read_src(b: &Body, i: usize, src: &[Src]) -> Src {
    let word = b.data[i][0];
    if word & place::BARE_LOCAL != 0 {
        return Src::Local {
            l: word & !place::BARE_LOCAL,
            at: u32_of(i),
        };
    }
    let Some(&len) = b.extra.get(word as usize) else {
        return Src::Other;
    };
    let rec = word as usize + 1;
    let end = rec + len as usize;
    let (root, mut x) = match b.extra.get(rec..rec + 2) {
        Some(&[place::ROOT_LOCAL, l]) => (Src::Local { l, at: u32_of(i) }, rec + 2),
        Some(&[place::ROOT_GLOBAL, _]) => (Src::Global, rec + 3),
        Some(&[place::ROOT_VALUE, v]) => {
            let s = match Ref(v).as_inst() {
                Some(x) if (x.0 as usize) < i => src[x.0 as usize],
                _ => Src::Fresh,
            };
            let s = match s {
                Src::Fresh | Src::Part => Src::Part,
                s => s,
            };
            (s, rec + 2)
        }
        _ => return Src::Other,
    };
    while x < end {
        match b.extra[x] & place::PROJ_KIND {
            place::PROJ_DEREF => return Src::Deref,
            place::PROJ_ELEM => x += 2,
            _ => x += 1,
        }
    }
    root
}

/// The local whose header a change through value `v` reaches: `v` read,
/// through coercions and reads of values, from a place rooted at a local
/// through no dereference.
pub(crate) fn changed_local(b: &Body, mut v: u32) -> Option<u32> {
    for _ in 0..64 {
        let i = Ref(v).as_inst()?.0 as usize;
        match b.tags.get(i)? {
            Tag::Coerce => v = b.data[i][0],
            Tag::Read => {
                let word = b.data[i][0];
                if word & place::BARE_LOCAL != 0 {
                    return Some(word & !place::BARE_LOCAL);
                }
                let (root, steps) = place_root(b, word)?;
                if steps
                    .iter()
                    .any(|&w| w & place::PROJ_KIND == place::PROJ_DEREF)
                {
                    return None;
                }
                match root {
                    (place::ROOT_LOCAL, l) => return Some(l),
                    (place::ROOT_VALUE, w) => v = w,
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
    None
}

/// A place record's root kind and word, and its projection words.
pub(crate) fn place_root(b: &Body, word: u32) -> Option<((u32, u32), &[u32])> {
    let len = *b.extra.get(word as usize)? as usize;
    let rec = b.extra.get(word as usize + 1..word as usize + 1 + len)?;
    match rec {
        [place::ROOT_GLOBAL, d, _, rest @ ..] => Some(((place::ROOT_GLOBAL, *d), rest)),
        [k, w, rest @ ..] => Some(((*k, *w), rest)),
        _ => None,
    }
}

/// Whether a change through the place at `word` (a `Store` or a `Ref`)
/// reaches a list on its path in place, and the local or value it is
/// rooted at: an element step before any dereference.
fn place_change(b: &Body, word: u32) -> Option<(u32, u32)> {
    if word & place::BARE_LOCAL != 0 {
        return None;
    }
    let (root, steps) = place_root(b, word)?;
    let mut x = 0;
    while x < steps.len() {
        match steps[x] & place::PROJ_KIND {
            place::PROJ_DEREF => return None,
            place::PROJ_ELEM => return Some(root),
            _ => x += 1,
        }
    }
    None
}

/// Whether the place at `word` goes through a dereference.
pub(crate) fn through_deref(b: &Body, word: u32) -> bool {
    if word & place::BARE_LOCAL != 0 {
        return false;
    }
    let Some((_, steps)) = place_root(b, word) else {
        return false;
    };
    let mut x = 0;
    while x < steps.len() {
        match steps[x] & place::PROJ_KIND {
            place::PROJ_DEREF => return true,
            place::PROJ_ELEM => x += 2,
            _ => x += 1,
        }
    }
    false
}

/// Whether the place at `word` goes through a dereference and then only
/// struct fields: the referent or a field of it (`*r = v`, `r.x = v`).
fn into_referent(b: &Body, word: u32) -> bool {
    if word & place::BARE_LOCAL != 0 {
        return false;
    }
    let Some((_, steps)) = place_root(b, word) else {
        return false;
    };
    let mut deref = false;
    let mut x = 0;
    while x < steps.len() {
        match steps[x] & place::PROJ_KIND {
            place::PROJ_DEREF => deref = true,
            place::PROJ_FIELD => {}
            place::PROJ_ELEM => {
                deref = false;
                x += 1;
            }
            _ => deref = false,
        }
        x += 1;
    }
    deref
}

/// The changing locals: the roots of every change rules 5 and 6 make
/// unique (an element step on a `Store` or `Ref` place, a collection
/// operand of a change, an in-place argument), and the locals a `&mut`
/// reference takes. Each change site is a row of `sites`.
fn changes(b: &Body, cx: &dyn OwnCx, t: &mut Tables, sub: &[u32]) {
    let mark = |t: &mut Tables, l: u32, i: usize| {
        if let Some(c) = t.changing.get_mut(l as usize) {
            *c = true;
            t.sites.push((l, u32_of(i)));
        }
    };
    for i in 0..b.len() {
        let [a, w] = b.data[i];
        if may_change(b, i) {
            t.writes[i + 1] = 1;
        }
        match b.tags[i] {
            Tag::Store | Tag::Ref => {
                // A `&mut` reference to the local's storage, or into its
                // list's buffer, writes it while a callee may run.
                if b.tags[i] == Tag::Ref && w & ref_word::MUT != 0 && !through_deref(b, a) {
                    let l = if a & place::BARE_LOCAL != 0 {
                        Some(a & !place::BARE_LOCAL)
                    } else {
                        match place_root(b, a) {
                            Some(((place::ROOT_LOCAL, l), _)) => Some(l),
                            Some(((place::ROOT_VALUE, v), _)) => changed_local(b, v),
                            _ => None,
                        }
                    };
                    if let Some(l) = l {
                        mark(t, l, i);
                        t.aliased[l as usize] = true;
                    }
                }
                if let Some((kind, root)) = place_change(b, a) {
                    match kind {
                        place::ROOT_LOCAL => mark(t, root, i),
                        place::ROOT_VALUE => {
                            if let Some(l) = changed_local(b, root) {
                                mark(t, l, i);
                            }
                        }
                        _ => {}
                    }
                } else if !through_deref(b, a)
                    && let Some(((place::ROOT_VALUE, v), _)) = (a & place::BARE_LOCAL == 0)
                        .then(|| place_root(b, a))
                        .flatten()
                    && let Some(l) = changed_local(b, v)
                {
                    mark(t, l, i);
                }
            }
            Tag::Intrinsic
                if matches!(
                    IntrinsicOp::from_u32(a),
                    Some(
                        IntrinsicOp::ListPush
                            | IntrinsicOp::MapSet
                            | IntrinsicOp::MapRemove
                            | IntrinsicOp::MapIndexMut
                    )
                ) =>
            {
                if let Some(&coll) = b.record(w).first()
                    && let Some(l) = changed_local(b, coll)
                {
                    mark(t, l, i);
                }
            }
            Tag::Call | Tag::Await => {
                let Some(callee) = Callee::from_words(b.record(a)) else {
                    continue;
                };
                let n = b.record(w).len().saturating_sub(3);
                for k in 0..n {
                    let arg = b.record(w)[k];
                    if cx.pass(&callee, k, n) == Pass::InPlace
                        && let Some(l) = changed_local(b, arg)
                    {
                        mark(t, l, i);
                    }
                }
            }
            _ => {}
        }
    }
    for i in 0..b.len() {
        t.writes[i + 1] += t.writes[i];
    }
    t.sites.sort_unstable();
    // A change inside a closure that captures the local can run while a
    // callee runs.
    for k in 0..t.sites.len() {
        let (l, i) = t.sites[k];
        let s = sub[i as usize];
        if s != 0 && t.caps.binary_search(&(s, l)).is_ok() {
            t.aliased[l as usize] = true;
        }
    }
}

/// Whether instruction `i` may change a header in place that the body
/// does not own, or what it holds: a call, a change of a collection (a
/// push, a map store or removal, a write through a map value), a `Store`
/// through an element or a dereference (an element reference writes a
/// buffer), a `Ref` of an element, a `&mut` reference.
fn may_change(b: &Body, i: usize) -> bool {
    let [a, w] = b.data[i];
    match b.tags[i] {
        Tag::Call
        | Tag::CallValue
        | Tag::CallDyn
        | Tag::DefaultCall
        | Tag::Await
        | Tag::AwaitValue
        | Tag::AwaitAll
        | Tag::AwaitRace => true,
        Tag::Intrinsic => matches!(
            IntrinsicOp::from_u32(a),
            Some(
                IntrinsicOp::ListPush
                    | IntrinsicOp::MapSet
                    | IntrinsicOp::MapRemove
                    | IntrinsicOp::MapIndexMut
            )
        ),
        Tag::Ref if w & ref_word::MUT != 0 => true,
        Tag::Store | Tag::Ref => {
            a & place::BARE_LOCAL == 0
                && place_root(b, a).is_some_and(|(_, steps)| {
                    let mut x = 0;
                    while x < steps.len() {
                        match steps[x] & place::PROJ_KIND {
                            place::PROJ_ELEM => return true,
                            place::PROJ_DEREF if b.tags[i] == Tag::Store => return true,
                            _ => x += 1,
                        }
                    }
                    false
                })
        }
        _ => false,
    }
}

/// Whether local `l` has a change site strictly between instructions
/// `from` and `to`.
fn changed_between(t: &Tables, l: u32, from: u32, to: u32) -> bool {
    let k = t.sites.partition_point(|&s| s <= (l, from));
    t.sites.get(k).is_some_and(|&(x, i)| x == l && i < to)
}

/// Rule 7: decides every header copy of `b`. The copies at a use are
/// marks on `ins.hcopy` (the copy's type, or `Ty::VOID`), parallel to
/// insertion's use table; the entry and capture copies are edits.
pub(crate) fn decide(
    b: &mut Body,
    cx: &dyn OwnCx,
    ins: &mut insert::Tables,
    t: &mut Tables,
    edits: &mut splice::Tables,
) {
    let n = b.len();
    t.reset(n, b.local_ty.len());
    ins.hcopy.clear();
    ins.hcopy.resize(ins.uses.len(), Ty::VOID);
    for i in 0..n {
        if b.tags[i] == Tag::Closure && !b.is_instantiation(i) {
            let rec = b.record(b.data[i][1]);
            let (start, len) = (rec[0] as usize, rec[1] as usize);
            for c in start..start + len {
                t.caps.push((b.data[i][0], b.cap_local[c].raw()));
            }
        }
    }
    t.caps.sort_unstable();
    t.caps.dedup();
    for s in 1..b.sub_params.len() {
        let r = b.sub_params[s];
        for k in 0..r.len {
            let l = b.extra[(r.start + 1 + k) as usize];
            if let Some(p) = t.sub_param.get_mut(l as usize) {
                *p = true;
            }
        }
    }
    changes(b, cx, t, &ins.sub);
    sources(b, &mut t.src);
    let copy_of = |b: &Body, v: u32| -> Option<(usize, HeaderCopy)> {
        let x = Ref(v).as_inst()?.0 as usize;
        if x >= n || is_fresh(b, x) {
            return None;
        }
        cx.header_copy(b.ty[x]).map(|h| (x, h))
    };
    accesses(b, t);
    // 1: stores into whole locals, in instruction order: a binding's link
    // stands for it in the reads after its store.
    for i in 0..n {
        if b.tags[i] != Tag::Store {
            continue;
        }
        let [a, w] = b.data[i];
        if let Some((l, true)) = b.stored_local(i).map(|(l, whole)| (l.raw(), whole))
            && let Some((v, h)) = copy_of(b, w)
        {
            let src = resolved(t, t.src[v]);
            let lu = l as usize;
            if src == Src::Handle {
                t.owns[lu] = false;
            } else if t.changing[lu] {
                mark(ins, v, i, Loc::Data(1), h.ty);
            } else if fixed(t, src) {
                t.owns[lu] = false;
            } else if moves_out(b, ins, t, v) {
                // The header leaves a dying local that held it alone.
            } else if shares_safely(b, ins, t, i, l, src) {
                t.owns[lu] = false;
                t.link[lu] = src;
                if let Src::Local { l: s, .. } = src {
                    t.linked[s as usize] = true;
                }
            } else {
                mark(ins, v, i, Loc::Data(1), h.ty);
            }
        } else if into_referent(b, a)
            && let Some((v, h)) = copy_of(b, w)
        {
            // 3: through a dereference.
            mark(ins, v, i, Loc::Data(1), h.ty);
        }
    }
    // 5: entry copies of changing parameters.
    let mut params = Vec::new();
    for s in 0..b.sub_root.len() {
        params.clear();
        if s == 0 {
            params.extend(
                (0..b.local_flags.len())
                    .filter(|&l| b.local_flags[l] & local_flags::PARAM != 0 && !t.sub_param[l])
                    .map(u32_of),
            );
        } else {
            let r = b.sub_params[s];
            params.extend((0..r.len).map(|k| b.extra[(r.start + 1 + k) as usize]));
        }
        let root = b.sub_root[s];
        if root as usize >= n {
            continue;
        }
        for &l in &params {
            let ty = b.local_ty[l as usize];
            if !t.changing[l as usize] || cx.in_place_view(ty) {
                continue;
            }
            let Some(h) = cx.header_copy(ty) else {
                continue;
            };
            t.entry_copied[l as usize] = true;
            let syn = b.local_syn[l as usize];
            copy_back(b, edits, root, 0, l, ty, h.ty, syn);
        }
    }
    for i in 0..n {
        match b.tags[i] {
            // 2: results.
            Tag::Return => result(b, cx, ins, t, i, Loc::Data(0), b.data[i][0]),
            // 3: a struct update's source.
            Tag::CopyStruct => {
                if let Some((v, h)) = copy_of(b, b.data[i][0]) {
                    mark(ins, v, i, Loc::Data(0), h.ty);
                }
            }
            // 4: value arguments.
            Tag::Call | Tag::Await | Tag::CallValue => arguments(b, cx, ins, t, i),
            // 6: captures.
            Tag::Closure if !b.is_instantiation(i) && ins.blk[i] != NONE => {
                let rec = b.record(b.data[i][1]);
                let (start, len) = (rec[0] as usize, rec[1] as usize);
                let outer = ins.sub[i];
                for c in start..start + len {
                    let l = b.cap_local[c].raw();
                    if b.cap_mode.get(c) == Some(&CaptureMode::Shared)
                        || (outer != 0 && t.caps.binary_search(&(outer, l)).is_ok())
                    {
                        continue;
                    }
                    let param = b.local_flags[l as usize] & local_flags::PARAM != 0
                        || t.sub_param[l as usize];
                    let shares = (param && !t.entry_copied[l as usize]) || !t.owns[l as usize];
                    let ty = b.local_ty[l as usize];
                    if !shares {
                        continue;
                    }
                    let Some(h) = cx.header_copy(ty) else {
                        continue;
                    };
                    let (blk, pos, syn) = (ins.blk[i], ins.pos[i], b.syn[i]);
                    copy_back(b, edits, blk, pos, l, ty, h.ty, syn);
                }
            }
            _ => {}
        }
    }
    if b.kind == BodyKind::Fn
        && let Some(&root) = b.sub_root.first()
        && (root as usize) < n
    {
        result(
            b,
            cx,
            ins,
            t,
            root as usize,
            Loc::Data(1),
            b.data[root as usize][1],
        );
    }
}

/// `src` with a binding's link standing for a read of it: a read of a
/// binding that shares its source's header is a read of that source.
fn resolved(t: &Tables, src: Src) -> Src {
    match src {
        Src::Local { l, at } => match t.link[l as usize] {
            Src::Fresh => src,
            Src::Local { l: s, .. } => Src::Local { l: s, at },
            linked => linked,
        },
        _ => src,
    }
}

/// Whether `src` is fixed: a part of a fresh value, or a read of a local
/// no change reaches.
fn fixed(t: &Tables, src: Src) -> bool {
    match src {
        Src::Part => true,
        Src::Local { l, .. } => !t.changing[l as usize],
        _ => false,
    }
}

/// The local accesses (`acc`), the addressed locals, each local's store
/// count, and each instruction's statement span (`first`).
fn accesses(b: &Body, t: &mut Tables) {
    for i in 0..b.len() {
        let mut f = u32_of(i);
        if b.tags[i] == Tag::Block {
            for &m in b.record(b.data[i][0]) {
                if (m as usize) < i {
                    f = f.min(t.first[m as usize]);
                }
            }
        }
        let first = &t.first;
        splice::for_blocks(b, i, |blk| {
            if (blk as usize) < i {
                f = f.min(first[blk as usize]);
            }
        });
        t.first[i] = f;
        let a = b.data[i][0];
        if !matches!(b.tags[i], Tag::Read | Tag::Store | Tag::Ref) {
            continue;
        }
        // A reference rooted at a value read from a local's place is one
        // into that local too.
        if b.tags[i] == Tag::Ref
            && a & place::BARE_LOCAL == 0
            && let Some(((place::ROOT_VALUE, v), _)) = place_root(b, a)
            && let Some(l) = changed_local(b, v)
            && let Some(x) = t.addressed.get_mut(l as usize)
        {
            *x = true;
        }
        let root = if a & place::BARE_LOCAL != 0 {
            Some(a & !place::BARE_LOCAL)
        } else {
            match place_root(b, a) {
                Some(((place::ROOT_LOCAL, l), _)) => Some(l),
                _ => None,
            }
        };
        let Some(l) = root.filter(|&l| (l as usize) < t.stores.len()) else {
            continue;
        };
        t.acc.push((l, u32_of(i)));
        // A reference into the local's list addresses its buffer too.
        match b.tags[i] {
            Tag::Store if b.local_in_place(a).is_some() => t.stores[l as usize] += 1,
            Tag::Ref => t.addressed[l as usize] = true,
            _ => {}
        }
    }
    t.acc.sort_unstable();
}

/// The accesses of local `l`, from `acc`.
fn accesses_of(t: &Tables, l: u32) -> &[(u32, u32)] {
    let lo = t.acc.partition_point(|&(x, _)| x < l);
    let hi = t.acc.partition_point(|&(x, _)| x <= l);
    &t.acc[lo..hi]
}

/// Whether the blocks around instruction `i`, up to its sub-body's root,
/// hold it in a loop (`looped`) or in a cleanup suite, which runs at its
/// scope's exits.
fn around(b: &Body, ins: &insert::Tables, i: usize, looped: bool) -> bool {
    let mut blk = ins.blk[i];
    while blk != NONE && !ins.is_root[blk as usize] {
        let o = ins.owner[blk as usize];
        if o == NONE {
            return false;
        }
        match b.tags[o as usize] {
            Tag::Loop if looped => return true,
            Tag::Scope if b.data[o as usize][0] != blk => return true,
            _ => {}
        }
        blk = ins.blk[o as usize];
    }
    false
}

/// The first rule 1 refinement, a header that moves: the value stored is a
/// read of a whole local of the body's own, at its last access, that holds
/// a header no other place holds (no parameter, captured, addressed or
/// shared by a binding, every stored value fresh or copied). The local
/// ends there, so the store takes its header as the only holder. The read
/// stands in no loop or cleanup suite, and every other access of the local
/// comes before it.
fn moves_out(b: &Body, ins: &mut insert::Tables, t: &Tables, v: usize) -> bool {
    let mut y = v;
    while b.tags[y] == Tag::Coerce {
        match Ref(b.data[y][0]).as_inst() {
            Some(z) if (z.0 as usize) < y => y = z.0 as usize,
            _ => return false,
        }
    }
    let Some(s) = b.local_read(y).map(hd_base::LocalId::raw) else {
        return false;
    };
    let su = s as usize;
    let flags = b.local_flags[su];
    if flags & (local_flags::PARAM | local_flags::CAPTURED) != 0
        || t.sub_param[su]
        || t.addressed[su]
        || t.linked[su]
        || !t.owns[su]
        || ins.blk[y] == NONE
        || around(b, ins, y, true)
    {
        return false;
    }
    ins.mark_chain(ins.blk[y], ins.pos[y]);
    let mut last = true;
    for &(_, a) in accesses_of(t, s) {
        let a = a as usize;
        if a == y {
            continue;
        }
        let before = ins.blk[a] != NONE
            && !around(b, ins, a, false)
            && ins
                .meet(ins.blk[a], ins.pos[a])
                .is_some_and(|(level, p)| p < ins.chain[level as usize].1);
        if !before {
            last = false;
            break;
        }
    }
    ins.unmark_chain();
    last
}

/// The second rule 1 refinement, a binding that shares safely: local `l`,
/// stored once by store `i`, captured and addressed by nothing, is read
/// only in the statements of the store's block after it, by uses that are
/// no call's argument and no block's value, and nothing that may change
/// the source's header in place stands between the store and the last use
/// of what it reads: for a local no closure or reference reaches, a change
/// of it; for any other source, a call or any change.
/// `SnapEnd`'s first operand reads nothing (the backend ignores it), so
/// the loop source a `for` keeps for it shares the list.
fn shares_safely(
    b: &Body,
    ins: &mut insert::Tables,
    t: &mut Tables,
    i: usize,
    l: u32,
    src: Src,
) -> bool {
    let lu = l as usize;
    let flags = b.local_flags[lu];
    if t.stores[lu] != 1
        || t.addressed[lu]
        || flags & (local_flags::PARAM | local_flags::CAPTURED) != 0
        || t.sub_param[lu]
        || ins.blk[i] == NONE
    {
        return false;
    }
    let (blk, ps) = (ins.blk[i], ins.pos[i]);
    ins.mark_chain(blk, ps);
    let mut end = Some(ps);
    let mut stack = std::mem::take(&mut t.stack);
    for &(_, a) in accesses_of(t, l) {
        let a = a as usize;
        if a == i {
            continue;
        }
        end = end.and_then(|e| read_end(b, ins, &mut stack, a, e, ps));
        if end.is_none() {
            break;
        }
    }
    t.stack = stack;
    ins.unmark_chain();
    let Some(pe) = end else {
        return false;
    };
    if pe <= ps {
        return true;
    }
    let members = b.record(b.data[blk as usize][0]);
    let lo = t.first[members[ps as usize + 1] as usize];
    let hi = members[pe as usize] + 1;
    match src {
        Src::Local { l: s, .. } if !t.aliased[s as usize] => {
            let k = t.sites.partition_point(|&x| x < (s, lo));
            !t.sites.get(k).is_some_and(|&(x, at)| x == s && at < hi)
        }
        _ => t.writes[hi as usize] == t.writes[lo as usize],
    }
}

/// The last statement position, in the marked block, at which read `a`'s
/// value is used, given `end` so far: `None` when the read or a use stands
/// outside the block or before the store at `ps`, or the value flows on
/// as a block's value.
fn read_end(
    b: &Body,
    ins: &insert::Tables,
    stack: &mut Vec<u32>,
    a: usize,
    end: u32,
    ps: u32,
) -> Option<u32> {
    if b.tags[a] != Tag::Read || ins.blk[a] == NONE {
        return None;
    }
    let at = |x: usize| -> Option<u32> {
        let (level, p) = ins.meet(ins.blk[x], ins.pos[x])?;
        (level == 0 && p > ps).then_some(p)
    };
    let snap_end = |u: usize, op: &super::Operand| {
        b.tags[u] == Tag::Intrinsic
            && IntrinsicOp::from_u32(b.data[u][0]) == Some(IntrinsicOp::SnapEnd)
            && op.at == Loc::Extra(b.data[u][1] + 1)
    };
    // A read only `SnapEnd` names reads nothing.
    let uses = &ins.uses[ins.start[a] as usize..ins.start[a + 1] as usize];
    if !uses.is_empty() && uses.iter().all(|(u, op)| snap_end(*u as usize, op)) {
        return Some(end);
    }
    let mut end = end.max(at(a)?);
    // The value's uses, through coercions and reads of its parts.
    stack.clear();
    stack.push(u32_of(a));
    while let Some(v) = stack.pop() {
        let v = v as usize;
        for k in ins.start[v] as usize..ins.start[v + 1] as usize {
            let (u, op) = ins.uses[k];
            let u = u as usize;
            // A block's value flows on, and a callee may keep an argument
            // past the call (a reference to its parameter in its result).
            if matches!(op.role, Role::Tail | Role::Arg(_)) {
                return None;
            }
            if snap_end(u, &op) {
                continue;
            }
            end = end.max(at(u)?);
            // A coercion passes the value on, and a read rooted at it reads
            // a part of it.
            if matches!(b.tags[u], Tag::Coerce | Tag::Read) {
                stack.push(u32_of(u));
            }
        }
    }
    Some(end)
}

/// Marks the use of value `v` by instruction `u` at `at` for a copy of
/// type `ty`.
fn mark(ins: &mut insert::Tables, v: usize, u: usize, at: Loc, ty: Ty) {
    let (lo, hi) = (ins.start[v] as usize, ins.start[v + 1] as usize);
    for k in lo..hi {
        let (x, op) = ins.uses[k];
        if x as usize == u && op.at == at {
            ins.hcopy[k] = ty;
            return;
        }
    }
}

/// 2: a function's result `v`, used by `u` at `at`.
fn result(
    b: &Body,
    cx: &dyn OwnCx,
    ins: &mut insert::Tables,
    t: &Tables,
    u: usize,
    at: Loc,
    v: u32,
) {
    let Some(x) = Ref(v).as_inst().map(|x| x.0 as usize) else {
        return;
    };
    if x >= b.len() || is_fresh(b, x) {
        return;
    }
    let Some(h) = cx.header_copy(b.ty[x]) else {
        return;
    };
    // A read of a local of its own, through coercions.
    let mut y = x;
    while b.tags[y] == Tag::Coerce {
        match Ref(b.data[y][0]).as_inst() {
            Some(z) if (z.0 as usize) < y => y = z.0 as usize,
            _ => break,
        }
    }
    if let Some(l) = b.local_read(y) {
        let own = b.local_flags[l.idx()] & (local_flags::PARAM | local_flags::CAPTURED) == 0
            && !t.sub_param[l.idx()]
            && t.owns[l.idx()];
        if own {
            return;
        }
    }
    mark(ins, x, u, at, h.ty);
}

/// 4: the value arguments of call `i` whose source may change while the
/// callee runs.
fn arguments(b: &Body, cx: &dyn OwnCx, ins: &mut insert::Tables, t: &Tables, i: usize) {
    let [a, w] = b.data[i];
    let (args, callee, fn_ty) = if b.tags[i] == Tag::CallValue {
        (
            b.record(w),
            None,
            Ref(a).as_inst().map(|x| b.ty[x.0 as usize]),
        )
    } else {
        let r = b.record(w);
        (
            &r[..r.len().saturating_sub(3)],
            Callee::from_words(b.record(a)),
            None,
        )
    };
    let n = args.len();
    for (k, &v) in args.iter().enumerate() {
        let Some(x) = Ref(v).as_inst().map(|x| x.0 as usize) else {
            continue;
        };
        if x >= b.len() || is_fresh(b, x) {
            continue;
        }
        let Some(h) = cx.header_copy(b.ty[x]) else {
            continue;
        };
        let pass = match (&callee, fn_ty) {
            (Some(c), _) => cx.pass(c, k, n),
            (None, Some(f)) => cx.pass_value(f, k, n),
            _ => continue,
        };
        if pass == Pass::InPlace
            || (h.coll && callee.as_ref().is_some_and(|c| cx.receiver(c, k, n)))
        {
            continue;
        }
        let copy = match resolved(t, t.src[x]) {
            Src::Global | Src::Deref => true,
            Src::Local { l, at } => t.aliased[l as usize] || changed_between(t, l, at, u32_of(i)),
            _ => false,
        };
        if copy {
            let at = Loc::Extra(arg_word(b, i, k));
            mark(ins, x, i, at, h.ty);
        }
    }
}

/// The `extra` position of argument `k` of call `i`.
fn arg_word(b: &Body, i: usize, k: usize) -> u32 {
    b.data[i][1] + 1 + u32_of(k)
}

/// 5 and 6: `$l` read, copied and stored back, before position `pos` of
/// block `blk`: `%r = Read copy [$l]`, `%c = ValueCopy %r`, `Drop [%r]`,
/// `Store [$l] %c`, as insertion makes of the checker's copy today.
#[allow(clippy::too_many_arguments)]
fn copy_back(
    b: &mut Body,
    edits: &mut splice::Tables,
    blk: u32,
    pos: u32,
    l: u32,
    ty: Ty,
    copy_ty: Ty,
    syn: NodeIdx,
) {
    let n = u32_of(b.len());
    let word = l | place::BARE_LOCAL;
    let r = edits.insert(blk, pos, Tag::Read, [word, ReadMode::Copy as u32], ty, syn);
    let rec = u32_of(b.extra.len());
    b.extra.extend([1, n + r]);
    let c = edits.insert(
        blk,
        pos,
        Tag::Intrinsic,
        [IntrinsicOp::ValueCopy as u32, rec],
        copy_ty,
        syn,
    );
    let at = u32_of(b.extra.len());
    b.extra.extend([2, place::ROOT_VALUE, n + r]);
    edits.insert(blk, pos, Tag::Drop, [at, NONE], Ty::VOID, syn);
    edits.insert(blk, pos, Tag::Store, [word, n + c], Ty::VOID, syn);
}
