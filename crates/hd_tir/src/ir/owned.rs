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
    Body, INLINED, NONE, Op, Place, PlaceRoot, Proj, ReadMode, Ref, Root, Tag, Topology,
    VerifyError, is_suspension, local_flags, place, u32_of, verify_structure,
};
use crate::own::flow::{
    Backward, BitArena, Flow, Forward, Suites, backward, forward, get_bit, set_bit,
};
use crate::own::parts::Slots;
use crate::own::{OwnCx, Role, consumes, operands, tracked};

/// Verifies a body after the ownership passes: the structural invariants
/// and invariant 23 ([the module](self)), with the facts `cx` gives the
/// passes.
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
        topo: &topo,
        places: &places,
        addressed: &addressed,
    };
    decided(b, &mut errs);
    takes_outside_suspension(b, &topo, &mut errs);
    lent_refs(b, &mut errs);
    change_places(b, &topo, &mut errs);
    hoisted(b, &topo, own, &mut errs);
    Empties::new(&cx).walk(&mut errs);
    borrow_spans(&cx, &mut errs);
    binding_locals(&cx, own, &mut errs);
    Balance::new(b, &topo, own).walk(&mut errs);
    copies(b, &mut errs);
    errs
}

/// What the inliner's copies promise (`crate::inline`). Each anchor with
/// the `INLINED` bit falls in a `Body::inlined` row; the rows are sorted
/// and disjoint, and each call's anchor is the caller's own. A place in a
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

/// No `Ref` takes its local's count of a cell in a sub-body that holds a
/// suspension point: the frame saves the cell's local at each point, and
/// a resume enters blocks in the middle (move's rule 4).
fn takes_outside_suspension(b: &Body, topo: &Topology, errs: &mut Vec<VerifyError>) {
    let takes = |i: usize| b.tags[i] == Tag::Ref && b.data[i][1] & super::ref_word::TAKES != 0;
    if !(0..b.len()).any(takes) {
        return;
    }
    let subs = &topo.sub;
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
                PlaceRoot::Value(v) if b.is_borrow(v.0) => {
                    let w = b.data[v.0 as usize][0];
                    w & place::BARE_LOCAL != 0 && unowned((w & !place::BARE_LOCAL) as usize)
                }
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
fn change_places(b: &Body, topo: &Topology, errs: &mut Vec<VerifyError>) {
    let n = b.len();
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
            let (blk, cb) = (topo.blk[r], topo.blk[c]);
            let (pos, cp) = (topo.pos[r] as usize, topo.pos[c] as usize);
            let list = if blk == NONE {
                &[][..]
            } else {
                b.record(b.data[blk as usize][0])
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
fn hoisted(b: &Body, topo: &Topology, cx: &dyn OwnCx, errs: &mut Vec<VerifyError>) {
    use super::unique_word::{COVERED, HOISTED};
    use crate::own::hoist;
    let n = b.len();
    let any = (0..n).any(|i| b.tags[i] == Tag::IsUnique && b.data[i][1] != 0);
    if !any && b.stable_lists.is_empty() {
        return;
    }
    let (blk, pos, first) = (&topo.blk, &topo.pos, &topo.first);
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
    topo: &'a Topology,
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
    /// (value, move or drop number) for each move or drop of a place
    /// rooted at an instruction's value, sorted: running that instruction
    /// again makes a new value, which refills the place.
    by_value: Vec<(u32, u32)>,
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
        s.arena.words = self.at.len().div_ceil(64);
        for &root in &b.sub_root {
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
            set_bit(s, k as usize, true);
        }
    }

    /// A `Store` refills every emptied place it covers.
    fn refill(&self, x: u32, s: &mut Set) {
        let Some(q) = &self.cx.places[x as usize] else {
            return;
        };
        for (k, &e) in self.at.iter().enumerate() {
            if get_bit(s, k)
                && let Some(p) = &self.cx.places[e as usize]
                && covers(q, p)
            {
                set_bit(s, k, false);
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
            if get_bit(s, k)
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

impl Forward for Empties<'_> {
    const SUITES: Suites = Suites::FromExits;

    fn stmt(&mut self, b: &Body, x: u32, s: &mut BitArena, st: usize) {
        self.enter(b, x, s, st);
        let st = s.row_mut(st);
        match b.tags[x as usize] {
            Tag::Read | Tag::IsUnique | Tag::Pin => {
                self.check(x, st);
                self.empty(x, st);
            }
            Tag::Drop => self.empty(x, st),
            Tag::Store => self.refill(x, st),
            _ => {}
        }
    }

    /// A new value of `x` (a loop's next round) refills its places.
    fn enter(&mut self, _: &Body, x: u32, s: &mut BitArena, st: usize) {
        let from = self.by_value.partition_point(|&(v, _)| v < x);
        for &(_, k) in self.by_value[from..].iter().take_while(|&&(v, _)| v == x) {
            s.set(st, k, false);
        }
    }
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
        if b.tags[i] != Tag::Store || !b.is_borrow(b.data[i][1]) {
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
            Tag::Store if !b.is_borrow(b.data[i][1]) => "an owned value stored into",
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
        if b.is_borrow(u32_of(i)) {
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
            && b.is_borrow(b.data[i][1])
            && let Some(Place {
                root: PlaceRoot::Local(l),
                proj,
            }) = &cx.places[i]
            && proj.is_empty()
        {
            holds[l.idx()] = b.data[i][1];
        }
    }
    let mut walk = Live {
        cx,
        bit,
        at,
        holds,
        reported: vec![false; n],
        errs: Vec::new(),
    };
    let mut back = Flow::default();
    back.arena.words = walk.at.len().div_ceil(64);
    backward(b, cx.topo, &mut back, &mut walk);
    errs.append(&mut walk.errs);
}

/// The borrow walk's state ([`borrow_spans`]).
struct Live<'a> {
    cx: &'a Cx<'a>,
    /// Each borrow's bit, or `NONE`, and each bit's borrow.
    bit: Vec<u32>,
    at: Vec<u32>,
    holds: Vec<u32>,
    reported: Vec<bool>,
    errs: Vec<VerifyError>,
}

impl Live<'_> {
    /// An error at `x` for the first live borrow whose place `hit` names.
    fn check(&mut self, x: u32, s: &BitArena, live: usize, hit: impl Fn(u32, &Place) -> bool) {
        if self.reported[x as usize] {
            return;
        }
        for (k, &r) in self.at.iter().enumerate() {
            if !s.get(live, u32_of(k)) || r == x {
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
    fn stmt(&mut self, b: &Body, x: u32, s: &mut BitArena, live: usize) {
        let i = x as usize;
        let w = b.data[i][1];
        if self.bit[i] != NONE {
            s.set(live, self.bit[i], false);
        }
        let writes = match b.tags[i] {
            Tag::Store | Tag::Drop | Tag::IsUnique => true,
            Tag::Read => w == ReadMode::Move as u32,
            Tag::Ref => w & super::ref_word::MUT != 0,
            _ => false,
        };
        // A write through a borrow's own value (into the buffer its list
        // header names, or the referent its reference names) keeps that
        // value as it is.
        if writes && let Some(q) = &self.cx.places[i] {
            self.check(x, s, live, |r, p| {
                q.root != PlaceRoot::Value(Ref(r)) && self.cx.overlap(p, q)
            });
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
        if is_suspension(b.tags[i]) {
            self.check(x, s, live, |_, _| true);
        }
    }

    /// A use of value `v`: it, and every borrow it lives on.
    fn uses(&mut self, b: &Body, v: u32, s: &mut BitArena, live: usize) {
        let mut x = v;
        for _ in 0..b.len() {
            let Some(&k) = self.bit.get(x as usize) else {
                return;
            };
            if k == NONE {
                return;
            }
            s.set(live, k, true);
            match self.cx.places[x as usize].as_ref().map(|p| p.root) {
                Some(PlaceRoot::Value(r)) if (r.0 as usize) < b.len() => x = r.0,
                Some(PlaceRoot::Local(l)) if self.holds[l.idx()] != NONE => {
                    x = self.holds[l.idx()];
                }
                _ => return,
            }
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
    topo: &'a Topology,
    cx: &'a dyn OwnCx,
    /// Each tracked value's number among the tracked values, or `NONE`.
    num: Vec<u32>,
    /// Each local's number among the locals, all of them.
    words: usize,
    lwords: usize,
    /// The parts of values and locals that `move` reads and `Drop`s take,
    /// keyed by value, then by `n + local`. The walk keeps a bit per slot
    /// after the locals' bits, so a value taken in parts is owned until
    /// its parts cover every counted leaf.
    slots: Slots,
    pwords: usize,
    /// The exit set of the sub-body being walked, by local, and the locals
    /// the passes own: a counted type's, or one a cell may hold.
    exit: Vec<bool>,
    owned: Vec<bool>,
    ops: Vec<crate::own::Operand>,
    reported: Vec<bool>,
    errs: Vec<VerifyError>,
}

impl<'a> Balance<'a> {
    fn new(b: &'a Body, topo: &'a Topology, cx: &'a dyn OwnCx) -> Self {
        let n = b.len();
        let mut num = vec![NONE; n];
        let mut k = 0u32;
        for (i, x) in num.iter_mut().enumerate() {
            if tracked(b, cx, i) {
                *x = k;
                k += 1;
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
        let takes = crate::own::parts::takes(b, |_| true).filter_map(|(i, a)| {
            let p = b.view(a);
            let key = match p.root {
                _ if p.whole() => return None,
                Root::Value(v) => v,
                Root::Local(l) => u32_of(n) + l,
                _ => return None,
            };
            Some((key, u32_of(i), a))
        });
        let mut slots = Slots::default();
        let pwords = (slots.build(b, n, n + b.local_ty.len(), takes) as usize).div_ceil(64);
        Balance {
            b,
            topo,
            cx,
            num,
            words: (k as usize).div_ceil(64),
            lwords: b.local_ty.len().div_ceil(64),
            slots,
            pwords,
            exit: vec![false; b.local_ty.len()],
            owned: held,
            ops: Vec::new(),
            reported: vec![false; n],
            errs: Vec::new(),
        }
    }

    fn walk(mut self, errs: &mut Vec<VerifyError>) {
        let (b, topo) = (self.b, self.topo);
        let mut s = Flow::default();
        s.arena.words = 3 * self.words + self.lwords + self.pwords;
        for (sub, &root) in b.sub_root.iter().enumerate() {
            if root as usize >= b.len() {
                continue;
            }
            self.exit.fill(false);
            for &(at, l) in &b.exit_drops {
                if at.idx() == sub
                    && let Some(x) = self.exit.get_mut(l.idx())
                {
                    *x = true;
                }
            }
            let st = s.arena.alloc(None);
            // An entry local outside the exit set is a lent parameter the
            // body does not own.
            for l in entry_locals(b, sub) {
                if self.exit[l] {
                    self.set_full(s.arena.row_mut(st), l, true);
                }
            }
            if forward(b, topo, &mut s, &mut self, root, st) {
                self.tail(b, root, &mut s.arena, st);
                self.at_exit(root, s.arena.row(st));
            }
            s.arena.free(st);
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
            set_bit(st, 3 * self.words * 64 + l, on);
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
            Tag::Store if !b.is_borrow(w) => {
                // Into the local itself: one through it, into what it
                // refers to, leaves it as it was.
                if let Some(l) = b.local_in_place(a).map(hd_base::LocalId::idx) {
                    self.set_full(st, l, true);
                    for s in self.slots.of_key(u32_of(b.len() + l)) {
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
            for s in self.slots.of_key(x) {
                self.put_slot(st, s, false);
            }
        }
    }

    fn put_slot(&self, st: &mut Set, s: u32, on: bool) {
        set_bit(st, (3 * self.words + self.lwords) * 64 + s as usize, on);
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
        let rest = self.slots.rest(b, self.cx, key, ty, |s, word| {
            let on = get_bit(st, base + s as usize);
            bad |= on && b.decode_place(word).is_none();
            on
        });
        rest || bad
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
            if get_bit(st, 3 * self.words * 64 + l) && !self.exit[l] {
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
            if get_bit(st, 3 * self.words * 64 + l) && !self.exit[l] {
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
