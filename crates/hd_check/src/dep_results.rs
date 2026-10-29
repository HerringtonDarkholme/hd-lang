//! Dependent results (spec/lang/13-values-and-references.md, "Dependent
//! Results"): a function's result may hold a local reference, which
//! depends on the parameters whose reference slots share its lifetime
//! ([`DepSig`]): a written lifetime variable, or the one its signature
//! elides.
//!
//! Once a body is checked, [`Ck::check_dependent_results`] solves the
//! origins of its local-holding values over the body's TIR, once and per
//! binding, as Rust's NLL does for regions (`ref.dep.origin.*`), with one
//! origin set per reference slot of each value (`ref.dep.slot`). An
//! origin is a storage root a local reference may point into: what a
//! parameter's reference slot refers to, a local's or a parameter's own
//! storage, the hidden local of `&e` on a value, or a top-level binding.
//! A returned value's slot may depend only on the parameter slots of its
//! lifetime (`ref.dep.return`), a call's result slot depends on the
//! argument slots of its lifetime (`ref.dep.call`), a store through a
//! reference lands in a place of the body (`ref.dep.store`). An element
//! reference takes the origin of its collection's root place
//! (`ref.dep.origin.element`).
//!
//! A value's slots are its reference slots at the top, outside another
//! reference ([`tops`]); what a reference's referent holds comes from its
//! origins' storage. Where a projection or a construction cannot be
//! lined up with the slots, each slot takes the union of all of them,
//! which only adds origins.

use hd_base::SliceSort as _;
use hd_base::{DefId, HashMap, HashSet, NodeIdx, Symbol};
use hd_diag::Code;
use hd_resolve::ItemData;
use hd_syntax::{GreenTree, NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{Body, BodyKind, Callee, NONE, PlaceRoot, Proj, Role, Tag, Word, slots};
use hd_types::{Ty, TyData, Types};

use crate::body::{BodyCx, Ck};
use crate::call::subst_owner;
use crate::local_refs::holds_local_ref;

/// The lifetime of an input slot that writes none: its own, `ANON` plus
/// the slot's index. A written lifetime is its 1-based place in scope.
const ANON: u32 = 1 << 16;

/// What a result slot that writes no lifetime depends on (`ref.dep.elide`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Elided {
    /// The receiver's lifetime, or the parameters' one lifetime.
    Lifetime(u32),
    /// No reference position: `reference-result-without-source`.
    Missing,
    /// Two or more lifetimes, and no reference receiver:
    /// `ambiguous-reference-source`.
    Ambiguous,
}

/// A signature's dependency map (`ref.dep.slot`): the lifetime of each
/// reference slot of its parameters and of its result, in
/// [`Types::ref_slots`] order over the declared types.
pub(crate) struct DepSig {
    /// Each parameter's declared type and its slots' lifetimes.
    params: Vec<(Ty, Vec<u32>)>,
    /// Each result slot's lifetime; `None` when it has no source.
    outs: Vec<Option<u32>>,
    ret: Ty,
    elided: Elided,
}

impl DepSig {
    /// The map of a signature: `written` is `FnSig::lifetimes`, one entry
    /// per slot of `params` and then of `ret`, 0 where none is written.
    fn new(
        pool: Types<'_>,
        params: &[(Symbol, Ty)],
        is_self: impl Fn(Symbol) -> bool,
        ret: Ty,
        written: &[u16],
    ) -> DepSig {
        let ins: usize = params.iter().map(|&(_, t)| pool.ref_slots(t)).sum();
        let outs_n = pool.ref_slots(ret);
        let trust_in = written.len() >= ins;
        let trust_out = written.len() == ins + outs_n;
        let mut g = 0;
        let mut ps = Vec::with_capacity(params.len());
        for &(_, t) in params {
            let n = pool.ref_slots(t);
            let ids: Vec<u32> = (g..g + n)
                .map(|k| match written.get(k) {
                    Some(&l) if trust_in && l != 0 => u32::from(l),
                    _ => ANON + w(k),
                })
                .collect();
            g += n;
            ps.push((t, ids));
        }
        let receiver = params
            .first()
            .is_some_and(|&(s, t)| is_self(s) && local_ref(pool, t).is_some_and(|(c, _)| !c));
        let elided = match ps.first() {
            Some((_, ids)) if receiver && !ids.is_empty() => Elided::Lifetime(ids[0]),
            _ => {
                let mut ids: Vec<u32> = params
                    .iter()
                    .zip(&ps)
                    .filter(|((s, _), _)| !is_self(*s))
                    .flat_map(|(_, (_, ids))| ids.iter().copied())
                    .collect();
                ids.sort_stable();
                ids.dedup();
                match ids.as_slice() {
                    [one] => Elided::Lifetime(*one),
                    [] => Elided::Missing,
                    _ => Elided::Ambiguous,
                }
            }
        };
        let outs = (0..outs_n)
            .map(|j| match written.get(ins + j) {
                Some(&l) if trust_out && l != 0 => Some(u32::from(l)),
                _ => match elided {
                    Elided::Lifetime(id) => Some(id),
                    _ => None,
                },
            })
            .collect();
        DepSig {
            params: ps,
            outs,
            ret,
            elided,
        }
    }

    /// Whether the result depends on any input.
    fn depends(&self) -> bool {
        self.outs.iter().any(Option::is_some) || matches!(self.elided, Elided::Lifetime(_))
    }

    /// Every lifetime the result may depend on, for a value whose slots do
    /// not line up with the result's.
    fn all_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.outs.iter().flatten().copied().collect();
        if let Elided::Lifetime(id) = self.elided {
            ids.push(id);
        }
        ids.sort_stable();
        ids.dedup();
        ids
    }

    /// The lifetimes of the result's top slots ([`tops`]), each a set:
    /// one lifetime per slot, or for another shape every lifetime per
    /// slot of `t`.
    fn result_ids(&self, pool: Types<'_>, t: Ty) -> Vec<Vec<u32>> {
        let have = tops(pool, t);
        let want = tops(pool, self.ret);
        if have.len() == want.len() && !have.is_empty() {
            return want
                .iter()
                .map(|&(r, _)| self.outs.get(r).copied().flatten().into_iter().collect())
                .collect();
        }
        vec![self.all_ids(); width(pool, t)]
    }

    /// The input slots `(parameter, slot)` of lifetime `id`.
    fn inputs(&self, id: u32) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.params
            .iter()
            .enumerate()
            .flat_map(move |(p, (_, ids))| {
                ids.iter()
                    .enumerate()
                    .filter(move |&(_, l)| *l == id)
                    .map(move |(s, _)| (p, s))
            })
    }

    /// Whether the result slot of lifetime `id` may depend on `o`: a
    /// parameter slot of that lifetime, or a reference slot behind one,
    /// whose referent outlives it (`ref.dep.return`).
    fn allows(&self, pool: Types<'_>, o: Origin, id: u32) -> bool {
        let Origin::Param(p, s) = o else {
            return false;
        };
        let Some((t, ids)) = self.params.get(p as usize) else {
            return false;
        };
        let s = s as usize;
        if ids.get(s) == Some(&id) {
            return true;
        }
        let all = slots_of(pool, *t);
        (0..s.min(all.len())).any(|s0| {
            ids.get(s0) == Some(&id) && s <= s0 + pool.ref_slots(referent(pool, all[s0].0))
        })
    }
}

/// Each reference slot of `t` in [`Types::ref_slots`] order: the
/// reference type, and whether it is at the top, outside another
/// reference.
fn slots_of(pool: Types<'_>, t: Ty) -> Vec<(Ty, bool)> {
    fn walk(pool: Types<'_>, t: Ty, top: bool, depth: u32, out: &mut Vec<(Ty, bool)>) {
        if depth > 32 || !pool.has_ref(t) {
            return;
        }
        let d = depth + 1;
        match pool.get(t) {
            TyData::Ref {
                counted: false,
                inner,
                ..
            } => {
                out.push((t, top));
                walk(pool, inner, false, d, out);
            }
            TyData::Mut(i) | TyData::Option(i) => walk(pool, i, top, d, out),
            TyData::Tuple { elems, rest } => {
                for &e in pool.list_items(elems) {
                    walk(pool, e, top, d, out);
                }
                if let Some(r) = rest {
                    walk(pool, r, top, d, out);
                }
            }
            TyData::Adt { args, .. } | TyData::TraitValue { args, .. } => {
                for &a in pool.list_items(args) {
                    walk(pool, a, top, d, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(pool, t, true, 0, &mut out);
    out
}

/// The top slots of `t`: each one's index among the reference slots and
/// its reference type.
fn tops(pool: Types<'_>, t: Ty) -> Vec<(usize, Ty)> {
    slots_of(pool, t)
        .into_iter()
        .enumerate()
        .filter(|(_, (_, top))| *top)
        .map(|(k, (t, _))| (k, t))
        .collect()
}

/// How many origin sets a value of type `t` has: one per top slot, or one
/// for a trait value with none.
fn width(pool: Types<'_>, t: Ty) -> usize {
    let n = tops(pool, t).len();
    if n == 0 && may_hold(pool, t) { 1 } else { n }
}

/// What reference type `t` refers to.
fn referent(pool: Types<'_>, t: Ty) -> Ty {
    match pool.get(t) {
        TyData::Ref { inner, .. } => inner,
        TyData::Mut(i) => referent(pool, i),
        _ => Ty::VOID,
    }
}

/// `Some((counted, to a trait value))` when `t` is a reference type,
/// under any `mut`.
fn local_ref(pool: Types<'_>, t: Ty) -> Option<(bool, bool)> {
    match pool.get(t) {
        TyData::Ref { counted, inner, .. } => Some((counted, is_dyn(pool, inner))),
        TyData::Mut(i) => local_ref(pool, i),
        _ => None,
    }
}

fn is_dyn(pool: Types<'_>, t: Ty) -> bool {
    match pool.get(t) {
        TyData::TraitValue { .. } => true,
        TyData::Mut(i) => is_dyn(pool, i),
        _ => false,
    }
}

/// Whether a value of type `t` may have origins: it holds a local
/// reference, or it is a trait value, which may hold one
/// (`trait.dyn.ref.convert`).
fn may_hold(pool: Types<'_>, t: Ty) -> bool {
    holds_local_ref(pool, t) || is_dyn(pool, t)
}

/// A storage root a local reference may point into (`ref.dep.origin.set`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
enum Origin {
    /// What reference slot `s` of parameter `$p` refers to, which the
    /// caller owns: `p*`.
    Param(u32, u32),
    /// The own storage of local `$l`, a parameter's included.
    Local(u32),
    /// The hidden local of `&e` on a value, at the `Ref`.
    Hidden(u32),
    /// A top-level binding.
    Global(u32),
}

/// An instruction's, a local's or a parameter's index as a word.
fn w(i: usize) -> u32 {
    u32::try_from(i).expect("an index")
}

/// An origin set, sorted.
type Set = Vec<Origin>;

fn union(into: &mut Set, from: &[Origin]) -> bool {
    let n = into.len();
    for o in from {
        if let Err(at) = into.binary_search(o) {
            into.insert(at, *o);
        }
    }
    into.len() != n
}

/// The union of a value's slots.
fn flat(v: &[Set]) -> Set {
    let mut s = Set::new();
    for x in v {
        union(&mut s, x);
    }
    s
}

/// Adds `from` to `into` slot by slot, or when the two do not line up,
/// all of `from` to every slot.
fn union_into(into: &mut [Set], from: &[Set]) -> bool {
    let mut changed = false;
    if into.len() == from.len() {
        for (a, b) in into.iter_mut().zip(from) {
            changed |= union(a, b);
        }
    } else {
        let all = flat(from);
        for a in into.iter_mut() {
            changed |= union(a, &all);
        }
    }
    changed
}

/// `v` as `n` slots: itself, or each slot the union of all of it.
fn fit(v: &[Set], n: usize) -> Vec<Set> {
    if v.len() == n {
        v.to_vec()
    } else {
        vec![flat(v); n]
    }
}

/// The origin sets of one body, the least solution of its constraints.
struct Solve<'a> {
    b: &'a Body,
    pool: Types<'a>,
    cx: &'a BodyCx<'a>,
    /// Each instruction's value, one set per slot.
    val: Vec<Vec<Set>>,
    /// What each local's or hidden local's storage holds: `O(b)`, one set
    /// per slot of a local, one for a hidden local.
    held: HashMap<Origin, Vec<Set>>,
    /// The dependent calls' callees.
    deps: &'a HashMap<u32, DepSig>,
    /// Each parameter's type, which `Origin::Param` slots index.
    params: Vec<Ty>,
    /// Stores of a local-holding value into a place the caller owns.
    out: HashSet<u32>,
    changed: bool,
}

impl Solve<'_> {
    fn of(&self, r: u32) -> &[Set] {
        if r == NONE || r & hd_tir::ir::Ref::CONST_BIT != 0 {
            return &[];
        }
        self.val.get(r as usize).map_or(&[], Vec::as_slice)
    }

    fn ty(&self, r: u32) -> Option<Ty> {
        (r & hd_tir::ir::Ref::CONST_BIT == 0)
            .then(|| self.b.ty.get(r as usize).copied())
            .flatten()
    }

    /// What storage `o` holds, one set per slot of its type.
    fn held(&self, o: Origin) -> Vec<Set> {
        match o {
            Origin::Param(p, s) => {
                let Some(&t) = self.params.get(p as usize) else {
                    return vec![vec![o]];
                };
                let Some(&(rt, _)) = slots_of(self.pool, t).get(s as usize) else {
                    return vec![vec![o]];
                };
                let inner = referent(self.pool, rt);
                let ts = tops(self.pool, inner);
                if ts.is_empty() {
                    return if may_hold(self.pool, inner) {
                        vec![vec![o]]
                    } else {
                        vec![]
                    };
                }
                ts.iter()
                    .map(|&(r, _)| vec![Origin::Param(p, s + 1 + w(r))])
                    .collect()
            }
            Origin::Local(_) | Origin::Hidden(_) => self.held.get(&o).cloned().unwrap_or_default(),
            Origin::Global(_) => vec![],
        }
    }

    /// Adds `from` to what storage `o` holds, in its slots `range` when
    /// that is known.
    fn hold(&mut self, o: Origin, from: &[Set], range: Option<(usize, usize)>) {
        if from.iter().all(Vec::is_empty) {
            return;
        }
        let n = match o {
            Origin::Local(l) => width(self.pool, self.b.local_ty[l as usize]),
            Origin::Hidden(h) => {
                width(self.pool, referent(self.pool, self.b.ty[h as usize])).max(1)
            }
            _ => return,
        };
        let e = self.held.entry(o).or_insert_with(|| vec![Set::new(); n]);
        let changed = match range {
            Some((s, k)) if s + k <= e.len() && from.len() == k => {
                union_into(&mut e[s..s + k], from)
            }
            _ => union_into(e, from),
        };
        self.changed |= changed;
    }

    /// Field `j` of type `t`, of variant `variant` for an enum: its type,
    /// and the run of `t`'s top slots it holds, when that is known.
    fn field(&self, t: Ty, variant: Option<u32>, j: u32) -> Option<(Ty, Option<(usize, usize)>)> {
        let pool = self.pool;
        let n = |t: Ty| tops(pool, t).len();
        match (pool.get(t), variant) {
            (TyData::Mut(i), _) => self.field(i, variant, j),
            (TyData::Tuple { elems, .. }, None) => {
                let es = pool.list_items(elems);
                let e = *es.get(j as usize)?;
                let start = es[..j as usize].iter().map(|&x| n(x)).sum();
                Some((e, Some((start, n(e)))))
            }
            (TyData::Option(i), Some(1)) if j == 0 => Some((i, Some((0, n(i))))),
            (TyData::Adt { def, args }, _) => {
                let fields = match (&self.cx.lookup.item(def)?.data, variant) {
                    (ItemData::Data(fs), None) => fs,
                    (ItemData::Enum { shared, variants }, Some(k)) if shared.is_empty() => {
                        &variants.get(k as usize)?.fields
                    }
                    _ => return None,
                };
                let f = fields.get(j as usize)?.ty;
                let argv = pool.list_items(args);
                let child = subst_owner(pool, def, argv, f);
                let range = match pool.get(f) {
                    TyData::Param(p) if p.owner == def && (p.index as usize) < argv.len() => {
                        let a = p.index as usize;
                        let start = argv[..a].iter().map(|&x| n(x)).sum();
                        Some((start, n(argv[a])))
                    }
                    _ => None,
                };
                Some((child, range))
            }
            _ => None,
        }
    }

    /// The slots `proj` reaches from a value `cur` of type `t`, as `want`
    /// slots. A dereference of a local reference reads what its origins'
    /// storage holds.
    fn project(&self, mut cur: Vec<Set>, t: Ty, proj: &[Proj], want: usize) -> Vec<Set> {
        let pool = self.pool;
        let mut ty = Some(t);
        let mut variant = None;
        for p in proj {
            let Some(t) = ty else { break };
            match *p {
                Proj::Variant(k) => variant = Some(k),
                Proj::Field(j) => match self.field(t, variant.take(), j) {
                    Some((child, Some((s, k))))
                        if cur.len() == tops(pool, t).len() && s + k <= cur.len() =>
                    {
                        cur = cur[s..s + k].to_vec();
                        ty = Some(child);
                    }
                    Some((child, _)) => {
                        cur = fit(&cur, width(pool, child));
                        ty = Some(child);
                    }
                    None => ty = None,
                },
                Proj::Deref => match local_ref(pool, t) {
                    Some((false, false)) => {
                        let inner = referent(pool, t);
                        let mut h = vec![Set::new(); width(pool, inner)];
                        for o in flat(&cur) {
                            union_into(&mut h, &self.held(o));
                        }
                        cur = h;
                        ty = Some(inner);
                    }
                    // A trait value through a reference is a view of the
                    // reference's data, with its origins.
                    _ => ty = None,
                },
                Proj::Elem(_) => ty = None,
            }
        }
        fit(&cur, want)
    }

    /// The run of local `l`'s slots that the field projections `proj`
    /// reach, when that is known.
    fn local_range(&self, l: u32, proj: &[Proj]) -> Option<(usize, usize)> {
        let mut t = self.b.local_ty[l as usize];
        let (mut start, mut len) = (0, tops(self.pool, t).len());
        let mut variant = None;
        for p in proj {
            match *p {
                Proj::Variant(k) => variant = Some(k),
                Proj::Field(j) => {
                    let (child, Some((s, k))) = self.field(t, variant.take(), j)? else {
                        return None;
                    };
                    if s + k > len {
                        return None;
                    }
                    (start, len, t) = (start + s, k, child);
                }
                _ => return None,
            }
        }
        (len > 0).then_some((start, len))
    }

    /// The storage the place `word` of instruction `i` lies in, and the
    /// origins of the value it holds as `want` slots
    /// (`ref.dep.origin.place`).
    fn place(&self, i: u32, word: u32, want: usize) -> (Set, Vec<Set>) {
        let empty = vec![Set::new(); want];
        let Some(p) = self.b.decode_place(word) else {
            return (vec![], empty);
        };
        match p.root {
            PlaceRoot::Local(l) => {
                let o = Origin::Local(l.raw());
                let t = self.b.local_ty[l.raw() as usize];
                let h = fit(&self.held(o), width(self.pool, t));
                (vec![o], self.project(h, t, &p.proj, want))
            }
            // An element's origin is the collection's root place
            // (`ref.dep.origin.element`). The checker has already read
            // that collection value, so follow the read back to its
            // place; a collection made as a value has a hidden local.
            PlaceRoot::Value(v) if matches!(p.proj.first(), Some(Proj::Elem(_))) => {
                let mut v = v.0;
                while v & hd_tir::ir::Ref::CONST_BIT == 0 && self.b.tags[v as usize] == Tag::Coerce
                {
                    v = self.b.data[v as usize][0];
                }
                if v & hd_tir::ir::Ref::CONST_BIT == 0 && self.b.tags[v as usize] == Tag::Read {
                    (self.place(v, self.b.data[v as usize][0], 0).0, empty)
                } else {
                    (vec![Origin::Hidden(i)], empty)
                }
            }
            PlaceRoot::Value(v) if p.proj.first() == Some(&Proj::Deref) => {
                let t = self.ty(v.0).unwrap_or(Ty::VOID);
                let kind = self.ty(v.0).and_then(|t| local_ref(self.pool, t));
                match kind {
                    // A trait value through a reference is a view of the
                    // reference's data, with its origins.
                    Some((false, true)) => {
                        let s = flat(self.of(v.0));
                        (s.clone(), vec![s; want])
                    }
                    // A reborrow: what the reference points into.
                    Some((false, false)) => {
                        let s = flat(self.of(v.0));
                        (s, self.project(self.of(v.0).to_vec(), t, &p.proj, want))
                    }
                    // Crossing a counted reference changes nothing: the
                    // place lies where the reference was read from.
                    _ => {
                        let s = match self.b.tags.get(v.0 as usize) {
                            Some(Tag::Read) => self.place(v.0, self.b.data[v.0 as usize][0], 0).0,
                            _ => vec![Origin::Hidden(v.0)],
                        };
                        let dyn_ = kind.is_some_and(|(_, d)| d);
                        (s.clone(), if dyn_ { vec![s; want] } else { empty })
                    }
                }
            }
            PlaceRoot::Value(v) => {
                let h = match self.ty(v.0) {
                    Some(t) => self.project(self.of(v.0).to_vec(), t, &p.proj, want),
                    None => empty,
                };
                (vec![Origin::Hidden(i)], h)
            }
            PlaceRoot::Global(d, _) => (vec![Origin::Global(d.raw())], empty),
            _ => (vec![], empty),
        }
    }

    /// The origins of the source argument `a`: a local-holding value's,
    /// or for a counted reference lent as a local one, the storage it was
    /// read from (`ref.dep.origin.lend`).
    fn lent(&self, a: u32) -> Set {
        let b = self.b;
        let mut v = a as usize;
        while b.tags[v] == Tag::Coerce && b.data[v][0] & hd_tir::ir::Ref::CONST_BIT == 0 {
            v = b.data[v][0] as usize;
        }
        match b.tags[v] {
            Tag::Read => self.place(w(v), b.data[v][0], 0).0,
            _ => vec![Origin::Hidden(w(v))],
        }
    }

    /// Every origin `set` reaches through the storage of its origins.
    fn reach(&self, set: Set) -> Set {
        let mut out = set.clone();
        let mut stack = set;
        while let Some(o) = stack.pop() {
            for h in self.held(o) {
                for x in h {
                    if union(&mut out, &[x]) {
                        stack.push(x);
                    }
                }
            }
        }
        out
    }

    /// Slot `s` of a value `cur` of type `at`, passed for a parameter of
    /// declared type `pt`: a top slot's set, or a slot behind a reference
    /// through what its origins' storage holds. A slot whose referent
    /// holds references adds what that holds, since the callee may return
    /// it (`ref.dep.call`).
    fn slot_in(&self, cur: &[Set], at: Ty, pt: Ty, s: usize, depth: u32) -> Set {
        let pool = self.pool;
        let (ta, tp) = (tops(pool, at), tops(pool, pt));
        if depth > 32 || ta.len() != tp.len() || cur.len() != tp.len() {
            return self.reach(flat(cur));
        }
        let Some(k) = tp.iter().rposition(|&(f, _)| f <= s) else {
            return self.reach(flat(cur));
        };
        let (f, rt) = tp[k];
        let inner = referent(pool, rt);
        if s == f {
            return if pool.ref_slots(inner) > 0 {
                self.reach(cur[k].clone())
            } else {
                cur[k].clone()
            };
        }
        let inner_a = referent(pool, ta[k].1);
        let mut h = vec![Set::new(); width(pool, inner_a)];
        for &o in &cur[k] {
            union_into(&mut h, &self.held(o));
        }
        self.slot_in(&h, inner_a, inner, s - f - 1, depth + 1)
    }

    /// The origins call `i` passes in the slots of lifetime `id`.
    fn lifetime_args(&self, i: usize, sig: &DepSig, id: u32) -> Set {
        let b = self.b;
        let mut out = Set::new();
        for (p, s) in sig.inputs(id) {
            let Some(a) = source_arg(b, i, p) else {
                continue;
            };
            let Some(at) = self.ty(a) else { continue };
            let set = if b.tags[a as usize] == Tag::Ref || may_hold(self.pool, at) {
                self.slot_in(self.of(a), at, sig.params[p].0, s, 0)
            } else {
                self.lent(a)
            };
            union(&mut out, &set);
        }
        out
    }

    /// A dependent call's result, `n` slots: each slot the arguments'
    /// origins in the slots of its lifetime (`ref.dep.call`).
    fn call(&self, i: usize, sig: &DepSig, n: usize) -> Vec<Set> {
        let v: Vec<Set> = sig
            .result_ids(self.pool, self.b.ty[i])
            .iter()
            .map(|ids| {
                let mut s = Set::new();
                for &id in ids {
                    union(&mut s, &self.lifetime_args(i, sig, id));
                }
                s
            })
            .collect();
        fit(&v, n)
    }

    /// A tuple, struct or variant construction: each part in the slots
    /// of its field (`ref.dep.origin.parts`).
    fn construct(&self, i: usize, n: usize) -> Vec<Set> {
        let b = self.b;
        let t = b.ty[i];
        let mut out = vec![Set::new(); n];
        let variant = (b.tags[i] == Tag::NewVariant).then_some(b.data[i][0]);
        if b.tags[i] == Tag::NewTuple && b.data[i][0] != NONE {
            union_into(&mut out, self.of(b.data[i][0]));
        }
        let lined = tops(self.pool, t).len() == n;
        for (j, &v) in b.record(b.data[i][1]).iter().enumerate() {
            match self.field(t, variant, w(j)) {
                Some((_, Some((s, k)))) if lined && s + k <= n => {
                    union_into(&mut out[s..s + k], self.of(v));
                }
                _ => {
                    union_into(&mut out, self.of(v));
                }
            }
        }
        out
    }

    fn step(&mut self, i: usize) {
        let b = self.b;
        let tag = b.tags[i];
        if tag == Tag::Store {
            let [word, v] = b.data[i];
            let value = self.of(v).to_vec();
            if value.iter().all(Vec::is_empty) {
                return;
            }
            let p = b.decode_place(word);
            let through = p
                .as_ref()
                .is_some_and(|p| p.proj.first() == Some(&Proj::Deref));
            let range = match &p {
                Some(p) if !through => match p.root {
                    PlaceRoot::Local(l) => self.local_range(l.raw(), &p.proj),
                    _ => None,
                },
                _ => None,
            };
            let (storage, _) = self.place(w(i), word, 0);
            for s in storage {
                match s {
                    Origin::Param(..) if through => {
                        let t = b.ty.get(v as usize).copied().unwrap_or(Ty::VOID);
                        if holds_local_ref(self.pool, t) {
                            self.out.insert(w(i));
                        }
                    }
                    Origin::Local(_) | Origin::Hidden(_) => self.hold(s, &value, range),
                    _ => {}
                }
            }
            return;
        }
        if tag == Tag::Break {
            let [label, v] = b.data[i];
            let value = self.of(v).to_vec();
            if let Some(&t) = b.label_inst.get(label as usize)
                && union_into(&mut self.val[t as usize], &value)
            {
                self.changed = true;
            }
            return;
        }
        let n = self.val[i].len();
        let new = match tag {
            Tag::Ref => {
                let inner = width(self.pool, referent(self.pool, b.ty[i]));
                let (s, v) = self.place(w(i), b.data[i][0], inner);
                if s == [Origin::Hidden(w(i))] {
                    self.hold(s[0], &v, None);
                }
                vec![s]
            }
            // Only a local-holding value has origins.
            _ if n == 0 => return,
            Tag::Read => self.place(w(i), b.data[i][0], n).1,
            Tag::Call | Tag::Await | Tag::CallDyn => match self.deps.get(&w(i)) {
                Some(sig) => self.call(i, sig, n),
                None => return,
            },
            Tag::Coerce => self.of(b.data[i][0]).to_vec(),
            Tag::Prim | Tag::Intrinsic | Tag::CallValue | Tag::CallHost | Tag::Return => return,
            Tag::NewTuple | Tag::NewStruct | Tag::NewVariant => self.construct(i, n),
            _ => {
                let mut s = vec![Set::new(); n];
                slots(b, i, |_, w, word| {
                    if matches!(word, Word::Block | Word::Value(Role::Tail | Role::Consume)) {
                        union_into(&mut s, self.of(w));
                    }
                });
                s
            }
        };
        if union_into(&mut self.val[i], &new) {
            self.changed = true;
        }
    }
}

/// The value call `i` passes for parameter `k`, a trait value's receiver
/// included.
fn source_arg(b: &Body, i: usize, k: usize) -> Option<u32> {
    let k = match (b.tags[i], k) {
        (Tag::CallDyn, 0) => return Some(b.data[i][0]),
        (Tag::CallDyn, k) => k - 1,
        (_, k) => k,
    };
    let mut found = None;
    let want = Word::Value(Role::Arg(w(k)));
    slots(b, i, |_, v, word| {
        if word == want {
            found = Some(v);
        }
    });
    found
}

impl Ck<'_, '_> {
    /// The dependency map of a callee's signature (`ref.dep.call`,
    /// `ref.dep.trait`): a trait method's from the trait's signature.
    fn callee_dep(&self, def: DefId) -> Option<DepSig> {
        let sig = self.sig_of(def).ok()?;
        let pool = self.pool();
        if !holds_local_ref(pool, sig.ret) {
            return None;
        }
        let names = &self.cx.names;
        let dep = DepSig::new(
            pool,
            &sig.params,
            |s| names.text(s) == "self",
            sig.ret,
            &sig.lifetimes,
        );
        dep.depends().then_some(dep)
    }

    /// The dependent results of the finished body, its result `ret`
    /// (module docs). `fun` is the function's declaration.
    pub(crate) fn check_dependent_results(
        &mut self,
        root: hd_tir::ir::Ref,
        ret: Ty,
        fun: Option<NodeRef<'_>>,
    ) {
        let pool = self.pool();
        let body = self.b.body();
        // The body's own map: parameter k is local k.
        let fn_body = body.kind == BodyKind::Fn && fun.is_some() && holds_local_ref(pool, ret);
        let own = match fn_body.then(|| self.sig_of(body.item)) {
            Some(Ok(sig)) => {
                let names = &self.cx.names;
                Some(DepSig::new(
                    pool,
                    &sig.params,
                    |s| names.text(s) == "self",
                    ret,
                    &sig.lifetimes,
                ))
            }
            _ => None,
        };
        let mut deps: HashMap<u32, DepSig> = HashMap::default();
        let mut through = false;
        for i in 0..body.tags.len() {
            match body.tags[i] {
                // A block's or a loop's value may leave a local's scope.
                Tag::Block | Tag::Loop if w(i) != root.0 && holds_local_ref(pool, body.ty[i]) => {
                    through = true;
                }
                Tag::Call | Tag::Await | Tag::CallDyn if holds_local_ref(pool, body.ty[i]) => {
                    let def = match body.tags[i] {
                        Tag::CallDyn => body.record(body.data[i][1]).first().copied(),
                        _ => match Callee::from_words(body.record(body.data[i][0])) {
                            Some(Callee::Item { def, .. }) => Some(def.raw()),
                            Some(Callee::TraitMethod { method, .. }) => Some(method.raw()),
                            None => None,
                        },
                    };
                    if let Some(dep) = def.and_then(|d| self.callee_dep(DefId::from_raw(d))) {
                        deps.insert(w(i), dep);
                    }
                }
                Tag::Store => {
                    let [_, v] = body.data[i];
                    through |= v & hd_tir::ir::Ref::CONST_BIT == 0
                        && body
                            .ty
                            .get(v as usize)
                            .is_some_and(|&t| holds_local_ref(pool, t));
                }
                _ => {}
            }
        }
        let elided = own.as_ref().map(|o| o.elided);
        if let (Some(Elided::Missing | Elided::Ambiguous), Some(f)) = (elided, fun)
            && self.result_omitted
        {
            self.source_error(elided == Some(Elided::Missing), f);
        }
        let own = own.filter(DepSig::depends);
        if own.is_none() && !through {
            return;
        }
        let body = self.b.body();
        let params: Vec<Ty> = (0..body.local_ty.len())
            .map(|l| match &own {
                Some(o) if l < o.params.len() => o.params[l].0,
                _ => body.local_ty[l],
            })
            .collect();
        let val = (0..body.tags.len())
            .map(|i| {
                let n = if body.tags[i] == Tag::Ref {
                    1
                } else {
                    width(pool, body.ty[i])
                };
                vec![Set::new(); n]
            })
            .collect();
        let mut s = Solve {
            b: body,
            pool,
            cx: self.cx,
            val,
            held: HashMap::default(),
            deps: &deps,
            params,
            out: HashSet::default(),
            changed: true,
        };
        for l in 0..body.local_ty.len() {
            let lt = body.local_ty[l];
            if body.local_flags[l] & hd_tir::ir::local_flags::PARAM != 0
                && holds_local_ref(pool, lt)
            {
                let pt = s.params[l];
                let tp = tops(pool, pt);
                let mine: Vec<Set> = tp
                    .iter()
                    .map(|&(f, _)| vec![Origin::Param(w(l), w(f))])
                    .collect();
                let mine = if mine.is_empty() {
                    vec![vec![Origin::Param(w(l), 0)]]
                } else {
                    mine
                };
                s.held
                    .insert(Origin::Local(w(l)), fit(&mine, width(pool, lt)));
            }
        }
        while s.changed {
            s.changed = false;
            for i in 0..body.tags.len() {
                s.step(i);
            }
        }
        let mut stores: Vec<NodeIdx> = s.out.iter().map(|&i| body.syn[i as usize]).collect();
        stores.sort_stable_by_key(|n| n.raw());
        let mut bad: Vec<(NodeIdx, Origin, u32)> = Vec::new();
        if let Some(own) = &own {
            let ids = own.result_ids(pool, ret);
            for r in returned(body, root.0) {
                let v = fit(s.of(r), ids.len());
                for (set, ids) in v.iter().zip(&ids) {
                    let Some(&id) = ids.first() else { continue };
                    if let Some(&o) = set
                        .iter()
                        .find(|&&o| !ids.iter().any(|&id| own.allows(pool, o, id)))
                    {
                        bad.push((site(&s, r, o, 0), o, id));
                        break;
                    }
                }
            }
        }
        // A returned value's origins are `ref.dep.return`'s.
        let mut skip: HashSet<u32> = HashSet::default();
        if own.is_some() {
            skip.extend(returned(body, root.0));
        }
        skip.insert(root.0);
        let outlives = scoped_uses(&s, &skip, self.cx);
        for at in stores {
            if at == NodeIdx::NONE {
                continue;
            }
            let node = self.cx.src.parse.tree.node(at);
            self.err(
                Code::LocalReferenceEscapes,
                node,
                "a local reference never escapes to a place the caller owns; store it only in a place of this function",
            );
        }
        let mut seen = HashSet::default();
        if let Some(own) = &own {
            for (at, o, id) in bad {
                if at == NodeIdx::NONE || !seen.insert(at) {
                    continue;
                }
                let msg = self.origin_message(own, o, id, fun);
                let node = self.cx.src.parse.tree.node(at);
                self.err(Code::LocalReferenceEscapes, node, &msg);
            }
        }
        for (at, msg) in outlives {
            let node = self.cx.src.parse.tree.node(at);
            self.err(Code::LocalReferenceOutlivesLocal, node, &msg);
        }
    }

    /// `reference-result-without-source` or `ambiguous-reference-source`
    /// for an omitted result type that holds a local reference.
    fn source_error(&mut self, missing: bool, f: NodeRef<'_>) {
        if missing {
            self.err(
                Code::ReferenceResultWithoutSource,
                f,
                "a reference result needs a reference parameter to borrow from",
            );
        } else {
            self.err(
                Code::AmbiguousReferenceSource,
                f,
                "the result may borrow from more than one reference parameter; write the result type with a lifetime",
            );
        }
    }

    /// The lifetimes in scope at the function `fun`, an enclosing
    /// implementation's first, with their `'`.
    fn lifetime_names(&self, fun: NodeRef<'_>) -> Vec<String> {
        let src = &self.cx.src;
        let x = fun.index().raw();
        let mut owners: Vec<NodeRef<'_>> = src
            .parse
            .tree
            .root()
            .children()
            .filter(|c| {
                let (lo, hi) = range(*c);
                c.kind() == SyntaxKind::ImplDecl && lo < x && x < hi
            })
            .collect();
        owners.push(fun);
        owners
            .iter()
            .filter_map(|o| o.child(SyntaxKind::GenericParameterList))
            .flat_map(NodeRef::children)
            .filter(|c| c.kind() == SyntaxKind::LifetimeParameter)
            .filter_map(|p| p.direct_token(&src.parse.tokens, TokenKind::Lifetime))
            .map(|t| src.text(t).to_owned())
            .collect()
    }

    /// `ref.dep.return.message`: the root the result slot of lifetime `id`
    /// must not depend on.
    fn origin_message(&self, own: &DepSig, o: Origin, id: u32, fun: Option<NodeRef<'_>>) -> String {
        let body = self.b.body();
        let name = |l: u32| self.cx.names.text(body.local_name[l as usize]).to_owned();
        let lifetimes = fun.map(|f| self.lifetime_names(f)).unwrap_or_default();
        let lifetime = |l: u32| {
            (l < ANON)
                .then(|| lifetimes.get(l as usize - 1).cloned())
                .flatten()
        };
        let f = fun
            .and_then(|f| f.name(&self.cx.src.parse.tokens))
            .map_or_else(
                || "this function".to_owned(),
                |t| format!("`{}`", self.cx.src.text(t)),
            );
        let carries = |p: u32| {
            own.params
                .get(p as usize)
                .is_some_and(|(_, ids)| ids.contains(&id))
        };
        let what = match o {
            Origin::Local(l)
                if body.local_flags[l as usize] & hd_tir::ir::local_flags::PARAM != 0 =>
            {
                format!(
                    "`{}` is a parameter, whose own storage the call owns",
                    name(l)
                )
            }
            Origin::Local(l) if name(l).starts_with('$') => {
                format!("the value refers to a temporary of {f}")
            }
            Origin::Local(l) => format!("`{}` is a local of {f}", name(l)),
            Origin::Param(p, s) => {
                let other = own
                    .params
                    .get(p as usize)
                    .and_then(|(_, ids)| ids.get(s as usize))
                    .and_then(|&l| lifetime(l));
                match other {
                    Some(l) => format!("the value borrows from `{}` with lifetime `{l}`", name(p)),
                    None if carries(p) => {
                        format!("the value borrows from another reference in `{}`", name(p))
                    }
                    None => format!("`{}` is another parameter", name(p)),
                }
            }
            Origin::Hidden(_) => format!("the value refers to a temporary of {f}"),
            Origin::Global(_) => "the value refers to a top-level binding".to_owned(),
        };
        let mut sources: Vec<String> = (0..own.params.len())
            .filter(|&p| carries(w(p)))
            .map(|p| format!("`{}`", name(w(p))))
            .collect();
        sources.dedup();
        let from = match sources.as_slice() {
            [] => "nothing".to_owned(),
            [one] => one.clone(),
            [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        };
        match lifetime(id) {
            Some(l) => format!("{what}; a `{l}` reference of the result depends only on {from}"),
            None => format!("{what}; the result depends only on {from}"),
        }
    }
}

/// The values the body returns: its root block's value and each `Return`
/// operand outside its closures.
fn returned(b: &Body, root: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut seen = HashSet::default();
    let mut stack = vec![root];
    while let Some(i) = stack.pop() {
        if i == NONE || i as usize >= b.tags.len() || !seen.insert(i) {
            continue;
        }
        match b.tags[i as usize] {
            Tag::Return => out.push(b.data[i as usize][0]),
            Tag::Closure => continue,
            _ => {}
        }
        if i == root && b.tags[i as usize] == Tag::Block {
            out.push(b.data[i as usize][1]);
        }
        slots(b, i as usize, |_, w, word| {
            if matches!(word, Word::Block | Word::Inst) {
                stack.push(w);
            }
        });
    }
    out.retain(|&v| v != NONE);
    out
}

/// The innermost part of value `v` whose origins hold `o`, for the
/// diagnostic's position: an arm, a block's value, a construction's part.
fn site(s: &Solve<'_>, v: u32, o: Origin, depth: u32) -> NodeIdx {
    let b = s.b;
    let here = b.syn.get(v as usize).copied().unwrap_or(NodeIdx::NONE);
    if depth > 64 || v & hd_tir::ir::Ref::CONST_BIT != 0 {
        return here;
    }
    let mut inner = None;
    if matches!(b.tags[v as usize], Tag::Coerce) {
        inner = Some(b.data[v as usize][0]);
    } else if !matches!(
        b.tags[v as usize],
        Tag::Read | Tag::Ref | Tag::Call | Tag::Await | Tag::CallDyn
    ) {
        slots(b, v as usize, |_, w, word| {
            if inner.is_none()
                && matches!(word, Word::Block | Word::Value(Role::Tail | Role::Consume))
                && s.of(w).iter().any(|x| x.contains(&o))
            {
                inner = Some(w);
            }
        });
    }
    match inner {
        Some(w) => {
            let at = site(s, w, o, depth + 1);
            if at == NodeIdx::NONE { here } else { at }
        }
        None => here,
    }
}

/// Where a local of the body is in scope (`ref.dep.scope`): the preorder
/// node range `[lo, hi)` that its uses lie in, and the node range of the
/// loop each of whose iterations makes it anew.
#[derive(Clone, Copy)]
struct Scope {
    lo: u32,
    hi: u32,
    lp: Option<(u32, u32)>,
}

/// The node that declares origin `o`, a binding's pattern or a compiler
/// local's or a temporary's expression, and whether it is a temporary or
/// a compiler local (`$cur`). `None` for what the caller owns and for a
/// parameter, which is in scope in the whole body.
fn decl_node(b: &Body, cx: &BodyCx<'_>, o: Origin) -> Option<(NodeIdx, bool)> {
    let (n, temp) = match o {
        Origin::Local(l) if b.local_flags[l as usize] & hd_tir::ir::local_flags::PARAM == 0 => (
            b.local_syn[l as usize],
            cx.names.text(b.local_name[l as usize]).starts_with('$'),
        ),
        Origin::Hidden(h) => (b.syn[h as usize], true),
        _ => return None,
    };
    (n != NodeIdx::NONE).then_some((n, temp))
}

/// The preorder range of node `n`: its index and one past its last
/// descendant.
fn range(n: NodeRef<'_>) -> (u32, u32) {
    (n.index().raw(), n.end().raw())
}

/// The first suite of node `p`: a loop's body.
fn first_block(p: NodeRef<'_>) -> Option<NodeRef<'_>> {
    p.children().find(|c| c.kind() == SyntaxKind::Block)
}

/// The scope of origin `o`, declared at node `n`. A `let`, `:=` or
/// pattern binding is in scope from the end of its statement or binding
/// to the end of its suite, a `match` arm's in its arm, a `for` pattern's
/// in the loop body, and a comprehension's in the comprehension. A
/// temporary or a compiler local is in scope from the start of its
/// statement to the end of its suite (`ref.dep.scope.temporary`).
fn scope_of(tree: &GreenTree, n: NodeIdx, temp: bool) -> Option<Scope> {
    let x = n.raw();
    let mut path = vec![tree.root()];
    while let Some(c) = path.last().and_then(|p| {
        p.children().find(|c| {
            let (lo, hi) = range(*c);
            lo <= x && x < hi
        })
    }) {
        path.push(c);
    }
    let mut lo = temp.then_some(x);
    let mut region = None;
    let mut lp = None;
    let mut looped = false;
    for k in (0..path.len()).rev() {
        let p = path[k];
        let (pl, ph) = range(p);
        let up = k.checked_sub(1).map(|j| path[j]);
        // The loop whose every iteration runs the declaration.
        if !looped {
            let per_iteration = |p: NodeRef<'_>| {
                let head = p.children().next().map(range);
                let body = first_block(p).map(range);
                [head, body].iter().flatten().any(|&(l, h)| l <= x && x < h)
            };
            match p.kind() {
                SyntaxKind::ForExpr | SyntaxKind::WhileExpr if per_iteration(p) => {
                    lp = Some((pl, ph));
                    looped = true;
                }
                SyntaxKind::ComprehensionExpr => {
                    lp = Some((pl, ph));
                    looped = true;
                }
                SyntaxKind::ClosureExpr | SyntaxKind::FnDecl => looped = true,
                _ => {}
            }
        }
        if region.is_some() {
            continue;
        }
        match p.kind() {
            SyntaxKind::LetStmt | SyntaxKind::BindingExpr if lo.is_none() => lo = Some(ph),
            SyntaxKind::MatchArm if lo.is_none() => region = Some((pl, ph)),
            SyntaxKind::ForExpr if lo.is_none() => region = first_block(p).map(range),
            SyntaxKind::ComprehensionFor if lo.is_none() => region = up.map(range),
            SyntaxKind::Block | SyntaxKind::ClosureExpr | SyntaxKind::FnDecl => {
                // A temporary's statement uses it from its start.
                let stmt = path.get(k + 1).map(|c| c.index().raw());
                let lo = lo?;
                region = Some((if temp { stmt.unwrap_or(lo) } else { lo }, ph));
            }
            _ => {}
        }
    }
    let (lo, hi) = region?;
    Some(Scope { lo, hi, lp })
}

/// `ref.dep.scope`: each use of a value with a local's origin lies in
/// that local's scope and, for a loop-body local, reads no binding
/// that outlives the iteration.
fn scoped_uses(s: &Solve<'_>, skip: &HashSet<u32>, cx: &BodyCx<'_>) -> Vec<(NodeIdx, String)> {
    let b = s.b;
    let tree = &cx.src.parse.tree;
    let mut scopes: HashMap<Origin, Option<Scope>> = HashMap::default();
    let mut bad: Vec<(NodeIdx, Origin, Option<Origin>, bool, u32)> = Vec::new();
    for i in 0..b.tags.len() {
        let at = b.syn[i];
        if at == NodeIdx::NONE || skip.contains(&w(i)) {
            continue;
        }
        // The value's origins, and those of each binding a place reads.
        let mut uses: Vec<(Option<Origin>, Origin)> =
            flat(&s.val[i]).into_iter().map(|o| (None, o)).collect();
        let deref = |word| {
            b.decode_place(word)
                .is_some_and(|p| p.proj.contains(&Proj::Deref))
        };
        let tag = b.tags[i];
        if matches!(tag, Tag::Read | Tag::Ref) || tag == Tag::Store && deref(b.data[i][0]) {
            for st in s.place(w(i), b.data[i][0], 0).0 {
                if matches!(st, Origin::Local(_) | Origin::Hidden(_)) {
                    uses.extend(flat(&s.held(st)).into_iter().map(|o| (Some(st), o)));
                }
            }
        }
        for (st, o) in uses {
            if st == Some(o) {
                continue;
            }
            let Some(sc) = *scopes
                .entry(o)
                .or_insert_with(|| decl_node(b, cx, o).and_then(|(n, t)| scope_of(tree, n, t)))
            else {
                continue;
            };
            let u = at.raw();
            let in_loop = |n: u32| sc.lp.is_some_and(|(lo, hi)| lo <= n && n < hi);
            let earlier = if sc.lo <= u && u < sc.hi {
                // In scope, but a binding declared outside the local's
                // loop may hold an earlier iteration's reference.
                let kept = st
                    .is_some_and(|st| decl_node(b, cx, st).is_none_or(|(n, _)| !in_loop(n.raw())));
                if !(sc.lp.is_some() && kept) {
                    continue;
                }
                true
            } else {
                in_loop(u) && u < sc.lo
            };
            bad.push((at, o, st, earlier, w(i)));
        }
    }
    bad.sort_stable_by_key(|x| x.0.raw());
    let mut seen = HashSet::default();
    bad.into_iter()
        .filter(|x| seen.insert(x.1))
        .map(|(at, o, st, earlier, i)| (at, scope_message(b, cx, o, st, earlier, i)))
        .collect()
}

/// `ref.dep.scope.message`: the binding or value that may point into
/// local `o` out of its scope, read at instruction `i` from `st`.
fn scope_message(
    b: &Body,
    cx: &BodyCx<'_>,
    o: Origin,
    st: Option<Origin>,
    earlier: bool,
    i: u32,
) -> String {
    let named = |l: u32| {
        let n = cx.names.text(b.local_name[l as usize]);
        (!n.starts_with('$')).then(|| format!("`{n}`"))
    };
    let holder = match st {
        Some(Origin::Local(l)) => named(l),
        _ => b.local_read(i as usize).and_then(|l| named(l.raw())),
    }
    .unwrap_or_else(|| "this value".to_owned());
    let target = match o {
        Origin::Local(l) => named(l),
        _ => None,
    }
    .unwrap_or_else(|| "a temporary".to_owned());
    if earlier {
        format!("{holder} may point into {target} from an earlier iteration of its loop")
    } else {
        format!("{holder} may point into {target}, whose block has ended")
    }
}
