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
//! iteration. An element reference is a loan of its whole collection,
//! which only a change of the collection's length conflicts with
//! (`ref.elem.access.length`).

use hd_base::{NodeIdx, SliceSort as _};
use hd_tir::ir::{
    Body, IntrinsicOp, NONE, PlaceRoot, Proj, Ref, Tag, Topology, Word, local_flags, slots,
};
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
            while b.ty.get(v as usize).is_some() && b.tags[v as usize] == Tag::Coerce {
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

/// What a site loan excludes (`ref.excl.shared`, `ref.excl.mut`,
/// `ref.elem.access.length`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Shared,
    Excl,
    /// An element reference: a loan of its whole collection, which only a
    /// change of the collection's length conflicts with.
    Elem,
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

/// The place loan `i` covers and its kind: a `Ref`'s place, or what a
/// `&mut` binding refers to where a call reads it as `&` or as `&mut`
/// (`ref.excl.call`), or a `&` binding where a call's `&mut` result holds
/// it. An element reference covers the collection its last element step
/// indexes, when only fields follow that step; any other path with an
/// element is left to the pin (`ref.elem.access.pin`).
fn loan_path(s: &Inputs<'_>, i: u32) -> Option<(Path, Kind)> {
    let (b, i) = (s.b, i as usize);
    let excl = is_mut(s.pool, b.ty[i]);
    let (l, mut path) = match b.tags[i] {
        Tag::Ref => path_of(b, b.data[i][0]),
        Tag::Read if excl || mut_result(s, w(i)).is_some() => {
            path_of(b, b.data[i][0]).map(|(l, mut p)| {
                p.push(Proj::Deref);
                (l, p)
            })
        }
        Tag::Coerce if b.same_value_coercion(i) && !excl => {
            let v = b.data[i][0] as usize;
            if b.ty.get(v).is_none() || b.tags[v] != Tag::Read || !is_mut(s.pool, b.ty[v]) {
                return None;
            }
            path_of(b, b.data[v][0]).map(|(l, mut p)| {
                p.push(Proj::Deref);
                (l, p)
            })
        }
        _ => None,
    }?;
    let Some(k) = path.iter().rposition(|s| matches!(s, Proj::Elem(_))) else {
        return Some(((l, path), if excl { Kind::Excl } else { Kind::Shared }));
    };
    if b.tags[i] != Tag::Ref || !path[k + 1..].iter().all(|s| matches!(s, Proj::Field(_))) {
        return None;
    }
    path.truncate(k);
    Some(((l, path), Kind::Elem))
}

/// The place a call's result holds for loan `i`, a site of `loan_path`:
/// the call that takes `i` and the place extended by the steps the
/// callee's result paths name for that argument (`ref.result.path.caller`).
/// While the call runs, `i` still covers its whole place.
fn held_path(s: &Inputs<'_>, i: u32, (l, path): &Path, kind: Kind) -> Option<(usize, Path)> {
    if kind == Kind::Elem {
        return None;
    }
    s.calls.iter().find_map(|(&c, sig)| {
        let (_, steps) = sig.paths.iter().find(|x| x.0 == i)?;
        let mut path = path.clone();
        path.extend(steps.iter().copied());
        Some((c as usize, (*l, path)))
    })
}

/// The call whose `&mut` result holds argument `i`, whatever the
/// argument's kind: after the call, its loan is exclusive (`ref.excl.call`).
fn mut_result(s: &Inputs<'_>, i: u32) -> Option<usize> {
    s.calls.iter().find_map(|(&c, sig)| {
        (sig.held.contains(&i) && is_mut(s.pool, s.b.ty[c as usize])).then_some(c as usize)
    })
}

/// Whether `Ref` `i` is a written `&` or `&mut` that takes a site loan
/// (`ref.excl.place`, `ref.excl.mut`), or a pattern's binding of a
/// payload below a reference, which is such a reference too
/// (`ref.match.borrow`).
pub(super) fn site(s: &Inputs<'_>, i: u32) -> bool {
    let at = s.b.syn[i as usize];
    at != NodeIdx::NONE
        && matches!(
            s.cx.src.parse.tree.node(at).kind(),
            SyntaxKind::RefExpr
                | SyntaxKind::BindingPattern
                | SyntaxKind::NamedPattern
                | SyntaxKind::DataPatternField
        )
        && s.b.tags[i as usize] == Tag::Ref
        && loan_path(s, i).is_some()
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
        Tag::Coerce => {
            let v = b.data[iu][0] as usize;
            b.ty.get(v).is_some() && b.tags[v] == Tag::Read && local_mut(s.pool, b.ty[v])
        }
        _ => false,
    };
    read && loan_path(s, i).is_some()
}

/// Whether `t`, under any `mut`, is a `&mut`, not a counted `*mut`.
fn local_mut(pool: Types<'_>, t: Ty) -> bool {
    match pool.get(t) {
        TyData::Ref {
            mutable, counted, ..
        } => mutable && !counted,
        TyData::Mut(i) => local_mut(pool, i),
        _ => false,
    }
}

/// Whether two key values are one key: one constant, or reads of one
/// `:=` binding, which nothing reassigns.
fn same_key(b: &Body, x: u32, y: u32) -> bool {
    let strip = |mut v: u32| {
        while (v as usize) < b.tags.len() && b.tags[v as usize] == Tag::Coerce {
            v = b.data[v as usize][0];
        }
        v
    };
    let (x, y) = (strip(x), strip(y));
    let short = |v: u32| {
        let at = b.data.get(v as usize)?[0];
        let p = b
            .decode_place(at)
            .filter(|_| b.tags[v as usize] == Tag::Read)?;
        match p.root {
            PlaceRoot::Local(l) if p.proj.is_empty() => {
                (b.local_flags[l.idx()] & local_flags::SHORT != 0).then_some(l)
            }
            _ => None,
        }
    };
    // A constant, with a string's text in place of its number.
    let value = |v: u32| {
        let (t, bits) = *b.consts.get((v & !Ref::CONST_BIT) as usize)?;
        let text = (t == Ty::STRING).then(|| b.strings.get(usize::try_from(bits).ok()?));
        Some(match text {
            Some(s) => (t, 0, Some(s?)),
            None => (t, bits, None),
        })
    };
    match (Ref(x).as_inst(), Ref(y).as_inst()) {
        (None, None) => value(x).is_some() && value(x) == value(y),
        (Some(_), Some(_)) => short(x).is_some_and(|l| short(y) == Some(l)),
        _ => false,
    }
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
                while v < b.tags.len() && b.tags[v] == Tag::Coerce {
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
                if loan_path(self.s, v).is_some()
                    && let Some(&top) = self.val(v).as_ref().and_then(|x| x.0.first())
                {
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
        let has = |v: &[Region], loan: u32| v.iter().any(|&g| sol.value(g).contains(loan));
        let (topo, (reads, sets)) = (Topology::new(b), self.reads());
        // Each site: its `Ref`, its place, and the bindings that hold it,
        // each with its stores of a value that does not.
        let mut sites = Vec::new();
        for (loan, o) in self.loans.iter().enumerate() {
            let Origin::Site(i) = *o else { continue };
            let Some((mut place, mut kind)) = loan_path(self.s, i) else {
                continue;
            };
            if let Some((_, p)) = held_path(self.s, i, &place, kind) {
                place = p;
            }
            // A `&mut` result's loan is exclusive from its call on.
            let mut from = i;
            if let (Kind::Shared, Some(c)) = (kind, mut_result(self.s, i)) {
                (kind, from) = (Kind::Excl, w(c));
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
            sites.push((i, from, place, kind, holders));
        }
        if sites.iter().all(|s| s.4.is_empty()) {
            return Vec::new();
        }
        let users = self.users();
        let order = Order {
            b,
            t: &topo,
            cx: self,
        };
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
        for (wi, (root, path), at, access) in moves.chain(writes).chain(touches) {
            let node = b.syn[wi as usize];
            if node == NodeIdx::NONE || out.iter().any(|x| nests(range(x.0), range(node))) {
                continue;
            }
            'site: for (i, from, (r, sp), kind, holders) in &sites {
                if *r != root
                    || !overlap(sp, &path)
                    || *i == wi
                    || access == Access::Use && *kind != Kind::Excl
                {
                    continue;
                }
                // `ref.elem.access.length`: only a change that may move the
                // collection's length, of it or of a place that holds it,
                // or a move of one (`ref.excl.move`).
                if *kind == Kind::Elem
                    && !(path.len() <= sp.len()
                        && (access == Access::Move || self.resizes(wi, path.len() == sp.len(), *i)))
                {
                    continue;
                }
                for (h, other) in holders {
                    let (i, h) = (*from, *h);
                    if !order.hop(i, at, h) || order.kills(other, i, at) {
                        continue;
                    }
                    let live =
                        |u: u32| order.hop(at, u, h) && !order.kills(&sets[h as usize], at, u);
                    if reads[h as usize].iter().any(|&u| live(u)) {
                        let inner =
                            *kind == Kind::Elem && sp.iter().any(|s| matches!(s, Proj::Field(_)));
                        let msg = if access == Access::Move {
                            self.move_message(root, h, *kind, inner)
                        } else {
                            self.conflict_message(root, h, *kind, inner)
                        };
                        out.push((node, msg));
                        break 'site;
                    }
                }
            }
        }
        out
    }

    /// `ref.excl.args`: a `&mut` a call takes against the call's other
    /// arguments and receiver. One that holds a loan of an overlapping
    /// place conflicts, and so does a use of one after a written `&mut`.
    /// `ref.excl.args.shared`: a `&` a call takes against a write or a
    /// move of an overlapping place in a later argument. `ref.elem.conflict`: an
    /// element reference argument against a length change of its
    /// collection. A call that holds an error of `found` already gets no
    /// other.
    pub(super) fn call_conflicts(
        &self,
        sol: &Solution,
        found: &[(NodeIdx, String)],
    ) -> Vec<(NodeIdx, String)> {
        let b = self.s.b;
        let range = |n: NodeIdx| super::super::range(self.s.cx.src.parse.tree.node(n));
        // Each site loan: its id, its `Ref`, its place, and whether it is
        // an element reference's.
        let (elems, sites): (Vec<_>, Vec<_>) = (self.loans.iter().enumerate())
            .filter_map(|(loan, o)| match *o {
                Origin::Site(i) => loan_path(self.s, i).map(|(p, k)| (w(loan), i, p, k)),
                _ => None,
            })
            .partition(|x| x.3 == Kind::Elem);
        let elems: Vec<(u32, u32, Path)> = elems.into_iter().map(|x| (x.0, x.1, x.2)).collect();
        // A site a call's result holds covers only its result path after
        // that call.
        let sites: Vec<_> = (sites.into_iter())
            .map(|(loan, i, p, k)| (loan, i, held_path(self.s, i, &p, k), p))
            .collect();
        let (mut uses, mut writes, mut moves) = (None, None, None);
        let mut out: Vec<(NodeIdx, String)> = Vec::new();
        for c in 0..b.tags.len() {
            if !matches!(
                b.tags[c],
                Tag::Call | Tag::CallDyn | Tag::CallValue | Tag::Await
            ) || b.syn[c] == NodeIdx::NONE
            {
                continue;
            }
            let (lo, hi) = range(b.syn[c]);
            if found.iter().any(|x| (lo..hi).contains(&range(x.0).0)) {
                continue;
            }
            let mut args = Vec::new();
            slots(b, c, |_, v, word| {
                if matches!(word, Word::Value(_)) && (v as usize) < c {
                    args.push(v);
                }
            });
            // An element reference falls under `ref.elem.conflict`.
            let elem = |j: u32, q: &Path| {
                b.tags[j as usize] == Tag::Ref && q.1.iter().any(|s| matches!(s, Proj::Elem(_)))
            };
            for &v in &args {
                let writes = writes.get_or_insert_with(|| self.writes(&self.users()));
                let moves = moves.get_or_insert_with(|| self.moves());
                if let Some(e) = self.elem_arg_conflict(sol, (c, v), &elems, writes, moves) {
                    if !out.iter().any(|x| x.0 == e.0) {
                        out.push(e);
                    }
                    break;
                }
                // An element reference falls under `ref.elem.conflict`.
                let ((root, path), excl) = match loan_path(self.s, v) {
                    Some((p, Kind::Shared)) => (p, false),
                    Some((p, Kind::Excl)) => (p, true),
                    _ => continue,
                };
                let hit = |q: &Path| q.0 == root && overlap(&path, &q.1);
                let mut hits = Vec::new();
                // A `&`: a write or a move in a later argument.
                let mut moving = false;
                if !excl {
                    for (i, q, at) in writes.iter() {
                        if v < *at && (*at as usize) < c && !elem(*i, q) && hit(q) {
                            hits.extend([*i, *at]);
                        }
                    }
                    if let Some((i, _)) = (moves.iter()).find(|(i, q)| {
                        v < *i && (*i as usize) < c && hit(q) && b.syn[*i as usize] != NodeIdx::NONE
                    }) {
                        hits.insert(0, *i);
                        moving = true;
                    }
                }
                for &u in args.iter().filter(|&&u| excl && u != v) {
                    let held = self.val(u).map(|x| x.0).unwrap_or_default();
                    if sites.iter().any(|(loan, i, after, q)| {
                        let q = after.as_ref().filter(|x| x.0 != c).map_or(q, |x| &x.1);
                        *i != v && hit(q) && held.iter().any(|&g| sol.value(g).contains(*loan))
                    }) {
                        hits.push(u);
                    }
                }
                if excl && site(self.s, v) {
                    let uses = uses.get_or_insert_with(|| self.accesses());
                    let after = |j: u32, q: &Path| v < j && (j as usize) < c && !elem(j, q);
                    hits.extend(
                        uses.iter()
                            .filter(|(j, q)| after(*j, q) && hit(q))
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

    /// `ref.elem.conflict`: argument `v` of call `c` holds an element
    /// reference of `elems`, and the call takes `&mut` of its collection or
    /// of a place that holds it in another argument, or a later argument
    /// changes the collection's length (`ref.elem.access.length`) or moves
    /// it (`ref.excl.move`).
    fn elem_arg_conflict(
        &self,
        sol: &Solution,
        (c, v): (usize, u32),
        elems: &[(u32, u32, Path)],
        writes: &[(u32, Path, u32)],
        moves: &[(u32, Path)],
    ) -> Option<(NodeIdx, String)> {
        let b = self.s.b;
        let held = self.val(v).map(|x| x.0).unwrap_or_default();
        let name = |r: u32, sp: &[Proj]| {
            let n = self.s.cx.names.text(b.local_name[r as usize]);
            let place = if n.starts_with('$') {
                "a temporary".to_owned()
            } else {
                format!("`{n}`")
            };
            let inner = sp.iter().any(|s| matches!(s, Proj::Field(_)));
            (place, inner)
        };
        for (loan, i, (root, sp)) in elems {
            if !held.iter().any(|&g| sol.value(g).contains(*loan)) {
                continue;
            }
            for (mi, (r, q)) in moves {
                let node = b.syn[*mi as usize];
                if v < *mi
                    && (*mi as usize) < c
                    && r == root
                    && q.len() <= sp.len()
                    && overlap(sp, q)
                    && node != NodeIdx::NONE
                {
                    let (place, inner) = name(*r, sp);
                    let what = if inner {
                        "an element of a collection in it"
                    } else {
                        "an element of it"
                    };
                    return Some((
                        node,
                        format!(
                            "this moves {place} while another argument of the same call refers to {what}; move it after the call"
                        ),
                    ));
                }
            }
            for &(wi, (r, ref q), at) in writes {
                let during = at as usize == c && wi != v || v < at && (at as usize) < c;
                if !during
                    || r != *root
                    || q.len() > sp.len()
                    || !overlap(sp, q)
                    || !self.resizes(wi, q.len() == sp.len(), *i)
                {
                    continue;
                }
                let node = [wi, at].map(|x| b.syn[x as usize]);
                let Some(node) = node.into_iter().find(|&n| n != NodeIdx::NONE) else {
                    continue;
                };
                let (mut place, inner) = name(r, sp);
                if inner {
                    place = format!("a collection in {place}");
                }
                return Some((
                    node,
                    format!(
                        "this may change the length of {place} while another argument of the same call refers to an element of it; pass the element's index instead, or make the change outside the call"
                    ),
                ));
            }
        }
        None
    }

    /// Whether write `i` may change the length of the collection element
    /// reference `site` indexes, `own` when it writes that collection: no
    /// store, which replaces a value (`ref.elem.pinned.allowed`,
    /// `ref.elem.replace`), no `reserve`, and no map store of the key
    /// `site` refers to, which writes that element.
    fn resizes(&self, i: u32, own: bool, site: u32) -> bool {
        let b = self.s.b;
        let [op, rec] = b.data[i as usize];
        match (b.tags[i as usize], IntrinsicOp::from_u32(op)) {
            (Tag::Store, _) | (Tag::Intrinsic, Some(IntrinsicOp::ListReserve)) => false,
            (Tag::Intrinsic, Some(IntrinsicOp::MapSet)) if own => {
                let entry = b.decode_place(b.data[site as usize][0]).and_then(|p| {
                    p.proj.iter().rev().find_map(|s| match s {
                        Proj::Elem(e) => Some(e.as_inst().map(|e| e.0 as usize)),
                        _ => None,
                    })?
                });
                let key = entry
                    .filter(|&e| b.tags.get(e) == Some(&Tag::Intrinsic))
                    .filter(|&e| IntrinsicOp::from_u32(b.data[e][0]) == Some(IntrinsicOp::MapEntry))
                    .and_then(|e| b.record(b.data[e][1]).get(1).copied());
                !matches!((key, b.record(rec).get(1)), (Some(k), Some(&s)) if same_key(b, k, s))
            }
            _ => true,
        }
    }

    /// The message of a move of local `root` or a part of it while a
    /// loan of it is live (`ref.excl.move`), `inner` when an element
    /// reference's collection is a part of it.
    fn move_message(&self, root: u32, holder: u32, kind: Kind, inner: bool) -> String {
        let (b, names) = (self.s.b, &self.s.cx.names);
        let name = |l: u32| {
            let n = names.text(b.local_name[l as usize]);
            (!n.starts_with('$')).then(|| format!("`{n}`"))
        };
        let place = name(root).unwrap_or_else(|| "a temporary".to_owned());
        let what = match (kind, inner) {
            (Kind::Elem, true) => "an element of a collection in it",
            (Kind::Elem, false) => "an element of it",
            _ => "it",
        };
        match name(holder) {
            Some(h) => format!(
                "this moves {place} while {h} refers to {what} and is used later; move it after {h}'s last use"
            ),
            None => format!(
                "this moves {place} while a reference to {what} is used later; move it after that use"
            ),
        }
    }

    /// The message of a conflict on local `root`, `inner` when the
    /// collection is a part of it.
    fn conflict_message(&self, root: u32, holder: u32, kind: Kind, inner: bool) -> String {
        let (b, names) = (self.s.b, &self.s.cx.names);
        let name = |l: u32| {
            let n = names.text(b.local_name[l as usize]);
            (!n.starts_with('$')).then(|| format!("`{n}`"))
        };
        let mut place = name(root).unwrap_or_else(|| "a temporary".to_owned());
        if inner {
            place = format!("a collection in {place}");
        }
        match (name(holder), kind) {
            (Some(h), Kind::Elem) => format!(
                "this may change the length of {place} while {h} refers to an element of it and is used later; make the change after {h}'s last use"
            ),
            (None, Kind::Elem) => format!(
                "this may change the length of {place} while a reference to an element of it is used later; make the change after that use"
            ),
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
