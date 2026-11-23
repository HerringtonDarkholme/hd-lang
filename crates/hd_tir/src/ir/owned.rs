//! Invariant 23 (`docs/compiler/tir-v2.md`, "Invariants" 4): after the
//! ownership passes, ownership balances. The verifier checks it on a body
//! the passes rewrote, with every structural invariant but 22, which holds
//! only before them.
//!
//! Checked here:
//! - every `Read` has a decided mode (`copy`, `move` or `borrow`);
//! - every owned value has exactly one consumer or one `Drop` on each path
//!   that computes it: none uses it after its consumer or its drop, none
//!   is still owned where its sub-body ends or where a loop goes round
//!   again, and an owned local that may hold a value at its sub-body's
//!   exit is in the exit set ([`Balance`]). Which uses consume is
//!   `crate::own::operands` with the callees' parameter modes and the
//!   types' counted leaves from [`OwnCx`]. A `move` read of a whole value
//!   consumes it; a `move` read or a `Drop` of a part of one takes that
//!   part, after which only more parts or a `Drop` of the rest may follow;
//! - no `Read` or `IsUnique` of a place follows its `move` or its
//!   `Drop` on any path without a `Store` in between that refills it;
//! - no `borrow` value is live across a write to an overlapping place (a
//!   `Store`, a `Drop`, a `move` read, an `IsUnique` or a mutable `Ref`)
//!   or a suspension point, where a borrow is live while anything that
//!   lives on it is ([`borrow_spans`]). A borrow whose referent is no
//!   frame slot may cross a suspension point, as a `fn!`'s local
//!   references live in its frame (`ref.dep.suspension`,
//!   `req.lowering.frame`): one through a reference a local holds, that
//!   reference itself, and a field path of a parameter the body never
//!   stores. Nor one of a place behind a
//!   parameter's reference across a call that may write, unless the body
//!   is exclusive (`sub_flags::EXCLUSIVE`), the parameter is declared `&T`
//!   or `&mut T` and the call does not receive the reference itself;
//! - a local the body does not own holds no count: a binding local, which
//!   a `borrow` value (or a coercion of one that keeps its value) is
//!   stored into, takes only borrows and is never
//!   moved out of or addressed, nor dropped unless such a borrow keeps
//!   its drops; a counted parameter outside the
//!   exit set, which the body does not own, is also never stored into or
//!   changed in place (an `IsUnique` of its own storage, which
//!   would give up a count of its caller's); neither is in the exit set,
//!   except a binding local whose borrow keeps its drops
//!   ([`binding_locals`]);
//! - at each suspension point, whose cancellation leaves through the
//!   exit, every owned value has a row in the unwind set and every owned
//!   local that may hold a value is in the exit set ([`Balance`]);
//! - the exit set's rows are sorted, once each, and name a sub-body and a
//!   local of the body; the unwind set's are sorted, once each, and name a
//!   suspension point and a value;
//! - an `IsUnique` is plain, `HOISTED`, `FRESH`, `REPEAT`, or a deferred
//!   initializer/check pair; a hoisted initializer stands
//!   right before a loop (among other hoisted ones), names a local's place
//!   with one leading dereference at most and fields, whose local no `Ref`
//!   roots at; in that loop nothing copies it (a read of it or of a place
//!   holding it whose value a use keeps), takes a `Ref` that overlaps it,
//!   writes a place holding it, pins or suspends, and no call may write
//!   unless it is a whole local no closure captures ([`hoisted`]);
//! - a collection change's operand that reads a place rooted at a local
//!   or a top-level binding reads it right after the place's `IsUnique`
//!   (or a hoisted one names the place), with nothing that may write
//!   between that read and the change; any other operand, and the root
//!   of an `IsUnique` rooted at a value, reads no such place through
//!   other reads, so the buffer a check or a change leaves reaches the
//!   place it replaced ([`change_places`]).
//!
//! A census groups facts and sparse part keys by sub-body. The walks use
//! dense instruction tables and bitsets sized to that sub-body's moves,
//! borrows, values, locals and parts, including imported borrow dependencies.

use super::{
    Body, Callee, ChoiceKind, ELEM, NONE, Op, Phase, Place, PlaceRoot, Proj, ReadMode, Ref, Root,
    Tag, Topology, TypeWord, VerifyError, is_suspension, local_flags, place, record_type_words,
    ref_word, sub_flags, u32_of, verify_structure,
};
use crate::own::flow::{
    Backward, BitArena, Flow, Forward, Suites, backward_blocks, charge, forward, get_bit, set_bit,
};
use crate::own::parts::Slots;
use crate::own::{OwnCx, Role, consumes_counts as consumes, operands, tracked};
use hd_base::SliceSort as _;
use hd_types::{Ty, TyData, TyList};

/// Verifies a body after the ownership passes: the structural invariants,
/// invariant 23 ([the module](self)) and, on an instance body, 24
/// ([`instance`]), with the facts `cx` gives the passes.
#[must_use]
pub fn verify_owned(b: &Body, cx: &dyn OwnCx) -> Vec<VerifyError> {
    let topo = Topology::new(b);
    let mut errs = verify_structure(b, &topo);
    let places: Vec<Option<Place>> = (0..b.len())
        .map(|i| match b.tags[i].operands().0 {
            Op::Place => b.decode_place(b.data[i][0]),
            _ => None,
        })
        .collect();
    let reference_uses = crate::own::derive::reference_uses(b, cx);
    let forwarding = crate::own::derive::rc_sources(b, cx);
    let reference_roots = crate::own::derive::frame_reference_origins(b, &forwarding, || {}).roots;
    let neutral_stores = crate::own::derive::count_neutral_stores(b, cx);
    let frame_lends =
        crate::own::derive::frame_lending_roots(b, cx, &reference_roots, &neutral_stores);
    let reference_values = reference_values(b);
    let own = cx;
    let cx = Cx {
        b,
        topo: &topo,
        places: &places,
        addressed: &reference_uses.escaping,
        reference_calls: &reference_uses.calls,
        reference_values: &reference_values,
        reference_roots: &reference_roots,
        frame_lends: &frame_lends,
        neutral_stores: &neutral_stores,
    };
    decided(b, &mut errs);
    if b.phase == Phase::Elaborated {
        lowered(b, &topo, &mut errs);
    }
    instance(b, own, &mut errs);
    crate::own::return_loan::verify(b, own, |at, message| err(&mut errs, at, message));
    crate::own::counts::verify(b, own, &topo, |at, message| err(&mut errs, at, message));
    change_places(b, &topo, &mut errs);
    hoisted(b, &topo, own, &mut errs);
    Empties::new(&cx, own).walk(&mut errs);
    borrow_spans(&cx, own, &mut errs);
    binding_locals(&cx, own, &mut errs);
    borrowed_wrappers(b, &forwarding, &mut errs);
    Balance::new(b, &topo, own, &neutral_stores).walk(&mut errs);
    errs
}

/// A wrapper `Scope` a borrow goes through ([`Body::borrowed_alias`]) has
/// that borrow's `Break` as its one way out, so its value is that borrow.
fn borrowed_wrappers(b: &Body, sources: &[u32], errs: &mut Vec<VerifyError>) {
    if !b.forwarded_reads.is_empty() {
        let roots = b.forwarding_reads(sources, || {});
        for (at, &(value, read)) in b.forwarded_reads.iter().enumerate() {
            if roots.get(value as usize) != Some(&read)
                || read == NONE
                || value == read
                || (at > 0 && b.forwarded_reads[at - 1].0 >= value)
            {
                err(
                    errs,
                    value,
                    "an invalid or unsorted forwarded Read identity".into(),
                );
            }
        }
    }
    // Per instruction: the `Break`s to the label it is, if any.
    let mut breaks = vec![0u32; b.len()];
    for i in 0..b.len() {
        if b.tags[i] == Tag::Break
            && let Some(&at) = b.label_inst.get(b.data[i][0] as usize)
            && let Some(c) = breaks.get_mut(at as usize)
        {
            *c += 1;
        }
    }
    for s in 0..b.len() {
        if b.tags[s] == Tag::Scope
            && b.borrowed_alias(u32_of(s)).is_some()
            && breaks[b.data[s][0] as usize] != 1
        {
            err(
                errs,
                u32_of(s),
                "a borrow leaves a block that another `Break` also leaves".into(),
            );
        }
    }
}

fn err(errs: &mut Vec<VerifyError>, inst: u32, what: String) {
    errs.push(VerifyError {
        invariant: 23,
        inst,
        what,
    });
}

/// A collection change's operand that reads a place rooted at a local or
/// a top-level binding (insertion's rule 5) is a `Read` of that place in
/// the change's block, right after the place's `IsUnique` (or anywhere,
/// where a hoisted `IsUnique` names the place), with only reads,
/// arithmetic steps, coercions, constructions and drops between it and
/// the change: nothing that may write the place or
/// replace its buffer. Any other operand, and the root value of an
/// `IsUnique` rooted at a value, reads no place rooted at a local or a
/// binding through reads and coercions: once a collection is its buffer's
/// address, a check or a change can store the buffer it leaves only into
/// the place it names, and one rooted at such a value would leave the
/// place with the old one.
fn change_places(b: &Body, topo: &Topology, errs: &mut Vec<VerifyError>) {
    let n = b.len();
    let unique = |x: usize, p: &Place| {
        b.tags[x] == Tag::IsUnique
            && !matches!(
                b.data[x][1],
                super::unique_word::DEFERRED_INIT | super::unique_word::DEFERRED_CACHE
            )
            && b.decode_place(b.data[x][0]).as_ref() == Some(p)
    };
    let quiet = |&x: &u32| {
        matches!(
            b.tags[x as usize],
            Tag::Read
                | Tag::Prim
                | Tag::Coerce
                | Tag::NewTuple
                | Tag::NewStruct
                | Tag::NewVariant
                | Tag::Drop
        )
    };
    // Whether value `v` reads a place rooted at a local or a binding.
    let in_memory = |p: &Place| {
        p.proj
            .iter()
            .any(|s| matches!(s, Proj::Elem(_) | Proj::Deref))
    };
    // Whether value `v` reads a place rooted at a local or a binding, or
    // goes into memory on the way: an element, a referent.
    let placed = |mut v: u32| {
        let mut memory = false;
        for _ in 0..64 {
            let r = v as usize;
            if Ref(v).as_inst().is_none() || r >= n {
                return memory;
            }
            match (b.tags[r], b.decode_place(b.data[r][0])) {
                (Tag::Coerce, _) if b.row_adaptation(r).is_none() => v = b.data[r][0],
                (Tag::Read, Some(p)) => match p.root {
                    PlaceRoot::Value(u) => {
                        memory |= in_memory(&p);
                        v = u.0;
                    }
                    PlaceRoot::Local(_) | PlaceRoot::Global(..) => return true,
                    _ => return memory,
                },
                _ => return memory,
            }
        }
        false
    };
    for x in (0..n).filter(|&x| b.tags[x] == Tag::IsUnique) {
        if let Some(PlaceRoot::Value(v)) = b.decode_place(b.data[x][0]).map(|p| p.root)
            && placed(v.0)
        {
            let what = format!(
                "`IsUnique` rooted at %{} leaves its buffer in no place",
                v.0
            );
            err(errs, u32_of(x), what);
        }
    }
    // The places hoisted `IsUnique`s name, collected once: a change in
    // their loop reads its place there, away from any `IsUnique`.
    let hoisted: hd_base::HashSet<Place> = (0..n)
        .filter(|&x| b.tags[x] == Tag::IsUnique && b.data[x][1] == super::unique_word::HOISTED)
        .filter_map(|x| b.decode_place(b.data[x][0]))
        .collect();
    charge(n);
    for c in 0..n {
        for (_, v) in crate::own::changed(b, c) {
            let r = v as usize;
            let Some(p) = (Ref(v).as_inst().is_some() && r < n && b.tags[r] == Tag::Read)
                .then(|| b.decode_place(b.data[r][0]))
                .flatten()
                .filter(|p| match p.root {
                    PlaceRoot::Local(_) | PlaceRoot::Global(..) => true,
                    // Memory behind another value: a referent, a buffer.
                    PlaceRoot::Value(u) => !placed(u.0) && in_memory(p),
                    _ => false,
                })
            else {
                if placed(v) {
                    let what = format!("change operand %{r} leaves its buffer in no place");
                    err(errs, u32_of(c), what);
                }
                continue;
            };
            let (blk, cb) = (topo.blk[r], topo.blk[c]);
            let (pos, cp) = (topo.pos[r] as usize, topo.pos[c] as usize);
            let list = if blk == NONE {
                &[][..]
            } else {
                b.record(b.data[blk as usize][0])
            };
            charge(1);
            let checked = pos > 0 && unique(list[pos - 1] as usize, &p) || hoisted.contains(&p);
            if !checked || blk != cb || cp <= pos || !list[pos + 1..cp].iter().all(quiet) {
                let what =
                    format!("change operand %{r} is no read of its place after its `IsUnique`");
                err(errs, u32_of(c), what);
            }
        }
    }
}

/// Hoisting's `IsUnique`s (`docs/compiler/tir-v2.md`, "Hoisting"): what
/// the pass relies on, which its edit-batch mates could break.
fn hoisted(b: &Body, topo: &Topology, cx: &dyn OwnCx, errs: &mut Vec<VerifyError>) {
    use super::unique_word::{DEFERRED, DEFERRED_CACHE, DEFERRED_INIT, FRESH, HOISTED, REPEAT};
    use crate::own::hoist;
    let n = b.len();
    // A fresh buffer's `IsUnique` is of a place that keeps it fresh, and one
    // that runs isn't; a freeing `Drop` is of a buffer this body owns alone.
    let fresh_drop = |i: usize| b.tags[i] == Tag::Drop && super::drop_word::fresh(b.data[i][1]);
    if b.tags.contains(&Tag::IsUnique) || (0..n).any(fresh_drop) {
        let fresh = hoist::fresh_buffers(b, cx);
        for i in 0..n {
            if fresh_drop(i) && hoist::fresh_level(b, &fresh, b.data[i][0]) < hoist::OWNED {
                err(
                    errs,
                    u32_of(i),
                    "a freeing `Drop` of a buffer that may be shared or static".into(),
                );
            }
            if b.tags[i] != Tag::IsUnique {
                continue;
            }
            let w = b.data[i][1];
            let is_fresh = hoist::fresh_level(b, &fresh, b.data[i][0]) >= hoist::UNIQUE;
            if w == FRESH && !is_fresh {
                err(
                    errs,
                    u32_of(i),
                    "a fresh `IsUnique` of a place that may share its buffer".into(),
                );
            } else if matches!(w, 0 | HOISTED | DEFERRED | DEFERRED_INIT | DEFERRED_CACHE)
                && is_fresh
            {
                err(
                    errs,
                    u32_of(i),
                    "an `IsUnique` that may be shared, of a fresh buffer".into(),
                );
            }
        }
    }
    let any = (0..n).any(|i| b.tags[i] == Tag::IsUnique && b.data[i][1] != 0);
    if !any {
        return;
    }
    let element_checks = crate::own::ElementChecks::new(b);
    let (blk, pos, first) = (&topo.blk, &topo.pos, &topo.first);
    // Per local: a `Ref` of it, and one that may outlive its uses.
    let mut addressed = vec![false; b.local_ty.len()];
    let mut held = vec![false; b.local_ty.len()];
    let mut captured = vec![false; b.local_ty.len()];
    let ref_local = |v: usize| {
        (b.tags[v] == Tag::Ref)
            .then(|| b.local_in_place(b.data[v][0]))
            .flatten()
    };
    let mut refs = Vec::new();
    for i in 0..n {
        if let Some(l) = ref_local(i) {
            addressed[l.idx()] = true;
            held[l.idx()] |= !hoist::spent_ref(b.data[i][1]);
            refs.push((l.idx(), u32_of(i)));
        }
    }
    refs.sort_stable();
    for l in &b.cap_local {
        captured[l.idx()] = true;
    }
    // The flags also mark a local a reference was taken of.
    for (l, f) in b.local_flags.iter().enumerate() {
        if f & (super::local_flags::CAPTURED | super::local_flags::CAPTURED_ASSIGNED) != 0
            && !addressed[l]
        {
            captured[l] = true;
        }
    }
    let mut kept = vec![false; n];
    let mut last = vec![0; n];
    let mut ops = Vec::new();
    for i in 0..n {
        operands(b, i, &mut ops);
        for op in &ops {
            if hoist::keeps(b, u32_of(i), op.role) {
                kept[op.value as usize] = true;
            }
            if let Some(l) = ref_local(op.value as usize)
                && hoist::holds_use(b, cx, u32_of(i), op.role)
            {
                held[l.idx()] = true;
            }
            last[op.value as usize] = u32_of(i);
        }
    }
    // Whether a reference may reach local `local` while loop `lp` runs.
    let reached = |local: usize, lp: u32| {
        let from = refs.partition_point(|&(l, _)| l < local);
        addressed[local]
            && (held[local]
                || refs[from..]
                    .iter()
                    .take_while(|&&(l, _)| l == local)
                    .any(|&(_, r)| hoist::ref_in_loop(r, last[r as usize], first[lp as usize], lp)))
    };
    let (mut steps, mut raw, mut other) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..n {
        if b.tags[i] != Tag::IsUnique {
            continue;
        }
        let w = b.data[i][1];
        if w == REPEAT && !element_checks.repeats_unique(i) && !hoist::repeats(b, topo, i) {
            err(
                errs,
                u32_of(i),
                "a repeated `IsUnique` with no earlier check of its place in its block".into(),
            );
            continue;
        }
        if !matches!(
            w,
            0 | HOISTED | FRESH | REPEAT | DEFERRED_INIT | DEFERRED | DEFERRED_CACHE
        ) {
            err(
                errs,
                u32_of(i),
                format!("an `IsUnique` of unknown kind {w}"),
            );
            continue;
        }
        if !matches!(w, HOISTED | DEFERRED_INIT | DEFERRED_CACHE) {
            continue;
        }
        let at = blk[i];
        let next = (at != NONE)
            .then(|| {
                b.record(b.data[at as usize][0])[pos[i] as usize + 1..]
                    .iter()
                    .copied()
                    .find(|&x| {
                        let tag = b.tags[x as usize];
                        // A block in the list is one a statement runs.
                        tag != Tag::Block
                            && !(tag == Tag::IsUnique
                                && matches!(
                                    b.data[x as usize][1],
                                    HOISTED | DEFERRED_INIT | DEFERRED_CACHE
                                ))
                    })
            })
            .flatten();
        let Some(l) = next.filter(|&x| b.tags[x as usize] == Tag::Loop) else {
            err(
                errs,
                u32_of(i),
                "a hoisted `IsUnique` that no loop follows".into(),
            );
            continue;
        };
        let (local, _) = hoist::resolve(b, b.data[i][0], &mut steps, &mut raw);
        if local == NONE || !hoist::hoistable(&steps) || reached(local as usize, l) {
            err(
                errs,
                u32_of(i),
                "a hoisted `IsUnique` of a place that a reference may reach in its loop".into(),
            );
            continue;
        }
        let whole = steps.is_empty() && !captured[local as usize];
        // In an exclusive copy nothing else reaches what a reference
        // parameter refers to, so a call that does not receive it leaves
        // the place alone; a call that does is a kept read of it below.
        let excl = hoist::is_exclusive(b) && hoist::exclusive_place(b, cx, local, &steps);
        for x in first[l as usize]..l {
            let xi = x as usize;
            let tag = b.tags[xi];
            let [a, xw] = b.data[xi];
            let what = if is_suspension(tag) {
                Some("pins or suspends")
            } else if !whole && !excl && hoist::writes(b, cx, xi) {
                Some("calls what may write")
            } else if w == DEFERRED_CACHE
                && tag == Tag::Intrinsic
                && hoist::changes_header(super::IntrinsicOp::from_u32(a))
            {
                let coll = b.record(xw).first().copied().unwrap_or(NONE);
                let same = Ref(coll).as_inst().is_some_and(|read| {
                    let r = read.0 as usize;
                    if b.tags[r] != Tag::Read {
                        return false;
                    }
                    let (ol, _) = hoist::resolve(b, b.data[r][0], &mut other, &mut raw);
                    ol == local && other == steps
                });
                same.then_some("replaces its deferred cached header")
            } else if matches!(tag, Tag::Read | Tag::Store | Tag::Drop | Tag::Ref) {
                let (ol, temp) = hoist::resolve(b, a, &mut other, &mut raw);
                let mine = ol == local;
                let covers = mine && hoist::prefix(&other, &steps);
                let below = mine && other.len() > steps.len() && hoist::prefix(&steps, &other);
                // A local reference to one element that the loop uses up:
                // `&mut`, or plain shared.
                let local_mut = super::ref_word::MUT | super::ref_word::LOCAL;
                let elem_ref = below
                    && other[steps.len()] == ELEM
                    && (xw & local_mut == local_mut || hoist::lent_ref_word(xw))
                    && last[xi] < l;
                match tag {
                    Tag::Read if xw == ReadMode::Move as u32 && covers && !temp => Some("moves it"),
                    Tag::Read if covers && kept[xi] => Some("copies it"),
                    Tag::Store | Tag::Drop if covers && !temp => Some("writes it"),
                    Tag::Ref if (covers || below) && !temp && !elem_ref => {
                        Some("takes a reference to it")
                    }
                    _ => None,
                }
            } else {
                None
            };
            if let Some(what) = what {
                err(
                    errs,
                    u32_of(i),
                    format!("a hoisted `IsUnique`'s loop %{l} {what} at %{x}"),
                );
                break;
            }
        }
    }
}

/// Invariant 24: an instance body (`Phase::Instance`) holds no generic
/// form, since the emitter substitutes none. Rejected: a type naming a
/// type parameter, a projection or an opaque result, in any column or
/// record type word (`hd_mono::instantiate` and `close_body` close them);
/// a generic local function (`crate::ir::lift`, whose error the instance
/// marking leaves to this check); and a trait call chosen through
/// a bound at a receiver that is no trait value (`inline::rechoose`).
/// Cost: one pass over the columns and records, an O(1) flag test a type.
fn instance(b: &Body, cx: &dyn OwnCx, errs: &mut Vec<VerifyError>) {
    let instance = matches!(b.phase, Phase::Instance | Phase::Elaborated);
    let Some(pool) = cx.pool().filter(|_| instance) else {
        return;
    };
    let mut bad = |inst: u32, what: String| {
        errs.push(VerifyError {
            invariant: 24,
            inst,
            what,
        });
    };
    let types = pool.types();
    let open = |t: Ty| types.has_param(t) || types.has_assoc(t) || types.has_opaque(t);
    let generic = |t: Ty| format!("`{}` is generic", cx.shown(t));
    // A trait value's call, or one through a reference to it, dispatches.
    let trait_value = |t: Ty| {
        let t = match pool.get(t) {
            TyData::Ref { inner, .. } => inner,
            _ => t,
        };
        matches!(pool.get(t), TyData::TraitValue { .. })
    };
    if let Some(sub) = b.sub_flags.iter().position(|f| f & sub_flags::GENERIC != 0) {
        bad(
            b.sub_root[sub],
            format!("sub-body {sub} is a generic local function"),
        );
    }
    for (i, &t) in b.ty.iter().enumerate().filter(|e| open(*e.1)) {
        bad(u32_of(i), generic(t));
    }
    let consts = b.consts.iter().map(|c| &c.0);
    for &t in b.local_ty.iter().chain(consts).filter(|&&t| open(t)) {
        bad(NONE, format!("a local or constant: {}", generic(t)));
    }
    for i in (0..b.len()).filter(|&i| b.tags[i] == Tag::Call) {
        if let Some(Callee::TraitMethod {
            self_ty,
            choice: (ChoiceKind::Bound, _),
            ..
        }) = Callee::from_words(b.record(b.data[i][0]))
            && !types.has_param(self_ty)
            && !trait_value(self_ty)
        {
            bad(
                u32_of(i),
                format!(
                    "a trait call chosen through a bound at `{}`",
                    cx.shown(self_ty)
                ),
            );
        }
    }
    record_type_words(b, &mut |at, word| {
        let w = b.extra[at];
        let t = match word {
            TypeWord::Ty => Some(Ty(w)).filter(|&t| open(t)),
            TypeWord::List => pool
                .list_items(TyList(w))
                .iter()
                .copied()
                .find(|&t| open(t)),
        };
        if let Some(t) = t {
            bad(NONE, format!("record word {at}: {}", generic(t)));
        }
    });
}

/// The lowered-TIR contract a backend relies on, for a body after its last
/// lowering (`Phase::Elaborated`, `own::elaborate`), invariant 25: no
/// cleanup scope is left (`own::scopes`); every suspension point has its
/// cancel block; every flag a local's drops test is read in TIR, the
/// backend reading none (`own::drop_flags`; a cell's are its slots); and
/// every move of a local's or a value's place is followed, past its counts,
/// by its `Empty`, the backend moving nothing. O(instructions).
fn lowered(b: &Body, topo: &Topology, errs: &mut Vec<VerifyError>) {
    let mut bad = |inst: u32, what: String| {
        errs.push(VerifyError {
            invariant: 25,
            inst,
            what,
        });
    };
    let (cells, _) = crate::own::drop_flags::cells(b);
    let mut read = vec![false; b.local_ty.len()];
    for (i, &[a, _]) in b.data.iter().enumerate() {
        if b.tags[i] == Tag::Read && a & place::BARE_LOCAL != 0 {
            read[(a & !place::BARE_LOCAL) as usize] = true;
        }
    }
    for (i, (&tag, &[a, w])) in b.tags.iter().zip(&b.data).enumerate() {
        let suites = tag == Tag::Scope && w != NONE && !b.record(w).is_empty();
        let flag = matches!(tag, Tag::FlagSet | Tag::FlagClear)
            && !matches!(b.view(a).root, Root::Local(l) if cells[l as usize])
            && !read.get(w as usize).copied().unwrap_or(false);
        let what = if tag == Tag::Defer || suites {
            "a cleanup scope left to the backend"
        } else if is_suspension(tag) && b.cancel_block(u32_of(i)).is_none() {
            "a suspension point with no cancel block"
        } else if flag {
            "a drop flag no elaborated drop reads"
        } else if tag == Tag::Read && w == ReadMode::Move as u32 && !emptied(b, topo, i) {
            "a move with no `Empty` of its place after it"
        } else {
            continue;
        };
        bad(u32_of(i), what.into());
    }
}

/// Whether `Read move` `i`, of a local's or a value's place, is followed
/// past its counts by an `Empty` of that place (`own::elaborate`).
fn emptied(b: &Body, topo: &Topology, i: usize) -> bool {
    let p = b.view(b.data[i][0]);
    if topo.blk[i] == NONE || !matches!(p.root, Root::Local(_) | Root::Value(_)) || !p.in_place(b) {
        return true;
    }
    let (blk, pos) = crate::own::drop_flags::after(b, topo, i);
    let next = b.record(b.data[blk as usize][0]).get(pos as usize).copied();
    next.is_some_and(|e| {
        b.tags[e as usize] == Tag::Intrinsic
            && b.data[e as usize][0] == super::IntrinsicOp::Empty as u32
    })
}

/// The read modes and the side rows.
fn decided(b: &Body, errs: &mut Vec<VerifyError>) {
    for (i, (&tag, &[_, w])) in b.tags.iter().zip(&b.data).enumerate() {
        if tag != Tag::Read {
            continue;
        }
        match ReadMode::from_u32(w) {
            Some(ReadMode::Use) => {
                err(errs, u32_of(i), "a `Read` left `use` by the passes".into());
            }
            None => err(errs, u32_of(i), format!("a `Read` of unknown mode {w}")),
            Some(_) => {}
        }
    }
    for (k, &(sub, local)) in b.exit_drops.iter().enumerate() {
        if sub.idx() >= b.sub_root.len() || local.idx() >= b.local_ty.len() {
            err(
                errs,
                NONE,
                format!(
                    "exit drop row {k} names sub-body {} or local {}, which the body lacks",
                    sub.raw(),
                    local.raw()
                ),
            );
        }
        let key = |(s, l): (hd_base::SubId, hd_base::LocalId)| (s.raw(), l.raw());
        if k > 0 && key(b.exit_drops[k - 1]) >= key((sub, local)) {
            err(errs, NONE, format!("exit drop row {k} is out of order"));
        }
    }
    for (k, &(at, blk)) in b.cancel.iter().enumerate() {
        // A root's: its frame's drop before its first poll (`own::elaborate`).
        let point =
            b.tags.get(at as usize).is_some_and(|&t| is_suspension(t)) || b.sub_root.contains(&at);
        if !point || b.tags.get(blk as usize) != Some(&Tag::Block) {
            err(
                errs,
                at,
                format!("cancel row {k} names %{blk} at %{at}, which is no point's block"),
            );
        } else if k > 0 && b.cancel[k - 1].0 >= at {
            err(errs, at, format!("cancel row {k} is out of order"));
        }
    }
    for (k, &(s, l, _)) in b.storage.iter().enumerate() {
        if s.idx() >= b.sub_root.len() || l.idx() >= b.local_ty.len() {
            err(
                errs,
                NONE,
                format!("storage row {k} names sub {s:?} or {l:?}, which the body lacks"),
            );
        }
        if k > 0 && (b.storage[k - 1].0.raw(), b.storage[k - 1].1.raw()) >= (s.raw(), l.raw()) {
            err(errs, NONE, format!("storage row {k} is out of order"));
        }
    }
}

/// What both walks read: the body, each instruction's decoded place, and
/// the locals a reference or a shared capture can reach outside the one
/// call a local reference lends to.
struct Cx<'a> {
    b: &'a Body,
    topo: &'a Topology,
    places: &'a [Option<Place>],
    addressed: &'a [bool],
    reference_calls: &'a [u32],
    reference_values: &'a [u32],
    reference_roots: &'a [u32],
    frame_lends: &'a [bool],
    neutral_stores: &'a [bool],
}

/// The value stored once into each unaddressed local. Multiple stores,
/// parameters and references to the local's storage rule out substitution.
fn reference_values(b: &Body) -> Vec<u32> {
    let mut values = vec![NONE; b.local_ty.len()];
    let mut stores: Vec<u8> = b
        .local_flags
        .iter()
        .map(|f| if f & local_flags::PARAM != 0 { 2 } else { 0 })
        .collect();
    for i in 0..b.len() {
        let [a, w] = b.data[i];
        match b.tags[i] {
            Tag::Store => {
                let p = b.view(a);
                if let Some(l) = p.local() {
                    let l = l as usize;
                    stores[l] = stores[l].saturating_add(1);
                    values[l] = if stores[l] == 1 && p.whole() { w } else { NONE };
                }
            }
            Tag::Ref => {
                if let Some(l) = b.local_in_place(a) {
                    stores[l.idx()] = 2;
                    values[l.idx()] = NONE;
                }
            }
            _ => {}
        }
    }
    for (l, mode) in b.cap_local.iter().zip(&b.cap_mode) {
        if mode.shares() {
            values[l.idx()] = NONE;
        }
    }
    values
}

impl Cx<'_> {
    /// Resolve only direct local places and references whose one immutable
    /// definition names such a place. Unknown aliases retain the ordinary
    /// conservative overlap check.
    fn local_place(&self, p: &Place) -> Option<Place> {
        if matches!(p.root, PlaceRoot::Local(_))
            && p.proj
                .iter()
                .all(|s| matches!(s, Proj::Field(_) | Proj::Variant(_)))
        {
            return Some(p.clone());
        }
        let (Proj::Deref, rest) = p.proj.split_first()? else {
            return None;
        };
        if !rest
            .iter()
            .all(|s| matches!(s, Proj::Field(_) | Proj::Variant(_)))
        {
            return None;
        }
        let mut value = match p.root {
            PlaceRoot::Local(l) => *self.reference_values.get(l.idx())?,
            PlaceRoot::Value(r) => r.0,
            _ => return None,
        };
        // A valid definition chain visits each instruction at most once.
        // Bound malformed cycles by graph size, not a source alias limit.
        for _ in 0..self.b.len() {
            let i = Ref(value).as_inst()?.0 as usize;
            match self.b.tags.get(i)? {
                Tag::Ref => {
                    let mut origin = self.places[i].clone()?;
                    if !matches!(origin.root, PlaceRoot::Local(_))
                        || !origin
                            .proj
                            .iter()
                            .all(|s| matches!(s, Proj::Field(_) | Proj::Variant(_)))
                    {
                        return None;
                    }
                    origin.proj.extend_from_slice(rest);
                    return Some(origin);
                }
                Tag::Read => {
                    let Place {
                        root: PlaceRoot::Local(l),
                        proj,
                    } = self.places[i].as_ref()?
                    else {
                        return None;
                    };
                    if !proj.is_empty() {
                        return None;
                    }
                    value = *self.reference_values.get(l.idx())?;
                }
                Tag::Coerce if self.b.same_value_coercion(i) => value = self.b.data[i][0],
                _ => return None,
            }
        }
        None
    }

    /// Whether places `p` and `q` may name overlapping storage
    /// (`docs/compiler/tir-v2.md`, "Places"). With one root, one's
    /// projections are a prefix of the other's, element indices ignored.
    /// With two roots, one goes behind a reference or into a list's buffer
    /// and the other is where a reference can reach: behind one too, a
    /// top-level binding, or a local shared with a closure or addressed by
    /// a reference that outlives its one call. A value root alone is a
    /// temporary that nothing else reaches.
    fn overlap(&self, p: &Place, q: &Place) -> bool {
        if same_root(p.root, q.root) {
            return p.proj.iter().zip(&q.proj).all(|(x, y)| match (x, y) {
                (Proj::Field(a), Proj::Field(c)) | (Proj::Variant(a), Proj::Variant(c)) => a == c,
                (Proj::Deref, Proj::Deref) | (Proj::Elem(_), Proj::Elem(_)) => true,
                // Steps of two kinds at one position: assume the worst.
                _ => true,
            });
        }
        let behind = |p: &Place| {
            matches!(p.root, PlaceRoot::Ptr(_))
                || p.proj
                    .iter()
                    .any(|s| matches!(s, Proj::Deref | Proj::Elem(_)))
        };
        let reachable = |p: &Place| {
            behind(p)
                || match p.root {
                    PlaceRoot::Global(..) | PlaceRoot::Frame(_) => true,
                    PlaceRoot::Local(l) => self.addressed.get(l.idx()).copied().unwrap_or(true),
                    PlaceRoot::Value(_) | PlaceRoot::Ptr(_) => false,
                }
        };
        (behind(p) || behind(q)) && reachable(p) && reachable(q)
    }
}

fn same_root(a: PlaceRoot, c: PlaceRoot) -> bool {
    match (a, c) {
        (PlaceRoot::Global(x, _), PlaceRoot::Global(y, _)) => x == y,
        _ => a == c,
    }
}

/// Whether a `Store` into `q` refills all of the emptied place `p`: one
/// root, and `q`'s projections a prefix of `p`'s, element indices equal.
fn covers(q: &Place, p: &Place) -> bool {
    same_root(q.root, p.root) && q.proj.len() <= p.proj.len() && q.proj == p.proj[..q.proj.len()]
}

/// A bit set: a walk's state.
type Set = [u64];

/// The read-after-move walk: a forward walk ([`forward`]) over each
/// sub-body, carrying the set of places a `move` read or a `Drop` emptied.
/// A cleanup suite starts from the union of the states at its scope's
/// exits, since it runs at whichever exit the body takes: its end, or a
/// jump that leaves it.
struct Empties<'a> {
    cx: &'a Cx<'a>,
    /// Each instruction's number among the moves and drops, or `NONE`.
    id: Vec<u32>,
    /// The instruction of each move or drop.
    at: Vec<u32>,
    domains: Vec<Vec<usize>>,
    number: Vec<u32>,
    domain: usize,
    /// (value, move or drop number) for each move or drop of a place
    /// rooted at an instruction's value, sorted: running that instruction
    /// again makes a new value, which refills the place.
    by_value: Vec<(u32, u32)>,
    /// (instruction, move number) for each use through a reference into a
    /// part of a frame local that a `move` read empties, sorted.
    part_uses: Vec<(u32, u32)>,
    reported: Vec<bool>,
    errs: Vec<VerifyError>,
}

impl<'a> Empties<'a> {
    fn new(cx: &'a Cx<'a>, own: &dyn OwnCx) -> Self {
        let b = cx.b;
        let mut id = vec![NONE; b.len()];
        let mut at = Vec::new();
        for (i, slot) in id.iter_mut().enumerate() {
            // A read through a pointer moves the value out of the heap,
            // whose holding std's code keeps (`module.rt.ptr.holds`).
            let ptr = matches!(
                cx.places[i].as_ref().map(|p| p.root),
                Some(PlaceRoot::Ptr(_))
            );
            let empties = match b.tags[i] {
                Tag::Read => b.data[i][1] == ReadMode::Move as u32 && !ptr,
                Tag::Ref => b.data[i][1] & super::ref_word::TAKES != 0,
                Tag::Drop | Tag::LoanEnd => true,
                _ => false,
            };
            if empties && cx.places[i].is_some() {
                *slot = u32_of(at.len());
                at.push(u32_of(i));
            }
        }
        let mut domains = vec![Vec::new(); b.sub_root.len()];
        let mut number = vec![NONE; at.len()];
        for (global, &instruction) in at.iter().enumerate() {
            let Some(row) = domains.get_mut(cx.topo.sub[instruction as usize] as usize) else {
                continue;
            };
            number[global] = u32_of(row.len());
            row.push(global);
        }
        let mut by_value: Vec<(u32, u32)> = at
            .iter()
            .enumerate()
            .filter_map(|(k, &e)| match cx.places[e as usize].as_ref()?.root {
                PlaceRoot::Value(r) if r.as_inst().is_some() => Some((r.0, u32_of(k))),
                _ => None,
            })
            .collect();
        by_value.sort_stable();
        let moved: Vec<(u32, u32)> = at
            .iter()
            .enumerate()
            .filter(|&(_, &e)| b.tags[e as usize] == Tag::Read)
            .filter_map(|(k, &e)| {
                let p = b.view(b.data[e as usize][0]);
                let l = p.local().filter(|_| !p.whole())?;
                let framed = (0..b.len()).any(|i| {
                    b.tags[i] == Tag::Ref
                        && b.data[i][1] & super::ref_word::FRAME != 0
                        && b.view(b.data[i][0]).local() == Some(l)
                });
                framed.then_some((u32_of(k), l))
            })
            .collect();
        let mut part_uses = Vec::new();
        let mut t = crate::own::frame::Tables::default();
        for chunk in moved.chunks(64) {
            let targets: Vec<(u32, u32)> = chunk
                .iter()
                .map(|&(k, l)| (l, b.data[at[k as usize] as usize][0]))
                .collect();
            crate::own::frame::part_uses(b, own, &mut t, &targets, |i, n| {
                part_uses.push((i, chunk[n].0));
            });
        }
        part_uses.sort_stable();
        Empties {
            cx,
            id,
            at,
            domains,
            number,
            domain: 0,
            by_value,
            part_uses,
            reported: vec![false; b.len()],
            errs: Vec::new(),
        }
    }

    fn walk(mut self, errs: &mut Vec<VerifyError>) {
        if self.at.is_empty() {
            return;
        }
        let (b, topo) = (self.cx.b, self.cx.topo);
        let mut s = Flow::default();
        for (sub, &root) in b.sub_root.iter().enumerate() {
            self.domain = sub;
            s.arena.words = self.domains[sub].len().div_ceil(64);
            let st = s.arena.alloc(None);
            forward(b, topo, &mut s, &mut self, root, st);
            s.arena.free(st);
        }
        errs.append(&mut self.errs);
    }

    /// Instruction `x` empties its place: a `move` read or a `Drop`.
    fn empty(&mut self, x: u32, s: &mut Set) {
        let k = self.id[x as usize];
        if k != NONE {
            set_bit(s, self.number[k as usize] as usize, true);
        }
    }

    /// A `Store` refills every emptied place it covers.
    fn refill(&self, x: u32, s: &mut Set) {
        let Some(q) = &self.cx.places[x as usize] else {
            return;
        };
        for word in 0..s.len() {
            charge(1);
            let mut bits = s[word];
            while bits != 0 {
                charge(1);
                let k = word * 64 + bits.trailing_zeros() as usize;
                bits &= bits - 1;
                let global = self.domains[self.domain][k];
                let e = self.at[global];
                if let Some(p) = &self.cx.places[e as usize]
                    && covers(q, p)
                {
                    set_bit(s, k, false);
                }
            }
        }
    }

    /// Whether emptying `p` at `e` empties what `q` reads. A move of a
    /// whole local transfers its value, so it empties the local's slot but
    /// not storage the value owns. A `Drop` of a whole local, or a `Ref`
    /// that takes its count, ends the local's own hold on its value: a local
    /// that a reference or a closure reaches lives in a cell, whose holders
    /// keep it, so only a read of the local itself sees it empty.
    fn empties(&self, e: u32, p: &Place, q: &Place) -> bool {
        if matches!(
            self.cx.b.tags[e as usize],
            Tag::Read | Tag::Drop | Tag::LoanEnd | Tag::Ref
        ) && matches!(p.root, PlaceRoot::Local(_))
            && p.proj.is_empty()
        {
            return p.root == q.root;
        }
        if let (Some(p), Some(q)) = (self.cx.local_place(p), self.cx.local_place(q)) {
            return self.cx.overlap(&p, &q);
        }
        self.cx.overlap(p, q)
    }

    /// A read of `x`'s place must not overlap a place emptied before it.
    fn check(&mut self, x: u32, s: &Set) {
        let Some(q) = &self.cx.places[x as usize] else {
            return;
        };
        if self.reported[x as usize] {
            return;
        }
        for k in set_indices(s) {
            let global = self.domains[self.domain][k];
            let e = self.at[global];
            if let Some(p) = &self.cx.places[e as usize]
                && (self.empties(e, p, q)
                    || self.part_uses.binary_search(&(x, u32_of(global))).is_ok())
            {
                self.reported[x as usize] = true;
                let what = format!(
                    "`{}` of {q} after `{}` of {p} at %{e} empties it",
                    self.cx.b.tags[x as usize].name(),
                    self.cx.b.tags[e as usize].name()
                );
                err(&mut self.errs, x, what);
                return;
            }
        }
    }
}

impl Forward for Empties<'_> {
    const SUITES: Suites = Suites::FromExits;

    fn stmt(&mut self, b: &Body, x: u32, s: &mut BitArena, st: usize) {
        self.enter(b, x, s, st);
        let st = s.row_mut(st);
        match b.tags[x as usize] {
            Tag::Read | Tag::IsUnique => {
                self.check(x, st);
                self.empty(x, st);
            }
            Tag::Drop | Tag::LoanEnd => self.empty(x, st),
            Tag::Store => self.refill(x, st),
            _ => {}
        }
    }

    /// A new value of `x` (a loop's next round) refills its places.
    fn enter(&mut self, _: &Body, x: u32, s: &mut BitArena, st: usize) {
        let from = self.by_value.partition_point(|&(v, _)| v < x);
        for &(_, k) in self.by_value[from..].iter().take_while(|&&(v, _)| v == x) {
            if self.cx.topo.sub[self.at[k as usize] as usize] as usize == self.domain {
                s.set(st, self.number[k as usize], false);
            }
        }
    }
}

/// The locals the body does not own (`docs/compiler/tir-v2.md`,
/// "Borrow"). A binding local: a `Store` of a `borrow` read goes into all
/// of it, so it holds borrowed values only: every store into it stores
/// one, nothing moves out of it or addresses it, and nothing drops it nor
/// is it in an exit set. A lent parameter: a counted parameter outside the exit set,
/// which nothing stores into, drops or moves out of. Its immutable frame
/// address may stay inside the invocation in a body with no external write;
/// otherwise it is not addressed either. Nothing changes it in place:
/// an `IsUnique` of it would give up a count that is its caller's.
fn binding_locals(cx: &Cx<'_>, own: &dyn OwnCx, errs: &mut Vec<VerifyError>) {
    let b = cx.b;
    let mut binding = vec![false; b.local_ty.len()];
    let mut lent = vec![false; b.local_ty.len()];
    let mut any = false;
    if !b.sub_root.is_empty() {
        let mut exit = vec![false; b.local_ty.len()];
        for &(sub, l) in &b.exit_drops {
            if sub.idx() == 0 {
                exit[l.idx()] = true;
            }
        }
        for l in root_entry_locals(b, &mut || {}) {
            if !exit[l] && own.counted(b.local_ty[l]) {
                lent[l] = true;
                any = true;
            }
        }
    }
    for i in 0..b.len() {
        if b.tags[i] != Tag::Store || b.loan_value(b.data[i][1]).is_none() {
            continue;
        }
        match &cx.places[i] {
            Some(Place {
                root: PlaceRoot::Local(l),
                proj,
            }) if proj.is_empty() => {
                binding[l.idx()] = true;
                any = true;
            }
            _ => err(
                errs,
                u32_of(i),
                "a `borrow` value stored into a place that is no whole local".into(),
            ),
        }
    }
    if !any {
        return;
    }
    let no_external_write = !crate::own::writes_in(b, 0..b.len(), &|c| own.may_write(c));
    // Each flag local a parameter's ownership starts, as (place word,
    // parameter).
    let starts: Vec<(u32, u32)> = (0..b.len())
        .filter(|&s| b.tags[s] == Tag::Store)
        .filter_map(|s| Some((b.data[s][0], b.param_owned(b.data[s][1])?)))
        .collect();
    let mut frame_slots = vec![false; b.local_ty.len()];
    for &(sub, l, storage) in &b.storage {
        if sub.idx() == 0 && storage == super::Storage::Slot && (lent[l.idx()] || binding[l.idx()])
        {
            frame_slots[l.idx()] = true;
        }
    }
    for i in 0..b.len() {
        // The local's own storage: a place through it, into what it refers
        // to, leaves it as it is.
        let Some(l) = cx.places[i]
            .as_ref()
            .and_then(|_| b.local_in_place(b.data[i][0]))
        else {
            continue;
        };
        let l = &l;
        if !binding[l.idx()] && !lent[l.idx()] {
            continue;
        }
        // A borrowed parameter still has private frame storage for its
        // descriptor. Frame planning proves an address stays inside this
        // invocation, including through local aggregates. In a body with
        // no external writes, an immutable address borrows that storage.
        // A binding's immutable address also lends when its complete uses
        // read through it or end at one call. The borrow walk keeps its
        // original source live through those uses, including coercions.
        let call = cx.reference_calls[i];
        if b.tags[i] == Tag::Ref
            && frame_slots[l.idx()]
            && cx.topo.sub[i] == 0
            && b.data[i][1] & (ref_word::LOCAL | ref_word::FRAME)
                == ref_word::LOCAL | ref_word::FRAME
            && (binding[l.idx()]
                && (cx.frame_lends[l.idx()] || b.local_flags[l.idx()] & local_flags::VIEW != 0)
                || !binding[l.idx()]
                    && b.data[i][1] & ref_word::MUT == 0
                    && (no_external_write
                        || (call != NONE
                            && b.data
                                .get(call as usize)
                                .and_then(|[at, _]| b.extra.get(*at as usize..))
                                .and_then(|words| words.split_first())
                                .and_then(|(&len, words)| words.get(..len as usize))
                                .and_then(Callee::from_words)
                                .is_some_and(|callee| !own.may_write(&callee)))))
        {
            continue;
        }
        if b.tags[i] == Tag::Store
            && binding[l.idx()]
            && cx.neutral_stores[i]
            && cx.places[i].as_ref().is_some_and(|p| {
                !p.proj.is_empty()
                    && p.proj
                        .iter()
                        .all(|step| matches!(step, Proj::Field(_) | Proj::Variant(_)))
            })
        {
            continue;
        }
        // A lent parameter's flag starts false: a drop under it never runs
        // (`own::elaborate`).
        if b.tags[i] == Tag::Drop && lent[l.idx()] && owned_guard(b, cx.topo, i, l.raw(), &starts) {
            continue;
        }
        let what = match b.tags[i] {
            Tag::Store if lent[l.idx()] => "a store into",
            Tag::Store if b.loan_value(b.data[i][1]).is_none() => "an owned value stored into",
            Tag::Drop => "a `Drop` of",
            Tag::Read if b.data[i][1] == ReadMode::Move as u32 => "a `move` out of",
            Tag::Ref => "a reference to",
            Tag::IsUnique => "a change in place of",
            _ => continue,
        };
        err(
            errs,
            u32_of(i),
            format!("{what} ${}, which the body does not own", l.raw()),
        );
    }
    for &(_, l) in &b.exit_drops {
        if binding.get(l.idx()).copied().unwrap_or(false) {
            err(
                errs,
                NONE,
                format!("${} holds a borrowed value but is in the exit set", l.raw()),
            );
        }
    }
}

/// The borrow walk: no `borrow` value is live across a write to a place
/// overlapping its own (a `Store`, a `Drop`, an `IsUnique`, a `move` read,
/// a mutable `Ref`) or across a suspension point. "Live" is a backward
/// liveness walk over the structured control flow of each sub-body, one
/// bit per borrow, with the states of `Break` and `Continue` targets and
/// match arms kept while their targets are walked, and each loop run to a
/// fixed point. A use of a value is a use of every borrow it lives on: a
/// `borrow` read rooted at a borrow, and a read of a binding local, whose
/// store holds a borrow, live on that borrow. A control instruction reads
/// its operands before it runs its blocks, so a match's scrutinee is not
/// live in its arms unless they use it. Before an instruction runs its
/// blocks, no borrow they make has been made: a path that skips the block
/// (an `Or`'s right operand, an `If` without an `else`) reaches a read of
/// the binding local that holds it only where that local was never stored,
/// which definite initialization rules out, so the walk keeps such a borrow
/// live from no point before the instruction.
fn borrow_spans(cx: &Cx<'_>, own: &dyn OwnCx, errs: &mut Vec<VerifyError>) {
    let b = cx.b;
    let n = b.len();
    let mut bit = vec![NONE; n];
    let mut at = Vec::new();
    for (i, x) in bit.iter_mut().enumerate() {
        if b.is_borrow(u32_of(i)) {
            *x = u32_of(at.len());
            at.push(u32_of(i));
        }
    }
    if at.is_empty() {
        return;
    }
    // Calls that may write, and the parameters exclusive mode reads behind.
    let may: Vec<bool> = (0..n)
        .map(|i| match b.tags[i] {
            Tag::Call => {
                super::Callee::from_words(b.record(b.data[i][0])).is_none_or(|c| own.may_write(&c))
            }
            Tag::CallValue | Tag::CallDyn | Tag::CallHost | Tag::DefaultCall | Tag::Hook => true,
            Tag::Intrinsic => {
                super::IntrinsicOp::from_u32(b.data[i][0]) == Some(super::IntrinsicOp::Item)
            }
            _ => false,
        })
        .collect();
    let mut excl = vec![false; b.local_ty.len()];
    if b.sub_flags
        .first()
        .is_some_and(|f| f & super::sub_flags::EXCLUSIVE != 0)
    {
        let params = crate::own::params(b);
        for (k, &l) in params.iter().enumerate() {
            excl[l as usize] = own.local_ref_param(b.item, k, params.len());
        }
    }
    // The borrow each binding local holds.
    let mut holds = vec![NONE; b.local_ty.len()];
    for i in 0..n {
        if b.tags[i] == Tag::Store
            && b.loan_value(b.data[i][1]).is_some()
            && let Some(Place {
                root: PlaceRoot::Local(l),
                proj,
            }) = &cx.places[i]
            && proj.is_empty()
        {
            holds[l.idx()] = b.data[i][1];
        }
    }
    // Each borrow, by bit, at every instruction that runs a block it is in.
    let mut inside = vec![Vec::new(); n];
    for (k, &r) in at.iter().enumerate() {
        let mut blk = cx.topo.blk[r as usize];
        while blk != NONE {
            let owner = cx.topo.owner[blk as usize];
            if owner == NONE {
                break;
            }
            inside[owner as usize].push(u32_of(k));
            blk = cx.topo.blk[owner as usize];
        }
    }
    // Per local: a parameter the body never stores, whose count the
    // frame holds across every suspension point. A store through the
    // parameter's reference (`[*$0 .text]`) writes what it refers to, not
    // the parameter, as the borrow pass counts (`own::borrow`'s stores).
    let by_ref: Vec<bool> = b
        .local_ty
        .iter()
        .map(|&t| {
            own.pool()
                .is_some_and(|p| matches!(p.get(t), TyData::Ref { .. }))
        })
        .collect();
    let mut frame: Vec<bool> = b
        .local_flags
        .iter()
        .map(|&f| f & local_flags::PARAM != 0)
        .collect();
    for i in 0..n {
        if b.tags[i] == Tag::Store
            && let Some(l) = b.local_in_place(b.data[i][0])
        {
            frame[l.idx()] = false;
        }
    }
    // A frame local's references live in this body: each use of what may
    // point into one may write it, there and nowhere else.
    let mut frame_uses = Vec::new();
    crate::own::frame_reference_uses(b, cx.topo, own, &mut frame_uses);
    // A reference into what a binding holds holds no count of it, so each use
    // of what may point there is a use of the borrow the binding holds.
    let mut root_uses = Vec::new();
    crate::own::frame::roots(b, own, |i, l| {
        if holds[l as usize] != NONE && !matches!(b.tags[i as usize], Tag::Drop | Tag::LoanEnd) {
            root_uses.push((i, l));
        }
    });
    root_uses.sort_stable();
    root_uses.dedup();
    let words = (0..n)
        .map(|r| {
            b.tags[r] == Tag::Read
                && own.trait_value(b.ty[r])
                && matches!(&cx.places[r], Some(p) if p.proj == [Proj::Deref])
        })
        .collect();
    let mut capture_drops = crate::own::capture_env::ReferenceDrops::default();
    capture_drops.build(b, own, cx.topo);
    let domains = borrow_domains(cx, &bit, &holds, &root_uses, at.len());
    let number = vec![NONE; at.len()];
    let mut walk = Live {
        record: true,
        cx,
        words,
        frame_uses,
        root_uses,
        capture_drops,
        may,
        excl,
        frame,
        by_ref,
        bit,
        at,
        holds,
        inside,
        number,
        domains,
        domain: 0,
        pending: Vec::new(),
        touched: Vec::new(),
        seen: vec![false; n],
        reported: vec![false; n],
        errs: Vec::new(),
    };
    let mut back = Flow::default();
    for (sub, &root) in b.sub_root.iter().enumerate() {
        walk.domain = sub;
        back.arena.words = walk.domains[sub].len().div_ceil(64);
        for (local, &global) in walk.domains[sub].iter().enumerate() {
            walk.number[global] = u32_of(local);
        }
        backward_blocks(b, cx.topo, &mut back, &mut walk, &[root]);
        for &global in &walk.domains[sub] {
            walk.number[global] = NONE;
        }
    }
    errs.append(&mut walk.errs);
}

/// One node of the borrow-dependency graph, shared by domain discovery
/// and the backward transfer. A parameter join contributes its leaf reads.
fn expand_borrow(
    cx: &Cx<'_>,
    bit: &[u32],
    holds: &[u32],
    x: u32,
    pending: &mut Vec<u32>,
    mut add: impl FnMut(u32),
) {
    let b = cx.b;
    let i = x as usize;
    if bit[i] != NONE {
        add(bit[i]);
    }
    if let Some(&local) = cx.reference_roots.get(i)
        && local != NONE
        && holds[local as usize] != NONE
    {
        pending.push(holds[local as usize]);
    } else if let Some(alias) = b.borrowed_alias(x) {
        pending.push(alias);
    } else if let Some(reads) = b.parameter_join_reads(x)
        && reads.iter().all(|&r| b.is_borrow(r))
    {
        for r in reads {
            if bit[r as usize] != NONE {
                add(bit[r as usize]);
            }
        }
    } else if bit[i] != NONE {
        let root = cx.places[i].as_ref().map(|p| p.root);
        if let Some(PlaceRoot::Value(value)) = root {
            pending.push(value.0);
        } else if let Some(PlaceRoot::Local(local)) = root
            && holds[local.idx()] != NONE
        {
            pending.push(holds[local.idx()]);
        }
    }
}

/// A sub-body needs its own borrows and the dependencies its uses can
/// reach through captured bindings. Preserve those foreign facts too;
/// partitioning by definition alone would lose their conflict checks.
fn borrow_domains(
    cx: &Cx<'_>,
    bit: &[u32],
    holds: &[u32],
    root_uses: &[(u32, u32)],
    facts: usize,
) -> Vec<Vec<usize>> {
    let b = cx.b;
    let mut seeds = vec![Vec::new(); b.sub_root.len()];
    for i in 0..b.len() {
        charge(1);
        let Some(row) = seeds.get_mut(cx.topo.sub[i] as usize) else {
            continue;
        };
        row.push(u32_of(i));
        super::slots(b, i, |_, value, word| {
            charge(1);
            if matches!(word, super::Word::Value(_)) {
                row.push(value);
            }
        });
        if let Some(Place {
            root: PlaceRoot::Local(local),
            ..
        }) = &cx.places[i]
            && holds[local.idx()] != NONE
        {
            row.push(holds[local.idx()]);
        }
    }
    for &(instruction, local) in root_uses {
        if let Some(row) = seeds.get_mut(cx.topo.sub[instruction as usize] as usize) {
            row.push(holds[local as usize]);
        }
    }
    let mut seen = vec![NONE; b.len()];
    let mut present = vec![NONE; facts];
    let mut domains = vec![Vec::new(); b.sub_root.len()];
    for (sub, mut pending) in seeds.into_iter().enumerate() {
        let stamp = u32_of(sub);
        let row = &mut domains[sub];
        let mut add = |fact: u32| {
            if fact != NONE && present[fact as usize] != stamp {
                present[fact as usize] = stamp;
                row.push(fact as usize);
            }
        };
        while let Some(x) = pending.pop() {
            let i = x as usize;
            if i >= b.len() || seen[i] == stamp {
                continue;
            }
            charge(1);
            seen[i] = stamp;
            expand_borrow(cx, bit, holds, x, &mut pending, &mut add);
        }
        row.sort_stable();
    }
    domains
}

/// The borrow walk's state ([`borrow_spans`]).
struct Live<'a> {
    record: bool,
    cx: &'a Cx<'a>,
    /// Per borrow: a trait value read through a reference, which is that
    /// reference's own words. Its object is held by the reference's count,
    /// so no call can change it.
    words: Vec<bool>,
    /// Each use of what may point into a frame local, as (instruction,
    /// local), by instruction.
    frame_uses: Vec<(u32, u32)>,
    /// Each use of what may point into what a binding local holds, as
    /// (instruction, local), by instruction.
    root_uses: Vec<(u32, u32)>,
    capture_drops: crate::own::capture_env::ReferenceDrops,
    /// Per instruction: a call that may write. Per local: a parameter
    /// whose referent exclusive mode lets a borrow live across such calls.
    may: Vec<bool>,
    excl: Vec<bool>,
    /// Per local: a parameter the body never stores, which a borrow of all
    /// or a field path of may live across a suspension point.
    frame: Vec<bool>,
    /// Per local: one that holds a reference. A borrow of it whole is that
    /// reference, and one through it is of what it refers to: neither is
    /// of a frame slot.
    by_ref: Vec<bool>,
    /// Each borrow's bit, or `NONE`, and each bit's borrow.
    bit: Vec<u32>,
    at: Vec<u32>,
    holds: Vec<u32>,
    /// Per instruction, the bits of the borrows inside the blocks it runs.
    inside: Vec<Vec<u32>>,
    domains: Vec<Vec<usize>>,
    number: Vec<u32>,
    domain: usize,
    pending: Vec<u32>,
    touched: Vec<u32>,
    seen: Vec<bool>,
    reported: Vec<bool>,
    errs: Vec<VerifyError>,
}

/// The parameter whose referent place `p` lies in: `[*$p ...]`, or
/// through a whole read of `$p`.
fn behind_param(cx: &Cx<'_>, p: &Place) -> Option<u32> {
    let b = cx.b;
    if p.proj.first() != Some(&Proj::Deref) {
        return None;
    }
    let l = match p.root {
        PlaceRoot::Local(l) => l,
        PlaceRoot::Value(v) if b.tags.get(v.0 as usize) == Some(&Tag::Read) => {
            match &cx.places[v.0 as usize] {
                Some(Place {
                    root: PlaceRoot::Local(l),
                    proj,
                }) if proj.is_empty() => *l,
                _ => return None,
            }
        }
        _ => return None,
    };
    (b.local_flags[l.idx()] & local_flags::PARAM != 0).then(|| l.raw())
}

impl Live<'_> {
    fn mark(&self, arena: &mut BitArena, state: usize, global: u32, on: bool) {
        let local = self.number[global as usize];
        // A clear of a foreign block's borrow is immaterial when that
        // fact is absent from this sub-body's dependency domain.
        if local != NONE {
            arena.set(state, local, on);
        }
    }

    /// An error at `x` for the first live borrow whose place `hit` names.
    fn check(&mut self, x: u32, s: &BitArena, live: usize, hit: impl Fn(u32, &Place) -> bool) {
        if !self.record || self.reported[x as usize] {
            return;
        }
        for k in set_indices(s.row(live)) {
            let r = self.at[self.domains[self.domain][k]];
            if r == x {
                continue;
            }
            let Some(p) = &self.cx.places[r as usize] else {
                continue;
            };
            if hit(r, p) {
                self.reported[x as usize] = true;
                let what = format!(
                    "a borrow of {p} lives across `{}` at %{x}",
                    self.cx.b.tags[x as usize].name()
                );
                err(&mut self.errs, r, what);
                return;
            }
        }
    }
}

impl Backward for Live<'_> {
    fn recording(&mut self, record: bool) {
        self.record = record;
    }
    fn stmt(&mut self, b: &Body, x: u32, s: &mut BitArena, live: usize) {
        let i = x as usize;
        let w = b.data[i][1];
        if self.bit[i] != NONE {
            self.mark(s, live, self.bit[i], false);
        }
        let writes = match b.tags[i] {
            // A provider reference's `Drop` releases nothing: the provider
            // outlives the body (`Body::provider_ref`).
            Tag::Drop => {
                !matches!(&self.cx.places[i], Some(Place { root: PlaceRoot::Value(v), proj })
                if proj.is_empty() && b.provider_ref(v.0 as usize))
            }
            Tag::Store => true,
            // A fresh or repeated buffer's check runs nowhere: the buffer
            // a borrow names stays.
            Tag::IsUnique => !matches!(w, super::unique_word::FRESH | super::unique_word::REPEAT),
            Tag::Read => w == ReadMode::Move as u32,
            Tag::Ref => w & super::ref_word::MUT != 0,
            _ => false,
        };
        // A write through a borrow's own value (into the buffer its list
        // header names, or the referent its reference names) keeps that
        // value as it is.
        if writes && let Some(q) = &self.cx.places[i] {
            let words = std::mem::take(&mut self.words);
            let capture_drop = self.capture_drops.at(i);
            self.check(x, s, live, |r, p| {
                q.root != PlaceRoot::Value(Ref(r))
                    && !(capture_drop && q.root == p.root && p.proj.first() == Some(&Proj::Deref))
                    && if words[r as usize] {
                        let held = Place {
                            root: p.root,
                            proj: Vec::new(),
                        };
                        self.cx.overlap(&held, q)
                    } else {
                        self.cx.overlap(p, q)
                    }
            });
            self.words = words;
        }
        let k = self.frame_uses.partition_point(|u| u.0 < x);
        for u in k..self.frame_uses.len() {
            let (at, l) = self.frame_uses[u];
            if at != x {
                break;
            }
            self.check(
                x,
                s,
                live,
                |_, p| matches!(p.root, PlaceRoot::Local(m) if m.raw() == l),
            );
        }
        if self.may[i] {
            let mut ops = Vec::new();
            operands(b, i, &mut ops);
            let received: Vec<u32> = ops
                .iter()
                .filter_map(|o| match &self.cx.places[o.value as usize] {
                    Some(Place {
                        root: PlaceRoot::Local(l),
                        proj,
                    }) if b.tags[o.value as usize] == Tag::Read && proj.is_empty() => Some(l.raw()),
                    _ => None,
                })
                .collect();
            let (cx, excl) = (self.cx, std::mem::take(&mut self.excl));
            let words = std::mem::take(&mut self.words);
            self.check(x, s, live, |r, p| {
                !words[r as usize]
                    && behind_param(cx, p)
                        .is_some_and(|l| !excl[l as usize] || received.contains(&l))
            });
            self.excl = excl;
            self.words = words;
        }
    }

    /// Before `x` runs its blocks, none of the borrows in them is made.
    fn ran(&mut self, _: &Body, x: u32, s: &mut BitArena, live: usize) {
        for &k in &self.inside[x as usize] {
            self.mark(s, live, k, false);
        }
    }

    /// Besides the operands (a borrow's own `Drop`, which the walk skips,
    /// releases only a value an instance made fresh, which lives on
    /// nothing): the borrow a binding local holds, and a suspension point.
    fn reads(&mut self, b: &Body, x: u32, s: &mut BitArena, live: usize) {
        let i = x as usize;
        // A read through a binding local reads the borrow it holds.
        if let Some(Place {
            root: PlaceRoot::Local(l),
            ..
        }) = &self.cx.places[i]
            && self.holds[l.idx()] != NONE
        {
            self.uses(b, self.holds[l.idx()], s, live);
        }
        let k = self.root_uses.partition_point(|u| u.0 < x);
        for u in k..self.root_uses.len() {
            let (at, l) = self.root_uses[u];
            if at != x {
                break;
            }
            self.uses(b, self.holds[l as usize], s, live);
        }
        if is_suspension(b.tags[i]) {
            let frame = std::mem::take(&mut self.frame);
            let words = std::mem::take(&mut self.words);
            let by_ref = std::mem::take(&mut self.by_ref);
            self.check(x, s, live, |r, p| {
                let PlaceRoot::Local(l) = p.root else {
                    return true;
                };
                let l = l.idx();
                let fields = p.proj.iter().all(|s| matches!(s, Proj::Field(_)));
                // Through a reference, or the reference a binding holds.
                let referent = by_ref[l] && matches!(p.proj.first(), None | Some(Proj::Deref));
                !(referent || frame[l] && (words[r as usize] || fields))
            });
            self.frame = frame;
            self.words = words;
            self.by_ref = by_ref;
        }
    }

    /// A use of value `v`: it, and every borrow it lives on. A coercion
    /// of a borrow is that borrow ([`Body::is_borrowed_coercion`]).
    fn uses(&mut self, b: &Body, v: u32, s: &mut BitArena, live: usize) {
        let mut pending = std::mem::take(&mut self.pending);
        let mut touched = std::mem::take(&mut self.touched);
        pending.push(v);
        while let Some(value) = pending.pop() {
            let i = value as usize;
            if i >= b.len() || self.seen[i] {
                continue;
            }
            charge(1);
            self.seen[i] = true;
            touched.push(value);
            expand_borrow(
                self.cx,
                &self.bit,
                &self.holds,
                value,
                &mut pending,
                |global| self.mark(s, live, global, true),
            );
        }
        for value in touched.drain(..) {
            self.seen[value as usize] = false;
        }
        self.pending = pending;
        self.touched = touched;
    }
}

/// Set bits only, charging the actual domain words and live facts visited.
fn set_indices(words: &[u64]) -> impl Iterator<Item = usize> + '_ {
    words.iter().enumerate().flat_map(|(word, &bits)| {
        charge(1);
        let mut bits = bits;
        std::iter::from_fn(move || {
            if bits == 0 {
                return None;
            }
            charge(1);
            let bit = bits.trailing_zeros() as usize;
            bits &= bits - 1;
            Some(word * 64 + bit)
        })
    })
}

#[derive(Clone, Default)]
struct BalanceDomain {
    values: Vec<u32>,
    locals: Vec<usize>,
    /// Original value/local key and its sub-body-specific part key.
    keys: Vec<(u32, u32)>,
    parts: usize,
}

/// The one-consumer walk: a forward walk over the structured control flow
/// of each sub-body, carrying three bit sets: the owned values that may
/// still hold their count, the values that may have met their consumer or
/// their `Drop`, and the owned locals that may hold a value. Joins are
/// unions ("on any path"); a loop runs to a fixed point.
///
/// A value used after its consumer or its drop, or dropped twice, on some
/// path, is an error; so is a value still owned where its sub-body ends or
/// where a loop that computes it goes round again, and a local that may
/// hold a value at the exit outside the exit set. A `Drop` of a value that
/// a path did not compute drops an empty place, which does nothing.
struct Balance<'a> {
    b: &'a Body,
    topo: &'a Topology,
    cx: &'a dyn OwnCx,
    neutral_stores: &'a [bool],
    /// Each tracked value's number among the tracked values, or `NONE`.
    num: Vec<u32>,
    domains: Vec<BalanceDomain>,
    domain: usize,
    local_num: Vec<u32>,
    part_keys: Vec<u32>,
    part_num: Vec<u32>,
    /// Widths of this sub-body's value, local and part domains.
    words: usize,
    lwords: usize,
    /// The parts of values and locals that `move` reads and `Drop`s take,
    /// keyed by value, then by `n + local`. The walk keeps a bit per slot
    /// after the locals' bits, so a value taken in parts is owned until
    /// its parts cover every counted leaf.
    slots: Slots,
    pwords: usize,
    empty_variants: std::cell::OnceCell<crate::own::parts::EmptyVariants>,
    /// The exit set of the sub-body being walked, by local, and the locals
    /// the passes own: a counted type's, or one a cell may hold.
    exit: Vec<bool>,
    owned: Vec<bool>,
    ops: Vec<crate::own::Operand>,
    reported: Vec<bool>,
    errs: Vec<VerifyError>,
}

impl<'a> Balance<'a> {
    fn new(b: &'a Body, topo: &'a Topology, cx: &'a dyn OwnCx, neutral_stores: &'a [bool]) -> Self {
        let n = b.len();
        let num = vec![NONE; n];
        let mut tracked_values = vec![false; n];
        let mut domains = vec![BalanceDomain::default(); b.sub_root.len()];
        for (i, tracked_value) in tracked_values.iter_mut().enumerate() {
            charge(1);
            *tracked_value = tracked(b, cx, i);
            if *tracked_value && let Some(domain) = domains.get_mut(topo.sub[i] as usize) {
                domain.values.push(u32_of(i));
            }
        }
        // Include referenced foreign values too: a verifier must preserve
        // their old checks even on a malformed cross-sub-body operand.
        for i in 0..n {
            charge(1);
            if let Some(domain) = domains.get_mut(topo.sub[i] as usize) {
                super::slots(b, i, |_, value, word| {
                    charge(1);
                    if matches!(word, super::Word::Value(_))
                        && tracked_values.get(value as usize).copied().unwrap_or(false)
                    {
                        domain.values.push(value);
                    }
                });
            }
        }
        for domain in &mut domains {
            domain.values.sort_stable();
            domain.values.dedup();
        }
        let mut held: Vec<bool> = b.local_ty.iter().map(|t| cx.counted(*t)).collect();
        for i in 0..n {
            let a = b.data[i][0];
            // A reference to the local's own storage puts it in a cell.
            if b.tags[i] == Tag::Ref
                && let Some(x) = b.local_in_place(a).and_then(|l| held.get_mut(l.idx()))
            {
                *x = true;
            }
        }
        for (l, m) in b.cap_local.iter().zip(&b.cap_mode) {
            if m.shares()
                && let Some(x) = held.get_mut(l.idx())
            {
                *x = true;
            }
        }
        let mut keys = hd_base::HashMap::default();
        let takes: Vec<_> = crate::own::parts::takes(b, |_| true)
            .filter_map(|(i, a)| {
                let p = b.view(a);
                let root = match p.root {
                    _ if p.whole() => return None,
                    Root::Value(v) => v,
                    Root::Local(l) => u32_of(n) + l,
                    _ => return None,
                };
                let sub = topo.sub[i] as usize;
                let domain = domains.get_mut(sub)?;
                let count = u32_of(keys.len());
                let key = *keys.entry((sub, root)).or_insert_with(|| {
                    domain.keys.push((root, count));
                    count
                });
                Some((key, u32_of(i), a))
            })
            .collect();
        let mut slots = Slots::default();
        slots.build(b, n, keys.len(), takes.into_iter());
        let mut part_num = vec![NONE; slots.word.len()];
        for domain in &mut domains {
            for &(_, key) in &domain.keys {
                for slot in slots.of_key(key) {
                    part_num[slot as usize] = u32_of(domain.parts);
                    domain.parts += 1;
                }
            }
        }
        Balance {
            b,
            topo,
            cx,
            neutral_stores,
            num,
            domains,
            domain: 0,
            local_num: vec![NONE; b.local_ty.len()],
            part_keys: vec![NONE; n + b.local_ty.len()],
            part_num,
            words: 0,
            lwords: 0,
            slots,
            pwords: 0,
            empty_variants: std::cell::OnceCell::new(),
            exit: vec![false; b.local_ty.len()],
            owned: held,
            ops: Vec::new(),
            reported: vec![false; n],
            errs: Vec::new(),
        }
    }

    fn walk(self, errs: &mut Vec<VerifyError>) {
        self.walk_counted(errs, || charge(1));
    }

    fn walk_counted(mut self, errs: &mut Vec<VerifyError>, mut work: impl FnMut()) {
        let (b, topo) = (self.b, self.topo);
        let locals = SubBodyLocals::new(b, &mut work);
        for (sub, domain) in self.domains.iter_mut().enumerate() {
            domain.locals.extend_from_slice(&locals.entry[sub]);
            domain.locals.extend_from_slice(&locals.exit[sub]);
        }
        for i in 0..b.len() {
            work();
            if b.tags[i].operands().0 == Op::Place
                && let Some(local) = b.view(b.data[i][0]).local()
                && let Some(domain) = self.domains.get_mut(topo.sub[i] as usize)
            {
                domain.locals.push(local as usize);
            }
        }
        for domain in &mut self.domains {
            domain.locals.sort_stable();
            domain.locals.dedup();
        }
        let mut s = Flow::default();
        for (sub, &root) in b.sub_root.iter().enumerate() {
            work();
            if root as usize >= b.len() {
                continue;
            }
            self.domain = sub;
            let domain = &self.domains[sub];
            self.words = domain.values.len().div_ceil(64);
            self.lwords = domain.locals.len().div_ceil(64);
            self.pwords = domain.parts.div_ceil(64);
            s.arena.words = 3 * self.words + self.lwords + self.pwords;
            for (number, &value) in domain.values.iter().enumerate() {
                self.num[value as usize] = u32_of(number);
            }
            for (number, &local) in domain.locals.iter().enumerate() {
                self.local_num[local] = u32_of(number);
            }
            for &(root, key) in &domain.keys {
                self.part_keys[root as usize] = key;
            }
            for &l in &locals.exit[sub] {
                work();
                if let Some(x) = self.exit.get_mut(l) {
                    *x = true;
                }
            }
            let st = s.arena.alloc(None);
            // An entry local outside the exit set is a lent parameter the
            // body does not own.
            for &l in &locals.entry[sub] {
                work();
                if self.exit[l] {
                    self.set_full(s.arena.row_mut(st), l, true);
                }
            }
            if forward(b, topo, &mut s, &mut self, root, st) {
                self.tail(b, root, &mut s.arena, st);
                self.at_exit(root, s.arena.row(st));
            }
            s.arena.free(st);
            // Clear only this sub-body's rows, including duplicate rows
            // on malformed input; never revisit unrelated locals.
            for &l in &locals.exit[sub] {
                work();
                if let Some(x) = self.exit.get_mut(l) {
                    *x = false;
                }
            }
            for &value in &self.domains[sub].values {
                self.num[value as usize] = NONE;
            }
            for &local in &self.domains[sub].locals {
                self.local_num[local] = NONE;
            }
            for &(root, _) in &self.domains[sub].keys {
                self.part_keys[root as usize] = NONE;
            }
        }
        errs.append(&mut self.errs);
    }

    fn err(&mut self, inst: u32, what: String) {
        if inst != NONE && (inst as usize) < self.reported.len() {
            if self.reported[inst as usize] {
                return;
            }
            self.reported[inst as usize] = true;
        }
        err(&mut self.errs, inst, what);
    }

    fn bit(&self, st: &Set, set: usize, k: u32) -> bool {
        get_bit(st, set * self.words * 64 + k as usize)
    }

    fn put(&self, st: &mut Set, set: usize, k: u32, on: bool) {
        set_bit(st, set * self.words * 64 + k as usize, on);
    }

    fn set_full(&self, st: &mut Set, l: usize, on: bool) {
        if self.owned.get(l).copied().unwrap_or(false) {
            let number = self.local_num[l];
            if number != NONE {
                set_bit(st, 3 * self.words * 64 + number as usize, on);
            }
        }
    }

    /// Value `v` used at `x`: an error after its consumer or drop; gone
    /// when the use consumes it.
    fn used(&mut self, x: u32, v: u32, consumed: bool, st: &mut Set) {
        let k = self.num[v as usize];
        if k == NONE {
            return;
        }
        if self.bit(st, 1, k) {
            self.err(x, format!("%{x} uses %{v} after its consumer or its drop"));
        }
        if consumed {
            self.put(st, 0, k, false);
            self.put(st, 1, k, true);
        }
    }

    /// The uses of instruction `x`'s operands, its effect on locals, then
    /// its own value.
    ///
    /// A `move` read of a whole value `[%v]` consumes it. A `move` read
    /// or a `Drop` of a part of a value (`[%v as .1 .0]`) takes that part:
    /// the value is then **taken in parts** (set 2), so further parts may
    /// go, another part may be read, and a `Drop` of the whole may release
    /// what is left, but nothing else may use it.
    fn uses(&mut self, x: u32, st: &mut Set) {
        let b = self.b;
        let [a, w] = b.data[x as usize];
        // The value at the root of a move read's or a drop's place, and
        // whether the place is a part of it.
        let p = b.view(a);
        let taken = match b.tags[x as usize] {
            Tag::Read if w == ReadMode::Move as u32 => p.value(),
            Tag::Drop => p.value().filter(|_| !p.whole()),
            _ => None,
        }
        .map(|v| (v, !p.whole()));
        let mut ops = std::mem::take(&mut self.ops);
        operands(b, x as usize, &mut ops);
        for op in &ops {
            if let Some((v, part)) = taken
                && op.value == v
                && op.at == crate::own::Loc::Extra(a + 2)
            {
                if part {
                    // The parts taken so far leave the rest owned, unless
                    // they cover every counted leaf.
                    self.part(x, v, st);
                } else {
                    self.used(x, v, true, st);
                }
                continue;
            }
            // A read of a part that the parts taken so far leave.
            if b.tags[x as usize] == Tag::Read
                && !p.whole()
                && p.value() == Some(op.value)
                && op.at == crate::own::Loc::Extra(a + 2)
                && self.part_left(op.value, a, st)
            {
                continue;
            }
            match op.role {
                Role::Dropped => {
                    let k = self.num[op.value as usize];
                    if k != NONE {
                        if self.bit(st, 1, k) && !self.bit(st, 2, k) {
                            self.err(x, format!("%{x} drops %{} twice", op.value));
                        }
                        self.put(st, 0, k, false);
                        self.put(st, 1, k, true);
                        self.put(st, 2, k, false);
                    }
                }
                Role::Tail => {}
                _ => {
                    let consumed = consumes(b, self.cx, x as usize, op, NONE);
                    self.used(x, op.value, consumed, st);
                }
            }
        }
        self.ops = ops;
        let local = |word: u32| b.view(word).local().map(|l| l as usize);
        match b.tags[x as usize] {
            // A local a borrow is stored into holds no count. A store
            // refills what parts were taken.
            Tag::Store
                if b.loan_value(w).is_none()
                    && (b.view(a).whole() || !self.neutral_stores[x as usize]) =>
            {
                // A trivial field write introduces no ownership and does
                // not refill a moved counted field beside it.
                // Into the local itself: one through it, into what it
                // refers to, leaves it as it was.
                if let Some(l) = b.local_in_place(a).map(hd_base::LocalId::idx) {
                    self.set_full(st, l, true);
                    for s in self.part_slots(u32_of(b.len() + l)) {
                        self.put_slot(st, s, false);
                    }
                }
            }
            // A `move` read or a `Drop` of a part of a local takes it.
            Tag::Read | Tag::Drop
                if (b.tags[x as usize] == Tag::Drop || w == ReadMode::Move as u32)
                    && !b.view(a).whole()
                    && let Some(l) = local(a) =>
            {
                self.local_part(x, l, st);
            }
            Tag::LoanEnd if a & place::BARE_LOCAL != 0 => {
                self.set_full(st, (a & !place::BARE_LOCAL) as usize, false);
            }
            // A whole local, bare or through a place word (an inlined
            // closure's read of its capture), drops or moves out all of it.
            Tag::Read | Tag::Drop
                if (b.tags[x as usize] == Tag::Drop || w == ReadMode::Move as u32)
                    && (a & place::BARE_LOCAL != 0 || b.view(a).whole())
                    && let Some(l) = local(a) =>
            {
                self.set_full(st, l, false);
            }
            Tag::Ref if w & super::ref_word::TAKES != 0 => {
                if let Some(l) = local(a) {
                    self.set_full(st, l, false);
                }
            }
            _ => {}
        }
        if is_suspension(b.tags[x as usize]) {
            self.at_point(x, st);
        }
        let k = self.num[x as usize];
        if k != NONE {
            self.put(st, 0, k, true);
            self.put(st, 1, k, false);
            self.put(st, 2, k, false);
            // A value computed again has none of its parts taken.
            for s in self.part_slots(x) {
                self.put_slot(st, s, false);
            }
        }
    }

    /// Whether value `v` is taken in parts and none of them overlaps the
    /// place at `word`.
    fn part_left(&self, v: u32, word: u32, st: &Set) -> bool {
        let k = self.num[v as usize];
        if k == NONE || !self.bit(st, 1, k) || !self.bit(st, 2, k) {
            return false;
        }
        let base = (3 * self.words + self.lwords) * 64;
        let b = self.b;
        let steps = |w: u32| {
            let view = b.view(w);
            &b.extra[view.from as usize..view.to as usize]
        };
        self.part_slots(v).all(|s| {
            !get_bit(st, base + self.part_num[s as usize] as usize)
                || !steps(self.slots.word[s as usize])
                    .iter()
                    .zip(steps(word))
                    .all(|(x, y)| x == y)
        })
    }

    fn part_slots(&self, root: u32) -> std::ops::Range<u32> {
        let key = self.part_keys[root as usize];
        if key == NONE {
            0..0
        } else {
            self.slots.of_key(key)
        }
    }

    fn put_slot(&self, st: &mut Set, s: u32, on: bool) {
        let bit = self.part_num[s as usize] as usize;
        set_bit(st, (3 * self.words + self.lwords) * 64 + bit, on);
    }

    /// Instruction `x` takes a part of value `v`: an error after its
    /// consumer or its drop, unless only parts went before. The value stays
    /// owned while the parts taken on the path leave a counted leaf.
    fn part(&mut self, x: u32, v: u32, st: &mut Set) {
        let k = self.num[v as usize];
        if k == NONE {
            return;
        }
        if self.bit(st, 1, k) && !self.bit(st, 2, k) {
            self.err(
                x,
                format!("%{x} takes a part of %{v} after its consumer or its drop"),
            );
        }
        let s = self.slots.of[x as usize];
        if s != NONE {
            self.put_slot(st, s, true);
        }
        let rest = self.rest(v, self.b.ty[v as usize], st);
        self.put(st, 0, k, rest);
        self.put(st, 1, k, true);
        self.put(st, 2, k, true);
    }

    /// Instruction `x` takes a part of local `l`: the local is empty once
    /// the parts taken on the path cover every counted leaf.
    fn local_part(&mut self, x: u32, l: usize, st: &mut Set) {
        let s = self.slots.of[x as usize];
        if s == NONE {
            return;
        }
        self.put_slot(st, s, true);
        if !self.rest(u32_of(self.b.len() + l), self.b.local_ty[l], st) {
            self.set_full(st, l, false);
        }
    }

    /// Whether the value or local at slot key `key`, of type `ty`, still
    /// holds a counted leaf beside the parts taken on the path. A taken
    /// place that does not decode leaves one.
    fn rest(&mut self, key: u32, ty: hd_types::Ty, st: &Set) -> bool {
        let (b, base) = (self.b, (3 * self.words + self.lwords) * 64);
        let mut bad = false;
        let key = self.part_keys[key as usize];
        if key == NONE {
            return self.cx.counted(ty);
        }
        let rest = self.slots.rest(b, self.cx, key, ty, |s, word| {
            let on = get_bit(st, base + self.part_num[s as usize] as usize);
            bad |= on && b.decode_place(word).is_none();
            on
        });
        rest || bad
    }

    /// Where a sub-body ends: no value owned, and every local that may
    /// hold a value in the exit set.
    fn at_exit(&mut self, x: u32, st: &Set) {
        if let Some(k) = set_indices(&st[..self.words]).next() {
            let v = self.domains[self.domain].values[k];
            self.err(
                x,
                format!("%{v} is still owned where its sub-body ends at %{x}"),
            );
        }
        let missing = self.live_locals(st).find(|&l| !self.exit[l]);
        if let Some(l) = missing {
            self.err(
                x,
                format!("local ${l} may hold a value at the exit, which does not drop it"),
            );
        }
    }

    fn live_locals<'s>(&'s self, st: &'s Set) -> impl Iterator<Item = usize> + 's {
        let start = 3 * self.words;
        set_indices(&st[start..start + self.lwords])
            .map(|bit| self.domains[self.domain].locals[bit])
    }

    /// Suspension point `x`, whose cancellation leaves through the exit:
    /// every value owned there has a `Release` in its cancel block, and
    /// every local that may hold a value is in the exit set.
    fn at_point(&mut self, x: u32, st: &Set) {
        let b = self.b;
        let blk = b
            .cancel_block(x)
            .filter(|&c| b.tags.get(c as usize) == Some(&Tag::Block));
        let list = blk.map_or(&[][..], |c| b.record(b.data[c as usize][0]));
        let mut released: Vec<u32> = list
            .iter()
            .filter_map(|&s| b.data.get(s as usize).map(|d| d[0]))
            .collect();
        released.sort_stable();
        for position in 0..self.domains[self.domain].values.len() {
            charge(1);
            let v = self.domains[self.domain].values[position] as usize;
            let k = self.num[v];
            if k != NONE && self.bit(st, 0, k) && released.binary_search(&u32_of(v)).is_err() {
                let what =
                    format!("%{v} is owned at suspension point %{x}, whose cancel block lacks it");
                self.err(x, what);
                return;
            }
        }
        let missing = self.live_locals(st).find(|&l| !self.exit[l]);
        if let Some(l) = missing {
            self.err(x, format!("local ${l} may hold a value at suspension point %{x}, and the exit does not drop it"));
        }
    }

    /// Where loop `lp` goes round again: no value it computes still owned.
    fn round(&mut self, x: u32, lp: u32, st: &Set) {
        for position in 0..self.domains[self.domain].values.len() {
            charge(1);
            let v = self.domains[self.domain].values[position] as usize;
            let k = self.num[v];
            if !self.bit(st, 0, k) {
                continue;
            }
            let mut l = self.topo.loop_of[v];
            while l != NONE {
                if l == lp {
                    let what = format!("%{v} is still owned where loop %{lp} goes round at %{x}");
                    self.err(x, what);
                    return;
                }
                l = self.topo.loop_of[l as usize];
            }
        }
    }
}

impl Forward for Balance<'_> {
    const SUITES: Suites = Suites::FromEnd { union: false };

    fn stmt(&mut self, _: &Body, x: u32, s: &mut BitArena, st: usize) {
        self.uses(x, s.row_mut(st));
    }

    /// A point's value is not made on its cancel exit, which starts by
    /// releasing what the point holds (`own::known`'s `Release`s).
    fn cancel(&mut self, b: &Body, x: u32, s: &mut BitArena, st: usize) {
        let row = s.row_mut(st);
        let k = self.num[x as usize];
        if k != NONE {
            self.put(row, 0, k, false);
        }
        let Some(blk) = b
            .cancel_block(x)
            .filter(|&c| b.tags.get(c as usize) == Some(&Tag::Block))
        else {
            return;
        };
        for &r in b.record(b.data[blk as usize][0]) {
            if b.tags[r as usize] == Tag::Release
                && let Some(&k) = self.num.get(b.data[r as usize][0] as usize)
                && k != NONE
            {
                self.put(row, 0, k, false);
            }
        }
    }

    /// The operand uses of a control instruction, before the blocks it
    /// runs.
    fn enter(&mut self, b: &Body, x: u32, s: &mut BitArena, st: usize) {
        let mut ops = std::mem::take(&mut self.ops);
        operands(b, x as usize, &mut ops);
        for op in &ops {
            let consumed = consumes(b, self.cx, x as usize, op, NONE);
            self.used(x, op.value, consumed, s.row_mut(st));
        }
        self.ops = ops;
    }

    fn case(&mut self, b: &Body, x: u32, blk: u32, s: &mut BitArena, st: usize) {
        if b.tags[x as usize] != Tag::SwitchTag {
            return;
        }
        if let Some(v) = self
            .empty_variants
            .get_or_init(|| crate::own::parts::EmptyVariants::build(b, self.cx))
            .get(x, blk)
        {
            let k = self.num[v as usize];
            if k != NONE {
                self.put(s.row_mut(st), 0, k, false);
            }
        }
    }

    fn leave(&mut self, _: &Body, x: u32, s: &mut BitArena, st: usize) {
        let k = self.num[x as usize];
        if k != NONE {
            let st = s.row_mut(st);
            self.put(st, 0, k, true);
            self.put(st, 1, k, false);
            self.put(st, 2, k, false);
        }
    }

    /// Block `blk`'s tail, taken by the block's value when its owner uses
    /// it, at the block's end.
    fn tail(&mut self, b: &Body, blk: u32, s: &mut BitArena, st: usize) {
        let w = b.data[blk as usize][1];
        if w == NONE || Ref(w).as_inst().is_none() || w as usize >= b.len() {
            return;
        }
        let owner = if self.topo.is_root[blk as usize] {
            NONE
        } else {
            self.topo.owner[blk as usize]
        };
        let op = crate::own::Operand {
            value: w,
            role: Role::Tail,
            at: crate::own::Loc::Data(1),
        };
        let consumed = consumes(b, self.cx, blk as usize, &op, owner);
        self.used(blk, w, consumed, s.row_mut(st));
    }

    fn back(&mut self, _: &Body, x: u32, lp: u32, s: &mut BitArena, st: usize) {
        self.round(x, lp, s.row(st));
    }

    fn exit(&mut self, _: &Body, x: u32, s: &mut BitArena, st: usize) {
        self.at_exit(x, s.row(st));
    }
}

/// Sub-body metadata is indexed once, like rustc's per-MIR-body local
/// domains. hd stores several bodies together, so group their rows first.
/// Cost: O(sub-bodies + locals + parameters + instructions + captures +
/// exit rows), with each represented capture and exit row stored once.
struct SubBodyLocals {
    entry: Vec<Vec<usize>>,
    exit: Vec<Vec<usize>>,
}

impl SubBodyLocals {
    fn new(b: &Body, work: &mut impl FnMut()) -> Self {
        let mut entry: Vec<Vec<usize>> = (0..b.sub_root.len())
            .map(|_| {
                work();
                Vec::new()
            })
            .collect();
        let mut exit = vec![Vec::new(); b.sub_root.len()];
        if let Some(root) = entry.first_mut() {
            *root = root_entry_locals(b, work);
        }
        for (sub, row) in entry.iter_mut().enumerate().skip(1) {
            if sub < b.sub_params.len() {
                row.extend(b.sub_param_locals(sub).iter().map(|&local| {
                    work();
                    local as usize
                }));
            }
        }
        for i in 0..b.len() {
            work();
            if b.tags[i] != Tag::Closure || b.is_instantiation(i) {
                continue;
            }
            let sub = b.data[i][0] as usize;
            if sub == 0 {
                continue;
            }
            if let Some(row) = entry.get_mut(sub) {
                let rec = b.record(b.data[i][1]);
                for c in rec[0] as usize..(rec[0] + rec[1]) as usize {
                    work();
                    row.push(b.cap_local[c].idx());
                }
            }
        }
        for &(sub, local) in &b.exit_drops {
            work();
            if let Some(row) = exit.get_mut(sub.idx()) {
                row.push(local.idx());
            }
        }
        Self { entry, exit }
    }
}

/// Root parameters exclude all parameters belonging to local functions.
/// The other caller (binding-local verification) needs only this root row.
fn root_entry_locals(b: &Body, work: &mut impl FnMut()) -> Vec<usize> {
    let mut in_sub = vec![false; b.local_ty.len()];
    for sub in 1..b.sub_params.len() {
        for &local in b.sub_param_locals(sub) {
            work();
            if let Some(x) = in_sub.get_mut(local as usize) {
                *x = true;
            }
        }
    }
    b.local_flags
        .iter()
        .zip(&in_sub)
        .enumerate()
        .filter_map(|(l, (&flags, &sub))| {
            work();
            (flags & super::local_flags::PARAM != 0 && !sub).then_some(l)
        })
        .collect()
}

/// Whether statement `i` runs only where this instance owns parameter
/// `l`: an `If` around it tests `ParamOwned` of `l`, or a flag local
/// that starts as it (`starts`).
fn owned_guard(b: &Body, topo: &super::Topology, i: usize, l: u32, starts: &[(u32, u32)]) -> bool {
    let flag = |v: u32| {
        b.tags.get(v as usize) == Some(&Tag::Read) && starts.contains(&(b.data[v as usize][0], l))
    };
    let mut at = topo
        .owner
        .get(topo.blk[i] as usize)
        .copied()
        .unwrap_or(NONE);
    while at != NONE {
        let c = b.data[at as usize][0];
        if b.tags[at as usize] == Tag::If && (b.param_owned(c) == Some(l) || flag(c)) {
            return true;
        }
        at = topo
            .owner
            .get(topo.blk[at as usize] as usize)
            .copied()
            .unwrap_or(NONE);
    }
    false
}

#[cfg(test)]
mod deferred_tests {
    use super::*;
    use crate::ir::{BodyKind, TirBuilder, TirSink, local_flags, place, unique_word};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    /// A loop of `pushes` pushes onto a list local whose `IsUnique` is
    /// hoisted before the loop (`hoisted`), or absent.
    fn hoisted_pushes(pushes: usize, hoisted: bool) -> Result<Body, String> {
        use crate::ir::{IntrinsicOp, ReadMode};
        let mut build = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = build.open_block();
        let local = build.local(Ty::STRING, Symbol::NONE, local_flags::PARAM, NodeIdx::NONE);
        let word = local.raw() | place::BARE_LOCAL;
        if hoisted {
            let w = unique_word::HOISTED;
            build.emit(Tag::IsUnique, word, w, Ty::VOID, NodeIdx::NONE);
        }
        let lp = build.open_loop();
        let body = build.open_block();
        for _ in 0..pushes {
            let mode = ReadMode::Borrow as u32;
            let list = build.emit(Tag::Read, word, mode, Ty::STRING, NodeIdx::NONE);
            let value = build.const_value(Ty::I32, 0);
            let args = build.refs_record(&[list, value]);
            let push = IntrinsicOp::ListPush as u32;
            build.emit(Tag::Intrinsic, push, args, Ty::VOID, NodeIdx::NONE);
        }
        let body = build.close_block(body, None, Ty::VOID, NodeIdx::NONE);
        build.close_loop(lp, body, Ty::VOID, NodeIdx::NONE);
        let root = build.close_block(root, None, Ty::VOID, NodeIdx::NONE);
        build
            .finish_tolerating(root, &[], &[22])
            .map_err(|e| format!("{e:?}"))
    }

    /// Each change in a loop finds its hoisted `IsUnique` with one probe
    /// of the hoisted places, so the work grows with the changes, not
    /// with changes times instructions; without the check each is
    /// rejected.
    #[test]
    fn a_hoisted_loops_changes_are_checked_in_linear_work() -> Result<(), String> {
        let mut work = Vec::new();
        for pushes in [10, 100] {
            let body = hoisted_pushes(pushes, true)?;
            let topo = Topology::new(&body);
            let mut errors = Vec::new();
            let ((), units) = crate::own::flow::work::measure(|| {
                change_places(&body, &topo, &mut errors);
            });
            assert!(errors.is_empty(), "{errors:?}");
            work.push((units, body.len()));
        }
        // One unit per instruction for the set, one per change.
        assert_eq!(work[0].0, work[0].1 + 10, "{work:?}");
        assert_eq!(work[1].0, work[1].1 + 100, "{work:?}");
        let body = hoisted_pushes(3, false)?;
        let mut errors = Vec::new();
        change_places(&body, &Topology::new(&body), &mut errors);
        let after = "is no read of its place after its `IsUnique`";
        assert_eq!(errors.iter().filter(|e| e.what.contains(after)).count(), 3);
        Ok(())
    }

    #[test]
    fn a_deferred_cache_rejects_buffer_growth() -> Result<(), String> {
        use crate::ir::{IntrinsicOp, ReadMode};
        let mut build = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = build.open_block();
        // The ownership unit context uses string as a counted-header stand-in.
        let local = build.local(Ty::STRING, Symbol::NONE, local_flags::PARAM, NodeIdx::NONE);
        let word = local.raw() | place::BARE_LOCAL;
        build.emit(
            Tag::IsUnique,
            word,
            unique_word::DEFERRED_CACHE,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let lp = build.open_loop();
        let body = build.open_block();
        build.emit(
            Tag::IsUnique,
            word,
            unique_word::DEFERRED,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let header = build.emit(
            Tag::Read,
            word,
            ReadMode::Borrow as u32,
            Ty::STRING,
            NodeIdx::NONE,
        );
        let value = build.const_value(Ty::I32, 0);
        let args = build.refs_record(&[header, value]);
        build.emit(
            Tag::Intrinsic,
            IntrinsicOp::ListPush as u32,
            args,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let body = build.close_block(body, None, Ty::VOID, NodeIdx::NONE);
        build.close_loop(lp, body, Ty::VOID, NodeIdx::NONE);
        let root = build.close_block(root, None, Ty::VOID, NodeIdx::NONE);
        let body = build
            .finish_tolerating(root, &[], &[22])
            .map_err(|e| format!("{e:?}"))?;
        let errors = verify_owned(&body, &crate::own::tests::Cx);
        assert!(
            errors
                .iter()
                .any(|e| e.what.contains("replaces its deferred cached header")),
            "{errors:?}"
        );
        Ok(())
    }
}

#[cfg(test)]
mod forwarded_read_tests {
    use super::*;
    use crate::ir::{BodyKind, Coercion, TirBuilder, TirSink};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    fn chain() -> (Body, u32, u32) {
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let syn = NodeIdx::from_raw(0);
        let root = b.open_block();
        let local = b.local(Ty::STRING, Symbol::from_raw(0), local_flags::PARAM, syn);
        let read = b.get(local, Ty::STRING, syn);
        let first = b.coerce(Coercion::Identity, NONE, read, Ty::STRING, syn);
        let second = b.coerce(Coercion::Identity, NONE, first, Ty::STRING, syn);
        b.emit(Tag::Return, second.0, NONE, Ty::NEVER, syn);
        let root = b.close_block(root, None, Ty::NEVER, syn);
        let mut b = b.finish(root, &[]).expect("valid forwarding chain");
        b.data[read.0 as usize][1] = ReadMode::Borrow as u32;
        b.index_forwarded_reads(&[]);
        (b, second.0, read.0)
    }

    #[test]
    fn forwarded_read_metadata_rejects_stale_self_and_unsorted_rows() {
        let (b, value, read) = chain();
        assert_eq!(b.forwarded_reads, [(value, read)]);
        let mut errors = vec![];
        borrowed_wrappers(
            &b,
            &crate::own::derive::rc_sources(&b, &crate::own::tests::Cx),
            &mut errors,
        );
        assert!(errors.is_empty(), "{errors:?}");
        for rows in [
            vec![(value, value)],
            vec![(read, read)],
            vec![(value, NONE)],
            vec![(value, read), (value, read)],
            vec![(u32_of(b.len()), read)],
        ] {
            let mut malformed = b.clone();
            malformed.forwarded_reads = rows;
            let mut errors = vec![];
            borrowed_wrappers(
                &malformed,
                &crate::own::derive::rc_sources(&malformed, &crate::own::tests::Cx),
                &mut errors,
            );
            assert!(
                errors
                    .iter()
                    .any(|e| e.what.contains("forwarded Read identity"))
            );
        }
        let back = crate::ir::parse(&crate::ir::print(&b)).expect("text roundtrip");
        assert_eq!(back, b);
    }

    #[test]
    fn splicing_remaps_both_sides_of_a_forwarded_read() {
        let (mut b, value, read) = chain();
        let topo = Topology::new(&b);
        let inserts = [crate::own::splice::BatchInsert {
            block: b.sub_root[0],
            pos: 0,
            tag: Tag::Read,
            data: b.data[read as usize],
            ty: Ty::STRING,
            syn: NodeIdx::from_raw(0),
        }];
        let map = crate::own::splice::insert_batch(&mut b, &topo, &inserts)
            .expect("insert unrelated read before chain");
        assert_ne!(map[read as usize], read);
        assert_eq!(
            b.forwarded_reads,
            [(map[value as usize], map[read as usize])]
        );
        assert_eq!(
            b.borrowed_alias(map[value as usize]),
            Some(map[read as usize])
        );
        let mut errors = vec![];
        borrowed_wrappers(
            &b,
            &crate::own::derive::rc_sources(&b, &crate::own::tests::Cx),
            &mut errors,
        );
        assert!(errors.is_empty(), "{errors:?}");
    }
}

#[cfg(test)]
mod sub_body_metadata_tests {
    use super::*;
    use crate::ir::{BodyKind, TirBuilder, TirSink};
    use hd_base::{DefId, NodeIdx, SubId, Symbol};
    use hd_types::Ty;

    fn closures(n: usize) -> Result<Body, String> {
        let mut build = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = build.open_block();
        let mut exits = Vec::new();
        for sub in 1..=n {
            let captured = build.local(Ty::STRING, Symbol::NONE, local_flags::PARAM, NodeIdx::NONE);
            let parameter =
                build.local(Ty::STRING, Symbol::NONE, local_flags::PARAM, NodeIdx::NONE);
            let closure = build.open_sub(&[parameter]);
            let inner = build.open_block();
            build.capture(closure, captured);
            let inner = build.close_block(inner, None, Ty::VOID, NodeIdx::NONE);
            build.close_sub(closure, inner, Ty::I32, NodeIdx::NONE);
            exits.extend([
                (SubId::from_raw(0), captured),
                (SubId::from_raw(u32_of(sub)), captured),
                (SubId::from_raw(u32_of(sub)), parameter),
            ]);
        }
        let root = build.close_block(root, None, Ty::VOID, NodeIdx::NONE);
        let mut body = build
            .finish(root, &[])
            .map_err(|errors| format!("{errors:?}"))?;
        exits.sort_stable_by_key(|(sub, local)| (sub.raw(), local.raw()));
        body.exit_drops = exits;
        Ok(body)
    }

    #[test]
    fn entry_and_exit_rows_keep_parameters_captures_and_sub_bodies_separate() -> Result<(), String>
    {
        let body = closures(2)?;
        let index = SubBodyLocals::new(&body, &mut || {});
        assert_eq!(index.entry, [vec![0, 2], vec![1, 0], vec![3, 2]]);
        assert_eq!(index.exit, [vec![0, 2], vec![0, 1], vec![2, 3]]);
        assert_eq!(root_entry_locals(&body, &mut || {}), index.entry[0]);
        Ok(())
    }

    #[test]
    fn borrow_domains_keep_a_captured_bindings_foreign_borrow() -> Result<(), String> {
        let mut build = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = build.open_block();
        let owner = build.local(Ty::STRING, Symbol::NONE, local_flags::PARAM, NodeIdx::NONE);
        let binding = build.local(Ty::STRING, Symbol::NONE, 0, NodeIdx::NONE);
        let outside = build.emit(
            Tag::Read,
            owner.raw() | place::BARE_LOCAL,
            ReadMode::Use as u32,
            Ty::STRING,
            NodeIdx::NONE,
        );
        build.emit(
            Tag::Store,
            binding.raw() | place::BARE_LOCAL,
            outside.0,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let sub = build.open_sub(&[]);
        let inner = build.open_block();
        build.capture(sub, binding);
        let inside = build.emit(
            Tag::Read,
            binding.raw() | place::BARE_LOCAL,
            ReadMode::Use as u32,
            Ty::STRING,
            NodeIdx::NONE,
        );
        let inner = build.close_block(inner, None, Ty::VOID, NodeIdx::NONE);
        build.close_sub(sub, inner, Ty::I32, NodeIdx::NONE);
        let root = build.close_block(root, None, Ty::VOID, NodeIdx::NONE);
        let mut body = build.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
        body.data[outside.0 as usize][1] = ReadMode::Borrow as u32;
        body.data[inside.0 as usize][1] = ReadMode::Borrow as u32;
        let topo = Topology::new(&body);
        let places: Vec<_> = (0..body.len())
            .map(|i| {
                (body.tags[i].operands().0 == Op::Place)
                    .then(|| body.decode_place(body.data[i][0]))
                    .flatten()
            })
            .collect();
        let roots = vec![NONE; body.len()];
        let cx = Cx {
            b: &body,
            topo: &topo,
            places: &places,
            addressed: &[],
            reference_calls: &[],
            reference_values: &[],
            reference_roots: &roots,
            frame_lends: &[],
            neutral_stores: &[],
        };
        let mut bit = vec![NONE; body.len()];
        bit[outside.0 as usize] = 0;
        bit[inside.0 as usize] = 1;
        let mut holds = vec![NONE; body.local_ty.len()];
        holds[binding.idx()] = outside.0;
        assert_eq!(
            borrow_domains(&cx, &bit, &holds, &[], 2),
            [vec![0], vec![0, 1]]
        );
        Ok(())
    }

    fn owned_closures(width: usize, depth: usize) -> Result<Body, String> {
        use crate::ir::Providers;
        use hd_types::TyList;
        let mut build = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = build.open_block();
        for _ in 0..width {
            let mut captured =
                build.local(Ty::STRING, Symbol::NONE, local_flags::PARAM, NodeIdx::NONE);
            let mut stack = Vec::new();
            for _ in 0..depth {
                let parameter =
                    build.local(Ty::STRING, Symbol::NONE, local_flags::PARAM, NodeIdx::NONE);
                let sub = build.open_sub(&[parameter]);
                let block = build.open_block();
                build.capture(sub, captured);
                let read = build.emit(
                    Tag::Read,
                    parameter.raw() | place::BARE_LOCAL,
                    ReadMode::Use as u32,
                    Ty::STRING,
                    NodeIdx::NONE,
                );
                build.call(
                    &Callee::Item {
                        def: DefId::from_raw(8),
                        targs: TyList::EMPTY,
                    },
                    &[read],
                    Providers::None,
                    Ty::I32,
                    NodeIdx::NONE,
                );
                stack.push((sub, block, parameter));
                captured = parameter;
            }
            while let Some((sub, block, parameter)) = stack.pop() {
                let read = build.emit(
                    Tag::Read,
                    parameter.raw() | place::BARE_LOCAL,
                    ReadMode::Use as u32,
                    Ty::STRING,
                    NodeIdx::NONE,
                );
                build.call(
                    &Callee::Item {
                        def: DefId::from_raw(9),
                        targs: TyList::EMPTY,
                    },
                    &[read],
                    Providers::None,
                    Ty::VOID,
                    NodeIdx::NONE,
                );
                let block = build.close_block(block, None, Ty::VOID, NodeIdx::NONE);
                build.close_sub(sub, block, Ty::STRING, NodeIdx::NONE);
            }
        }
        let root = build.close_block(root, None, Ty::VOID, NodeIdx::NONE);
        let mut body = build
            .finish(root, &[])
            .map_err(|errors| format!("{errors:?}"))?;
        crate::own::run(
            &mut body,
            &crate::own::tests::Cx,
            &mut crate::own::OwnScratch::default(),
        )?;
        Ok(body)
    }

    #[test]
    fn whole_verifier_work_is_linear_in_closures() -> Result<(), String> {
        for depth in [1, 20] {
            let mut costs = Vec::new();
            for width in [100, 1_000] {
                let body = owned_closures(width, depth)?;
                assert!(body.tags.iter().enumerate().any(
                    |(i, tag)| *tag == Tag::Read && body.data[i][1] == ReadMode::Borrow as u32
                ));
                assert!(
                    body.tags
                        .iter()
                        .enumerate()
                        .any(|(i, tag)| *tag == Tag::Read
                            && body.data[i][1] == ReadMode::Move as u32)
                );
                let (errors, work) =
                    crate::own::flow::work::measure(|| verify_owned(&body, &crate::own::tests::Cx));
                assert!(errors.is_empty(), "first error: {:?}", errors.first());
                costs.push(work);
            }
            assert!(costs[0] > 0);
            assert!(costs[1] <= 11 * costs[0], "depth={depth}: {costs:?}");
        }
        Ok(())
    }

    #[test]
    fn sub_body_metadata_work_is_linear_in_closures() -> Result<(), String> {
        fn measured(n: usize) -> Result<usize, String> {
            let body = closures(n)?;
            let topo = Topology::new(&body);
            let neutral = vec![false; body.len()];
            let mut errors = Vec::new();
            let mut work = 0;
            Balance::new(&body, &topo, &crate::own::tests::Cx, &neutral)
                .walk_counted(&mut errors, || work += 1);
            assert!(errors.is_empty(), "{errors:?}");
            Ok(work)
        }
        let fixed = measured(0)?;
        let small = measured(100)? - fixed;
        let large = measured(1_000)? - fixed;
        assert!(small > 0);
        assert_eq!(
            large,
            10 * small,
            "100 closures: {small}; 1000 closures: {large}"
        );
        Ok(())
    }
}

#[cfg(test)]
mod instance_tests;
