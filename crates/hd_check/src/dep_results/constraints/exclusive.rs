//! Exclusivity (`ref.excl`, stages 7a to 7d2 of
//! `docs/future-work/RETURNED_REFERENCES_PATH.md`). Each written `Ref` of
//! a place a local's path reaches is a loan of its own, [`Origin::Site`],
//! which the solver carries with the local's: shared for `&`, exclusive
//! for `&mut`. So is each reference a call takes for a receiver or an
//! argument, which reaches the call's result when the result depends on
//! it. A write to a place that overlaps a site's, or any use of it for an
//! exclusive site, is `conflicting-access` when a binding that holds the
//! site is read after it on some path. The order comes from
//! the block structure: an instruction runs after the earlier ones of its
//! block's statements, and a loop's body runs again, so a read earlier in
//! a loop follows a write later in it when the binding outlives one
//! iteration. An element reference borrows its whole collection
//! (`ref.elem.access`): `&xs[i]` is a `&` of `xs` and `&mut xs[i]` a
//! `&mut` of it, as `Index.index` and `IndexMut.index_mut` take them.

use hd_base::{HashMap, NodeIdx, SliceSort as _};
use hd_tir::ir::{
    Body, IntrinsicOp, NONE, PlaceRoot, Proj, Ref, Tag, Topology, Word, local_flags, slots,
};
use hd_types::AdtId;
use hd_types::{Ty, TyData, Types};

use hd_syntax::SyntaxKind;

use super::{Gen, Inputs, Origin, Region, Solution, is_mut, w};

/// A place as a local and a path from it: fields, variants,
/// dereferences, and elements, which overlap any other step.
type Path = (u32, Vec<Proj>);

/// The path of place `word`, through the reads its value root is made of.
pub(super) fn path_of(b: &Body, word: u32) -> Option<Path> {
    let p = b.decode_place(word)?;
    match p.root {
        PlaceRoot::Local(l) => Some((l.raw(), p.proj)),
        PlaceRoot::Value(v) => {
            let mut v = v.0;
            while b.ty.get(v as usize).is_some()
                && b.tags[v as usize] == Tag::Coerce
                && b.row_adaptation(v as usize).is_none()
            {
                v = b.data[v as usize][0];
            }
            if b.ty.get(v as usize).is_none() || b.tags[v as usize] != Tag::Read {
                return None;
            }
            let (l, mut path) = path_of(b, b.data[v as usize][0])?;
            path.extend(p.proj);
            Some((l, path))
        }
        _ => None,
    }
}

/// What a site loan excludes (`ref.excl.shared`, `ref.excl.mut`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Shared,
    Excl,
}

/// A site loan of `call_conflicts`: its id, its `Ref`, the place a call's
/// result holds after that call, its place, and whether it is `&mut`.
type SiteLoan = (u32, u32, Option<(usize, Path)>, Path, bool);

/// What `Gen::call_conflicts` reads of the whole body, built once per
/// body (#2134): each value's first user, and the writes, moves and uses
/// of places, each sorted by where it happens, so the accesses between an
/// argument and its call are one binary search and the window itself.
struct CallTables {
    /// `Gen::writes`, by where each write happens.
    writes: Vec<(u32, Path, u32)>,
    /// `Gen::moves`, by instruction.
    moves: Vec<(u32, Path)>,
    /// `Gen::accesses`, by instruction.
    touches: Vec<(u32, Path)>,
}

/// Whether instruction `i` may write a place, as [`Gen::writes`] lists:
/// a store, a `&mut` taken or passed on, or a built-in call that changes
/// its receiver.
pub(in crate::dep_results) fn writes_place(pool: Types<'_>, b: &Body, i: usize) -> bool {
    match b.tags[i] {
        Tag::Store => true,
        Tag::Ref | Tag::Read => is_mut(pool, b.ty[i]),
        Tag::Intrinsic => matches!(
            IntrinsicOp::from_u32(b.data[i][0]),
            Some(
                IntrinsicOp::ListPush
                    | IntrinsicOp::ListReserve
                    | IntrinsicOp::MapSet
                    | IntrinsicOp::MapRemove
            )
        ),
        _ => false,
    }
}

/// Whether instruction `i` takes a reference to an element place.
pub(in crate::dep_results) fn elem_ref(b: &Body, i: usize) -> bool {
    b.tags[i] == Tag::Ref
        && b.decode_place(b.data[i][0])
            .is_some_and(|p| p.proj.iter().any(|s| matches!(s, Proj::Elem(_))))
}

/// The receiver read of instruction `c` when `c` is a built-in `push` or
/// map `remove` whose receiver is an element of a collection: the call
/// changes that element in place.
pub(in crate::dep_results) fn elem_receiver(b: &Body, c: usize) -> Option<u32> {
    if b.tags[c] != Tag::Intrinsic
        || !matches!(
            IntrinsicOp::from_u32(b.data[c][0]),
            Some(IntrinsicOp::ListPush | IntrinsicOp::MapRemove)
        )
    {
        return None;
    }
    let &recv = (b.data[c][1] != NONE)
        .then(|| b.record(b.data[c][1]))?
        .first()?;
    let p = (b.tags.get(recv as usize) == Some(&Tag::Read))
        .then(|| b.decode_place(b.data[recv as usize][0]))??;
    p.proj
        .iter()
        .any(|s| matches!(s, Proj::Elem(_)))
        .then_some(recv)
}

/// The entries of `xs`, sorted by `key`, with `lo < key < hi`.
fn window<T>(xs: &[T], key: impl Fn(&T) -> u32, lo: u32, hi: u32) -> &[T] {
    let a = xs.partition_point(|x| key(x) <= lo);
    let b = xs.partition_point(|x| key(x) < hi).max(a);
    crate::work::scan_work(b - a);
    &xs[a..b]
}

/// How an instruction accesses a place a loan may cover.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Access {
    /// A move of the place or a part of it (`ref.excl.move`).
    Move,
    /// A write (`ref.excl.write`).
    Write,
    /// Any other use, which only an exclusive loan excludes.
    Use,
}

/// The place loan `i` takes, as `loan_path` reads it.
fn raw_loan_path(s: &Inputs<'_>, i: u32) -> Option<Path> {
    let (b, i) = (s.b, i as usize);
    let excl = is_mut(s.pool, b.ty[i]);
    match b.tags[i] {
        Tag::Ref => path_of(b, b.data[i][0]),
        Tag::Read
            if excl || super::local_ref(s.pool, b.ty[i]).is_some_and(|(counted, _)| counted) =>
        {
            path_of(b, b.data[i][0]).map(|(l, mut p)| {
                p.push(Proj::Deref);
                (l, p)
            })
        }
        Tag::Coerce if b.same_value_coercion(i) && !excl => {
            let v = b.data[i][0] as usize;
            if b.ty.get(v).is_none()
                || b.tags[v] != Tag::Read
                || !is_mut(s.pool, b.ty[v])
                    && !super::local_ref(s.pool, b.ty[v]).is_some_and(|(counted, _)| counted)
            {
                return None;
            }
            path_of(b, b.data[v][0]).map(|(l, mut p)| {
                p.push(Proj::Deref);
                (l, p)
            })
        }
        _ => None,
    }
}

/// The place loan `i` covers and its kind: a `Ref`'s place, or what a
/// `&mut` binding refers to where a call reads it as `&` or as `&mut`
/// (`ref.excl.call`). An element reference covers the collection its
/// first element step indexes (`ref.elem.access`).
fn loan_path(s: &Inputs<'_>, i: u32) -> Option<(Path, Kind)> {
    let excl = is_mut(s.pool, s.b.ty[i as usize]);
    let (l, mut path) = raw_loan_path(s, i)?;
    if let Some(k) = path.iter().position(|p| matches!(p, Proj::Elem(_))) {
        if through_local_ref(s, l, &path, k) {
            return None;
        }
        path.truncate(k);
    }
    Some(((l, path), if excl { Kind::Excl } else { Kind::Shared }))
}

/// Whether `path` from local `root` reads through a local reference held
/// in its element at step `element`: the reference's own source, which
/// its region carries, is what the loan keeps, as Rust's loan through a
/// `&` stops at that dereference. A counted pointer there keeps the
/// collection's loan, since replacing the slot would release its object.
fn through_local_ref(s: &Inputs<'_>, root: u32, path: &[Proj], element: usize) -> bool {
    let mut ty = s.b.local_ty[root as usize];
    let mut variant = None;
    for (at, step) in path.iter().enumerate() {
        ty = match *step {
            Proj::Elem(_) => {
                let Some((def, args)) = s.pool.adt(ty) else {
                    return false;
                };
                let known = &s.cx.names.known;
                let slot = usize::from(def == known.map);
                if def != known.list && def != known.map {
                    return false;
                }
                let Some(&inner) = s.pool.list_items(args).get(slot) else {
                    return false;
                };
                inner
            }
            Proj::Deref => {
                let TyData::Ref { counted, inner, .. } = s.pool.get(ty) else {
                    return false;
                };
                if at > element {
                    return !counted;
                }
                inner
            }
            Proj::Variant(k) => {
                variant = Some(k);
                ty
            }
            Proj::Field(j) => {
                let Some((inner, _)) = s.ls.field_slots(s.cx, ty, variant.take(), j) else {
                    return false;
                };
                inner
            }
        };
    }
    false
}

/// The place a call's result holds for loan `i`, a site of `loan_path`:
/// the call that takes `i` and the place extended by the steps the
/// callee's result paths name for that argument (`ref.result.path.caller`).
/// While the call runs, `i` still covers its whole place.
fn held_path(s: &Inputs<'_>, i: u32, (l, path): &Path) -> Option<(usize, Path)> {
    s.calls.iter().find_map(|(&c, sig)| {
        let (_, steps) = sig.paths.iter().find(|x| x.0 == i)?;
        let mut path = path.clone();
        path.extend(steps.iter().copied());
        Some((c as usize, (*l, path)))
    })
}

/// Whether `Ref` `i` is a written `&` or `&mut` that takes a site loan
/// (`ref.excl.place`, `ref.excl.mut`), or a pattern's binding of a
/// payload below a reference, which is such a reference too
/// (`ref.match.borrow`).
pub(super) fn site(s: &Inputs<'_>, i: u32) -> bool {
    let at = s.b.syn[i as usize];
    at != NodeIdx::NONE
        && s.b.tags[i as usize] == Tag::Ref
        && (matches!(
            s.cx.src.parse.tree.node(at).kind(),
            SyntaxKind::RefExpr
                | SyntaxKind::BindingPattern
                | SyntaxKind::NamedPattern
                | SyntaxKind::DataPatternField
        ) || counted_kind_ref(s, i))
        && loan_path(s, i).is_some()
}

/// The existing Ref representation of an implicit counted-to-local lend:
/// the pointee address of an already evaluated counted reference. Its
/// source expression is not necessarily spelled as an explicit `RefExpr`.
fn counted_kind_ref(s: &Inputs<'_>, i: u32) -> bool {
    let b = s.b;
    let place = b.view(b.data[i as usize][0]);
    let hd_tir::ir::Root::Value(source) = place.root else {
        return false;
    };
    let Some(source) = Ref(source).as_inst() else {
        return false;
    };
    let mut steps = place.steps(b);
    steps.next() == Some(Proj::Deref)
        && steps.next().is_none()
        && b.ty
            .get(source.0 as usize)
            .is_some_and(|&ty| super::local_ref(s.pool, ty).is_some_and(|(counted, _)| counted))
        && super::local_ref(s.pool, b.ty[i as usize]).is_some_and(|(counted, _)| !counted)
}

/// Whether instruction `i` reborrows a `&mut`, a site loan like the
/// explicit `&mut *c` or `&*c` (`ref.move.reborrow.excl`): a read that a
/// typed `let`, an assignment or a field declared `&mut` reborrows, or a
/// conversion of a `&mut` to `&`. A call's argument takes the same loan
/// in [`Gen::call_loans`].
pub(super) fn reborrow(s: &Inputs<'_>, i: u32) -> bool {
    let (b, iu) = (s.b, i as usize);
    let read = match b.tags[iu] {
        Tag::Read => s.reborrows.contains(&i),
        Tag::Coerce if b.row_adaptation(iu).is_none() => {
            let v = b.data[iu][0] as usize;
            b.ty.get(v).is_some()
                && b.tags[v] == Tag::Read
                && crate::moves::is_mut_ref(s.pool, b.ty[v])
        }
        _ => false,
    };
    read && loan_path(s, i).is_some()
}

/// Whether call argument `v` takes a site loan (`ref.excl.call`).
pub(super) fn call_site(s: &Inputs<'_>, v: u32) -> bool {
    loan_path(s, v).is_some()
}

/// Whether instruction `i` may take a run-time loan
/// (`ref.excl.alias.loan`): a `Ref`, or a `Read` a call lends, whose place
/// may cross an object a pointer, a shared variable's cell or a top-level
/// binding's slot holds: a dereference on its path or on the path of the
/// read it is rooted at, a global root, a captured local's root, or a
/// `Read` of a pointer. The backend takes the loans of the crossings that
/// hold a counted object; one with none takes nothing.
pub(super) fn runtime_site(s: &Inputs<'_>, i: u32) -> bool {
    crate::dep_results::runtime_site(s.b, &s.runtime, i)
}

/// Whether argument `u` passes local `root` itself as a counted pointer,
/// `*T` or `*mut T`, through any same-value coercion.
fn passes_pointer(s: &Inputs<'_>, u: u32, root: u32) -> bool {
    let b = s.b;
    let counted = |t: Ty| matches!(s.pool.get(t), TyData::Ref { counted: true, .. });
    if b.ty.get(u as usize).is_none_or(|&t| !counted(t)) {
        return false;
    }
    let mut v = u;
    while (v as usize) < b.tags.len()
        && b.tags[v as usize] == Tag::Coerce
        && b.row_adaptation(v as usize).is_none()
    {
        v = b.data[v as usize][0];
    }
    (v as usize) < b.tags.len()
        && b.tags[v as usize] == Tag::Read
        && b.decode_place(b.data[v as usize][0]).is_some_and(|p| {
            matches!(p.root, PlaceRoot::Local(l) if l.raw() == root) && p.proj.is_empty()
        })
}

/// Whether two paths of one local overlap: no step of the one is a field
/// other than the other's at the same depth (`ref.excl.place`).
fn overlap(a: &[Proj], b: &[Proj]) -> bool {
    !a.iter()
        .zip(b)
        .any(|p| matches!(p, (Proj::Field(x), Proj::Field(y)) if x != y))
}

impl Gen<'_, '_> {
    /// The writes of the body: each instruction that writes a place
    /// (`ref.excl.write`), the place, and where the write happens. A
    /// `&mut` a call takes writes at the call, a bound one where it is
    /// taken.
    fn writes(&self, users: &[u32]) -> Vec<(u32, Path, u32)> {
        let (b, pool) = (self.s.b, self.s.pool);
        let mut out = Vec::new();
        for (i, &u) in users.iter().enumerate() {
            let [a, d] = b.data[i];
            let (u, calls) = (u as usize, |t: Option<&Tag>| {
                matches!(
                    t,
                    Some(Tag::Call | Tag::CallDyn | Tag::CallValue | Tag::Await)
                )
            });
            // The place, where it is written, and whether through a `&mut`.
            let (word, at, through) = match b.tags[i] {
                Tag::Store if !self.declares(i) => (a, i, false),
                // A `&mut` a call takes or a binding keeps, or a `&mut`
                // binding a call reborrows.
                Tag::Ref if is_mut(pool, b.ty[i]) => {
                    (a, if calls(b.tags.get(u)) { u } else { i }, false)
                }
                Tag::Read if is_mut(pool, b.ty[i]) && calls(b.tags.get(u)) => (a, u, true),
                Tag::Intrinsic
                    if matches!(
                        IntrinsicOp::from_u32(a),
                        Some(
                            IntrinsicOp::ListPush
                                | IntrinsicOp::ListReserve
                                | IntrinsicOp::MapSet
                                | IntrinsicOp::MapRemove
                        )
                    ) =>
                {
                    match b.record(d).first() {
                        Some(&c) if b.tags.get(c as usize) == Some(&Tag::Read) => {
                            (b.data[c as usize][0], i, false)
                        }
                        _ => continue,
                    }
                }
                _ => continue,
            };
            if let Some((l, mut p)) = path_of(b, word) {
                if through {
                    p.push(Proj::Deref);
                }
                out.push((w(i), (l, p), w(at)));
            }
        }
        out
    }

    /// The instruction that first takes each value as an operand.
    fn users(&self) -> Vec<u32> {
        let b = self.s.b;
        crate::work::scan_work(b.tags.len());
        let mut users = vec![NONE; b.tags.len()];
        for j in 0..b.tags.len() {
            slots(b, j, |_, v, word| {
                if matches!(word, Word::Value(_)) && (v as usize) < j && users[v as usize] == NONE {
                    users[v as usize] = w(j);
                }
            });
        }
        users
    }

    /// Whether store `i` declares the local it stores: a new place, which
    /// no reference points into yet, as in a loop's next iteration.
    fn declares(&self, i: usize) -> bool {
        let b = self.s.b;
        let (Some(p), at) = (b.decode_place(b.data[i][0]), b.syn[i]) else {
            return false;
        };
        let PlaceRoot::Local(l) = p.root else {
            return false;
        };
        let decl = b.local_syn[l.idx()];
        if !p.proj.is_empty() || at == NodeIdx::NONE || decl == NodeIdx::NONE {
            return false;
        }
        let (lo, hi) = super::super::range(self.s.cx.src.parse.tree.node(at));
        lo <= decl.raw() && decl.raw() < hi
    }

    /// The instructions that read each local: a read, a reference or a
    /// store through it, and a closure that captures it; and the stores
    /// of each whole local.
    fn reads(&self) -> (Vec<Vec<u32>>, Vec<Vec<u32>>) {
        let b = self.s.b;
        let mut out = vec![Vec::new(); self.locals as usize];
        let mut sets = vec![Vec::new(); self.locals as usize];
        for i in 0..b.tags.len() {
            let tag = b.tags[i];
            if tag == Tag::Store
                && let Some(p) = b.decode_place(b.data[i][0])
                && let (PlaceRoot::Local(l), true) = (p.root, p.proj.is_empty())
            {
                sets[l.idx()].push(w(i));
            }
            if let Tag::Read | Tag::Ref | Tag::Store = tag
                && let Some(p) = b.decode_place(b.data[i][0])
                && let PlaceRoot::Local(l) = p.root
                && (tag != Tag::Store || p.proj.first() == Some(&Proj::Deref))
            {
                out[l.idx()].push(w(i));
            }
            if tag == Tag::Closure
                && !b.is_instantiation(i)
                && let [start, len, ..] = *b.record(b.data[i][1])
            {
                for c in start..start.saturating_add(len) {
                    if let Some(l) = b.cap_local.get(c as usize) {
                        out[l.idx()].push(w(i));
                    }
                }
            }
        }
        (out, sets)
    }

    /// The moves of the body (`ref.excl.move`): each read that moves a
    /// binding or a part of it, and the place it moves.
    fn moves(&self) -> Vec<(u32, Path)> {
        let b = self.s.b;
        let mut out: Vec<(u32, Path)> = (self.s.moves.iter())
            .filter_map(|&i| Some((i, path_of(b, b.data[i as usize][0])?)))
            .collect();
        out.sort_stable_by_key(|x| x.0);
        out
    }

    /// The uses of a place for an exclusive loan (`ref.excl.mut`): each
    /// read, reference and store of it but a declaring one, and each
    /// closure that captures its local. A read that only roots a longer
    /// place is that place's use.
    fn accesses(&self) -> Vec<(u32, Path)> {
        let b = self.s.b;
        let mut base = vec![false; b.tags.len()];
        for i in 0..b.tags.len() {
            if let Tag::Read | Tag::Ref | Tag::Store = b.tags[i]
                && let Some(p) = b.decode_place(b.data[i][0])
                && let PlaceRoot::Value(v) = p.root
            {
                let mut v = v.0 as usize;
                while v < b.tags.len() && b.tags[v] == Tag::Coerce && b.row_adaptation(v).is_none()
                {
                    base[v] = true;
                    v = b.data[v][0] as usize;
                }
                if let Some(x) = base.get_mut(v) {
                    *x = true;
                }
            }
        }
        let mut out = Vec::new();
        for i in (0..b.tags.len()).filter(|&i| !base[i]) {
            match b.tags[i] {
                Tag::Read | Tag::Ref => {}
                Tag::Store if !self.declares(i) => {}
                Tag::Closure if !b.is_instantiation(i) => {
                    if let [start, len, ..] = *b.record(b.data[i][1]) {
                        for c in start..start.saturating_add(len) {
                            if let Some(l) = b.cap_local.get(c as usize) {
                                out.push((w(i), (l.raw(), Vec::new())));
                            }
                        }
                    }
                    continue;
                }
                _ => continue,
            }
            if let Some(p) = path_of(b, b.data[i][0]) {
                out.push((w(i), p));
            }
        }
        out
    }

    /// `ref.excl.call`: each reference a call takes for a receiver
    /// or an argument is a site loan, which the call's result holds when
    /// it depends on that argument.
    pub(super) fn call_loans(&mut self) {
        let b = self.s.b;
        for c in 0..b.tags.len() {
            if !matches!(
                b.tags[c],
                Tag::Call | Tag::CallDyn | Tag::CallValue | Tag::Await
            ) {
                continue;
            }
            let mut args = Vec::new();
            slots(b, c, |_, v, word| {
                if matches!(word, Word::Value(_)) && (v as usize) < c {
                    args.push(v);
                }
            });
            for v in args {
                let Some(&top) = self.val(v).as_ref().and_then(|x| x.0.first()) else {
                    continue;
                };
                if call_site(self.s, v) {
                    self.seed(top, Origin::Site(v));
                }
            }
        }
    }

    /// `ref.excl.shared`, `ref.excl.mut`: each write to a place that
    /// overlaps a site's, or each use of one for an exclusive site, while
    /// a binding that holds the site is still read.
    pub(super) fn conflicts(&self, sol: &Solution) -> Vec<(NodeIdx, String)> {
        let b = self.s.b;
        if !self.loans.iter().any(|o| matches!(o, Origin::Site(_))) {
            return Vec::new();
        }
        let has = |v: &[Region], loan: u32| v.iter().any(|&g| sol.holds(g, loan));
        let (topo, (reads, sets)) = (Topology::new(b), self.reads());
        let order = Order {
            b,
            t: &topo,
            cx: self,
        };
        // Each site: its `Ref`, its place, the bindings that hold it, each
        // with its stores of a value that does not, and an element
        // reference loop's loop.
        let mut sites = Vec::new();
        for (loan, o) in self.loans.iter().enumerate() {
            let Origin::Site(i) = *o else { continue };
            let Some((mut place, kind)) = loan_path(self.s, i) else {
                continue;
            };
            let elem = (raw_loan_path(self.s, i))
                .is_some_and(|(_, p)| p.iter().any(|s| matches!(s, Proj::Elem(_))));
            if let Some((_, p)) = held_path(self.s, i, &place) {
                place = p;
            }
            let holders: Vec<(u32, Vec<u32>)> = (0..self.locals)
                .filter(|&h| h != place.0 && has(&self.local(h).0, w(loan)))
                .map(|h| {
                    let other = (sets[h as usize].iter().copied())
                        .filter(|&s| {
                            !self
                                .val(b.data[s as usize][1])
                                .is_some_and(|v| has(&v.0, w(loan)))
                        })
                        .collect();
                    (h, other)
                })
                .collect();
            let lp = elem.then(|| order.ref_loop(i)).flatten();
            sites.push((i, place, kind, holders, lp));
        }
        if sites.iter().all(|s| s.3.is_empty() && s.4.is_none()) {
            return Vec::new();
        }
        let users = self.users();
        let mut out: Vec<(NodeIdx, String)> = Vec::new();
        // An access: the instruction, its place, where it happens, and
        // whether it writes or moves its place. A move comes first, so its
        // error says so.
        let moves = self
            .moves()
            .into_iter()
            .map(|(i, p)| (i, p, i, Access::Move));
        let writes = self
            .writes(&users)
            .into_iter()
            .map(|(i, p, at)| (i, p, at, Access::Write));
        let touches = self
            .accesses()
            .into_iter()
            .map(|(i, p)| (i, p, i, Access::Use));
        // One error for a node, or a node inside or around it.
        let range = |n: NodeIdx| super::super::range(self.s.cx.src.parse.tree.node(n));
        let nests = |(a, b): (u32, u32), (c, d): (u32, u32)| a <= c && d <= b || c <= a && b <= d;
        for (wi, place, at, access) in moves.chain(writes).chain(touches) {
            let node = b.syn[wi as usize];
            if node == NodeIdx::NONE || out.iter().any(|x| nests(range(x.0), range(node))) {
                continue;
            }
            'site: for (i, (r, sp), kind, holders, lp) in &sites {
                let (root, path) = &place;
                let root = *root;
                if *r != root
                    || !overlap(sp, path)
                    || *i == wi
                    || access == Access::Use && *kind != Kind::Excl
                {
                    continue;
                }
                for (h, other) in holders {
                    let (i, h) = (*i, *h);
                    if !order.hop(i, at, h) || order.kills(other, i, at) {
                        continue;
                    }
                    let live =
                        |u: u32| order.hop(at, u, h) && !order.kills(&sets[h as usize], at, u);
                    // A closure parameter at a call's lifetime holds its
                    // loan until the call ends, so for the rest of the
                    // closure's body (`fn.closure.lifetime.call`).
                    let held = (self.bound.iter())
                        .any(|x| x.0 == h && (x.2.0..x.2.1).contains(&node.raw()));
                    if held || reads[h as usize].iter().any(|&u| live(u)) {
                        let msg = if access == Access::Move {
                            self.move_message(root, h)
                        } else {
                            self.conflict_message(root, Some(h), *kind)
                        };
                        out.push((node, msg));
                        break 'site;
                    }
                }
                // A loop over element references is one access for the
                // whole loop (`flow.for.ref.access`), whatever its binding
                // still reads, a wildcard's included.
                if let Some(l) = *lp
                    && access != Access::Move
                    && order.chain(at).contains(&l)
                {
                    let msg = self.conflict_message(root, None, *kind);
                    let msg = match msg.split_once(" while ") {
                        Some((head, _)) => {
                            format!("{head} in a loop over its elements; do it after the loop")
                        }
                        None => msg,
                    };
                    out.push((node, msg));
                    break 'site;
                }
                // A mutable source reference is the loop's loan, even
                // when none of its yielded bindings is read afterward.
                if let (Kind::Excl, Some(l), Some(_)) =
                    (*kind, order.source_loop(*i), holders.first())
                    && order.chain(at).contains(&l)
                {
                    let name = self.s.cx.names.text(b.local_name[root as usize]);
                    let place = if name.starts_with('$') {
                        "the borrowed collection".to_owned()
                    } else {
                        format!("`{name}`")
                    };
                    out.push((node, format!("this accesses {place} while its `&mut` borrowing loop is active; use the loop binding or wait until the loop ends")));
                    break 'site;
                }
            }
        }
        out
    }

    /// `ref.excl.args`: a `&mut` a call takes against the call's other
    /// arguments and receiver. One that holds a loan of an overlapping
    /// place conflicts, and so does a use of one after a written `&mut`.
    /// `ref.excl.args.shared`: a `&` a call takes against a write or a
    /// move of an overlapping place in a later argument. An element
    /// reference is a loan of its collection, so `swap(&mut xs[0],
    /// &mut xs[1])` conflicts as two `&mut xs` do. A call that holds an
    /// error of `found` already gets no other.
    pub(super) fn call_conflicts(
        &self,
        sol: &Solution,
        found: &[(NodeIdx, String)],
    ) -> Vec<(NodeIdx, String)> {
        let b = self.s.b;
        let range = |n: NodeIdx| super::super::range(self.s.cx.src.parse.tree.node(n));
        // Each site loan: its id, its `Ref`, the place a call's result
        // holds after that call, its place, and whether it is `&mut`.
        let sites: Vec<SiteLoan> = (self.loans.iter().enumerate())
            .filter_map(|(loan, o)| match *o {
                Origin::Site(i) => {
                    let site = loan_path(self.s, i);
                    site.map(|(p, k)| (w(loan), i, held_path(self.s, i, &p), p, k == Kind::Excl))
                }
                _ => None,
            })
            .collect();
        // Each loan's index in `sites`, so an argument's loans are read
        // from its regions' values, never by a scan of every loan per
        // argument.
        let mut slot = vec![None; self.loans.len()];
        for (k, x) in sites.iter().enumerate() {
            slot[x.0 as usize] = Some(w(k));
        }
        // Per solved component an argument reaches, the loans its value
        // holds: `lists[by_comp[c]]`.
        let (mut by_comp, mut lists): (HashMap<u32, u32>, Vec<Vec<u32>>) = Default::default();
        let mut tables: Option<CallTables> = None;
        let mut out: Vec<(NodeIdx, String)> = Vec::new();
        let tables_of = || {
            let firsts = self.users();
            let mut writes = self.writes(&firsts);
            writes.sort_stable_by_key(|x| x.2);
            CallTables {
                writes,
                moves: self.moves(),
                touches: self.accesses(),
            }
        };
        for c in 0..b.tags.len() {
            let receiver = elem_receiver(b, c);
            if !(receiver.is_some()
                || matches!(
                    b.tags[c],
                    Tag::Call | Tag::CallDyn | Tag::CallValue | Tag::Await
                ))
                || b.syn[c] == NodeIdx::NONE
            {
                continue;
            }
            let (lo, hi) = range(b.syn[c]);
            if found.iter().any(|x| (lo..hi).contains(&range(x.0).0)) {
                continue;
            }
            if let Some(recv) = receiver {
                let t = tables.get_or_insert_with(tables_of);
                if let Some(e) = self.elem_receiver_conflict(c, recv, t)
                    && !out.iter().any(|x| x.0 == e.0)
                {
                    out.push(e);
                }
                continue;
            }
            let mut args = Vec::new();
            slots(b, c, |_, v, word| {
                if matches!(word, Word::Value(_)) && (v as usize) < c {
                    args.push(v);
                }
            });
            if args.is_empty() {
                continue;
            }
            let t = tables.get_or_insert_with(tables_of);
            // Each argument's loans: its one component's list as it is,
            // or the sorted union of its components' lists.
            let comps: Vec<Vec<u32>> = (args.iter())
                .map(|&u| self.held_lists(sol, u, &slot, (&mut by_comp, &mut lists)))
                .collect();
            let union = |ks: &[u32]| {
                let mut m: Vec<u32> = (ks.iter())
                    .flat_map(|&k| lists[k as usize].iter().copied())
                    .collect();
                m.sort_stable();
                m.dedup();
                m
            };
            let merged: Vec<Vec<u32>> = (comps.iter())
                .map(|ks| if ks.len() == 1 { Vec::new() } else { union(ks) })
                .collect();
            let held: Vec<&[u32]> = (comps.iter().zip(&merged))
                .map(|(ks, m)| match ks.as_slice() {
                    [k] => lists[*k as usize].as_slice(),
                    _ => m.as_slice(),
                })
                .collect();
            for &v in &args {
                let ((root, path), excl) = match loan_path(self.s, v) {
                    Some((p, Kind::Shared)) => (p, false),
                    Some((p, Kind::Excl)) => (p, true),
                    None => continue,
                };
                let hit = |q: &Path| q.0 == root && overlap(&path, &q.1);
                let mut hits = Vec::new();
                let cw = w(c);
                // A `&`: a write or a move in a later argument, in the
                // order the body lists them.
                let mut moving = false;
                if !excl {
                    let mut later: Vec<(u32, u32)> = window(&t.writes, |x| x.2, v, cw)
                        .iter()
                        .filter(|(_, q, _)| hit(q))
                        .map(|x| (x.0, x.2))
                        .collect();
                    later.sort_stable_by_key(|x| x.0);
                    hits.extend(later.into_iter().flat_map(|(i, at)| [i, at]));
                    if let Some((i, _)) = window(&t.moves, |x| x.0, v, cw)
                        .iter()
                        .find(|(i, q)| hit(q) && b.syn[*i as usize] != NodeIdx::NONE)
                    {
                        hits.insert(0, *i);
                        moving = true;
                    }
                }
                // Another argument that holds a loan of an overlapping
                // place, when either loan is `&mut`, as a tuple holding
                // `&mut xs[0]` beside a written `&xs` does.
                let mut beside_mut = false;
                // A written `&` (a counted pointer passed on is its own
                // owner, not a reference to its place).
                let written = b.tags[v as usize] == Tag::Ref;
                for (u, h) in args.iter().zip(&held).filter(|x| *x.0 != v) {
                    if h.iter().any(|&k| {
                        let (_, i, after, q, k_excl) = &sites[k as usize];
                        let q = after.as_ref().filter(|x| x.0 != c).map_or(q, |x| &x.1);
                        *i != v && (excl || *k_excl && written) && hit(q)
                    }) {
                        hits.push(if excl { *u } else { v });
                        beside_mut |= !excl;
                    }
                }
                // A pointer argument accesses its whole pointee for the
                // call (`ref.excl.args.pointer`): a `*T` beside a `&mut` of
                // a place behind it reads that place.
                if excl && b.tags[v as usize] == Tag::Ref && path.first() == Some(&Proj::Deref) {
                    hits.extend(
                        (args.iter())
                            .filter(|&&u| u != v && passes_pointer(self.s, u, root))
                            .copied(),
                    );
                }
                if excl && site(self.s, v) {
                    hits.extend(
                        window(&t.touches, |x| x.0, v, cw)
                            .iter()
                            .filter(|(_, q)| hit(q))
                            .map(|x| x.0),
                    );
                }
                // One error for a call.
                let node = hits
                    .iter()
                    .map(|&u| b.syn[u as usize])
                    .find(|&n| n != NodeIdx::NONE);
                if let Some(node) = node.filter(|n| !out.iter().any(|x| x.0 == *n)) {
                    let n = self.s.cx.names.text(b.local_name[root as usize]);
                    let place = if n.starts_with('$') {
                        "a temporary".to_owned()
                    } else {
                        format!("`{n}`")
                    };
                    out.push((node, if excl {
                        format!("this uses {place} while the same call takes `&mut` of it; copy what this argument needs into a local before the call")
                    } else if beside_mut && hits.first() == Some(&v) {
                        format!("this refers to {place} while another argument of the same call takes `&mut` of it; copy what this argument needs into a local before the call")
                    } else if moving {
                        format!("this moves {place} while the same call takes `&` of it; move it after the call")
                    } else {
                        format!("this writes {place} while the same call takes `&` of it; compute this argument into a local before the call")
                    }));
                    break;
                }
            }
        }
        out
    }

    /// `ref.elem.access.live`, `fn.method.eval-order`: the element
    /// receiver read `recv` of built-in call `c` accesses its collection
    /// from that read until the call, so a change of the collection or of
    /// a place holding it in an argument conflicts. One binary search in
    /// `t.writes` and the writes between the two.
    fn elem_receiver_conflict(
        &self,
        c: usize,
        recv: u32,
        t: &CallTables,
    ) -> Option<(NodeIdx, String)> {
        let b = self.s.b;
        let (root, mut sp) = path_of(b, b.data[recv as usize][0])?;
        sp.truncate(sp.iter().rposition(|s| matches!(s, Proj::Elem(_)))?);
        let (wi, at) = window(&t.writes, |x| x.2, recv, w(c))
            .iter()
            .filter(|(_, (r, q), _)| *r == root && q.len() <= sp.len() && overlap(&sp, q))
            .map(|x| (x.0, x.2))
            .min()?;
        let node = [wi, at]
            .into_iter()
            .map(|x| b.syn[x as usize])
            .find(|&n| n != NodeIdx::NONE)?;
        let n = self.s.cx.names.text(b.local_name[root as usize]);
        let place = if n.starts_with('$') {
            "a temporary".to_owned()
        } else {
            format!("`{n}`")
        };
        Some((
            node,
            format!(
                "this may change {place} while the call's receiver is an element of it; make the change before the call"
            ),
        ))
    }

    /// The lists of the site loans value `u` holds, one per solved
    /// component its regions reach, as indexes into `lists`. A
    /// component's list is made once, from its value's loans and `slot`,
    /// each loan's index in `call_conflicts`' site list, so its loans are
    /// read once per body.
    fn held_lists(
        &self,
        sol: &Solution,
        u: u32,
        slot: &[Option<u32>],
        (by_comp, lists): (&mut HashMap<u32, u32>, &mut Vec<Vec<u32>>),
    ) -> Vec<u32> {
        let mut out: Vec<u32> = (self.val(u).map(|x| x.0).unwrap_or_default().into_iter())
            .map(|g| {
                *by_comp.entry(sol.component(g)).or_insert_with(|| {
                    crate::work::scan_work(1);
                    lists.push(sol.loans(g).filter_map(|l| slot[l as usize]).collect());
                    w(lists.len() - 1)
                })
            })
            .collect();
        out.sort_stable();
        out.dedup();
        out
    }

    /// The message of a move of local `root` or a part of it while
    /// `holder` holds a loan of it (`ref.excl.move`).
    fn move_message(&self, root: u32, holder: u32) -> String {
        let place = self.name(root).unwrap_or_else(|| "a temporary".to_owned());
        match self.name(holder) {
            Some(h) => format!(
                "this moves {place} while {h} refers to it and is used later; move it after {h}'s last use"
            ),
            None => format!(
                "this moves {place} while a reference to it is used later; move it after that use"
            ),
        }
    }

    /// The message of a conflict on local `root` with a loan `holder`
    /// holds, if a named binding does.
    fn conflict_message(&self, root: u32, holder: Option<u32>, kind: Kind) -> String {
        let place = self.name(root).unwrap_or_else(|| "a temporary".to_owned());
        match (holder.and_then(|h| self.name(h)), kind) {
            (Some(h), Kind::Excl) => format!(
                "this uses {place} while {h} refers to it as `&mut` and is used later; use {place} after {h}'s last use, or through {h}"
            ),
            (None, Kind::Excl) => format!(
                "this uses {place} while a `&mut` reference to it is used later; use {place} after that use"
            ),
            (Some(h), Kind::Shared) => format!(
                "this changes {place} while {h} refers to it and is used later; make the change after {h}'s last use"
            ),
            (None, Kind::Shared) => format!(
                "this changes {place} while a reference to it is used later; make the change after that use"
            ),
        }
    }

    /// Local `l`'s name in backquotes, none for a compiler temporary.
    fn name(&self, l: u32) -> Option<String> {
        let n = self.s.cx.names.text(self.s.b.local_name[l as usize]);
        (!n.starts_with('$')).then(|| format!("`{n}`"))
    }
}

/// The order of a body's instructions by its blocks.
struct Order<'x, 's, 'a> {
    b: &'x Body,
    t: &'x Topology,
    cx: &'x Gen<'s, 'a>,
}

impl Order<'_, '_, '_> {
    /// The instruction that holds `x`: the one that runs its block, else
    /// the block that lists it.
    fn up(&self, x: u32) -> Option<u32> {
        let (t, i) = (self.t, x as usize);
        if i >= t.blk.len() || t.is_root[i] {
            return None;
        }
        let p = if t.owner[i] == NONE {
            t.blk[i]
        } else {
            t.owner[i]
        };
        (p != NONE).then_some(p)
    }

    /// A loop whose written source expression made reference `i`.
    /// Its exclusive source loan covers every iteration's whole body.
    fn source_loop(&self, i: u32) -> Option<u32> {
        let b = self.b;
        let at = b.syn[i as usize];
        if at == NodeIdx::NONE || b.tags[i as usize] != Tag::Ref {
            return None;
        }
        let pool = self.cx.s.pool;
        let TyData::Ref { inner, .. } = pool.get(b.ty[i as usize]) else {
            return None;
        };
        if !matches!(pool.get(inner), TyData::Adt(AdtId::Declared(def), _) if def == self.cx.s.cx.names.known.map)
        {
            return None;
        }
        let tree = &self.cx.s.cx.src.parse.tree;
        (0..b.tags.len()).find_map(|l| {
            if b.tags[l] != Tag::Loop || b.syn[l] == NodeIdx::NONE {
                return None;
            }
            let node = tree.node(b.syn[l]);
            if node.kind() != SyntaxKind::ForExpr {
                return None;
            }
            let source = crate::elem_refs::strip_parens(node.children().nth(1)?);
            (source.index() == at).then_some(w(l))
        })
    }

    /// The loop of `for p in &xs` or `for p in &mut xs` whose element
    /// reference `Ref` `i` is: the innermost loop around it, when its
    /// place starts at a read of `xs` made before that loop, once.
    fn ref_loop(&self, i: u32) -> Option<u32> {
        let b = self.b;
        let p = b.decode_place(b.data[i as usize][0])?;
        let PlaceRoot::Value(r) = p.root else {
            return None;
        };
        let r = r.as_inst()?.0;
        if b.tags.get(r as usize) != Some(&Tag::Read) {
            return None;
        }
        let l = (self.chain(i).into_iter()).find(|&a| b.tags[a as usize] == Tag::Loop)?;
        (!self.chain(r).contains(&l)).then_some(l)
    }

    fn chain(&self, x: u32) -> Vec<u32> {
        std::iter::successors(Some(x), |&y| self.up(y))
            .take(self.t.blk.len())
            .collect()
    }

    /// Whether `y` may run after `x` along a path through local `h`'s
    /// lifetime: later in the block structure, or in a later iteration of
    /// a loop that holds both and that `h` was declared outside.
    fn hop(&self, x: u32, y: u32, h: u32) -> bool {
        let (cx, cy) = (self.chain(x), self.chain(y));
        let Some(k) = cy.iter().position(|a| cx.contains(a)) else {
            return x < y;
        };
        let lca = cy[k];
        let again = cy[k..]
            .iter()
            .any(|&l| self.b.tags[l as usize] == Tag::Loop && self.outside(h, l));
        let before = x < y || self.decides(&cx, &cy, k, lca);
        again || before && !self.apart(&cx, &cy, k, lca) && !self.ends(&cx, lca)
    }

    /// Whether `x` lies in the block that decides match `lca` and `y` in
    /// one of its arms, which runs after it whatever their order in the
    /// body: a pattern's binding (`ref.match.borrow`) is made there.
    fn decides(&self, cx: &[u32], cy: &[u32], k: usize, lca: u32) -> bool {
        let b = self.b;
        if b.tags[lca as usize] != Tag::Match || k == 0 {
            return false;
        }
        let Some(j) = cx.iter().position(|&a| a == lca).filter(|&j| j > 0) else {
            return false;
        };
        let first = b.record(b.data[lca as usize][1]).first().copied();
        Some(cx[j - 1]) == first && Some(cy[k - 1]) != first
    }

    /// Whether one of the stores `sets` of a binding runs on every path
    /// from `x` forward to `y`: a statement between theirs in a block that
    /// holds both. What the binding holds after it, it read after `x`.
    fn kills(&self, sets: &[u32], x: u32, y: u32) -> bool {
        let (cx, cy) = (self.chain(x), self.chain(y));
        let at = |c: &[u32], blk: u32| {
            let k = c.iter().position(|&a| a == blk)?;
            k.checked_sub(1).map(|j| self.t.pos[c[j] as usize])
        };
        x < y && sets.iter().any(|&s| {
            let (blk, pos) = (self.t.blk[s as usize], self.t.pos[s as usize]);
            blk != NONE
                && matches!((at(&cx, blk), at(&cy, blk)), (Some(a), Some(b)) if a < pos && pos < b)
        })
    }

    /// Whether `x` and `y` lie in two arms of a branch.
    fn apart(&self, cx: &[u32], cy: &[u32], k: usize, lca: u32) -> bool {
        let Some(j) = cx.iter().position(|&a| a == lca) else {
            return false;
        };
        if j == 0 || k == 0 {
            return false;
        }
        let (bx, by) = (cx[j - 1], cy[k - 1]);
        let b = self.b;
        match b.tags[lca as usize] {
            Tag::If | Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => {
                bx != by
            }
            // A match's first block decides, and its arms follow it.
            Tag::Match => {
                let first = b.record(b.data[lca as usize][1]).first().copied();
                bx != by && Some(bx) != first && Some(by) != first
            }
            _ => false,
        }
    }

    /// Whether a statement after `x`'s, in a block between `x` and `lca`,
    /// never finishes and leaves no way out of that block forward: a
    /// `return`, a `continue`, a call that never returns.
    fn ends(&self, cx: &[u32], lca: u32) -> bool {
        let b = self.b;
        cx.windows(2)
            .take_while(|p| p[0] != lca && p[1] != lca)
            .any(|p| {
                let (c, blk) = (p[0], p[1] as usize);
                b.tags[blk] == Tag::Block
                    && self.t.blk[c as usize] == p[1]
                    && (b.record(b.data[blk][0]).iter())
                        .skip(self.t.pos[c as usize] as usize + 1)
                        .any(|&s| {
                            let tag = b.tags.get(s as usize);
                            matches!(
                                tag,
                                Some(Tag::Return | Tag::Continue | Tag::Call | Tag::CallDyn)
                            ) && b.ty[s as usize] == Ty::NEVER
                        })
            })
    }

    /// Whether local `h` was declared outside loop `l`, so that it keeps
    /// what an iteration put in it.
    fn outside(&self, h: u32, l: u32) -> bool {
        let b = self.b;
        let decl = b.local_syn[h as usize];
        if b.local_flags[h as usize] & local_flags::PARAM != 0 || decl == NodeIdx::NONE {
            return true;
        }
        let at = b.syn[l as usize];
        if at == NodeIdx::NONE {
            return true;
        }
        let (lo, hi) = super::super::range(self.cx.s.cx.src.parse.tree.node(at));
        !(lo <= decl.raw() && decl.raw() < hi)
    }
}
