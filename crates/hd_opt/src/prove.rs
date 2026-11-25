//! Bounds-check elimination over owned TIR (step 6 of
//! `docs/future-work/BACKEND_NEUTRAL_TIR.md`, #2175). Each element step of
//! a place is checked until a proof here clears its check
//! ([`hd_tir::ir::place::ELEM_UNCHECKED`]); the backend writes the check
//! only where the bit is clear. Go's `prove.go` folds its explicit
//! `IsInBounds` values the same way, from induction variables it finds
//! per loop (`findIndVar`), as the rising proof here does.
//!
//! It runs after the ownership passes, whose hoisted `IsUnique` may
//! replace a buffer, so a proof sees the final reads and writes.
//!
//! The proofs:
//! - an element of a private local list that a call already received in
//!   the same straight-line run, at the same index
//!   ([`ElementChecks::repeats_bounds`]);
//! - per loop, a rising index from 0 below an unchanged list's length
//!   ([`Lists::rising`]), a falling index below an index an earlier
//!   access checked ([`Lists::falling`]), and, in release, a masked
//!   index `y & (xs.len() - 1)` behind a test that the length is above
//!   zero ([`Lists::masked`]).
//!
//! In release, a loop that writes no memory then reads each load through
//! a reference once before it, and loses each `if i >= n: panic` guard its
//! test decides ([`loop_loads`], #2368).
//!
//! A list keeps its length where the hoisting pass's region row says so
//! (`Body::stable_lists`). Cost: one pass over the body for its stores,
//! the dominance and loop forest once per body with a loop
//! ([`Nest`]), and per loop the cheap tests, then one walk of its body
//! only once they pass.

use hd_base::LocalId;
use hd_base::SliceSort as _;
use hd_tir::ir::{
    Body, Callee, Coercion, IntrinsicOp, NONE, Nest, Place, PlaceRoot, PrimOp, Proj, Ref, Step,
    Tag, Topology, local_flags,
};
use hd_tir::own::ElementChecks;
use hd_types::{Prim, Ty};

mod loop_loads;
mod unique_flags;

/// The type and call facts the proofs read, which the backend's layout
/// decides: the driver answers them from `hd_layout` and the call record.
pub trait Cx {
    /// Whether `t` is a `List`.
    fn list(&self, t: Ty) -> bool;
    /// Whether the ownership passes track a value of `t`.
    fn tracked(&self, t: Ty) -> bool;
    /// Whether the elements of list type `t` are tracked.
    fn elems_tracked(&self, t: Ty) -> bool;
    /// The name of a hidden local holding a list's length.
    fn length_name(&self) -> hd_base::Symbol;
    /// The name of a hidden local holding a load hoisted out of a loop.
    fn load_name(&self) -> hd_base::Symbol;
    /// Whether every result of `callee` is a counted block nothing else
    /// holds (`hd_tir::own::OwnCx::returns_unique`).
    fn returns_unique(&self, callee: &Callee) -> bool {
        let _ = callee;
        false
    }
    /// Whether `t` is a tuple.
    fn tuple(&self, t: Ty) -> bool;
    /// The type reference type `t` refers to, and whether it is fat.
    fn ref_target(&self, t: Ty) -> Option<(Ty, bool)>;
    /// The type of struct, tuple or newtype field `k` of `t`; `None`
    /// proves nothing behind a field.
    fn field(&self, _t: Ty, _k: u32) -> Option<Ty> {
        None
    }
    /// The intrinsic `callee` names, if it names one.
    fn intrinsic(&self, callee: &Callee) -> Option<String>;
}

/// Clears the bounds check of each element access `b` proves in range.
/// `release` enables the masked proof. A sub-body with an await point is
/// left alone: a resume enters its blocks in the middle, where the
/// dominance the proofs read does not hold. One with none runs straight
/// through, even as a suspending function's (an entry `main!` that never
/// awaits runs as a plain function).
///
/// # Errors
/// The splice of the header statements failing, a malformed body.
pub fn run(b: &mut Body, cx: &dyn Cx, release: bool) -> Result<(), String> {
    let checks = ElementChecks::new(b);
    for i in 0..b.len() {
        if checks.repeats_bounds(i) {
            b.uncheck_elem(i, 0);
        }
    }
    let mut proven = Vec::new();
    let mut headers = Vec::new();
    let topo = b.tags.contains(&Tag::Loop).then(|| Topology::new(b));
    if let Some(topo) = &topo {
        let nest = Nest::new(b, topo);
        let sets = Sets::new(b);
        scans(2 * b.len());
        let mut awaits = vec![false; b.sub_root.len()];
        for i in 0..b.len() {
            if matches!(
                b.tags[i],
                Tag::Await | Tag::AwaitValue | Tag::AwaitAll | Tag::AwaitRace
            ) {
                awaits[topo.sub[i] as usize] = true;
            }
        }
        for l in (0..b.len()).filter(|&l| b.tags[l] == Tag::Loop) {
            let sub = topo.sub[l];
            if awaits[sub as usize] {
                continue;
            }
            let cell = |local: usize| {
                b.storage
                    .binary_search_by(|&(s, x, _)| (s.idx(), x.idx()).cmp(&(sub as usize, local)))
                    .is_ok()
            };
            let lists = Lists {
                b,
                topo,
                nest: &nest,
                cx,
                cell: &cell,
                sets: &sets,
            };
            proven.extend(lists.proofs(hd_tir::ir::u32_of(l), release));
            headers.extend(lists.kept(hd_tir::ir::u32_of(l)));
        }
        headers = outermost(headers, topo, &nest);
        uses(b, cx, topo, &mut headers);
    }
    // Two proofs may clear one access; each record is copied once.
    proven.sort_stable();
    proven.dedup();
    for i in proven {
        // The first element step: `[x [%i] ..]`, `[$l * .xs [%i] ..]`.
        if let Some(k) = b
            .decode_place(b.data[i][0])
            .and_then(|p| p.proj.iter().position(|s| matches!(s, Proj::Elem(_))))
        {
            b.uncheck_elem(i, k);
        }
    }
    if let Some(topo) = &topo
        && !headers.is_empty()
    {
        root_at_headers(b, cx, topo, &headers)?;
    }
    if release {
        loop_loads::run(b, cx)?;
        unique_flags::run(b, cx)?;
    }
    Ok(())
}

/// A list loop `l` keeps whole (a `Body::stable_lists` row with its
/// buffer bit): its header becomes value `%h`, which the loop's element
/// accesses root at, and its length a local written before the loop,
/// which they are checked against and its `ListLen`s read (#2175).
struct Header {
    l: u32,
    root: ListRoot,
    /// The row's place word and the list's type.
    word: u32,
    ty: Ty,
    /// The hoisted `IsUnique` whose value is the header after its check;
    /// with none, a new `Read borrow` of the place before the loop.
    unique: Option<u32>,
    /// Where the length is written: before the loop's hoisted checks,
    /// which a copy keeps the length across, so they stay guarded.
    at: (u32, u32),
    /// The loop's element accesses and `ListLen`s of the list.
    uses: Vec<u32>,
}

/// The headers whose list no enclosing loop already keeps: loops in
/// preorder (an outer loop starts no later and ends later), with per list
/// the stack of loops keeping it that may still enclose the next.
/// O(headers log headers).
fn outermost(mut headers: Vec<Header>, topo: &Topology, nest: &Nest) -> Vec<Header> {
    headers.sort_stable_by_key(|h| (topo.first[h.l as usize], std::cmp::Reverse(h.l)));
    let mut open: hd_base::HashMap<ListRoot, Vec<u32>> = hd_base::HashMap::default();
    headers.retain(|h| {
        let stack = open.entry(h.root.clone()).or_default();
        while stack.last().is_some_and(|&o| !nest.in_loop(h.l, o)) {
            stack.pop();
        }
        let kept = stack.is_empty();
        stack.push(h.l);
        kept
    });
    headers
}

/// Each header's element accesses and `ListLen`s of its list in its loop:
/// one pass over the body, each access's list looked up by its root, then
/// the loop holding it among that list's loops, which [`outermost`] left
/// disjoint, by binary search over their spans (`first..loop`).
/// O(instructions × log headers).
fn uses(b: &Body, cx: &dyn Cx, topo: &Topology, headers: &mut [Header]) {
    let mut spans: hd_base::HashMap<ListRoot, Vec<(u32, u32, usize)>> = hd_base::HashMap::default();
    for (k, h) in headers.iter().enumerate() {
        let span = (topo.first[h.l as usize], h.l, k);
        spans.entry(h.root.clone()).or_default().push(span);
    }
    for row in spans.values_mut() {
        row.sort_stable();
    }
    scans(b.len());
    for i in 0..b.len() {
        let root = match b.tags[i] {
            Tag::Intrinsic if IntrinsicOp::from_u32(b.data[i][0]) == Some(IntrinsicOp::ListLen) => {
                b.record(b.data[i][1])
                    .first()
                    .and_then(|&r| list_root(b, cx, r))
            }
            // `[%xs [%i] ..]`, or a place rooted at a local.
            Tag::Read | Tag::Store => match b.decode_place(b.data[i][0]) {
                Some(Place {
                    root: PlaceRoot::Value(v),
                    proj,
                }) if matches!(proj.first(), Some(Proj::Elem(_))) => list_root(b, cx, v.0),
                _ => rooted_elem(b, i).map(|(root, _)| root),
            },
            _ => None,
        };
        let x = hd_tir::ir::u32_of(i);
        if let Some(row) = root.and_then(|r| spans.get(&r))
            && let Some(&(_, l, k)) = row[..row.partition_point(|s| s.0 <= x)].last()
            && x < l
        {
            headers[k].uses.push(x);
        }
    }
}

/// Writes each header's length local and statements before its loop, in
/// one splice, and roots its loop's accesses at `%h`: an element access
/// becomes `[%h [%i] ..]`, checked against the length local unless a
/// proof cleared it, and a `ListLen` a read of the local. O(instructions
/// plus the headers' uses).
fn root_at_headers(
    b: &mut Body,
    cx: &dyn Cx,
    topo: &Topology,
    headers: &[Header],
) -> Result<(), String> {
    use hd_tir::ir::{ReadMode, place};
    let n = b.len();
    let mut inserts = Vec::new();
    for h in headers {
        let len = hd_tir::ir::u32_of(b.local_ty.len());
        b.local_ty.push(usize_ty());
        b.local_name.push(cx.length_name());
        b.local_syn.push(b.local_syn[h.root.local as usize]);
        b.local_flags
            .push(local_flags::ASSIGNED | local_flags::READ);
        // Insertion `k` is value `n + k` (`insert_batch`).
        let read = hd_tir::ir::u32_of(n + inserts.len());
        let rec = hd_tir::ir::u32_of(b.extra.len());
        b.extra.extend([1, read]);
        let syn = b.syn[h.l as usize];
        for (tag, data, ty) in [
            (Tag::Read, [h.word, ReadMode::Borrow as u32], h.ty),
            (
                Tag::Intrinsic,
                [IntrinsicOp::ListLen as u32, rec],
                usize_ty(),
            ),
            (Tag::Store, [len | place::BARE_LOCAL, read + 1], Ty::VOID),
        ] {
            let (block, pos) = h.at;
            inserts.push(hd_tir::own::BatchInsert {
                block,
                pos,
                tag,
                data,
                ty,
                syn,
            });
        }
        let header = match h.unique {
            Some(u) => {
                b.ty[u as usize] = h.ty;
                u
            }
            None => read,
        };
        for &x in &h.uses {
            let x = x as usize;
            if b.tags[x] == Tag::Intrinsic {
                b.tags[x] = Tag::Read;
                b.data[x] = [len | place::BARE_LOCAL, ReadMode::Copy as u32];
            } else {
                b.reroot_elem(x, Ref(header), LocalId::from_raw(len));
            }
        }
    }
    hd_tir::own::insert_batch(b, topo, &inserts).map(|_| ())
}

/// A list place as a `Body::stable_lists` row names it: a local and a
/// path of fields and dereferences from it (`[$l]`, `[*$l]`,
/// `[*$l .xs]`, #2187).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ListRoot {
    local: u32,
    path: Vec<Proj>,
}

impl ListRoot {
    /// The list at `path` from `local`, which holds fields and
    /// dereferences only.
    fn at(local: u32, path: Vec<Proj>) -> Option<Self> {
        path.iter()
            .all(|s| matches!(s, Proj::Field(_) | Proj::Deref))
            .then_some(ListRoot { local, path })
    }

    /// Whether the list is behind the reference the local holds, for
    /// a local's own list and its reference's referent only.
    fn indirect(&self) -> Option<bool> {
        match self.path.as_slice() {
            [] => Some(false),
            [Proj::Deref] => Some(true),
            _ => None,
        }
    }
}

/// Each local's stores, the locals a `Ref` names, and the element
/// accesses by the local their index reads (built on first use). One pass
/// over the body. Cost: O(instructions).
struct Sets {
    /// Each local's stores, with the value of a store into all of it
    /// (`NONE` for a field's).
    stores: Vec<Vec<(u32, u32)>>,
    referenced: Vec<bool>,
    /// Each block's number of `Break`s to its label.
    breaks: Vec<u32>,
    index_reads: std::sync::OnceLock<(Vec<u32>, Vec<u32>)>,
}

impl Sets {
    fn new(b: &Body) -> Self {
        let locals = b.local_ty.len();
        let mut sets = vec![Vec::new(); locals];
        let mut referenced = vec![false; locals];
        let mut breaks = vec![0; b.len()];
        for i in 0..b.len() {
            if b.tags[i] == Tag::Break
                && let Some(&blk) = b.label_inst.get(b.data[i][0] as usize)
                && let Some(n) = breaks.get_mut(blk as usize)
            {
                *n += 1;
            }
            if let Some((local, whole)) = b.stored_local(i) {
                let value = if whole { b.data[i][1] } else { NONE };
                sets[local.idx()].push((hd_tir::ir::u32_of(i), value));
            }
            if b.tags[i] == Tag::Ref
                && let Some(p) = b.decode_place(b.data[i][0])
                && let PlaceRoot::Local(l) = p.root
                && p.proj.iter().all(|s| matches!(s, Proj::Field(_)))
            {
                referenced[l.idx()] = true;
            }
        }
        Sets {
            stores: sets,
            referenced,
            breaks,
            index_reads: std::sync::OnceLock::new(),
        }
    }
}

/// An exact `usize` induction variable from 0 by 1 below a bound
/// ([`Lists::induction`]).
struct Induction {
    /// The variable, a local.
    current: usize,
    /// The loop test's condition.
    cond: u32,
    /// The store of `i + 1`.
    step: u32,
    /// The bound's source.
    len: u32,
}

/// One body's shapes for the list proofs: its blocks, dominance and loop
/// forest, stores, which locals live in storage, and the type facts.
struct Lists<'a> {
    b: &'a Body,
    topo: &'a Topology,
    nest: &'a Nest,
    cx: &'a dyn Cx,
    /// Whether a local lives in a cell or slot, where a call may reach it.
    cell: &'a dyn Fn(usize) -> bool,
    sets: &'a Sets,
}

/// Each comparison's negation, on `usize` operands.
const NEGATION: [(PrimOp, PrimOp); 6] = [
    (PrimOp::Lt, PrimOp::Ge),
    (PrimOp::Le, PrimOp::Gt),
    (PrimOp::Gt, PrimOp::Le),
    (PrimOp::Ge, PrimOp::Lt),
    (PrimOp::Eq, PrimOp::Ne),
    (PrimOp::Ne, PrimOp::Eq),
];

fn usize_ty() -> Ty {
    Ty::prim(Prim::Usize)
}

/// The local that value `r` reads when it is a plain local read.
#[must_use]
pub fn read_local(b: &Body, r: u32) -> Option<usize> {
    let i = Ref(r).as_inst()?.0 as usize;
    (i < b.len())
        .then(|| b.local_read(i))
        .flatten()
        .map(hd_base::LocalId::idx)
}

/// The list and index of `Read` or `Store` `i` whose place's first
/// element step indexes a list a local roots through fields and
/// dereferences (`[$l [%i] ..]`, `[$l * [%i] ..]`, `[$l * .xs [%i] ..]`).
fn rooted_elem(b: &Body, i: usize) -> Option<(ListRoot, u32)> {
    if !matches!(b.tags[i], Tag::Read | Tag::Store) {
        return None;
    }
    let mut p = b.decode_place(b.data[i][0])?;
    let PlaceRoot::Local(l) = p.root else {
        return None;
    };
    let (k, ix) = p.proj.iter().enumerate().find_map(|(k, s)| match *s {
        Proj::Elem(ix) => Some((k, ix)),
        _ => None,
    })?;
    p.proj.truncate(k);
    Some((ListRoot::at(l.raw(), p.proj)?, ix.0))
}

/// The list root value `r` reads: a local's list, or the list a
/// reference it holds or reads refers to.
fn list_root(b: &Body, cx: &dyn Cx, r: u32) -> Option<ListRoot> {
    let i = Ref(r).as_inst()?.0 as usize;
    if !cx.list(b.ty[i]) {
        return None;
    }
    match b.tags[i] {
        Tag::Read if let Some(l) = b.local_read(i) => Some(ListRoot {
            local: l.raw(),
            path: Vec::new(),
        }),
        // A field, element or payload of a value, not a read through a
        // reference.
        Tag::Read
            if b.read_step(i)
                .is_some_and(|(_, s)| !matches!(s, Step::DerefField(_))) =>
        {
            None
        }
        // A list a local roots through fields and dereferences
        // (`[*$l]`, `[$s .xs]`, `[*$l .xs]`), or one a reference value
        // read from a local refers to (`[*%r .xs]`).
        Tag::Read => {
            let p = b.decode_place(b.data[i][0])?;
            let local = match p.root {
                PlaceRoot::Local(l) => l.raw(),
                PlaceRoot::Value(v) if p.proj.first() == Some(&Proj::Deref) => {
                    hd_tir::ir::u32_of(read_local(b, v.0)?)
                }
                _ => return None,
            };
            ListRoot::at(local, p.proj)
        }
        Tag::Coerce => list_root(b, cx, b.data[i][0]),
        _ => None,
    }
}

/// The list value whose element `Store` `i` writes (`[%xs [%i] ..]`).
fn elem_store_list(b: &Body, i: usize) -> Option<(u32, u32)> {
    if b.tags[i] != Tag::Store {
        return None;
    }
    let p = b.decode_place(b.data[i][0])?;
    match (p.root, p.proj.first()) {
        (PlaceRoot::Value(r), Some(Proj::Elem(index))) => Some((r.0, index.0)),
        _ => None,
    }
}

/// The index of element access `x` in any form [`Lists::elem_access`]
/// reads, from the body alone.
fn access_index(b: &Body, x: usize) -> Option<u32> {
    if let Some((_, index)) = b.element_read(x) {
        return Some(index);
    }
    if let Some((_, index)) = elem_store_list(b, x) {
        return Some(index);
    }
    rooted_elem(b, x).map(|(_, index)| index)
}

impl Lists<'_> {
    /// The loop's proven element accesses: the rising and falling proofs
    /// for a canonical loop test, and in release the masked one.
    fn proofs(&self, l: u32, release: bool) -> Vec<usize> {
        let mut out = Vec::new();
        if let Some((test, false)) = self.loop_guard(l) {
            out.extend(self.rising(l, test));
            out.extend(self.falling(l, test));
        }
        if release {
            out.extend(self.masked(l));
        }
        out.into_iter().map(|x| x as usize).collect()
    }

    /// The lists loop `l` keeps whole (`Body::stable_lists` rows with the
    /// buffer bit) that the backend holds whole: a local's own list or
    /// its thin reference's referent, in no cell, whose elements hold no
    /// counts. Each with its hoisted `IsUnique`. O(the loop's rows and
    /// hoisted checks).
    fn kept(&self, l: u32) -> Vec<Header> {
        use hd_tir::ir::unique_word::{DEFERRED_CACHE, DEFERRED_INIT, HOISTED};
        let b = self.b;
        let mut out: Vec<Header> = Vec::new();
        // The hoisted checks waiting for the loop stand right before it:
        // the backend runs one any other statement follows there.
        let (blk, pos) = (self.topo.blk[l as usize], self.topo.pos[l as usize]);
        let before = &b.record(b.data[blk as usize][0])[..pos as usize];
        let first = before
            .iter()
            .rposition(|&u| {
                b.tags[u as usize] != Tag::IsUnique
                    || !matches!(
                        b.data[u as usize][1],
                        HOISTED | DEFERRED_INIT | DEFERRED_CACHE
                    )
            })
            .map_or(0, |k| k + 1);
        let mine = &before[first..];
        let at = (blk, hd_tir::ir::u32_of(first));
        for (root, [_, word, _, _, buffer]) in stable_rows(b, l) {
            let Some(indirect) = root.indirect().filter(|_| buffer == 1) else {
                continue;
            };
            let local = root.local as usize;
            let mut ty = b.local_ty[local];
            if indirect {
                ty = self.cx.ref_target(ty).map_or(ty, |(target, _)| target);
            }
            if (self.cell)(local) || !self.cx.list(ty) || self.cx.elems_tracked(ty) {
                continue;
            }
            // A row keeps its buffer only under an eager hoisted check,
            // or with no live check in the loop (`hoist::hoist_loop`).
            let unique = mine.iter().copied().find(|&u| {
                b.data[u as usize][1] == HOISTED
                    && b.decode_place(b.data[u as usize][0]).is_some_and(|q| {
                        q.root == PlaceRoot::Local(LocalId::from_raw(root.local))
                            && q.proj == root.path
                    })
            });
            out.push(Header {
                l,
                root,
                word,
                ty,
                unique,
                at,
                uses: Vec::new(),
            });
        }
        out
    }

    fn dominates(&self, d: u32, j: u32) -> bool {
        self.nest.dominates(self.topo, d, j)
    }

    fn in_loop(&self, i: u32, l: u32) -> bool {
        self.nest.in_loop(i, l)
    }

    fn loop_body(&self, l: u32) -> Vec<u32> {
        let out = self.nest.loop_body(l);
        scans(out.len());
        out
    }

    fn ty_of(&self, r: u32) -> Ty {
        if r == NONE {
            return Ty::VOID;
        }
        match Ref(r).as_inst() {
            Some(i) => self.b.ty[i.0 as usize],
            None => self.b.consts[(r & !Ref::CONST_BIT) as usize].0,
        }
    }

    /// The value of `usize` constant `r`.
    fn const_usize(&self, r: u32) -> Option<u64> {
        if r == NONE || Ref(r).as_inst().is_some() {
            return None;
        }
        let (ty, bits) = self.b.consts[(r & !Ref::CONST_BIT) as usize];
        (ty == usize_ty()).then_some(bits & 0xffff_ffff)
    }

    fn is_op(&self, r: u32, tag: Tag, op: u32) -> bool {
        Ref(r).as_inst().is_some()
            && self.b.tags[r as usize] == tag
            && self.b.data[r as usize][0] == op
    }

    fn list_root(&self, r: u32) -> Option<ListRoot> {
        list_root(self.b, self.cx, r)
    }

    /// Whether read `i` projects a struct or newtype field of a value.
    fn field_read(&self, i: usize) -> bool {
        matches!(self.b.read_step(i), Some((base, Step::Field(_))) if !self.cx.tuple(self.ty_of(base)))
    }

    fn scalar_loop_test(&self, i: u32, budget: &mut usize) -> bool {
        if *budget == 0 {
            return false;
        }
        *budget -= 1;
        let b = self.b;
        let [a, w] = b.data[i as usize];
        if self.cx.tracked(b.ty[i as usize]) {
            // The list a test measures (`while i < xs.len()`, #2186).
            return b.tags[i as usize] == Tag::Read && self.list_root(i).is_some();
        }
        match b.tags[i as usize] {
            Tag::Read if let Some(l) = b.local_read(i as usize) => !(self.cell)(l.idx()),
            Tag::Read => self.field_read(i as usize),
            Tag::Prim | Tag::Coerce => true,
            Tag::Intrinsic => IntrinsicOp::from_u32(a) == Some(IntrinsicOp::ListLen),
            Tag::And | Tag::Or => self.scalar_loop_test(w, budget),
            Tag::Block => b
                .record(a)
                .iter()
                .filter(|&&x| b.tags[x as usize] != Tag::Block)
                .all(|&x| self.scalar_loop_test(x, budget)),
            _ => false,
        }
    }

    /// A canonical loop test, before its body: the `If` after a scalar
    /// prefix one of whose branches only breaks out of the loop, and
    /// whether that branch is the first.
    fn loop_guard(&self, l: u32) -> Option<(u32, bool)> {
        let (test, prefix, [first, second]) = hd_tir::own::loop_test(self.b, l)?;
        let mut budget = self.b.len();
        let scalar = hd_tir::own::loop_statements(self.b, l)
            .take(prefix)
            .all(|x| self.scalar_loop_test(x, &mut budget));
        (first != second && scalar).then_some((test, first))
    }

    /// Follow immutable scalar aliases and fields of a just-built struct.
    /// A changed or addressable binding never supplies a fact.
    fn source(&self, mut r: u32) -> u32 {
        let b = self.b;
        for _ in 0..32 {
            let Some(i) = Ref(r).as_inst() else { break };
            let i = i.0 as usize;
            let [a, w] = b.data[i];
            match b.tags[i] {
                Tag::Read
                    if let Some(l) = read_local(b, r)
                        && !(self.cell)(l)
                        && b.local_flags[l]
                            & (local_flags::CAPTURED | local_flags::CAPTURED_ASSIGNED)
                            == 0 =>
                {
                    let [(set, value)] = self.sets.stores[l].as_slice() else {
                        break;
                    };
                    if !self.dominates(*set, r) {
                        break;
                    }
                    r = *value;
                }
                Tag::Coerce if b.record(w).first() == Some(&(Coercion::Identity as u32)) => r = a,
                // An inlined call's value: its scope's one way out.
                Tag::Scope
                    if let Some(v) = b.wrapper_value(i)
                        && self.sets.breaks.get(a as usize) == Some(&1) =>
                {
                    r = v;
                }
                Tag::Read
                    if self.field_read(i)
                        && let Some((from, Step::Field(k))) = b.read_step(i) =>
                {
                    let Some(bi) = Ref(self.source(from)).as_inst() else {
                        break;
                    };
                    if b.tags[bi.0 as usize] != Tag::NewStruct {
                        break;
                    }
                    let Some(&value) = b.record(b.data[bi.0 as usize][1]).get(k as usize) else {
                        break;
                    };
                    r = value;
                }
                _ => break,
            }
        }
        r
    }

    /// The list and `usize` index of element access `x`: an element read
    /// or store of a list value (`[%xs [%i]]`), or of a local's list or
    /// the list its reference refers to ([`rooted_elem`]).
    fn elem_access(&self, x: usize) -> Option<(ListRoot, u32)> {
        let b = self.b;
        let (root, index) = if let Some((list, index)) = b.element_read(x) {
            (self.list_root(list)?, index)
        } else if let Some((list, index)) = elem_store_list(b, x) {
            (self.list_root(list)?, index)
        } else {
            let (root, index) = rooted_elem(b, x)?;
            // A map's entry is an element step too.
            let mut ty = b.local_ty[root.local as usize];
            for step in &root.path {
                ty = match *step {
                    Proj::Field(k) => self.cx.field(ty, k)?,
                    _ => self.cx.ref_target(ty)?.0,
                };
            }
            if !self.cx.list(ty) {
                return None;
            }
            (root, index)
        };
        (self.ty_of(index) == usize_ty()).then_some((root, index))
    }

    /// The exact `usize` induction of loop `l` whose `test` goes on while
    /// `i < n`: `i` set to 0 before the loop and to `i + 1` in it, and the
    /// bound `n`'s source, set before the loop or read before the test on
    /// each iteration.
    fn induction(&self, l: u32, test: u32) -> Option<Induction> {
        let b = self.b;
        let cond = b.data[test as usize][0];
        // Where an `&&` test holds, so does its first operand.
        let mut lt = cond;
        while Ref(lt).as_inst().is_some() && b.tags[lt as usize] == Tag::And {
            lt = b.data[lt as usize][0];
        }
        if !self.is_op(lt, Tag::Prim, PrimOp::Lt as u32) {
            return None;
        }
        let [cur, end] = *b.record(b.data[lt as usize][1]) else {
            return None;
        };
        let current = read_local(b, cur)?;
        let bound = read_local(b, end);
        if self.ty_of(cur) != usize_ty()
            || self.ty_of(end) != usize_ty()
            || [Some(current), bound]
                .into_iter()
                .flatten()
                .any(|x| (self.cell)(x) || b.local_flags[x] & local_flags::CAPTURED_ASSIGNED != 0)
        {
            return None;
        }
        let [init, step] = init_and_step(&self.sets.stores[current], |x| self.in_loop(x, l))?;
        if self.in_loop(init.0, l)
            || !self.in_loop(step.0, l)
            || !self.dominates(init.0, l)
            || self.const_usize(self.source(init.1)) != Some(0)
            || !self.is_op(step.1, Tag::Prim, PrimOp::Add as u32)
        {
            return None;
        }
        let [before, one] = *b.record(b.data[step.1 as usize][1]) else {
            return None;
        };
        if read_local(b, before) != Some(current) || self.const_usize(one) != Some(1) {
            return None;
        }
        // The bound: a local set once before the loop (`for i in 0..n`),
        // or a value the test reads on each iteration (`while i <
        // xs.len()`, #2186).
        let len = match bound {
            Some(bound) => {
                let [(bound_set, value)] = self.sets.stores[bound].as_slice() else {
                    return None;
                };
                if !self.dominates(*bound_set, l) {
                    return None;
                }
                self.source(*value)
            }
            None => self.source(end),
        };
        // A constant bound holds everywhere.
        let held = Ref(len).as_inst().is_none()
            || self.computed_before(len, l)
            || bound.is_none() && self.computed_before(len, lt);
        held.then_some(Induction {
            current,
            cond,
            step: step.0,
            len,
        })
    }

    /// Whether value `v` is computed on every path to `j` before it: it
    /// dominates `j`, or it is the one way out of an inlined call's
    /// scope that does, as `n` in `for i in 0..xs.len()`.
    fn computed_before(&self, v: u32, j: u32) -> bool {
        let b = self.b;
        let mut v = v;
        for _ in 0..8 {
            if self.dominates(v, j) {
                return true;
            }
            let blk = self.topo.blk[v as usize];
            let s = blk.wrapping_add(1) as usize;
            if blk == NONE
                || b.tags.get(s) != Some(&Tag::Scope)
                || b.data[s][0] != blk
                || b.wrapper_value(s) != Some(v)
                || self.sets.breaks.get(blk as usize) != Some(&1)
            {
                return false;
            }
            v = hd_tir::ir::u32_of(s);
        }
        false
    }

    /// Whether `index` is induction `ind`'s value where `x` reads it:
    /// read after the test and before the step, and before `x`.
    fn induction_index(&self, ind: &Induction, index: u32, x: u32) -> bool {
        let index = self.source(index);
        read_local(self.b, index) == Some(ind.current)
            && self.dominates(ind.cond, index)
            && self.dominates(index, ind.step)
            && self.dominates(index, x)
    }

    /// Exact `usize` induction from zero to an unchanged list's length,
    /// `while i < n` or `while i < xs.len()`, with `i = i + 1` in the loop.
    fn rising(&self, l: u32, test: u32) -> Vec<u32> {
        let b = self.b;
        let Some(ind) = self.induction(l, test) else {
            return Vec::new();
        };
        let len = ind.len;
        if !self.is_op(len, Tag::Intrinsic, IntrinsicOp::ListLen as u32) {
            return Vec::new();
        }
        let Some(root) = b
            .record(b.data[len as usize][1])
            .first()
            .and_then(|&list| self.list_root(list))
        else {
            return Vec::new();
        };
        if !self.keeps_length(&root, l, len) {
            return Vec::new();
        }
        self.loop_body(l)
            .into_iter()
            .filter(|&x| {
                // A read or a store: the same place at the same index.
                let Some((list, index)) = self.elem_access(x as usize) else {
                    return false;
                };
                list == root && self.induction_index(&ind, index, x)
            })
            .collect()
    }

    /// Whether `usize` local `l` changes only by its own stores.
    fn plain_index_local(&self, l: usize) -> bool {
        !(self.cell)(l)
            && self.b.local_flags[l] & (local_flags::CAPTURED | local_flags::CAPTURED_ASSIGNED) == 0
            && !self.sets.referenced[l]
    }

    /// Whether instruction `x` runs only once loop test `test`, an `If`
    /// whose second arm leaves the loop, went on: in its first arm or
    /// after it.
    fn after_test(&self, test: u32, x: u32) -> bool {
        let go_on = self
            .b
            .record(self.b.data[test as usize][1])
            .first()
            .copied();
        self.dominates(test, x) || go_on.is_some_and(|arm| self.nest.child_above(arm, x).is_some())
    }

    /// Falling indices: in loop `l`, whose `test` goes on while `v > 0`
    /// (or `v != 0`), a `usize` local `v` that the loop changes only by
    /// one `v = v - 1` after the test, and that is set once before the
    /// loop from a local `u` that a dominating access of list `xs` used as
    /// its index, unchanged since, never grows past that index. While
    /// `xs` keeps its length, from that access through the loop, `xs[v]`
    /// anywhere in the loop and `xs[v - 1]` read after the test and before
    /// the decrement are within bounds.
    fn falling(&self, l: u32, test: u32) -> Vec<u32> {
        let b = self.b;
        let cond = b.data[test as usize][0];
        if Ref(cond).as_inst().is_none()
            || b.tags[cond as usize] != Tag::Prim
            || ![PrimOp::Gt as u32, PrimOp::Ne as u32].contains(&b.data[cond as usize][0])
        {
            return Vec::new();
        }
        let [cur, zero] = *b.record(b.data[cond as usize][1]) else {
            return Vec::new();
        };
        let Some(v) = read_local(b, cur) else {
            return Vec::new();
        };
        if self.ty_of(cur) != usize_ty()
            || self.const_usize(zero) != Some(0)
            || !self.plain_index_local(v)
        {
            return Vec::new();
        }
        let Some([init, dec]) = init_and_step(&self.sets.stores[v], |x| self.in_loop(x, l)) else {
            return Vec::new();
        };
        // The decrement runs once per iteration at most, after the test.
        if self.in_loop(init.0, l)
            || !self.dominates(init.0, l)
            || self.topo.loop_of[dec.0 as usize] != l
            || !self.is_op(dec.1, Tag::Prim, PrimOp::Sub as u32)
        {
            return Vec::new();
        }
        let [before, one] = *b.record(b.data[dec.1 as usize][1]) else {
            return Vec::new();
        };
        if read_local(b, before) != Some(v)
            || self.const_usize(one) != Some(1)
            || self.topo.loop_of[before as usize] != l
            || !self.after_test(test, before)
        {
            return Vec::new();
        }
        // The value `v` enters with: local `u`, read where it is set.
        let Some(u) = read_local(b, init.1) else {
            return Vec::new();
        };
        if self.ty_of(init.1) != usize_ty() || !self.plain_index_local(u) {
            return Vec::new();
        }
        // The lists an access checked that value against: its index reads
        // `u`, which no store changes between that read and `v`'s.
        let (at, reads) = self.sets.index_reads.get_or_init(|| {
            let mut edges = Vec::new();
            for x in 0..b.len() {
                if let Some(local) = access_index(b, x).and_then(|r| read_local(b, r)) {
                    edges.push((hd_tir::ir::u32_of(local), hd_tir::ir::u32_of(x)));
                }
            }
            scans(b.len());
            csr(b.local_ty.len(), &edges)
        });
        let mut roots: Vec<ListRoot> = Vec::new();
        for &x in &reads[at[u] as usize..at[u + 1] as usize] {
            let Some((root, index)) = self.elem_access(x as usize) else {
                continue;
            };
            if roots.contains(&root)
                || read_local(b, index) != Some(u)
                || !self.dominates(x, init.1)
            {
                continue;
            }
            // Every statement of the index read's block through the one
            // that holds the store of `v`: a path leaving them reaches
            // that store again only through the read.
            let blk = self.topo.blk[index as usize];
            let Some(holder) = self.nest.child_above(blk, init.0) else {
                continue;
            };
            let from = self.topo.pos[index as usize] as usize;
            let to = self.topo.pos[holder as usize] as usize;
            let between = self.nest.statements(b, blk, from, to);
            scans(between.len());
            if between.iter().any(|&y| {
                b.stored_local(y as usize)
                    .is_some_and(|(s, _)| s.idx() == u)
            }) {
                continue;
            }
            if self.keeps_length(&root, l, x) {
                roots.push(root);
            }
        }
        if roots.is_empty() {
            return Vec::new();
        }
        self.loop_body(l)
            .into_iter()
            .filter(|&y| {
                let Some((root, index)) = self.elem_access(y as usize) else {
                    return false;
                };
                if !roots.contains(&root) {
                    return false;
                }
                // A read of `v` in the loop, which never exceeds its entry
                // value.
                let index = self.source(index);
                if read_local(b, index) == Some(v) {
                    return self.in_loop(index, l);
                }
                // `v - 1` where the test found `v` above 0.
                if !self.is_op(index, Tag::Prim, PrimOp::Sub as u32) {
                    return false;
                }
                let [r, one] = *b.record(b.data[index as usize][1]) else {
                    return false;
                };
                read_local(b, r) == Some(v)
                    && self.const_usize(one) == Some(1)
                    && self.after_test(test, r)
                    && self.dominates(r, dec.0)
            })
            .collect()
    }

    /// Masked indices (#1713): an access `xs[y & (n - 1)]` among loop
    /// `l`'s own instructions, the `&`'s operands in either order, where
    /// `n` is `xs.len()` and the `&` runs only once a test showed a length
    /// of `xs` above zero ([`Lists::positive_operand`]). Unsigned,
    /// `y & m <= m`, and with the length above zero `n - 1` does not wrap.
    /// `xs` keeps its length from each of the two length reads through
    /// the loop. Cost: the loop's own instructions, and a walk of the
    /// enclosing blocks of each masked access.
    fn masked(&self, l: u32) -> Vec<u32> {
        let own = self.nest.own(l);
        scans(own.len());
        if !own
            .iter()
            .any(|&x| self.is_op(x, Tag::Prim, PrimOp::BitAnd as u32))
        {
            return Vec::new();
        }
        own.iter()
            .copied()
            .filter(|&x| {
                let Some((root, index)) = self.elem_access(x as usize) else {
                    return false;
                };
                let Some((n, and)) = self.masked_len(&root, index) else {
                    return false;
                };
                let Some(t) = self.positive_len(&root, and) else {
                    return false;
                };
                let keeps = |d: u32| self.dominates(d, x) && self.keeps_length(&root, l, d);
                keeps(n) && keeps(t)
            })
            .collect()
    }

    /// The `xs.len()` read of list `root` that value `r` stands for.
    fn len_of(&self, root: &ListRoot, r: u32) -> Option<u32> {
        let n = self.source(r);
        (self.is_op(n, Tag::Intrinsic, IntrinsicOp::ListLen as u32)
            && self
                .b
                .record(self.b.data[n as usize][1])
                .first()
                .is_some_and(|&list| self.list_root(list).as_ref() == Some(root)))
        .then_some(n)
    }

    /// The length read `n` and the `&` of index `index` when it is
    /// `y & (n - 1)` in either order, `n` a length of list `root`.
    fn masked_len(&self, root: &ListRoot, index: u32) -> Option<(u32, u32)> {
        let and = self.source(index);
        if !self.is_op(and, Tag::Prim, PrimOp::BitAnd as u32) {
            return None;
        }
        let mask = |m: u32| self.len_of(root, self.minus_one_of(self.source(m))?);
        let [y, z] = *self.b.record(self.b.data[and as usize][1]) else {
            return None;
        };
        Some((mask(y).or_else(|| mask(z))?, and))
    }

    /// A length read of list `root` that a test showed above zero before
    /// `at` runs: a test whose arm holds `at`, or an earlier statement of
    /// a block around `at` whose other arm never falls through.
    fn positive_len(&self, root: &ListRoot, at: u32) -> Option<u32> {
        let b = self.b;
        let shown = |test: u32, outcome: bool| {
            let c = self.source(b.data[test as usize][0]);
            self.positive_operand(c, outcome)
                .and_then(|n| self.len_of(root, n))
        };
        for (blk, pos) in Nest::block_path(self.topo, b, at)? {
            let owner = self.topo.owner[blk as usize];
            if owner != NONE && b.tags[owner as usize] == Tag::If {
                let arms = b.record(b.data[owner as usize][1]);
                if let Some(n) = shown(owner, arms.first() == Some(&blk)) {
                    return Some(n);
                }
            }
            for &s in &b.record(b.data[blk as usize][0])[..pos] {
                if b.tags[s as usize] != Tag::If {
                    continue;
                }
                let arms = b.record(b.data[s as usize][1]);
                for (k, outcome) in [(1, true), (0, false)] {
                    if let Some(&other) = arms.get(k)
                        && other != NONE
                        && self.arm_exit(other, s).is_some()
                        && let Some(n) = shown(s, outcome)
                    {
                        return Some(n);
                    }
                }
            }
        }
        None
    }

    /// Whether list `root` keeps its length from `from`, which dominates
    /// loop `l` or runs in it, through the loop: the hoisting pass's row
    /// for the list at the loop (`Body::stable_lists`), whose region, when
    /// `from` runs before the loop, starts before `from` and ends after
    /// the statement of `from`'s block that holds the loop. A frame cell's
    /// list has no row. O(log rows + the loop's rows + the block depth).
    fn keeps_length(&self, root: &ListRoot, l: u32, from: u32) -> bool {
        let Some([_, _, first, after, _]) =
            stable_rows(self.b, l).find_map(|(r, w)| (r == *root).then_some(w))
        else {
            return false;
        };
        if self.in_loop(from, l) {
            return true;
        }
        self.nest
            .child_above(self.topo.blk[from as usize], l)
            .is_some_and(|holder| (first == NONE || first < from) && holder < after)
    }

    /// The operand that `Prim` condition `c` shows above zero when it
    /// comes out `outcome`, on `usize`: `a < n` puts `n` above `a`, so
    /// above zero, and `n != 0` and `n >= 1` do directly; a failed
    /// comparison is its negation, as is `!c`.
    fn positive_operand(&self, c: u32, outcome: bool) -> Option<u32> {
        let b = self.b;
        if Ref(c).as_inst().is_none() || b.tags[c as usize] != Tag::Prim {
            return None;
        }
        let op = PrimOp::from_u32(b.data[c as usize][0])?;
        let [x, y] = *b.record(b.data[c as usize][1]) else {
            // `!c` comes out the other way: of the one-operand operations
            // only `!` gives a `bool`.
            let &[inner] = b.record(b.data[c as usize][1]).first_chunk::<1>()?;
            return (op == PrimOp::Not).then(|| self.positive_operand(inner, !outcome))?;
        };
        if self.ty_of(x) != usize_ty() {
            return None;
        }
        // On `usize` operands every `bool` operation is a comparison.
        let op = if outcome {
            op
        } else {
            NEGATION.iter().find(|(o, _)| *o == op)?.1
        };
        let at_least_one = |r: u32| self.const_usize(r).is_some_and(|k| k >= 1);
        match op {
            PrimOp::Lt => Some(y),
            PrimOp::Gt => Some(x),
            PrimOp::Le if at_least_one(x) => Some(y),
            PrimOp::Ge if at_least_one(y) => Some(x),
            PrimOp::Ne if self.const_usize(y) == Some(0) => Some(x),
            PrimOp::Ne if self.const_usize(x) == Some(0) => Some(y),
            PrimOp::Eq if at_least_one(y) => Some(x),
            PrimOp::Eq if at_least_one(x) => Some(y),
            _ => None,
        }
    }

    /// How arm `arm` of `If` `x` ends when it never ends normally: its
    /// last statement returns, panics, traps, or breaks or continues to a
    /// label past `x`, and no break inside it ends at the arm or at `x`.
    /// `Some(true)` when nothing in it leaves for a label outside it, so
    /// the run ends there; `Some(false)` when it may go on past the block
    /// that holds `x`. `None` when it may fall through.
    fn arm_exit(&self, arm: u32, x: u32) -> Option<bool> {
        let b = self.b;
        let last = *(b.tags[arm as usize] == Tag::Block)
            .then(|| b.record(b.data[arm as usize][0]))?
            .iter()
            .rev()
            .find(|&&s| b.tags[s as usize] != Tag::Block)?;
        let first = self.topo.first[arm as usize];
        let target = |i: u32| b.label_inst[b.data[i as usize][0] as usize];
        let ends = match b.tags[last as usize] {
            Tag::Return | Tag::Unreachable => true,
            Tag::Break | Tag::Continue if target(last) != arm && target(last) != x => false,
            Tag::Call if self.calls_panic(last) => true,
            _ => return None,
        };
        let (mut inside, mut past) = (true, true);
        for i in first..arm {
            if matches!(b.tags[i as usize], Tag::Break | Tag::Continue) {
                let t = target(i);
                past &= t != arm && t != x;
                inside &= (first..arm).contains(&t);
            }
        }
        past.then_some(ends && inside)
    }

    /// The intrinsic call `s` makes, if it calls one.
    fn intrinsic(&self, s: u32) -> Option<String> {
        let b = self.b;
        (b.tags[s as usize] == Tag::Call)
            .then(|| Callee::from_words(b.record(b.data[s as usize][0])))
            .flatten()
            .and_then(|c| self.cx.intrinsic(&c))
    }

    /// Whether instruction `s` calls the panic intrinsic.
    fn calls_panic(&self, s: u32) -> bool {
        self.intrinsic(s).as_deref() == Some("panic")
    }

    /// The `x` of instruction `v` when it is `x - 1`, or
    /// `x.wrapping_sub(1)`, which is the same subtraction in a release
    /// build.
    fn minus_one_of(&self, v: u32) -> Option<u32> {
        let b = self.b;
        let i = Ref(v).as_inst()?.0 as usize;
        let [r, one] = match b.tags[i] {
            Tag::Prim if b.data[i][0] == PrimOp::Sub as u32 => {
                *b.record(b.data[i][1]).first_chunk::<2>()?
            }
            // A call's record ends with three words past its arguments.
            Tag::Call if self.intrinsic(v).as_deref() == Some("int_wrapping_sub") => {
                *b.record(b.data[i][1]).first_chunk::<2>()?
            }
            _ => return None,
        };
        (self.const_usize(one) == Some(1)).then_some(r)
    }
}

/// The stable list places the hoisting pass lists for loop `l`
/// (`Body::stable_lists`, sorted by loop), each with its row: the place
/// word, the region's bounds and whether the loop keeps its buffer too
/// (else only its length). O(log rows + its rows).
fn stable_rows(b: &Body, l: u32) -> impl Iterator<Item = (ListRoot, [u32; 5])> + '_ {
    let rows = &b.stable_lists;
    let from = rows.partition_point(|r| r[0] < l);
    rows[from..]
        .iter()
        .take_while(move |r| r[0] == l)
        .filter_map(|&row| {
            let p = b.decode_place(row[1])?;
            let PlaceRoot::Local(local) = p.root else {
                return None;
            };
            Some((ListRoot::at(local.raw(), p.proj)?, row))
        })
}

/// A local's two stores (`Sets::sets`), the one `in_loop` rejects first:
/// an induction variable's start and step, when it has exactly two.
fn init_and_step(
    stores: &[(u32, u32)],
    in_loop: impl FnOnce(u32) -> bool,
) -> Option<[(u32, u32); 2]> {
    let [a, c] = stores else { return None };
    Some(if in_loop(a.0) { [*c, *a] } else { [*a, *c] })
}

thread_local! {
    static SCANS: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Counts, on this thread while `run` runs, the instructions the loop
/// proofs list: each body's tree and stores once, and each loop's or run
/// of statements' instructions they ask for.
pub fn count_scans<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let outer = SCANS.with(|w| w.replace(Some(0)));
    let out = run();
    let work = SCANS.with(|w| w.replace(outer)).unwrap_or(0);
    if let Some(outer) = outer {
        SCANS.with(|w| w.set(Some(outer + work)));
    }
    (out, work)
}

fn scans(n: usize) {
    hd_base::work::add(hd_base::work::Counter::LoopScans, n as u64);
    SCANS.with(|w| {
        if let Some(k) = w.get() {
            w.set(Some(k + n as u64));
        }
    });
}

/// Compressed rows of `edges` by source, over `nodes` nodes: node `x`'s
/// targets are `succ[at[x]..at[x + 1]]`. O(nodes + edges).
fn csr(nodes: usize, edges: &[(u32, u32)]) -> (Vec<u32>, Vec<u32>) {
    let mut at = vec![0u32; nodes + 1];
    for &(x, _) in edges {
        at[x as usize + 1] += 1;
    }
    for x in 0..nodes {
        at[x + 1] += at[x];
    }
    let mut fill = at.clone();
    let mut succ = vec![0u32; edges.len()];
    for &(x, y) in edges {
        succ[fill[x as usize] as usize] = y;
        fill[x as usize] += 1;
    }
    (at, succ)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_tir::ir::{BodyKind, Providers, ReadMode, TirBuilder, TirSink as _, place};

    const LIST: Ty = Ty(9000);
    const REF_LIST: Ty = Ty(9001);
    const RECORD: Ty = Ty(9002);
    const MAP: Ty = Ty(9003);
    const AT: NodeIdx = NodeIdx::NONE;
    struct Facts;
    impl Cx for Facts {
        fn list(&self, ty: Ty) -> bool {
            ty == LIST
        }
        fn tracked(&self, ty: Ty) -> bool {
            ty == LIST || ty == REF_LIST
        }
        fn elems_tracked(&self, _: Ty) -> bool {
            false
        }
        fn length_name(&self) -> Symbol {
            Symbol::NONE
        }
        fn load_name(&self) -> Symbol {
            Symbol::NONE
        }
        fn tuple(&self, _: Ty) -> bool {
            false
        }
        fn ref_target(&self, ty: Ty) -> Option<(Ty, bool)> {
            (ty == REF_LIST).then_some((LIST, false))
        }
        fn intrinsic(&self, callee: &Callee) -> Option<String> {
            match callee {
                Callee::Item { def, .. } => match def.raw() {
                    2 => Some("panic".into()),
                    3 => Some("int_wrapping_sub".into()),
                    _ => None,
                },
                Callee::TraitMethod { .. } => None,
            }
        }
    }
    fn inspect<R>(
        body: &Body,
        cell: &dyn Fn(usize) -> bool,
        run: impl FnOnce(&Lists<'_>) -> R,
    ) -> R {
        let topo = Topology::new(body);
        let nest = Nest::new(body, &topo);
        let sets = Sets::new(body);
        run(&Lists {
            b: body,
            topo: &topo,
            nest: &nest,
            cx: &Facts,
            cell,
            sets: &sets,
        })
    }
    fn build() -> TirBuilder {
        TirBuilder::new(DefId::from_raw(1), BodyKind::Fn)
    }
    fn local(b: &mut TirBuilder, ty: Ty) -> hd_base::LocalId {
        b.local(ty, Symbol::NONE, local_flags::PARAM, AT)
    }
    fn finish(b: TirBuilder, root: Ref) -> Body {
        b.finish_tolerating(root, &[], &[22])
            .expect("structural TIR")
    }
    fn read(b: &mut TirBuilder, root: PlaceRoot, proj: Vec<Proj>, ty: Ty) -> Ref {
        let word = b.place(&Place { root, proj });
        b.emit(Tag::Read, word, ReadMode::Use as u32, ty, AT)
    }
    fn element(b: &mut TirBuilder, list: Ref, index: Ref) -> Ref {
        read(b, PlaceRoot::Value(list), vec![Proj::Elem(index)], Ty::I32)
    }
    fn call(b: &mut TirBuilder, def: u32, args: &[Ref], ty: Ty) -> Ref {
        b.call(
            &Callee::Item {
                def: DefId::from_raw(def),
                targs: hd_types::TyList::EMPTY,
            },
            args,
            Providers::None,
            ty,
            AT,
        )
    }

    #[test]
    fn element_places_and_unknown_field_types_do_not_share_index_facts() {
        let mut b = build();
        let root = b.open_block();
        let xs = local(&mut b, LIST);
        let record = local(&mut b, RECORD);
        let index = b.const_value(usize_ty(), 2);
        let header = b.get(xs, LIST, AT);
        let copied = b.coerce(Coercion::Identity, NONE, header, LIST, AT);
        let loaded = element(&mut b, copied, index);
        let word = b.place(&Place {
            root: PlaceRoot::Value(header),
            proj: vec![Proj::Elem(index)],
        });
        let value = b.const_value(Ty::I32, 7);
        let stored = b.emit(Tag::Store, word, value.0, Ty::VOID, AT);
        let unknown = read(
            &mut b,
            PlaceRoot::Local(record),
            vec![Proj::Field(0), Proj::Elem(index)],
            Ty::I32,
        );
        let root = b.close_block(root, None, Ty::VOID, AT);
        let body = finish(b, root);
        assert_eq!(
            list_root(&body, &Facts, copied.0),
            list_root(&body, &Facts, header.0)
        );
        assert_eq!(access_index(&body, loaded.0 as usize), Some(index.0));
        assert_eq!(access_index(&body, stored.0 as usize), Some(index.0));
        assert_eq!(
            elem_store_list(&body, stored.0 as usize),
            Some((header.0, index.0))
        );
        inspect(&body, &|_| false, |facts| {
            assert_eq!(
                facts.elem_access(stored.0 as usize).map(|(_, index)| index),
                Some(index.0)
            );
            assert!(
                facts.elem_access(unknown.0 as usize).is_none(),
                "no field type means no list proof"
            );
            assert_eq!(facts.ty_of(NONE), Ty::VOID);
            assert!(
                !facts.scalar_loop_test(loaded.0, &mut 0),
                "exhausted traversal supplies no fact"
            );
        });
    }

    #[test]
    fn scalar_aliases_keep_identity_coercions_and_known_constructor_fields_only() {
        let mut b = build();
        let root = b.open_block();
        let one = b.const_value(usize_ty(), 1);
        let scalar = b.coerce(Coercion::Identity, NONE, one, usize_ty(), AT);
        let fields = b.refs_record(&[scalar]);
        let made = b.emit(Tag::NewStruct, NONE, fields, RECORD, AT);
        let known = read(
            &mut b,
            PlaceRoot::Value(made),
            vec![Proj::Field(0)],
            usize_ty(),
        );
        let missing = read(
            &mut b,
            PlaceRoot::Value(made),
            vec![Proj::Field(3)],
            usize_ty(),
        );
        let opaque = call(&mut b, 4, &[], RECORD);
        let unknown = read(
            &mut b,
            PlaceRoot::Value(opaque),
            vec![Proj::Field(0)],
            usize_ty(),
        );
        // A partially built TIR place must not invent a constructor for a constant.
        let not_a_record = read(
            &mut b,
            PlaceRoot::Value(one),
            vec![Proj::Field(0)],
            usize_ty(),
        );
        let root = b.close_block(root, None, Ty::VOID, AT);
        let body = finish(b, root);
        inspect(&body, &|_| false, |facts| {
            assert_eq!(facts.source(scalar.0), one.0);
            assert_eq!(facts.source(known.0), one.0);
            assert_eq!(facts.source(missing.0), missing.0);
            assert_eq!(facts.source(unknown.0), unknown.0);
            assert_eq!(facts.source(not_a_record.0), not_a_record.0);
        });
    }

    #[test]
    fn positive_comparisons_handle_both_outcomes_and_refuse_unknown_predicates() {
        let mut b = build();
        let root = b.open_block();
        let number = local(&mut b, usize_ty());
        let x = b.get(number, usize_ty(), AT);
        let zero = b.const_value(usize_ty(), 0);
        let one = b.const_value(usize_ty(), 1);
        let mut cases = Vec::new();
        for (op, args, outcome, want) in [
            (PrimOp::Lt, [zero, x], true, Some(x.0)),
            (PrimOp::Gt, [x, zero], true, Some(x.0)),
            (PrimOp::Le, [one, x], true, Some(x.0)),
            (PrimOp::Ge, [x, one], true, Some(x.0)),
            (PrimOp::Ne, [x, zero], true, Some(x.0)),
            (PrimOp::Ne, [zero, x], true, Some(x.0)),
            (PrimOp::Eq, [x, one], true, Some(x.0)),
            (PrimOp::Eq, [one, x], true, Some(x.0)),
            (PrimOp::Le, [x, zero], false, Some(x.0)),
            (PrimOp::Lt, [x, zero], true, Some(zero.0)),
            (PrimOp::Eq, [x, zero], true, None),
            (PrimOp::Add, [x, one], false, None),
        ] {
            cases.push((b.prim(op as u32, &args, Ty::BOOL, AT), outcome, want));
        }
        let comparison = b.prim(PrimOp::Le as u32, &[x, zero], Ty::BOOL, AT);
        let negated = b.prim(PrimOp::Not as u32, &[comparison], Ty::BOOL, AT);
        cases.push((negated, true, Some(x.0)));
        let signed = b.const_value(Ty::I32, 1);
        cases.push((
            b.prim(PrimOp::Gt as u32, &[signed, signed], Ty::BOOL, AT),
            true,
            None,
        ));
        cases.push((call(&mut b, 4, &[], Ty::BOOL), true, None));
        cases.push((b.prim(PrimOp::Neg as u32, &[x], usize_ty(), AT), true, None));
        let root = b.close_block(root, None, Ty::VOID, AT);
        let body = finish(b, root);
        inspect(&body, &|_| false, |facts| {
            for (condition, outcome, want) in cases {
                assert_eq!(facts.positive_operand(condition.0, outcome), want);
            }
            assert_eq!(facts.positive_operand(one.0, true), None);
        });
    }

    #[test]
    fn wrapping_subtraction_and_conditional_exit_forms_keep_their_meaning() {
        let mut b = build();
        let root = b.open_block();
        let number = local(&mut b, usize_ty());
        let x = b.get(number, usize_ty(), AT);
        let one = b.const_value(usize_ty(), 1);
        let wrapped = call(&mut b, 3, &[x, one], usize_ty());
        let condition = b.const_value(Ty::BOOL, 1);
        let panic_arm = b.open_block();
        call(&mut b, 2, &[], Ty::NEVER);
        let panic_arm = b.close_block(panic_arm, None, Ty::NEVER, AT);
        let arms = b.refs_record(&[panic_arm]);
        let test = b.emit(Tag::If, condition.0, arms, Ty::VOID, AT);
        let ordinary_arm = b.open_block();
        call(&mut b, 4, &[], Ty::VOID);
        let ordinary_arm = b.close_block(ordinary_arm, None, Ty::VOID, AT);
        let ordinary_arms = b.refs_record(&[ordinary_arm]);
        let ordinary = b.emit(Tag::If, condition.0, ordinary_arms, Ty::VOID, AT);
        let root = b.close_block(root, None, Ty::VOID, AT);
        let body = finish(b, root);
        inspect(&body, &|_| false, |facts| {
            assert_eq!(facts.minus_one_of(wrapped.0), Some(x.0));
            assert_eq!(facts.arm_exit(panic_arm.0, test.0), Some(true));
            assert_eq!(facts.arm_exit(ordinary_arm.0, ordinary.0), None);
        });
    }

    struct LoopCase {
        body: Body,
        loop_id: u32,
        test: u32,
        condition: u32,
        step: u32,
        access: u32,
        current: u32,
        before: u32,
        incoming: hd_base::LocalId,
    }
    fn test_branch(b: &mut TirBuilder, label: u32, condition: Ref) -> Ref {
        let yes = b.open_block();
        let yes = b.close_block(yes, None, Ty::VOID, AT);
        let no = b.open_block();
        b.emit(Tag::Break, label, NONE, Ty::NEVER, AT);
        let no = b.close_block(no, None, Ty::NEVER, AT);
        let arms = b.refs_record(&[yes, no]);
        b.emit(Tag::If, condition.0, arms, Ty::VOID, AT)
    }
    fn rising_loop(conditional_bound: bool) -> LoopCase {
        let mut b = build();
        let root = b.open_block();
        let xs = local(&mut b, LIST);
        let index = local(&mut b, usize_ty());
        let bound = local(&mut b, usize_ty());
        let zero = b.const_value(usize_ty(), 0);
        let one = b.const_value(usize_ty(), 1);
        b.set(index, zero, AT);
        let header = b.get(xs, LIST, AT);
        let args = b.refs_record(&[header]);
        let length = b.emit(
            Tag::Intrinsic,
            IntrinsicOp::ListLen as u32,
            args,
            usize_ty(),
            AT,
        );
        if conditional_bound {
            let flag = local(&mut b, Ty::BOOL);
            let flag = b.get(flag, Ty::BOOL, AT);
            let arm = b.open_block();
            b.set(bound, length, AT);
            let arm = b.close_block(arm, None, Ty::VOID, AT);
            let arms = b.refs_record(&[arm]);
            b.emit(Tag::If, flag.0, arms, Ty::VOID, AT);
        } else {
            b.set(bound, length, AT);
        }
        b.emit(
            Tag::IsUnique,
            xs.raw() | place::BARE_LOCAL,
            hd_tir::ir::unique_word::FRESH,
            Ty::VOID,
            AT,
        );
        b.emit(
            Tag::IsUnique,
            xs.raw() | place::BARE_LOCAL,
            hd_tir::ir::unique_word::HOISTED,
            Ty::VOID,
            AT,
        );
        let lp = b.open_loop();
        let block = b.open_block();
        let current = b.get(index, usize_ty(), AT);
        let end = b.get(bound, usize_ty(), AT);
        let condition = b.prim(PrimOp::Lt as u32, &[current, end], Ty::BOOL, AT);
        let test = test_branch(&mut b, lp.0.raw(), condition);
        let at = b.get(index, usize_ty(), AT);
        let header = b.get(xs, LIST, AT);
        let access = element(&mut b, header, at);
        let before = b.get(index, usize_ty(), AT);
        let step = b.prim(PrimOp::Add as u32, &[before, one], usize_ty(), AT);
        b.set(index, step, AT);
        let block = b.close_block(block, None, Ty::VOID, AT);
        let loop_id = b.close_loop(lp, block, Ty::VOID, AT);
        let root = b.close_block(root, None, Ty::VOID, AT);
        let mut body = finish(b, root);
        // Ownership splicing puts hoisted checks after the listed child block,
        // immediately before the Loop. Match that producer's member order.
        let mut members = body.record(body.data[root.0 as usize][0]).to_vec();
        let checks: Vec<_> = members
            .iter()
            .copied()
            .filter(|&i| body.tags[i as usize] == Tag::IsUnique)
            .collect();
        members.retain(|&i| body.tags[i as usize] != Tag::IsUnique);
        let position = members
            .iter()
            .position(|&i| i == loop_id.0)
            .expect("loop member");
        members.splice(position..position, checks);
        body.data[root.0 as usize][0] = body.push_record(&members);
        body.stable_lists
            .push([loop_id.0, xs.raw() | place::BARE_LOCAL, NONE, NONE, 1]);
        LoopCase {
            body,
            loop_id: loop_id.0,
            test: test.0,
            condition: condition.0,
            step: step.0,
            access: access.0,
            current: current.0,
            before: before.0,
            incoming: index,
        }
    }

    #[test]
    fn rising_proofs_require_binary_steps_and_a_dominating_bound() {
        let good = rising_loop(false);
        inspect(&good.body, &|_| false, |facts| {
            assert_eq!(facts.loop_guard(good.loop_id), Some((good.test, false)));
            assert_eq!(facts.rising(good.loop_id, good.test), [good.access]);
            assert!(facts.kept(good.loop_id)[0].unique.is_some());
        });
        for instruction in [good.condition, good.step] {
            let mut body = good.body.clone();
            body.data[instruction as usize][1] = body.push_record(&[good.before]);
            inspect(&body, &|_| false, |facts| {
                assert!(facts.rising(good.loop_id, good.test).is_empty());
            });
        }
        let conditional = rising_loop(true);
        inspect(&conditional.body, &|_| false, |facts| {
            assert!(
                facts
                    .rising(conditional.loop_id, conditional.test)
                    .is_empty()
            );
        });
        let mut extra = good.body.clone();
        let topology = Topology::new(&extra);
        let block = extra.data[good.loop_id as usize][0];
        let position = hd_tir::ir::u32_of(extra.record(extra.data[block as usize][0]).len());
        let mapping = hd_tir::own::insert_batch(
            &mut extra,
            &topology,
            &[hd_tir::own::BatchInsert {
                block,
                pos: position,
                tag: Tag::Store,
                data: [good.incoming.raw() | place::BARE_LOCAL, Ref::konst(0).0],
                ty: Ty::VOID,
                syn: AT,
            }],
        )
        .expect("third induction store");
        inspect(&extra, &|_| false, |facts| {
            assert!(
                facts
                    .rising(mapping[good.loop_id as usize], mapping[good.test as usize])
                    .is_empty()
            );
        });
        let mut body = good.body;
        let (((), inner), outer) =
            count_scans(|| count_scans(|| run(&mut body, &Facts, false).expect("header splice")));
        assert!(inner > 0);
        assert_eq!(outer, inner, "nested measurements retain actual proof work");
        assert!(
            body.tags
                .iter()
                .enumerate()
                .any(|(i, tag)| *tag == Tag::Read && body.elem_unchecked(i, 0))
        );
    }

    fn falling_loop(change_incoming: bool, detached_index: bool) -> LoopCase {
        let mut b = build();
        let root = b.open_block();
        let xs = local(&mut b, LIST);
        let ys = local(&mut b, LIST);
        let map = local(&mut b, MAP);
        let incoming = local(&mut b, usize_ty());
        let index = local(&mut b, usize_ty());
        let zero = b.const_value(usize_ty(), 0);
        let one = b.const_value(usize_ty(), 1);
        let first = if detached_index {
            let scope = b.open_scope();
            let child = b.open_block();
            let first = b.get(incoming, usize_ty(), AT);
            let child = b.close_block(child, Some(first), usize_ty(), AT);
            b.close_scope(scope, child, usize_ty(), AT);
            first
        } else {
            b.get(incoming, usize_ty(), AT)
        };
        read(
            &mut b,
            PlaceRoot::Local(map),
            vec![Proj::Elem(first)],
            Ty::I32,
        );
        let header = b.get(xs, LIST, AT);
        element(&mut b, header, first);
        element(&mut b, header, first);
        if change_incoming {
            b.set(incoming, zero, AT);
        }
        let start = b.get(incoming, usize_ty(), AT);
        b.set(index, start, AT);
        let lp = b.open_loop();
        let block = b.open_block();
        let current = b.get(index, usize_ty(), AT);
        let condition = b.prim(PrimOp::Gt as u32, &[current, zero], Ty::BOOL, AT);
        let test = test_branch(&mut b, lp.0.raw(), condition);
        let at = b.get(index, usize_ty(), AT);
        let header = b.get(xs, LIST, AT);
        let access = element(&mut b, header, at);
        let other = b.get(ys, LIST, AT);
        element(&mut b, other, at);
        let greater = b.prim(PrimOp::Add as u32, &[at, one], usize_ty(), AT);
        element(&mut b, header, greater);
        let incomplete = b.prim(PrimOp::Sub as u32, &[at], usize_ty(), AT);
        element(&mut b, header, incomplete);
        let before = b.get(index, usize_ty(), AT);
        let step = b.prim(PrimOp::Sub as u32, &[before, one], usize_ty(), AT);
        let previous = element(&mut b, header, step);
        b.set(index, step, AT);
        let block = b.close_block(block, None, Ty::VOID, AT);
        let loop_id = b.close_loop(lp, block, Ty::VOID, AT);
        let root = b.close_block(root, None, Ty::VOID, AT);
        let mut body = finish(b, root);
        body.stable_lists
            .push([loop_id.0, xs.raw() | place::BARE_LOCAL, NONE, NONE, 0]);
        let shape = LoopCase {
            body,
            loop_id: loop_id.0,
            test: test.0,
            condition: condition.0,
            step: step.0,
            access: access.0,
            current: current.0,
            before: before.0,
            incoming,
        };
        inspect(&shape.body, &|_| false, |facts| {
            if !change_incoming && !detached_index {
                assert_eq!(
                    facts.falling(shape.loop_id, shape.test),
                    [shape.access, previous.0]
                );
            }
        });
        shape
    }

    #[test]
    fn falling_proofs_refuse_changed_or_unavailable_predecessor_facts() {
        let good = falling_loop(false, false);
        for instruction in [good.condition, good.step] {
            let mut body = good.body.clone();
            body.data[instruction as usize][1] = body.push_record(&[good.current]);
            inspect(&body, &|_| false, |facts| {
                assert!(facts.falling(good.loop_id, good.test).is_empty());
            });
        }
        let mut body = good.body.clone();
        body.data[good.before as usize][0] = good.incoming.raw() | place::BARE_LOCAL;
        inspect(&body, &|_| false, |facts| {
            assert!(facts.falling(good.loop_id, good.test).is_empty());
        });
        let mut body = good.body;
        body.local_flags[good.incoming.idx()] |= local_flags::CAPTURED;
        inspect(&body, &|_| false, |facts| {
            assert!(facts.falling(good.loop_id, good.test).is_empty());
        });
        for bad in [falling_loop(true, false), falling_loop(false, true)] {
            inspect(&bad.body, &|_| false, |facts| {
                assert!(facts.falling(bad.loop_id, bad.test).is_empty());
            });
        }
    }

    #[test]
    fn masked_proofs_use_nonfallthrough_positive_guards_and_require_a_binary_mask() {
        for leaves_loop in [false, true] {
            let mut b = build();
            let root = b.open_block();
            let xs = local(&mut b, LIST);
            let y = local(&mut b, usize_ty());
            let header = b.get(xs, LIST, AT);
            let args = b.refs_record(&[header]);
            let length = b.emit(
                Tag::Intrinsic,
                IntrinsicOp::ListLen as u32,
                args,
                usize_ty(),
                AT,
            );
            let zero = b.const_value(usize_ty(), 0);
            let one = b.const_value(usize_ty(), 1);
            let lp = b.open_loop();
            let block = b.open_block();
            let empty = b.prim(PrimOp::Le as u32, &[length, zero], Ty::BOOL, AT);
            let exit = b.open_block();
            if leaves_loop {
                b.emit(Tag::Break, lp.0.raw(), NONE, Ty::NEVER, AT);
            } else {
                call(&mut b, 2, &[], Ty::NEVER);
            }
            let exit = b.close_block(exit, None, Ty::NEVER, AT);
            let arms = b.refs_record(&[exit]);
            b.emit(Tag::If, empty.0, arms, Ty::VOID, AT);
            let y = b.get(y, usize_ty(), AT);
            let mask = b.prim(PrimOp::Sub as u32, &[length, one], usize_ty(), AT);
            let index = b.prim(PrimOp::BitAnd as u32, &[y, mask], usize_ty(), AT);
            let bad = b.prim(PrimOp::BitAnd as u32, &[mask], usize_ty(), AT);
            let access = element(&mut b, header, index);
            let block = b.close_block(block, None, Ty::VOID, AT);
            let lp = b.close_loop(lp, block, Ty::VOID, AT);
            let root = b.close_block(root, None, Ty::VOID, AT);
            let mut body = finish(b, root);
            body.stable_lists
                .push([lp.0, xs.raw() | place::BARE_LOCAL, NONE, NONE, 0]);
            let list = ListRoot {
                local: xs.raw(),
                path: Vec::new(),
            };
            inspect(&body, &|_| false, |facts| {
                assert_eq!(facts.masked_len(&list, index.0), Some((length.0, index.0)));
                assert_eq!(facts.masked_len(&list, bad.0), None);
                assert_eq!(facts.positive_len(&list, index.0), Some(length.0));
                assert_eq!(facts.masked(lp.0), [access.0]);
            });
        }
    }

    #[test]
    fn an_arm_that_can_jump_back_to_its_if_or_own_end_is_not_a_terminating_guard() {
        for target in 0..3 {
            for continues in [false, true] {
                let mut b = build();
                let root = b.open_block();
                let flag = local(&mut b, Ty::BOOL);
                let flag = b.get(flag, Ty::BOOL, AT);
                let lp = b.open_loop();
                let block = b.open_block();
                let arm_label = hd_tir::ir::u32_of(b.body().label_inst.len());
                b.body_mut().label_inst.extend([NONE, NONE]);
                let if_label = arm_label + 1;
                let outer = b.open_block();
                let jump = b.open_block();
                let label = match target {
                    0 => arm_label,
                    1 => if_label,
                    _ => lp.0.raw(),
                };
                b.emit(Tag::Break, label, NONE, Ty::NEVER, AT);
                let jump = b.close_block(jump, None, Ty::NEVER, AT);
                let arms = b.refs_record(&[jump]);
                b.emit(Tag::If, flag.0, arms, Ty::VOID, AT);
                if continues {
                    b.emit(Tag::Continue, lp.0.raw(), NONE, Ty::NEVER, AT);
                } else {
                    b.emit(Tag::Return, NONE, NONE, Ty::NEVER, AT);
                }
                let outer = b.close_block(outer, None, Ty::NEVER, AT);
                let arms = b.refs_record(&[outer]);
                let test = b.emit(Tag::If, flag.0, arms, Ty::VOID, AT);
                b.body_mut().label_inst[arm_label as usize] = outer.0;
                b.body_mut().label_inst[if_label as usize] = test.0;
                let block = b.close_block(block, None, Ty::VOID, AT);
                b.close_loop(lp, block, Ty::VOID, AT);
                let root = b.close_block(root, None, Ty::VOID, AT);
                let body = finish(b, root);
                inspect(&body, &|_| false, |facts| {
                    assert_eq!(
                        facts.arm_exit(outer.0, test.0),
                        (target == 2).then_some(false)
                    );
                });
            }
        }
    }

    /// `k` loops after one another over a reference `xs`, each `for i in
    /// 0..n` with `n` loaded from `*xs .0` before it, whose body loads
    /// `*xs .0` and `*xs .1` and guards `i >= *xs .0` with a panic.
    fn read_only_loops(k: usize) -> Body {
        let mut b = build();
        let root = b.open_block();
        let xs = local(&mut b, REF_LIST);
        let zero = b.const_value(usize_ty(), 0);
        let one = b.const_value(usize_ty(), 1);
        let load = |b: &mut TirBuilder, field: u32| {
            let word = b.place(&Place {
                root: PlaceRoot::Local(xs),
                proj: vec![Proj::Deref, Proj::Field(field)],
            });
            b.emit(Tag::Read, word, ReadMode::Copy as u32, usize_ty(), AT)
        };
        for _ in 0..k {
            let index = b.local(usize_ty(), Symbol::NONE, 0, AT);
            let bound = b.local(usize_ty(), Symbol::NONE, 0, AT);
            b.set(index, zero, AT);
            let length = load(&mut b, 0);
            b.set(bound, length, AT);
            let lp = b.open_loop();
            let block = b.open_block();
            let current = b.get(index, usize_ty(), AT);
            let end = b.get(bound, usize_ty(), AT);
            let condition = b.prim(PrimOp::Lt as u32, &[current, end], Ty::BOOL, AT);
            test_branch(&mut b, lp.0.raw(), condition);
            let at = b.get(index, usize_ty(), AT);
            let len = load(&mut b, 0);
            let out = b.prim(PrimOp::Ge as u32, &[at, len], Ty::BOOL, AT);
            let panic = b.open_block();
            call(&mut b, 2, &[], Ty::VOID);
            let panic = b.close_block(panic, None, Ty::VOID, AT);
            let arms = b.refs_record(&[panic, Ref(NONE)]);
            b.emit(Tag::If, out.0, arms, Ty::VOID, AT);
            load(&mut b, 1);
            let before = b.get(index, usize_ty(), AT);
            let step = b.prim(PrimOp::Add as u32, &[before, one], usize_ty(), AT);
            b.set(index, step, AT);
            let block = b.close_block(block, None, Ty::VOID, AT);
            b.close_loop(lp, block, Ty::VOID, AT);
        }
        let root = b.close_block(root, None, Ty::VOID, AT);
        finish(b, root)
    }

    #[test]
    fn read_only_loops_hoist_their_loads_and_drop_decided_guards_in_linear_work() {
        let mut work = Vec::new();
        for k in [8, 16] {
            let mut body = read_only_loops(k);
            let before = body.len();
            let ((), scanned) =
                count_scans(|| run(&mut body, &Facts, true).expect("hoisted loads splice"));
            work.push(scanned);
            // Two insertions per loop and place (`*xs .0`, `*xs .1`) shift
            // each later instruction; the loads become reads of locals.
            assert_eq!(body.len(), before + 4 * k);
            let loads = body
                .tags
                .iter()
                .enumerate()
                .filter(|&(i, &t)| {
                    t == Tag::Read
                        && body
                            .decode_place(body.data[i][0])
                            .is_some_and(|p| p.proj.first() == Some(&Proj::Deref))
                })
                .count();
            // Each loop's two loads stand before it; the one before its
            // bound stays.
            assert_eq!(loads, 3 * k, "{k} loops");
            let decided = body
                .tags
                .iter()
                .enumerate()
                .filter(|&(i, &t)| t == Tag::If && body.data[i][0] & Ref::CONST_BIT != 0)
                .count();
            assert_eq!(decided, k, "every guard is decided");
        }
        // Twice the loops, at most twice the work: no loop rescans another.
        assert!(work[1] <= 2 * work[0], "{work:?}");
    }
}
