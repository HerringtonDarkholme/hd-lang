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
//! - no `Read`, `IsUnique` or `Pin` of a place follows its `move` or its
//!   `Drop` on any path without a `Store` in between that refills it;
//! - no `borrow` value is live across a write to an overlapping place (a
//!   `Store`, a `Drop`, a `move` read, an `IsUnique` or a mutable `Ref`)
//!   or a suspension point, where a borrow is live while anything that
//!   lives on it is ([`borrow_spans`]);
//! - a local the body does not own holds no count: a binding local, which
//!   a `borrow` value is stored into, takes only borrows and is never
//!   dropped, moved out of or addressed; a counted parameter outside the
//!   exit set, which the body does not own, is also never stored into or
//!   changed in place (an `IsUnique` or a `Pin` of its own storage, which
//!   would give up a count of its caller's); neither is in the exit set
//!   ([`binding_locals`]);
//! - at each suspension point, whose cancellation leaves through the
//!   exit, every owned value has a row in the unwind set and every owned
//!   local that may hold a value is in the exit set ([`Balance`]);
//! - the exit set's rows are sorted, once each, and name a sub-body and a
//!   local of the body; the unwind set's are sorted, once each, and name a
//!   suspension point and a value;
//! - no `Ref` takes its local's count of a cell in a sub-body that holds
//!   a suspension point ([`takes_outside_suspension`]);
//! - an `IsUnique` is plain, `HOISTED` or `COVERED`; a hoisted one stands
//!   right before a loop (among other hoisted ones), names a local's place
//!   with one leading dereference at most and fields, whose local no `Ref`
//!   roots at; in that loop nothing copies it (a read of it or of a place
//!   holding it whose value a use keeps), takes a `Ref` that overlaps it,
//!   writes a place holding it, pins or suspends, and no call may write
//!   unless it is a whole local no closure captures; a covered one names
//!   the place a hoisted one before an enclosing loop does; each
//!   stable-list row names a loop and a place of that form, in order
//!   ([`hoisted`]);
//! - a collection change's operand that reads a place rooted at a local
//!   or a top-level binding reads it right after the place's `IsUnique`
//!   (or a hoisted one names the place), with nothing that may write
//!   between that read and the change; any other operand, and the root
//!   of an `IsUnique` rooted at a value, reads no such place through
//!   other reads, so the buffer a check or a change leaves reaches the
//!   place it replaced ([`change_places`]);
//! - nothing tier 2 reserves: no `FlagSet`, `FlagClear` or `regions` row.
//!
//! The walks follow the body's layout: dense tables indexed by instruction,
//! bit sets over the body's moves and drops, values and locals, no hash
//! maps.

use super::{
    Body, INLINED, NONE, Op, Place, PlaceRoot, Proj, ReadMode, Ref, Tag, VerifyError,
    is_suspension, local_flags, owned_blocks, place, verify_structure,
};
use crate::own::{OwnCx, Role, consumes, operands, tracked};

/// Verifies a body after the ownership passes: the structural invariants
/// and invariant 23 ([the module](self)), with the facts `cx` gives the
/// passes.
#[must_use]
pub fn verify_owned(b: &Body, cx: &dyn OwnCx) -> Vec<VerifyError> {
    let mut errs = verify_structure(b);
    let places: Vec<Option<Place>> = (0..b.len())
        .map(|i| match b.tags[i].operands().0 {
            Op::Place => b.decode_place(b.data[i][0]),
            _ => None,
        })
        .collect();
    let mut addressed = vec![false; b.local_ty.len()];
    for (i, p) in places.iter().enumerate() {
        if b.tags[i] == Tag::Ref
            && p.is_some()
            && let Some(l) = b.local_in_place(b.data[i][0])
            && let Some(a) = addressed.get_mut(l.idx())
        {
            *a = true;
        }
    }
    for (l, m) in b.cap_local.iter().zip(&b.cap_mode) {
        if *m == super::CaptureMode::Shared
            && let Some(a) = addressed.get_mut(l.idx())
        {
            *a = true;
        }
    }
    let own = cx;
    let cx = Cx {
        b,
        places: &places,
        addressed: &addressed,
    };
    decided(b, &mut errs);
    takes_outside_suspension(b, &mut errs);
    lent_refs(b, &mut errs);
    change_places(b, &mut errs);
    hoisted(b, own, &mut errs);
    Empties::new(&cx).walk(&mut errs);
    borrow_spans(&cx, &mut errs);
    binding_locals(&cx, own, &mut errs);
    Balance::new(b, own).walk(&mut errs);
    copies(b, &mut errs);
    errs
}

/// What the inliner's copies promise (`crate::inline`). Each anchor with
/// the `INLINED` bit falls in a `Body::inlined` row; the rows are sorted
/// and disjoint, and each call's anchor is the caller's own. A
/// `Body::bound` row, sorted and once each, names a local that is no
/// parameter and that one whole `Store`, its binding, writes. A place in a
/// copy is rooted at a local the body defines: a parameter or a local some
/// whole `Store` writes.
fn copies(b: &Body, errs: &mut Vec<VerifyError>) {
    if !b.anchors.is_empty() && b.anchors.len() != b.len() {
        err(
            errs,
            NONE,
            format!("{} anchors for {} instructions", b.anchors.len(), b.len()),
        );
    }
    let mut end = 0;
    for &[first, len, _, call] in &b.inlined {
        if first < end || call & INLINED != 0 {
            err(
                errs,
                NONE,
                format!("inlined row at {first} out of order, or called in a copy"),
            );
        }
        end = first + len;
    }
    let mut stores = vec![0u32; b.local_ty.len()];
    for i in (0..b.len()).filter(|&i| b.tags[i] == Tag::Store) {
        let w = b.data[i][0];
        if let Some(n) = stores.get_mut((w & !place::BARE_LOCAL) as usize)
            && w & place::BARE_LOCAL != 0
        {
            *n += 1;
        }
    }
    let param = |l: usize| {
        b.local_flags
            .get(l)
            .is_some_and(|f| f & local_flags::PARAM != 0)
    };
    for (k, &(l, _)) in b.bound.iter().enumerate() {
        let sorted = k == 0 || b.bound[k - 1].0 < l;
        if !sorted || param(l as usize) || stores.get(l as usize) != Some(&1) {
            err(
                errs,
                NONE,
                format!("bound local ${l}: not sorted, a parameter, or not stored once"),
            );
        }
    }
    for (i, &a) in b
        .anchors
        .iter()
        .enumerate()
        .filter(|e| *e.1 & INLINED != 0 && *e.1 != NONE)
    {
        let x = a & !INLINED;
        let k = b.inlined.partition_point(|e| e[0] <= x);
        if k == 0 || x >= b.inlined[k - 1][0] + b.inlined[k - 1][1] {
            err(errs, u32_of(i), format!("anchor {x} is in no inlined copy"));
        }
        let (oa, ob) = b.tags.get(i).map_or((Op::None, Op::None), |t| t.operands());
        for (w, op) in b.data.get(i).into_iter().flatten().zip([oa, ob]) {
            if op == Op::Place
                && let Some(l) = b.local_in_place(*w).map(hd_base::LocalId::idx)
                && !param(l)
                && stores.get(l).is_some_and(|&n| n == 0)
            {
                err(
                    errs,
                    u32_of(i),
                    format!("a copy's place is rooted at ${l}, which nothing stores"),
                );
            }
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

fn u32_of(i: usize) -> u32 {
    u32::try_from(i).expect("insts")
}

/// No `Ref` takes its local's count of a cell in a sub-body that holds a
/// suspension point: the frame saves the cell's local at each point, and
/// a resume enters blocks in the middle (move's rule 4).
fn takes_outside_suspension(b: &Body, errs: &mut Vec<VerifyError>) {
    let takes = |i: usize| b.tags[i] == Tag::Ref && b.data[i][1] & super::ref_word::TAKES != 0;
    if !(0..b.len()).any(takes) {
        return;
    }
    let subs = b.inst_subs();
    let mut suspends = vec![false; b.sub_root.len()];
    for (i, &tag) in b.tags.iter().enumerate() {
        if is_suspension(tag)
            && let Some(s) = suspends.get_mut(subs[i] as usize)
        {
            *s = true;
        }
    }
    for i in (0..b.len()).filter(|&i| takes(i)) {
        if suspends.get(subs[i] as usize) == Some(&true) {
            err(
                errs,
                u32_of(i),
                "a `Ref` takes a cell's count in a sub-body with a suspension point".into(),
            );
        }
    }
}

/// A lent `Ref` (`ref_word::LENT`) points into what a parameter the body
/// owns none of refers to: its place is a dereference of such a parameter
/// (`[*$p ...]`, or of a borrowed read of all of `$p`) followed by fields
/// only, and the body never stores into `$p` itself. The caller keeps that
/// referent's block alive for the whole call, so the reference needs no
/// count of its own.
fn lent_refs(b: &Body, errs: &mut Vec<VerifyError>) {
    let lent = |i: usize| b.tags[i] == Tag::Ref && b.data[i][1] & super::ref_word::LENT != 0;
    if !(0..b.len()).any(lent) {
        return;
    }
    let mut stored = vec![false; b.local_ty.len()];
    for i in 0..b.len() {
        if matches!(b.tags[i], Tag::Store | Tag::Ref)
            && let Some(l) = b.local_in_place(b.data[i][0])
            && let Some(x) = stored.get_mut(l.idx())
        {
            *x = true;
        }
    }
    let unowned = |l: usize| {
        l < b.local_ty.len()
            && b.local_flags[l] & super::local_flags::PARAM != 0
            && !stored[l]
            && !b.exit_drops.iter().any(|&(_, x)| x.idx() == l)
    };
    for i in (0..b.len()).filter(|&i| lent(i)) {
        let ok = b.decode_place(b.data[i][0]).is_some_and(|p| {
            let fields = match p.proj.split_first() {
                Some((Proj::Deref, rest)) => rest.iter().all(|s| matches!(s, Proj::Field(_))),
                _ => false,
            };
            let param = match p.root {
                PlaceRoot::Local(l) => unowned(l.idx()),
                PlaceRoot::Value(v) => v.as_inst().is_some_and(|v| {
                    let v = v.0 as usize;
                    v < b.len()
                        && b.tags[v] == Tag::Read
                        && b.data[v][1] == ReadMode::Borrow as u32
                        && b.data[v][0] & place::BARE_LOCAL != 0
                        && unowned((b.data[v][0] & !place::BARE_LOCAL) as usize)
                }),
                _ => false,
            };
            fields && param
        });
        if !ok {
            err(
                errs,
                u32_of(i),
                "a lent `Ref` that is no field of what a lent parameter refers to".into(),
            );
        }
    }
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
fn change_places(b: &Body, errs: &mut Vec<VerifyError>) {
    let n = b.len();
    let mut at = vec![(usize::MAX, 0); n];
    for i in (0..n).filter(|&i| b.tags[i] == Tag::Block) {
        for (p, &m) in b.record(b.data[i][0]).iter().enumerate() {
            if let Some(x) = at.get_mut(m as usize) {
                *x = (i, p);
            }
        }
    }
    let unique = |x: usize, p: &Place| {
        b.tags[x] == Tag::IsUnique && b.decode_place(b.data[x][0]).as_ref() == Some(p)
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
                (Tag::Coerce, _) => v = b.data[r][0],
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
            let ((blk, pos), (cb, cp)) = (at[r], at[c]);
            let list = if blk == usize::MAX {
                &[][..]
            } else {
                b.record(b.data[blk][0])
            };
            let checked = pos > 0 && unique(list[pos - 1] as usize, &p)
                || (0..n).any(|x| unique(x, &p) && b.data[x][1] == super::unique_word::HOISTED);
            if !checked || blk != cb || cp <= pos || !list[pos + 1..cp].iter().all(quiet) {
                let what =
                    format!("change operand %{r} is no read of its place after its `IsUnique`");
                err(errs, u32_of(c), what);
            }
        }
    }
}

/// Hoisting's `IsUnique`s and stable lists (`docs/compiler/tir-v2.md`,
/// "Hoisting"): what the pass relies on, checked again.
fn hoisted(b: &Body, cx: &dyn OwnCx, errs: &mut Vec<VerifyError>) {
    use super::unique_word::{COVERED, HOISTED};
    use crate::own::hoist;
    let n = b.len();
    let any = (0..n).any(|i| b.tags[i] == Tag::IsUnique && b.data[i][1] != 0);
    if !any && b.stable_lists.is_empty() {
        return;
    }
    // Each statement's block and position; each instruction's first.
    let mut blk = vec![NONE; n];
    let mut pos = vec![0u32; n];
    let mut first: Vec<u32> = (0..n).map(u32_of).collect();
    for i in 0..n {
        if b.tags[i] == Tag::Block {
            for (p, &m) in b.record(b.data[i][0]).iter().enumerate() {
                if (m as usize) < n {
                    blk[m as usize] = u32_of(i);
                    pos[m as usize] = u32_of(p);
                    if (m as usize) < i {
                        first[i] = first[i].min(first[m as usize]);
                    }
                }
            }
        }
        let mut f = first[i];
        crate::own::splice_for_blocks(b, i, |x| {
            if (x as usize) < i {
                f = f.min(first[x as usize]);
            }
        });
        first[i] = f;
    }
    let mut addressed = vec![false; b.local_ty.len()];
    let mut captured = vec![false; b.local_ty.len()];
    for i in 0..n {
        if b.tags[i] == Tag::Ref
            && let Some(l) = b.local_in_place(b.data[i][0])
        {
            addressed[l.idx()] = true;
        }
    }
    for l in &b.cap_local {
        captured[l.idx()] = true;
    }
    for (l, f) in b.local_flags.iter().enumerate() {
        if f & (super::local_flags::CAPTURED | super::local_flags::CAPTURED_ASSIGNED) != 0 {
            captured[l] = true;
        }
    }
    let mut kept = vec![false; n];
    let mut ops = Vec::new();
    for i in 0..n {
        operands(b, i, &mut ops);
        for op in &ops {
            if hoist::keeps(b, u32_of(i), op.role) {
                kept[op.value as usize] = true;
            }
        }
    }
    let (mut steps, mut raw, mut other) = (Vec::new(), Vec::new(), Vec::new());
    // The hoisted ones: (loop, local, steps).
    let mut hoists: Vec<(u32, u32, Vec<u32>)> = Vec::new();
    for i in 0..n {
        if b.tags[i] != Tag::IsUnique {
            continue;
        }
        let w = b.data[i][1];
        if w != 0 && w != HOISTED && w != COVERED {
            err(
                errs,
                u32_of(i),
                format!("an `IsUnique` of unknown kind {w}"),
            );
            continue;
        }
        if w != HOISTED {
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
                            && !(tag == Tag::IsUnique && b.data[x as usize][1] == HOISTED)
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
        if local == NONE || !hoist::hoistable(&steps) || addressed[local as usize] {
            err(
                errs,
                u32_of(i),
                "a hoisted `IsUnique` of a place that is no unaddressed local's own".into(),
            );
            continue;
        }
        let whole = steps.is_empty() && !captured[local as usize];
        for x in first[l as usize]..l {
            let xi = x as usize;
            let tag = b.tags[xi];
            let [a, xw] = b.data[xi];
            let what = if is_suspension(tag) || tag == Tag::Pin {
                Some("pins or suspends")
            } else if !whole && hoist::writes(b, cx, xi) {
                Some("calls what may write")
            } else if matches!(tag, Tag::Read | Tag::Store | Tag::Drop | Tag::Ref) {
                let (ol, temp) = hoist::resolve(b, a, &mut other, &mut raw);
                let mine = ol == local;
                let covers = mine && hoist::prefix(&other, &steps);
                let below = mine && other.len() > steps.len() && hoist::prefix(&steps, &other);
                match tag {
                    Tag::Read if xw == ReadMode::Move as u32 && covers && !temp => Some("moves it"),
                    Tag::Read if covers && kept[xi] => Some("copies it"),
                    Tag::Store | Tag::Drop if covers && !temp => Some("writes it"),
                    Tag::Ref if (covers || below) && !temp => Some("takes a reference to it"),
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
        hoists.push((l, local, steps.clone()));
    }
    for i in 0..n {
        if b.tags[i] != Tag::IsUnique || b.data[i][1] != COVERED {
            continue;
        }
        let (local, _) = hoist::resolve(b, b.data[i][0], &mut steps, &mut raw);
        let covered = hoists.iter().any(|(l, hl, hs)| {
            *hl == local && *hs == steps && (first[*l as usize]..*l).contains(&u32_of(i))
        });
        if !covered {
            err(
                errs,
                u32_of(i),
                "a covered `IsUnique` that no hoisted one before an enclosing loop covers".into(),
            );
        }
    }
    for (k, &(l, word)) in b.stable_lists.iter().enumerate() {
        let (local, _) = hoist::resolve(b, word, &mut steps, &mut raw);
        if b.tags.get(l as usize) != Some(&Tag::Loop)
            || local == NONE
            || !hoist::hoistable(&steps)
            || (k > 0 && b.stable_lists[k - 1] >= (l, word))
        {
            err(
                errs,
                l,
                format!("stable list row {k} names no loop, no local's place, or is out of order"),
            );
        }
    }
}

/// The modes, the exit set and the tier-2 reservations.
fn decided(b: &Body, errs: &mut Vec<VerifyError>) {
    for (i, (&tag, &[_, w])) in b.tags.iter().zip(&b.data).enumerate() {
        match tag {
            Tag::Read => match ReadMode::from_u32(w) {
                Some(ReadMode::Use) => {
                    err(errs, u32_of(i), "a `Read` left `use` by the passes".into());
                }
                None => err(errs, u32_of(i), format!("a `Read` of unknown mode {w}")),
                Some(_) => {}
            },
            Tag::FlagSet | Tag::FlagClear => {
                err(errs, u32_of(i), format!("`{}` is tier 2's", tag.name()));
            }
            _ => {}
        }
    }
    if !b.regions.is_empty() {
        err(errs, NONE, "lifetime regions are tier 2's".into());
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
    for (k, &(at, v)) in b.unwind_drops.iter().enumerate() {
        let point = b.tags.get(at as usize).is_some_and(|&t| is_suspension(t));
        if !point || v as usize >= b.len() {
            err(
                errs,
                at,
                format!(
                    "unwind drop row {k} names %{v} at %{at}, which is no value at a suspension point"
                ),
            );
        }
        if k > 0 && b.unwind_drops[k - 1] >= (at, v) {
            err(errs, at, format!("unwind drop row {k} is out of order"));
        }
    }
    for (k, &(at, l)) in b.full_cells.iter().enumerate() {
        if at as usize >= b.len() || l as usize >= b.local_ty.len() {
            err(
                errs,
                NONE,
                format!("full cell row {k} names %{at} or ${l}, which the body lacks"),
            );
        }
        if k > 0 && b.full_cells[k - 1] >= (at, l) {
            err(errs, at, format!("full cell row {k} is out of order"));
        }
    }
}

/// What both walks read: the body, each instruction's decoded place, and
/// the locals a reference or a shared capture can reach.
struct Cx<'a> {
    b: &'a Body,
    places: &'a [Option<Place>],
    addressed: &'a [bool],
}

impl Cx<'_> {
    /// Whether places `p` and `q` may name overlapping storage
    /// (`docs/compiler/tir-v2.md`, "Places"). With one root, one's
    /// projections are a prefix of the other's, element indices ignored.
    /// With two roots, one goes behind a reference or into a list's buffer
    /// and the other is where a reference can reach: behind one too, a
    /// top-level binding, or a local that is addressed or shared with a
    /// closure. A value root alone is a temporary that nothing else
    /// reaches.
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

/// A bit set over the body's moves and drops.
type Set = Vec<u64>;

fn union(a: Option<Set>, c: Option<Set>) -> Option<Set> {
    match (a, c) {
        (Some(mut x), Some(y)) => {
            for (w, v) in x.iter_mut().zip(y) {
                *w |= v;
            }
            Some(x)
        }
        (x, y) => x.or(y),
    }
}

/// The read-after-move walk: a forward walk over the structured control
/// flow of each sub-body, carrying the set of places a `move` read or a
/// `Drop` emptied. Joins are unions ("on any path"); a loop runs to a fixed
/// point.
struct Empties<'a> {
    cx: &'a Cx<'a>,
    /// Each instruction's number among the moves and drops, or `NONE`.
    id: Vec<u32>,
    /// The instruction of each move or drop.
    at: Vec<u32>,
    /// (value, move or drop number) for each move or drop of a place
    /// rooted at an instruction's value, sorted: running that instruction
    /// again makes a new value, which refills the place.
    by_value: Vec<(u32, u32)>,
    /// The label each `Loop` or labeled `Block` instruction is the target
    /// of, or `NONE`.
    label_of: Vec<u32>,
    /// Per label, the union of the states its `Break`s and `Continue`s
    /// carry, during the walk of its target.
    brk: Vec<Option<Set>>,
    cont: Vec<Option<Set>>,
    /// The open matches' arm entry states, innermost last.
    arms: Vec<Vec<Option<Set>>>,
    /// The open cleanup scopes, innermost last, each with the union of the
    /// states at the exits that leave its body by a jump: its suites run
    /// there.
    scopes: Vec<Set>,
    /// Per label, how many scopes were open when its target's walk began:
    /// a jump to it leaves the ones opened after.
    label_scopes: Vec<usize>,
    reported: Vec<bool>,
    errs: Vec<VerifyError>,
}

impl<'a> Empties<'a> {
    fn new(cx: &'a Cx<'a>) -> Self {
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
                Tag::Drop => true,
                _ => false,
            };
            if empties && cx.places[i].is_some() {
                *slot = u32_of(at.len());
                at.push(u32_of(i));
            }
        }
        let mut label_of = vec![NONE; b.len()];
        for (l, &t) in b.label_inst.iter().enumerate() {
            if let Some(x) = label_of.get_mut(t as usize) {
                *x = u32_of(l);
            }
        }
        let mut by_value: Vec<(u32, u32)> = at
            .iter()
            .enumerate()
            .filter_map(|(k, &e)| match cx.places[e as usize].as_ref()?.root {
                PlaceRoot::Value(r) if r.as_inst().is_some() => Some((r.0, u32_of(k))),
                _ => None,
            })
            .collect();
        by_value.sort_unstable();
        Empties {
            cx,
            id,
            at,
            by_value,
            label_of,
            brk: vec![None; b.label_inst.len()],
            cont: vec![None; b.label_inst.len()],
            arms: Vec::new(),
            scopes: Vec::new(),
            label_scopes: vec![0; b.label_inst.len()],
            reported: vec![false; b.len()],
            errs: Vec::new(),
        }
    }

    /// A jump that leaves the scopes opened since `from`: their suites run
    /// from its state.
    fn leave_scopes(&mut self, from: usize, s: &Set) {
        for acc in self.scopes.iter_mut().skip(from) {
            for (w, v) in acc.iter_mut().zip(s) {
                *w |= v;
            }
        }
    }

    fn walk(mut self, errs: &mut Vec<VerifyError>) {
        if self.at.is_empty() {
            return;
        }
        let words = self.at.len().div_ceil(64);
        for &root in &self.cx.b.sub_root {
            self.block(root, vec![0; words]);
        }
        errs.append(&mut self.errs);
    }

    fn is_block(&self, x: u32) -> bool {
        self.cx.b.tags.get(x as usize) == Some(&Tag::Block)
    }

    /// Walks block `blk` from `s`; `None` when every path leaves it by a
    /// jump.
    fn block(&mut self, blk: u32, s: Set) -> Option<Set> {
        if !self.is_block(blk) {
            return Some(s);
        }
        let b = self.cx.b;
        let label = self.label_of[blk as usize];
        if label != NONE {
            self.label_scopes[label as usize] = self.scopes.len();
        }
        let mut cur = Some(s);
        for &x in b.record(b.data[blk as usize][0]) {
            let Some(st) = cur else {
                break;
            };
            // A block in the list runs where the instruction that owns it
            // runs it, as emission does; a closure's root block is walked as
            // its own sub-body.
            cur = if b.tags[x as usize] == Tag::Block {
                Some(st)
            } else {
                self.inst(x, st)
            };
        }
        let label = self.label_of[blk as usize];
        if label != NONE {
            cur = union(cur, self.brk[label as usize].take());
        }
        cur
    }

    fn inst(&mut self, x: u32, mut s: Set) -> Option<Set> {
        let b = self.cx.b;
        let [a, w] = b.data[x as usize];
        // A new value of `x` (a loop's next round) refills its places.
        let from = self.by_value.partition_point(|&(v, _)| v < x);
        for &(v, k) in &self.by_value[from..] {
            if v != x {
                break;
            }
            s[(k / 64) as usize] &= !(1u64 << (k % 64));
        }
        match b.tags[x as usize] {
            Tag::Read | Tag::IsUnique | Tag::Pin => {
                self.check(x, &s);
                self.empty(x, &mut s);
            }
            Tag::Drop => self.empty(x, &mut s),
            Tag::Store => self.refill(x, &mut s),
            Tag::If => {
                let r = b.record(w);
                let (then, els) = (r.first().copied(), r.get(1).copied());
                let t = then.map_or(Some(s.clone()), |t| self.block(t, s.clone()));
                let e = els.map_or(Some(s.clone()), |e| self.block(e, s));
                return union(t, e);
            }
            Tag::And | Tag::Or => {
                let right = self.block(w, s.clone());
                return union(Some(s), right);
            }
            Tag::With => return self.block(w, s),
            Tag::RefDyn => return self.block(a, s),
            Tag::Loop => return self.repeat(x, s),
            Tag::Match => return self.decide(x, s),
            Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => {
                let r = b.record(w).to_vec();
                let (cases, default) = r.split_at(r.len().saturating_sub(1));
                let mut out = None;
                for &blk in cases.iter().skip(1).step_by(2).chain(default) {
                    if self.is_block(blk) {
                        out = union(out, self.block(blk, s.clone()));
                    }
                }
                return out;
            }
            Tag::Guard => {
                let s = self.block(a, s)?;
                let r = b.record(w).to_vec();
                let mut out = None;
                for &blk in &r {
                    out = union(out, self.block(blk, s.clone()));
                }
                return out;
            }
            Tag::Scope => return self.scope(x, s),
            Tag::ToArm => {
                if let Some(arm) = self
                    .arms
                    .last_mut()
                    .and_then(|arms| arms.get_mut(a as usize))
                {
                    *arm = union(arm.take(), Some(s));
                }
                return None;
            }
            Tag::Break => {
                let from = self.label_scopes.get(a as usize).copied().unwrap_or(0);
                self.leave_scopes(from, &s);
                if let Some(t) = self.brk.get_mut(a as usize) {
                    *t = union(t.take(), Some(s));
                }
                return None;
            }
            Tag::Continue => {
                let from = self.label_scopes.get(a as usize).copied().unwrap_or(0);
                self.leave_scopes(from, &s);
                if let Some(t) = self.cont.get_mut(a as usize) {
                    *t = union(t.take(), Some(s));
                }
                return None;
            }
            Tag::Return => {
                self.leave_scopes(0, &s);
                return None;
            }
            Tag::Unreachable => return None,
            _ => {}
        }
        Some(s)
    }

    /// A loop: its body from the head state until the head stops growing
    /// (states only grow, so this ends); its exits are its `Break`s and,
    /// when it has one, its else block from the final head.
    fn repeat(&mut self, x: u32, s: Set) -> Option<Set> {
        let b = self.cx.b;
        let [body, els] = b.data[x as usize];
        let label = self.label_of[x as usize] as usize;
        let mut head = s;
        if let Some(x) = self.label_scopes.get_mut(label) {
            *x = self.scopes.len();
        }
        for _ in 0..=self.at.len() + 1 {
            if let Some(t) = self.brk.get_mut(label) {
                *t = None;
            }
            if let Some(t) = self.cont.get_mut(label) {
                *t = None;
            }
            let end = self.block(body, head.clone());
            let back = self.cont.get_mut(label).and_then(Option::take);
            let next = union(union(Some(head.clone()), end), back).expect("the head");
            if next == head {
                break;
            }
            head = next;
        }
        let exits = self.brk.get_mut(label).and_then(Option::take);
        if els != NONE && self.is_block(els) {
            return union(exits, self.block(els, head));
        }
        exits
    }

    /// A match: its decision tree from `s`, each arm from the union of the
    /// states its `ToArm`s carry.
    fn decide(&mut self, x: u32, s: Set) -> Option<Set> {
        let b = self.cx.b;
        let r = b.record(b.data[x as usize][1]).to_vec();
        let Some((&decision, arms)) = r.split_first() else {
            return Some(s);
        };
        self.arms.push(vec![None; arms.len()]);
        // Every path of the tree ends in `ToArm` or `Unreachable`.
        let _ = self.block(decision, s);
        let entries = self.arms.pop().unwrap_or_default();
        let mut out = None;
        for (&arm, entry) in arms.iter().zip(entries) {
            if let Some(e) = entry {
                out = union(out, self.block(arm, e));
            }
        }
        out
    }

    /// A cleanup scope: its body, then each suite from the union of the
    /// states at the body's exits, since a suite runs at whichever exit the
    /// body takes: its end, or a jump that leaves it.
    fn scope(&mut self, x: u32, s: Set) -> Option<Set> {
        let b = self.cx.b;
        let [body, suites] = b.data[x as usize];
        self.scopes.push(vec![0; s.len()]);
        let after = self.block(body, s);
        let mut seen = self.scopes.pop().unwrap_or_default();
        if let Some(a) = &after {
            for (w, v) in seen.iter_mut().zip(a) {
                *w |= v;
            }
        }
        let mut out = after;
        if suites != NONE {
            for &suite in &b.record(suites).to_vec() {
                let end = self.block(suite, seen.clone());
                if out.is_some() {
                    out = union(out, end);
                }
            }
        }
        out
    }

    /// Instruction `x` empties its place: a `move` read or a `Drop`.
    fn empty(&mut self, x: u32, s: &mut Set) {
        let k = self.id[x as usize];
        if k == NONE {
            return;
        }
        let (w, bit) = ((k / 64) as usize, 1u64 << (k % 64));
        s[w] |= bit;
    }

    /// A `Store` refills every emptied place it covers.
    fn refill(&self, x: u32, s: &mut Set) {
        let Some(q) = &self.cx.places[x as usize] else {
            return;
        };
        for (k, &e) in self.at.iter().enumerate() {
            let (w, bit) = (k / 64, 1u64 << (k % 64));
            if s[w] & bit != 0
                && let Some(p) = &self.cx.places[e as usize]
                && covers(q, p)
            {
                s[w] &= !bit;
            }
        }
    }

    /// Whether emptying `p` at `e` empties what `q` reads. A `Drop` of a
    /// whole local, or a `Ref` that takes its count, ends the local's own
    /// hold on its value: a local that a reference or a closure reaches
    /// lives in a cell, whose holders keep it, so only a read of the local
    /// itself sees it empty.
    fn empties(&self, e: u32, p: &Place, q: &Place) -> bool {
        if matches!(self.cx.b.tags[e as usize], Tag::Drop | Tag::Ref)
            && matches!(p.root, PlaceRoot::Local(_))
            && p.proj.is_empty()
        {
            return p.root == q.root;
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
        for (k, &e) in self.at.iter().enumerate() {
            if s[k / 64] & (1u64 << (k % 64)) != 0
                && let Some(p) = &self.cx.places[e as usize]
                && self.empties(e, p, q)
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

/// Whether `v` is a `borrow` read's value.
fn is_borrow(b: &Body, v: u32) -> bool {
    Ref(v).as_inst().is_some()
        && (v as usize) < b.len()
        && b.tags[v as usize] == Tag::Read
        && b.data[v as usize][1] == ReadMode::Borrow as u32
}

/// The locals the body does not own (`docs/compiler/tir-v2.md`,
/// "Borrow"). A binding local: a `Store` of a `borrow` read goes into all
/// of it, so it holds borrowed values only: every store into it stores
/// one, nothing drops it, moves out of it or addresses it, and it is in no
/// exit set. A lent parameter: a counted parameter outside the exit set,
/// which nothing stores into, drops, moves out of or addresses either,
/// and nothing changes in place: an `IsUnique` or a `Pin` of it would give
/// up or keep a count that is its caller's.
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
        for l in entry_locals(b, 0) {
            if !exit[l] && own.counted(b.local_ty[l]) != crate::own::Tri::No {
                lent[l] = true;
                any = true;
            }
        }
    }
    for i in 0..b.len() {
        if b.tags[i] != Tag::Store || !is_borrow(b, b.data[i][1]) {
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
        let what = match b.tags[i] {
            Tag::Store if lent[l.idx()] => "a store into",
            Tag::Store if !is_borrow(b, b.data[i][1]) => "an owned value stored into",
            Tag::Drop => "a `Drop` of",
            Tag::Read if b.data[i][1] == ReadMode::Move as u32 => "a `move` out of",
            Tag::Ref => "a reference to",
            Tag::IsUnique | Tag::Pin => "a change in place of",
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
/// live in its arms unless they use it.
fn borrow_spans(cx: &Cx<'_>, errs: &mut Vec<VerifyError>) {
    let b = cx.b;
    let n = b.len();
    let mut bit = vec![NONE; n];
    let mut at = Vec::new();
    for (i, x) in bit.iter_mut().enumerate() {
        if is_borrow(b, u32_of(i)) {
            *x = u32_of(at.len());
            at.push(u32_of(i));
        }
    }
    if at.is_empty() {
        return;
    }
    // The borrow each binding local holds.
    let mut holds = vec![NONE; b.local_ty.len()];
    for i in 0..n {
        if b.tags[i] == Tag::Store
            && is_borrow(b, b.data[i][1])
            && let Some(Place {
                root: PlaceRoot::Local(l),
                proj,
            }) = &cx.places[i]
            && proj.is_empty()
        {
            holds[l.idx()] = b.data[i][1];
        }
    }
    let mut label_of = vec![NONE; n];
    for (l, &t) in b.label_inst.iter().enumerate() {
        if let Some(x) = label_of.get_mut(t as usize) {
            *x = u32_of(l);
        }
    }
    let mut walk = Live {
        cx,
        bit,
        at,
        holds,
        label_of,
        brk: vec![None; b.label_inst.len()],
        cont: vec![None; b.label_inst.len()],
        arms: Vec::new(),
        ops: Vec::new(),
        reported: vec![false; n],
        errs: Vec::new(),
    };
    let words = walk.at.len().div_ceil(64);
    for &root in &b.sub_root {
        if (root as usize) < n {
            let mut live = vec![0u64; words];
            walk.block(root, &mut live);
        }
    }
    errs.append(&mut walk.errs);
}

/// The borrow walk's state ([`borrow_spans`]).
struct Live<'a> {
    cx: &'a Cx<'a>,
    /// Each borrow's bit, or `NONE`, and each bit's borrow.
    bit: Vec<u32>,
    at: Vec<u32>,
    holds: Vec<u32>,
    label_of: Vec<u32>,
    /// While a target is walked: the state after it, for a `Break`, and at
    /// a loop's head, for a `Continue`.
    brk: Vec<Option<Set>>,
    cont: Vec<Option<Set>>,
    /// The open matches' arm entry states.
    arms: Vec<Vec<Set>>,
    ops: Vec<crate::own::Operand>,
    reported: Vec<bool>,
    errs: Vec<VerifyError>,
}

impl Live<'_> {
    fn is_block(&self, x: u32) -> bool {
        self.cx.b.tags.get(x as usize) == Some(&Tag::Block)
    }

    /// A use of value `v`: it, and every borrow it lives on.
    fn uses(&self, live: &mut Set, v: u32) {
        let b = self.cx.b;
        let mut x = v;
        for _ in 0..b.len() {
            let Some(&k) = self.bit.get(x as usize) else {
                return;
            };
            if k == NONE {
                return;
            }
            live[(k / 64) as usize] |= 1 << (k % 64);
            match self.cx.places[x as usize].as_ref().map(|p| p.root) {
                Some(PlaceRoot::Value(r)) if (r.0 as usize) < b.len() => x = r.0,
                Some(PlaceRoot::Local(l)) if self.holds[l.idx()] != NONE => {
                    x = self.holds[l.idx()];
                }
                _ => return,
            }
        }
    }

    /// Block `blk` backward: `live` after it becomes `live` at its start.
    fn block(&mut self, blk: u32, live: &mut Set) {
        if !self.is_block(blk) {
            return;
        }
        let b = self.cx.b;
        let label = self.label_of[blk as usize];
        let saved = (label != NONE).then(|| self.brk[label as usize].replace(live.clone()));
        let tail = b.data[blk as usize][1];
        if tail != NONE && Ref(tail).as_inst().is_some() {
            self.uses(live, tail);
        }
        let members = b.record(b.data[blk as usize][0]).to_vec();
        for &x in members.iter().rev() {
            if b.tags[x as usize] != Tag::Block {
                self.inst(x, live);
            }
        }
        if let Some(old) = saved {
            self.brk[label as usize] = old;
        }
    }

    fn union(to: &mut Set, from: &Set) -> bool {
        let mut grew = false;
        for (w, v) in to.iter_mut().zip(from) {
            grew |= v & !*w != 0;
            *w |= v;
        }
        grew
    }

    /// Instruction `x` backward.
    fn inst(&mut self, x: u32, live: &mut Set) {
        let b = self.cx.b;
        let i = x as usize;
        let [a, w] = b.data[i];
        if self.bit[i] != NONE {
            let k = self.bit[i];
            live[(k / 64) as usize] &= !(1 << (k % 64));
        }
        let tag = b.tags[i];
        let writes = match tag {
            Tag::Store | Tag::Drop | Tag::IsUnique => true,
            Tag::Read => w == ReadMode::Move as u32,
            Tag::Ref => w & super::ref_word::MUT != 0,
            _ => false,
        };
        // A write through a borrow's own value (into the buffer its list
        // header names, or the referent its reference names) keeps that
        // value as it is.
        if writes && let Some(q) = &self.cx.places[i] {
            self.check(x, live, |r, p| {
                q.root != PlaceRoot::Value(Ref(r)) && self.cx.overlap(p, q)
            });
        }
        match tag {
            Tag::If => {
                let r = b.record(w);
                let (then, els) = (r.first().copied(), r.get(1).copied());
                let mut other = live.clone();
                if let Some(t) = then {
                    self.block(t, &mut other);
                }
                if let Some(e) = els {
                    self.block(e, live);
                }
                Self::union(live, &other);
            }
            Tag::And | Tag::Or => {
                let mut right = live.clone();
                self.block(w, &mut right);
                Self::union(live, &right);
            }
            Tag::With => self.block(w, live),
            Tag::RefDyn | Tag::Scope => self.block(a, live),
            Tag::Loop => self.repeat(x, live),
            Tag::Match => self.decide(x, live),
            Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => {
                let r = b.record(w).to_vec();
                let (cases, default) = r.split_at(r.len().saturating_sub(1));
                let mut acc = vec![0; live.len()];
                for &blk in cases.iter().skip(1).step_by(2).chain(default) {
                    let mut s = live.clone();
                    self.block(blk, &mut s);
                    Self::union(&mut acc, &s);
                }
                *live = acc;
            }
            Tag::Guard => {
                let mut acc = vec![0; live.len()];
                for &blk in b.record(w) {
                    let mut s = live.clone();
                    self.block(blk, &mut s);
                    Self::union(&mut acc, &s);
                }
                *live = acc;
                self.block(a, live);
            }
            Tag::ToArm => {
                match self.arms.last().and_then(|arms| arms.get(a as usize)) {
                    Some(s) => live.clone_from(s),
                    None => live.fill(0),
                }
                return;
            }
            Tag::Break => match self.brk.get(a as usize) {
                Some(Some(s)) => live.clone_from(s),
                _ => live.fill(0),
            },
            Tag::Continue => match self.cont.get(a as usize) {
                Some(Some(s)) => live.clone_from(s),
                _ => live.fill(0),
            },
            Tag::Return | Tag::Unreachable => live.fill(0),
            _ => {}
        }
        let mut ops = std::mem::take(&mut self.ops);
        operands(b, i, &mut ops);
        // A borrow's own `Drop` releases only a value an instance made
        // fresh, which lives on nothing: no use.
        for op in &ops {
            if op.role != Role::Tail && op.role != Role::Dropped {
                self.uses(live, op.value);
            }
        }
        self.ops = ops;
        // A read through a binding local reads the borrow it holds.
        if let Some(Place {
            root: PlaceRoot::Local(l),
            ..
        }) = &self.cx.places[i]
            && self.holds[l.idx()] != NONE
        {
            self.uses(live, self.holds[l.idx()]);
        }
        if is_suspension(tag) {
            self.check(x, live, |_, _| true);
        }
    }

    /// An error at `x` for the first live borrow whose place `hit` names.
    fn check(&mut self, x: u32, live: &Set, hit: impl Fn(u32, &Place) -> bool) {
        if self.reported[x as usize] {
            return;
        }
        for (k, &r) in self.at.iter().enumerate() {
            if live[k / 64] & (1 << (k % 64)) == 0 || r == x {
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

    /// Loop `x` backward, from its head to a fixed point.
    fn repeat(&mut self, x: u32, live: &mut Set) {
        let b = self.cx.b;
        let [body, els] = b.data[x as usize];
        let label = self.label_of[x as usize];
        let after = live.clone();
        let mut head = vec![0; live.len()];
        if els != NONE && self.is_block(els) {
            head.clone_from(&after);
            self.block(els, &mut head);
        }
        let saved = (label != NONE).then(|| {
            (
                self.brk[label as usize].replace(after.clone()),
                self.cont[label as usize].take(),
            )
        });
        for _ in 0..=self.at.len() + 1 {
            if label != NONE {
                self.cont[label as usize] = Some(head.clone());
            }
            let mut end = head.clone();
            self.block(body, &mut end);
            if !Self::union(&mut head, &end) {
                break;
            }
        }
        if let Some((bs, cs)) = saved {
            self.brk[label as usize] = bs;
            self.cont[label as usize] = cs;
        }
        *live = head;
    }

    /// Match `x` backward: each arm from after the match, then the decision
    /// tree from the arms' entry states.
    fn decide(&mut self, x: u32, live: &mut Set) {
        let b = self.cx.b;
        let r = b.record(b.data[x as usize][1]).to_vec();
        let Some((&decision, arms)) = r.split_first() else {
            return;
        };
        let label = self.label_of[x as usize];
        let saved = (label != NONE).then(|| self.brk[label as usize].replace(live.clone()));
        let mut entries = Vec::with_capacity(arms.len());
        for &arm in arms {
            let mut s = live.clone();
            self.block(arm, &mut s);
            entries.push(s);
        }
        self.arms.push(entries);
        let mut tree = vec![0; live.len()];
        self.block(decision, &mut tree);
        self.arms.pop();
        *live = tree;
        if let Some(old) = saved {
            self.brk[label as usize] = old;
        }
    }
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
    cx: &'a dyn OwnCx,
    /// Each tracked value's number among the tracked values, or `NONE`.
    num: Vec<u32>,
    /// Each local's number among the locals, all of them.
    words: usize,
    lwords: usize,
    /// The parts of values that `move` reads and `Drop`s take: each
    /// instruction's slot (a distinct place of one value), or `NONE`; the
    /// slots of each value, in compressed rows, with a place word of each.
    /// The walk keeps a bit per slot after the locals' bits, so a value
    /// taken in parts is owned until its parts cover every counted leaf.
    slot_of: Vec<u32>,
    slot_start: Vec<u32>,
    slot_word: Vec<u32>,
    pwords: usize,
    /// Each block's owner, a cleanup scope owning its suites; whether an
    /// instruction is a sub-body's root; the innermost loop whose body
    /// holds each instruction.
    owner: Vec<u32>,
    is_root: Vec<bool>,
    loop_of: Vec<u32>,
    label_of: Vec<u32>,
    brk: Vec<Option<Set>>,
    cont: Vec<Option<Set>>,
    arms: Vec<Vec<Option<Set>>>,
    /// The exit set of the sub-body being walked, by local, and the locals
    /// the passes own: a counted type's, or one a cell may hold.
    exit: Vec<bool>,
    owned: Vec<bool>,
    ops: Vec<crate::own::Operand>,
    reported: Vec<bool>,
    errs: Vec<VerifyError>,
}

impl<'a> Balance<'a> {
    fn new(b: &'a Body, cx: &'a dyn OwnCx) -> Self {
        let n = b.len();
        let mut num = vec![NONE; n];
        let mut k = 0u32;
        for (i, x) in num.iter_mut().enumerate() {
            if tracked(b, cx, i) {
                *x = k;
                k += 1;
            }
        }
        let mut owner = vec![NONE; n];
        for i in 0..n {
            for blk in owned_blocks(b, i) {
                if b.tags[i] == Tag::Defer && owner[blk as usize] != NONE {
                    continue;
                }
                owner[blk as usize] = u32_of(i);
            }
        }
        let mut is_root = vec![false; n];
        for &r in &b.sub_root {
            if let Some(x) = is_root.get_mut(r as usize) {
                *x = true;
            }
        }
        let mut parent = vec![NONE; n];
        for i in 0..n {
            if b.tags[i] == Tag::Block {
                for &m in b.record(b.data[i][0]) {
                    if let Some(p) = parent.get_mut(m as usize) {
                        *p = u32_of(i);
                    }
                }
            }
        }
        let mut loop_of = vec![NONE; n];
        for i in (0..n).rev() {
            if is_root[i] {
                continue;
            }
            let p = if parent[i] == NONE {
                owner[i]
            } else {
                parent[i]
            };
            if p == NONE || p as usize >= n {
                continue;
            }
            let p = p as usize;
            loop_of[i] =
                if b.tags[i] == Tag::Block && b.tags[p] == Tag::Loop && b.data[p][0] == u32_of(i) {
                    u32_of(p)
                } else {
                    loop_of[p]
                };
        }
        let mut label_of = vec![NONE; n];
        for (l, &t) in b.label_inst.iter().enumerate() {
            if let Some(x) = label_of.get_mut(t as usize) {
                *x = u32_of(l);
            }
        }
        let mut held: Vec<bool> = b
            .local_ty
            .iter()
            .map(|t| cx.counted(*t) != crate::own::Tri::No)
            .collect();
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
            if *m == super::CaptureMode::Shared
                && let Some(x) = held.get_mut(l.idx())
            {
                *x = true;
            }
        }
        let (slot_of, slot_start, slot_word) = part_slots(b);
        let pwords = slot_word.len().div_ceil(64);
        Balance {
            b,
            cx,
            num,
            words: (k as usize).div_ceil(64),
            lwords: b.local_ty.len().div_ceil(64),
            slot_of,
            slot_start,
            slot_word,
            pwords,
            owner,
            is_root,
            loop_of,
            label_of,
            brk: vec![None; b.label_inst.len()],
            cont: vec![None; b.label_inst.len()],
            arms: Vec::new(),
            exit: vec![false; b.local_ty.len()],
            owned: held,
            ops: Vec::new(),
            reported: vec![false; n],
            errs: Vec::new(),
        }
    }

    fn walk(mut self, errs: &mut Vec<VerifyError>) {
        for s in 0..self.b.sub_root.len() {
            let root = self.b.sub_root[s];
            if root as usize >= self.b.len() {
                continue;
            }
            for x in &mut self.exit {
                *x = false;
            }
            for &(sub, l) in &self.b.exit_drops {
                if sub.idx() == s
                    && let Some(x) = self.exit.get_mut(l.idx())
                {
                    *x = true;
                }
            }
            let mut st = vec![0; 3 * self.words + self.lwords + self.pwords];
            // An entry local outside the exit set is a lent parameter the
            // body does not own.
            for l in entry_locals(self.b, s) {
                if self.exit[l] {
                    self.set_full(&mut st, l, true);
                }
            }
            if let Some(mut end) = self.block(root, st) {
                self.tail(root, &mut end);
                self.at_exit(root, &end);
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
        st[set * self.words + (k / 64) as usize] & (1 << (k % 64)) != 0
    }

    fn put(&self, st: &mut Set, set: usize, k: u32, on: bool) {
        let w = set * self.words + (k / 64) as usize;
        if on {
            st[w] |= 1 << (k % 64);
        } else {
            st[w] &= !(1 << (k % 64));
        }
    }

    fn set_full(&self, st: &mut Set, l: usize, on: bool) {
        if !self.owned.get(l).copied().unwrap_or(false) {
            return;
        }
        let w = 3 * self.words + l / 64;
        if let Some(x) = st.get_mut(w) {
            if on {
                *x |= 1 << (l % 64);
            } else {
                *x &= !(1 << (l % 64));
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
    /// go and a `Drop` of the whole may release what is left, but nothing
    /// may use it.
    fn uses(&mut self, x: u32, st: &mut Set) {
        let b = self.b;
        let [a, w] = b.data[x as usize];
        // The value at the root of a move read's or a drop's place, and
        // whether the place is a part of it.
        let taken = match b.tags[x as usize] {
            Tag::Read if w == ReadMode::Move as u32 => value_root(b, a),
            Tag::Drop => value_root(b, a).filter(|&(_, part)| part),
            _ => None,
        };
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
        let local = |word: u32| -> Option<usize> {
            if word & place::BARE_LOCAL != 0 {
                return Some((word & !place::BARE_LOCAL) as usize);
            }
            match b.extra.get(word as usize + 1..word as usize + 3)? {
                [place::ROOT_LOCAL, l] => Some(*l as usize),
                _ => None,
            }
        };
        match b.tags[x as usize] {
            // A local a borrow is stored into holds no count. A store
            // refills what parts were taken.
            Tag::Store if !is_borrow(b, w) => {
                // Into the local itself: one through it, into what it
                // refers to, leaves it as it was.
                if let Some(l) = b.local_in_place(a).map(hd_base::LocalId::idx) {
                    self.set_full(st, l, true);
                    let key = b.len() + l;
                    for s in self.slot_start[key]..self.slot_start[key + 1] {
                        self.put_slot(st, s, false);
                    }
                }
            }
            // A `move` read or a `Drop` of a part of a local takes it.
            Tag::Read | Tag::Drop
                if (b.tags[x as usize] == Tag::Drop || w == ReadMode::Move as u32)
                    && let Some((l, true)) = local_root(b, a) =>
            {
                self.local_part(x, l, st);
            }
            Tag::Drop if a & place::BARE_LOCAL != 0 => {
                self.set_full(st, (a & !place::BARE_LOCAL) as usize, false);
            }
            Tag::Read if a & place::BARE_LOCAL != 0 && w == ReadMode::Move as u32 => {
                self.set_full(st, (a & !place::BARE_LOCAL) as usize, false);
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
            for s in self.slot_start[x as usize]..self.slot_start[x as usize + 1] {
                self.put_slot(st, s, false);
            }
        }
    }

    fn put_slot(&self, st: &mut Set, s: u32, on: bool) {
        let w = 3 * self.words + self.lwords + (s / 64) as usize;
        if on {
            st[w] |= 1 << (s % 64);
        } else {
            st[w] &= !(1 << (s % 64));
        }
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
        let s = self.slot_of[x as usize];
        if s != NONE {
            self.put_slot(st, s, true);
        }
        let (from, to) = (self.slot_start[v as usize], self.slot_start[v as usize + 1]);
        let base = 3 * self.words + self.lwords;
        let taken: Vec<u32> = (from..to)
            .filter(|&s| st[base + (s / 64) as usize] & (1 << (s % 64)) != 0)
            .map(|s| self.slot_word[s as usize])
            .collect();
        let rest = rest_counted(self.b, self.cx, self.b.ty[v as usize], &taken);
        self.put(st, 0, k, rest);
        self.put(st, 1, k, true);
        self.put(st, 2, k, true);
    }

    /// Instruction `x` takes a part of local `l`: the local is empty once
    /// the parts taken on the path cover every counted leaf.
    fn local_part(&mut self, x: u32, l: usize, st: &mut Set) {
        let s = self.slot_of[x as usize];
        if s == NONE {
            return;
        }
        self.put_slot(st, s, true);
        let key = self.b.len() + l;
        let base = 3 * self.words + self.lwords;
        let taken: Vec<u32> = (self.slot_start[key]..self.slot_start[key + 1])
            .filter(|&s| st[base + (s / 64) as usize] & (1 << (s % 64)) != 0)
            .map(|s| self.slot_word[s as usize])
            .collect();
        if !rest_counted(self.b, self.cx, self.b.local_ty[l], &taken) {
            self.set_full(st, l, false);
        }
    }

    /// Block `blk`'s tail, taken by the block's value when its owner uses
    /// it, at the block's end.
    fn tail(&mut self, blk: u32, st: &mut Set) {
        let w = self.b.data[blk as usize][1];
        if w == NONE || Ref(w).as_inst().is_none() || w as usize >= self.b.len() {
            return;
        }
        let owner = if self.is_root[blk as usize] {
            NONE
        } else {
            self.owner[blk as usize]
        };
        let op = crate::own::Operand {
            value: w,
            role: Role::Tail,
            at: crate::own::Loc::Data(1),
        };
        let consumed = consumes(self.b, self.cx, blk as usize, &op, owner);
        self.used(blk, w, consumed, st);
    }

    /// Where a sub-body ends: no value owned, and every local that may
    /// hold a value in the exit set.
    fn at_exit(&mut self, x: u32, st: &Set) {
        for k in 0..self.words * 64 {
            let k = u32_of(k);
            if k / 64 < u32_of(self.words) && self.bit(st, 0, k) {
                let v = self.num.iter().position(|&n| n == k).map_or(NONE, u32_of);
                self.err(
                    x,
                    format!("%{v} is still owned where its sub-body ends at %{x}"),
                );
                break;
            }
        }
        for l in 0..self.b.local_ty.len() {
            let w = 3 * self.words + l / 64;
            if st[w] & (1 << (l % 64)) != 0 && !self.exit[l] {
                self.err(
                    x,
                    format!("local ${l} may hold a value at the exit, which does not drop it"),
                );
                break;
            }
        }
    }

    /// Suspension point `x`, whose cancellation leaves through the exit:
    /// every value owned there has a row in the unwind set, which releases
    /// it, and every local that may hold a value is in the exit set.
    fn at_point(&mut self, x: u32, st: &Set) {
        let rows = &self.b.unwind_drops;
        let at = rows.partition_point(|r| r.0 < x);
        let mut row = rows[at..].iter().take_while(|r| r.0 == x).map(|r| r.1);
        let mut next = row.next();
        for v in 0..self.num.len() {
            let k = self.num[v];
            while next.is_some_and(|w| (w as usize) < v) {
                next = row.next();
            }
            if k != NONE && self.bit(st, 0, k) && next != Some(u32_of(v)) {
                let what =
                    format!("%{v} is owned at suspension point %{x}, whose unwind set lacks it");
                self.err(x, what);
                return;
            }
        }
        for l in 0..self.b.local_ty.len() {
            let w = 3 * self.words + l / 64;
            if st[w] & (1 << (l % 64)) != 0 && !self.exit[l] {
                self.err(
                    x,
                    format!("local ${l} may hold a value at suspension point %{x}, and the exit does not drop it"),
                );
                return;
            }
        }
    }

    /// Where loop `lp` goes round again: no value it computes still owned.
    fn round(&mut self, x: u32, lp: u32, st: &Set) {
        for (v, &k) in self.num.iter().enumerate() {
            if k == NONE || !self.bit(st, 0, k) {
                continue;
            }
            let mut l = self.loop_of[v];
            while l != NONE {
                if l == lp {
                    let what = format!("%{v} is still owned where loop %{lp} goes round at %{x}");
                    self.err(x, what);
                    return;
                }
                l = self.loop_of[l as usize];
            }
        }
    }

    fn is_block(&self, x: u32) -> bool {
        self.b.tags.get(x as usize) == Some(&Tag::Block)
    }

    /// Walks block `blk` from `st`; `None` when every path leaves it by a
    /// jump. Its tail is its owner's to take.
    fn block(&mut self, blk: u32, st: Set) -> Option<Set> {
        if !self.is_block(blk) {
            return Some(st);
        }
        let b = self.b;
        let mut cur = Some(st);
        for &x in b.record(b.data[blk as usize][0]) {
            let Some(s) = cur else {
                break;
            };
            cur = if b.tags[x as usize] == Tag::Block {
                Some(s)
            } else {
                self.inst(x, s)
            };
        }
        let label = self.label_of[blk as usize];
        if label != NONE {
            cur = union(cur, self.brk[label as usize].take());
        }
        cur
    }

    /// Walks block `blk` and takes its tail.
    fn valued(&mut self, blk: u32, st: Set) -> Option<Set> {
        let mut end = self.block(blk, st)?;
        if self.is_block(blk) {
            self.tail(blk, &mut end);
        }
        Some(end)
    }

    fn inst(&mut self, x: u32, mut st: Set) -> Option<Set> {
        let b = self.b;
        let [a, w] = b.data[x as usize];
        match b.tags[x as usize] {
            Tag::If => {
                self.uses_only(x, &mut st);
                let r = b.record(w);
                let (then, els) = (r.first().copied(), r.get(1).copied());
                let t = then.map_or(Some(st.clone()), |t| self.valued(t, st.clone()));
                let e = els.map_or(Some(st.clone()), |e| self.valued(e, st));
                return self.defined(x, union(t, e));
            }
            Tag::And | Tag::Or => {
                self.uses_only(x, &mut st);
                let right = self.valued(w, st.clone());
                return self.defined(x, union(Some(st), right));
            }
            Tag::With => {
                self.uses_only(x, &mut st);
                let end = self.valued(w, st);
                return self.defined(x, end);
            }
            Tag::RefDyn => {
                let end = self.valued(a, st);
                return self.defined(x, end);
            }
            Tag::Loop => {
                let end = self.repeat(x, st);
                return self.defined(x, end);
            }
            Tag::Match => {
                self.uses_only(x, &mut st);
                let end = self.decide(x, st);
                return self.defined(x, end);
            }
            Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => {
                self.uses_only(x, &mut st);
                let r = b.record(w).to_vec();
                let (cases, default) = r.split_at(r.len().saturating_sub(1));
                let mut out = None;
                for &blk in cases.iter().skip(1).step_by(2).chain(default) {
                    if self.is_block(blk) {
                        out = union(out, self.valued(blk, st.clone()));
                    }
                }
                // A switch outside a decision tree has a value.
                return self.defined(x, out);
            }
            Tag::Guard => {
                let s = self.valued(a, st)?;
                let r = b.record(w).to_vec();
                let mut out = None;
                for &blk in &r {
                    out = union(out, self.block(blk, s.clone()));
                }
                return out;
            }
            Tag::Scope => {
                let body = self.valued(a, st);
                let mut out = body.clone();
                if w != NONE
                    && let Some(s) = body
                {
                    for &suite in &b.record(w).to_vec() {
                        if let Some(end) = self.block(suite, s.clone()) {
                            out = Some(end);
                        }
                    }
                }
                return self.defined(x, out);
            }
            Tag::ToArm => {
                if let Some(arm) = self
                    .arms
                    .last_mut()
                    .and_then(|arms| arms.get_mut(a as usize))
                {
                    *arm = union(arm.take(), Some(st));
                }
                return None;
            }
            Tag::Break => {
                self.uses(x, &mut st);
                if let Some(t) = self.brk.get_mut(a as usize) {
                    *t = union(t.take(), Some(st));
                }
                return None;
            }
            Tag::Continue => {
                let target = b.label_inst.get(a as usize).copied().unwrap_or(NONE);
                self.round(x, target, &st);
                if let Some(t) = self.cont.get_mut(a as usize) {
                    *t = union(t.take(), Some(st));
                }
                return None;
            }
            Tag::Return => {
                self.uses(x, &mut st);
                self.at_exit(x, &st);
                return None;
            }
            Tag::Unreachable => return None,
            _ => self.uses(x, &mut st),
        }
        Some(st)
    }

    /// The operand uses of a control instruction, before the blocks it
    /// runs; its value comes after them ([`Balance::defined`]).
    fn uses_only(&mut self, x: u32, st: &mut Set) {
        let b = self.b;
        let mut ops = std::mem::take(&mut self.ops);
        operands(b, x as usize, &mut ops);
        for op in &ops {
            let consumed = consumes(b, self.cx, x as usize, op, NONE);
            self.used(x, op.value, consumed, st);
        }
        self.ops = ops;
    }

    fn defined(&mut self, x: u32, st: Option<Set>) -> Option<Set> {
        let mut st = st?;
        let k = self.num[x as usize];
        if k != NONE {
            self.put(&mut st, 0, k, true);
            self.put(&mut st, 1, k, false);
            self.put(&mut st, 2, k, false);
        }
        Some(st)
    }

    /// A loop: its body from the head state until the head stops growing;
    /// its exits are its `Break`s.
    fn repeat(&mut self, x: u32, st: Set) -> Option<Set> {
        let b = self.b;
        let body = b.data[x as usize][0];
        let label = self.label_of[x as usize] as usize;
        let mut head = st;
        let rounds = 3 * self.words * 64 + self.lwords * 64 + self.pwords * 64 + 2;
        for _ in 0..rounds {
            if let Some(t) = self.brk.get_mut(label) {
                *t = None;
            }
            if let Some(t) = self.cont.get_mut(label) {
                *t = None;
            }
            let end = self.block(body, head.clone());
            if let Some(e) = &end {
                self.round(x, x, e);
            }
            let back = self.cont.get_mut(label).and_then(Option::take);
            let next = union(union(Some(head.clone()), end), back).expect("the head");
            if next == head {
                break;
            }
            head = next;
        }
        self.brk.get_mut(label).and_then(Option::take)
    }

    /// A match: its decision tree from `st`, each arm from the union of
    /// the states its `ToArm`s carry.
    fn decide(&mut self, x: u32, st: Set) -> Option<Set> {
        let b = self.b;
        let r = b.record(b.data[x as usize][1]).to_vec();
        let Some((&decision, arms)) = r.split_first() else {
            return Some(st);
        };
        self.arms.push(vec![None; arms.len()]);
        let _ = self.block(decision, st);
        let entries = self.arms.pop().unwrap_or_default();
        let mut out = None;
        for (&arm, entry) in arms.iter().zip(entries) {
            if let Some(e) = entry {
                out = union(out, self.valued(arm, e));
            }
        }
        let label = self.label_of[x as usize];
        if label != NONE {
            out = union(out, self.brk[label as usize].take());
        }
        out
    }
}

/// The local at the root of the place at `word`, and whether the place is
/// a part of it; `None` for a place with another root.
fn local_root(b: &Body, word: u32) -> Option<(usize, bool)> {
    if word & place::BARE_LOCAL != 0 {
        return Some(((word & !place::BARE_LOCAL) as usize, false));
    }
    let at = word as usize;
    let n = *b.extra.get(at)?;
    match b.extra.get(at + 1..at + 3)? {
        &[place::ROOT_LOCAL, l] if (l as usize) < b.local_ty.len() => Some((l as usize, n > 2)),
        _ => None,
    }
}

/// The instruction value at the root of the place at `word`, and whether
/// the place is a part of it (a projection follows); `None` for a place
/// with another root.
fn value_root(b: &Body, word: u32) -> Option<(u32, bool)> {
    if word & place::BARE_LOCAL != 0 {
        return None;
    }
    let at = word as usize;
    let n = *b.extra.get(at)?;
    match b.extra.get(at + 1..at + 3)? {
        &[place::ROOT_VALUE, v] if Ref(v).as_inst().is_some() && (v as usize) < b.len() => {
            Some((v, n > 2))
        }
        _ => None,
    }
}

/// The part takes of values (a `move` read or a `Drop` of a part): each
/// instruction's slot, one per distinct place of a value; each value's
/// slots, in compressed rows by value; and a place word of each slot.
fn part_slots(b: &Body) -> (Vec<u32>, Vec<u32>, Vec<u32>) {
    let n = b.len();
    let mut takes: Vec<(u32, u32)> = Vec::new();
    for i in 0..n {
        let [a, w] = b.data[i];
        let take = match b.tags[i] {
            Tag::Read => w == ReadMode::Move as u32,
            Tag::Drop => true,
            _ => false,
        };
        if !take {
            continue;
        }
        if let Some((v, true)) = value_root(b, a) {
            takes.push((v, u32_of(i)));
        } else if let Some((l, true)) = local_root(b, a) {
            takes.push((u32_of(n + l), u32_of(i)));
        }
    }
    let rec = |x: u32| {
        let at = b.data[x as usize][0] as usize;
        let len = b.extra[at] as usize;
        &b.extra[at + 1..=at + len]
    };
    takes.sort_by(|p, q| p.0.cmp(&q.0).then_with(|| rec(p.1).cmp(rec(q.1))));
    let mut slot_of = vec![NONE; n];
    let keys = n + b.local_ty.len();
    let mut slot_start = vec![0u32; keys + 1];
    let mut slot_word = Vec::new();
    for (k, &(v, x)) in takes.iter().enumerate() {
        let fresh = k == 0 || takes[k - 1].0 != v || rec(takes[k - 1].1) != rec(x);
        if fresh {
            slot_word.push(b.data[x as usize][0]);
            slot_start[v as usize + 1] += 1;
        }
        slot_of[x as usize] = u32_of(slot_word.len() - 1);
    }
    for v in 0..keys {
        slot_start[v + 1] += slot_start[v];
    }
    (slot_of, slot_start, slot_word)
}

/// Whether value `v`, whose parts at the places `taken` (place words) a
/// `move` read or a `Drop` took, still holds a counted leaf beside them: a
/// counted field that no taken place covers, or a step whose type's parts
/// `cx` does not give.
fn rest_counted(b: &Body, cx: &dyn OwnCx, ty: hd_types::Ty, taken: &[u32]) -> bool {
    let paths: Vec<Vec<Proj>> = taken
        .iter()
        .filter_map(|&w| b.decode_place(w).map(|p| p.proj))
        .collect();
    if paths.len() != taken.len() {
        return true;
    }
    let all: Vec<&[Proj]> = paths.iter().map(Vec::as_slice).collect();
    uncovered(cx, ty, &all, 0, &mut Vec::new())
}

/// Whether a value of type `ty` at depth `depth` of the places `paths`
/// holds a counted leaf that none of them covers.
fn uncovered(
    cx: &dyn OwnCx,
    ty: hd_types::Ty,
    paths: &[&[Proj]],
    depth: usize,
    parts: &mut Vec<hd_types::Ty>,
) -> bool {
    if paths.is_empty() {
        return cx.counted(ty) != crate::own::Tri::No;
    }
    if paths.iter().any(|p| p.len() == depth) {
        return false;
    }
    // One variant per group: a value is one variant on a path.
    let mut variants: Vec<Option<u32>> = Vec::new();
    for p in paths {
        let v = match p[depth] {
            Proj::Variant(vr) => Some(vr),
            Proj::Field(_) => None,
            _ => return true,
        };
        if !variants.contains(&v) {
            variants.push(v);
        }
    }
    for variant in variants {
        let step = depth + usize::from(variant.is_some());
        let group: Vec<&[Proj]> = paths
            .iter()
            .copied()
            .filter(|p| match (variant, p[depth]) {
                (Some(vr), Proj::Variant(x)) => vr == x,
                (None, Proj::Field(_)) => true,
                _ => false,
            })
            .collect();
        if group.iter().any(|p| p.len() == step) {
            continue;
        }
        if !cx.parts(ty, variant, parts) {
            return true;
        }
        let fields = parts.clone();
        for (j, &ft) in fields.iter().enumerate() {
            if cx.counted(ft) == crate::own::Tri::No {
                continue;
            }
            let sub: Vec<&[Proj]> = group
                .iter()
                .copied()
                .filter(|p| p.get(step) == Some(&Proj::Field(u32_of(j))))
                .collect();
            if sub.is_empty() || uncovered(cx, ft, &sub, step + 1, parts) {
                return true;
            }
        }
    }
    false
}

/// The locals of sub-body `s` that hold a value on entry: its parameters,
/// and a closure body's captures.
pub(crate) fn entry_locals(b: &Body, s: usize) -> Vec<usize> {
    let mut out = Vec::new();
    if s == 0 {
        let mut in_sub = vec![false; b.local_ty.len()];
        for r in b.sub_params.iter().skip(1) {
            for k in 0..r.len {
                if let Some(x) = in_sub.get_mut(b.extra[(r.start + 1 + k) as usize] as usize) {
                    *x = true;
                }
            }
        }
        for (l, (&flags, &sub)) in b.local_flags.iter().zip(&in_sub).enumerate() {
            if flags & super::local_flags::PARAM != 0 && !sub {
                out.push(l);
            }
        }
        return out;
    }
    if let Some(r) = b.sub_params.get(s) {
        for k in 0..r.len {
            out.push(b.extra[(r.start + 1 + k) as usize] as usize);
        }
    }
    for i in 0..b.len() {
        if b.tags[i] == Tag::Closure && b.data[i][0] as usize == s && !b.is_instantiation(i) {
            let rec = b.record(b.data[i][1]);
            for c in rec[0] as usize..(rec[0] + rec[1]) as usize {
                out.push(b.cap_local[c].idx());
            }
        }
    }
    out
}
