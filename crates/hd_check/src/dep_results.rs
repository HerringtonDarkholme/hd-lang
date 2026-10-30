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
//! A value's slots are its reference slots and its function types'
//! capture slots at the top, outside another reference ([`tops`]); what a
//! reference's referent holds comes from its origins' storage. Where a
//! projection or a construction cannot be lined up with the slots, each
//! slot takes the union of all of them, which only adds origins.

use hd_base::SliceSort as _;
use hd_base::{DefId, HashMap, HashSet, NodeIdx, Symbol};
use hd_diag::Code;
use hd_resolve::ItemData;
use hd_syntax::{GreenTree, NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{
    Body, BodyKind, Callee, IntrinsicOp, NONE, PlaceRoot, Proj, Role, Tag, Word, slots,
};
use hd_types::{Ty, TyData, Types};

use crate::body::{BodyCx, Ck};
use crate::call::subst_owner;
use crate::local_refs::holds_local_ref;

/// The lifetime of an input slot that writes none: its own, `ANON` plus
/// the slot's index. A written lifetime is its 1-based place in scope.
const ANON: u32 = 1 << 16;

/// The lifetime of slot `k` of what type parameter `p` stands for at a
/// call: `TVAR + (p << 8) + k` (`ref.generic.call`), shared by every
/// position of that parameter.
const TVAR: u32 = 1 << 20;

/// The end of the [`TVAR`] range.
const TVAR_END: u32 = TVAR + (1 << 16);

/// A result slot that depends on every argument (`ref.generic.call.unaligned`).
const ANY: u32 = u32::MAX;

/// A capture slot that depends on what every argument's capture slots
/// hold (`ref.generic.call.unaligned`).
const ANY_FN: u32 = u32::MAX - 2;

/// A declared result slot with no source, while lining up.
const NO_SOURCE: u32 = u32::MAX - 1;

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
/// origin slot of its parameters and of its result, in [`slots_of`] order
/// over the declared types.
pub(crate) struct DepSig {
    /// Each parameter's declared type and its slots' lifetimes.
    params: Vec<(Ty, Vec<u32>)>,
    /// Each result slot's lifetime; `None` when it has no source.
    outs: Vec<Option<u32>>,
    ret: Ty,
    /// Elision for ordinary local-reference result slots.
    elided: Elided,
    /// Elision for a direct function result's hidden capture slot.
    capture_elided: Elided,
}

impl DepSig {
    /// The map of a signature: `written` is `FnSig::lifetimes`, one entry
    /// per reference slot of `params` and then of `ret`, 0 where none is
    /// written.
    fn new(
        pool: Types<'_>,
        params: &[(Symbol, Ty)],
        is_self: impl Fn(Symbol) -> bool,
        ret: Ty,
        written: &[u16],
    ) -> DepSig {
        let ref_ins: usize = params.iter().map(|&(_, t)| pool.ref_slots(t)).sum();
        let ref_outs = pool.ref_slots(ret);
        let trust_in = written.len() >= ref_ins;
        let trust_out = written.len() == ref_ins + ref_outs;
        let mut written_at = 0;
        let mut dep_at = 0;
        let ps: Vec<(Ty, Vec<u32>)> = params
            .iter()
            .map(|&(_, t)| {
                let ids = slot_ids(pool, t, &mut written_at, &mut dep_at, |k| {
                    written
                        .get(k)
                        .copied()
                        .filter(|&l| trust_in && l != 0)
                        .map(u32::from)
                });
                (t, ids)
            })
            .collect();
        let receiver = params
            .first()
            .is_some_and(|&(s, t)| is_self(s) && local_ref(pool, t).is_some_and(|(c, _)| !c));
        let elided = match ps.first() {
            Some((t, ids)) if receiver && !ref_ids_of(pool, *t, ids).is_empty() => {
                Elided::Lifetime(ref_ids_of(pool, *t, ids)[0])
            }
            _ => one_id(
                params
                    .iter()
                    .zip(&ps)
                    .filter(|((s, _), _)| !is_self(*s))
                    .flat_map(|(_, (t, ids))| ref_ids_of(pool, *t, ids)),
            ),
        };
        let capture_elided = one_id(
            ps.iter()
                .filter(|(t, _)| capture_slot(pool, *t))
                .filter_map(|(_, ids)| ids.first().copied()),
        );
        let mut r = 0;
        let outs = slots_of(pool, ret)
            .iter()
            .map(|&(st, _)| {
                if is_fn(pool, st) {
                    return match capture_elided {
                        Elided::Lifetime(id) => Some(id),
                        Elided::Missing | Elided::Ambiguous => None,
                    };
                }
                let j = r;
                r += 1;
                match written.get(ref_ins + j) {
                    Some(&l) if trust_out && l != 0 => Some(u32::from(l)),
                    _ => match elided {
                        Elided::Lifetime(id) => Some(id),
                        // An opaque type with no one lifetime to take borrows
                        // from every parameter (`types.opaque.borrows.source`).
                        _ if matches!(pool.get(st), TyData::Opaque { .. }) => Some(ANY),
                        _ => None,
                    },
                }
            })
            .collect();
        DepSig {
            params: ps,
            outs,
            ret,
            elided,
            capture_elided,
        }
    }

    /// The map a function type carries (`fn.type.lifetime`), for a call
    /// through a function value.
    fn of_fn(pool: Types<'_>, t: Ty) -> Option<DepSig> {
        let TyData::Fn { params, result, .. } = pool.get(referent_fn(pool, t)) else {
            return None;
        };
        let deps = pool.fn_ty_deps(t);
        let ins = deps.as_ref().map_or(&[][..], |(ins, _)| *ins);
        let written_outs = deps.as_ref().map_or(&[][..], |(_, outs)| *outs);
        let mut ref_at = 0;
        let mut dep_at = 0;
        let params = pool
            .list_items(params)
            .iter()
            .map(|&p| {
                let ids = slot_ids(pool, p, &mut ref_at, &mut dep_at, |k| ins.get(k).copied());
                (p, ids)
            })
            .collect::<Vec<_>>();
        let elided = one_id(params.iter().flat_map(|(p, ids)| ref_ids_of(pool, *p, ids)));
        let capture_elided = one_id(
            params
                .iter()
                .filter(|(p, _)| capture_slot(pool, *p))
                .filter_map(|(_, ids)| ids.first().copied()),
        );
        // A type that writes no map has no reference result slots.
        let mut r = 0;
        let outs = slots_of(pool, result)
            .iter()
            .filter_map(|&(st, _)| {
                if is_fn(pool, st) {
                    return Some(match capture_elided {
                        Elided::Lifetime(id) => Some(id),
                        Elided::Missing | Elided::Ambiguous => None,
                    });
                }
                let l = *written_outs.get(r)?;
                r += 1;
                Some(match l {
                    // `types.opaque.borrows.source`
                    0 if matches!(pool.get(st), TyData::Opaque { .. }) => Some(ANY),
                    0 => None,
                    l => Some(l),
                })
            })
            .collect();
        Some(DepSig {
            params,
            outs,
            ret: result,
            elided,
            capture_elided,
        })
    }

    /// The map as a function type carries it: the slots of the parameters
    /// from `skip` on, then the result's (`fn.type.lifetime.value`).
    /// `NONE` when a result slot depends on none of those, as on a bound
    /// receiver (`ref.local.escape.fn-value-result`).
    fn to_fn(&self, pool: Types<'_>, skip: usize) -> hd_types::FnDeps {
        let ins: Vec<u32> = self.params[skip.min(self.params.len())..]
            .iter()
            .flat_map(|(t, ids)| ref_ids_of(pool, *t, ids))
            .collect();
        let outs: Option<Vec<u32>> = ref_ids_of(pool, self.ret, &self.outs)
            .into_iter()
            .map(|l| l.filter(|l| ins.contains(l)))
            .collect();
        outs.map_or(hd_types::FnDeps::NONE, |outs| pool.fn_deps(&ins, &outs))
    }

    /// The map of a body whose declared result has opaque types, over
    /// its hidden result `hidden`: each slot of a hidden type takes its
    /// opaque type's lifetime, and none for an opaque type that does not
    /// borrow (`types.opaque.borrows.body`).
    fn open(self, pool: Types<'_>, hidden: Ty) -> DepSig {
        let mut it = self.outs.iter().map(|o| o.unwrap_or(NO_SOURCE));
        let mut out = Vec::new();
        let outs = if line_up(
            pool,
            self.ret,
            hidden,
            &mut it,
            &mut Vec::new(),
            &mut out,
            0,
        ) {
            out.into_iter()
                .map(|id| (id != NO_SOURCE).then_some(id))
                .collect()
        } else {
            vec![None; dep_slots(pool, hidden)]
        };
        DepSig {
            params: self.params,
            outs,
            ret: hidden,
            elided: self.elided,
            capture_elided: self.capture_elided,
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
        if let Elided::Lifetime(id) = self.capture_elided {
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

    /// The map at call `i` of `b`, over the types the call instantiates
    /// (`ref.generic.call`): each argument's type and the result's. A slot
    /// that a type parameter adds shares that parameter's lifetime. An
    /// argument whose type does not line up with its parameter's keeps the
    /// declared type, and then a result slot a type parameter adds depends
    /// on every argument (`ref.generic.call.unaligned`). With the callee
    /// and its generics, a parameter of a bare type parameter bounded by a
    /// lifetime puts its slots in that lifetime (`ref.generic.outlives.argument`).
    fn instantiate(
        self,
        pool: Types<'_>,
        b: &Body,
        i: usize,
        callee: Option<(DefId, &[hd_resolve::Generic])>,
    ) -> DepSig {
        let mut keys = Vec::new();
        let mut lossy = false;
        let mut params = Vec::with_capacity(self.params.len());
        for (p, (decl, ids)) in self.params.into_iter().enumerate() {
            let at = source_arg(b, i, p)
                .filter(|&a| a & hd_tir::ir::Ref::CONST_BIT == 0)
                .and_then(|a| b.ty.get(a as usize).copied());
            let mut out = Vec::new();
            let mut it = ids.iter().copied();
            match at {
                Some(t) if line_up(pool, decl, t, &mut it, &mut keys, &mut out, 0) => {
                    let outlives = callee.and_then(|(def, generics)| {
                        crate::local_refs::outlives_field(
                            pool,
                            def,
                            generics,
                            referent_fn(pool, decl),
                        )
                    });
                    if let Some(l) = outlives {
                        for id in &mut out {
                            if (TVAR..TVAR_END).contains(id) {
                                *id = u32::from(l);
                            }
                        }
                    }
                    params.push((t, out));
                }
                _ => {
                    lossy |= pool.has_param(decl);
                    params.push((decl, ids));
                }
            }
        }
        let ret = b.ty[i];
        let mut it = self.outs.iter().map(|o| o.unwrap_or(NO_SOURCE));
        let mut out = Vec::new();
        // A result slot that a type parameter adds with no argument slot of
        // its own, as the capture of `F < Fn() -> void & 'a` inside
        // `Button[F, 'a]`, depends on every argument.
        let unbound = |id: u32| {
            !params
                .iter()
                .any(|(_, ids): &(Ty, Vec<u32>)| ids.contains(&id))
        };
        // A capture slot depends only on capture slots.
        let any = |st: Ty| if is_fn(pool, st) { ANY_FN } else { ANY };
        let slots = slots_of(pool, ret);
        let outs = if line_up(pool, self.ret, ret, &mut it, &mut keys, &mut out, 0) {
            out.into_iter()
                .zip(&slots)
                .map(|(id, &(st, _))| match id {
                    NO_SOURCE => None,
                    id if (lossy || unbound(id)) && (TVAR..TVAR_END).contains(&id) => Some(any(st)),
                    id => Some(id),
                })
                .collect()
        } else {
            slots.iter().map(|&(st, _)| Some(any(st))).collect()
        };
        DepSig {
            params,
            outs,
            ret,
            elided: self.elided,
            capture_elided: self.capture_elided,
        }
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
        if id == ANY || id == ANY_FN {
            return true;
        }
        let s = s as usize;
        if ids.get(s) == Some(&id) {
            return true;
        }
        let all = slots_of(pool, *t);
        (0..s.min(all.len())).any(|s0| {
            ids.get(s0) == Some(&id) && s <= s0 + dep_slots(pool, referent(pool, all[s0].0))
        })
    }
}

/// The lifetimes of the slots of a declared type `t`
/// ([`slots_of`]): a reference slot's from `written` by its place among
/// the reference slots, counted by `ref_at`, or its own `ANON` one by its
/// place among all slots, counted by `dep_at`; a capture slot's its own.
fn slot_ids(
    pool: Types<'_>,
    t: Ty,
    ref_at: &mut usize,
    dep_at: &mut usize,
    written: impl Fn(usize) -> Option<u32>,
) -> Vec<u32> {
    let ids = slots_of(pool, t)
        .iter()
        .enumerate()
        .map(|(k, &(st, _))| {
            let own = ANON + w(*dep_at + k);
            if is_fn(pool, st) {
                return own;
            }
            *ref_at += 1;
            written(*ref_at - 1).unwrap_or(own)
        })
        .collect::<Vec<_>>();
    *dep_at += ids.len();
    ids
}

/// The entries of `ids`, one per slot of `t`, at its reference slots.
fn ref_ids_of<T: Copy>(pool: Types<'_>, t: Ty, ids: &[T]) -> Vec<T> {
    slots_of(pool, t)
        .iter()
        .zip(ids)
        .filter(|((st, _), _)| !is_fn(pool, *st))
        .map(|(_, &id)| id)
        .collect()
}

/// The one lifetime of `ids` (`ref.dep.elide`).
fn one_id(ids: impl Iterator<Item = u32>) -> Elided {
    let mut ids: Vec<u32> = ids.collect();
    ids.sort_stable();
    ids.dedup();
    match ids.as_slice() {
        [one] => Elided::Lifetime(*one),
        [] => Elided::Missing,
        _ => Elided::Ambiguous,
    }
}

/// Lines the instantiated type `t` up with the declared `d`, pushing each
/// slot's lifetime to `out` in [`slots_of`] order: a declared slot's from
/// `ids`, and a slot a type parameter adds its [`TVAR`], a function
/// type's capture slot included. False where the shapes differ.
fn line_up(
    pool: Types<'_>,
    d: Ty,
    t: Ty,
    ids: &mut dyn Iterator<Item = u32>,
    keys: &mut Vec<hd_types::ParamRef>,
    out: &mut Vec<u32>,
    depth: u32,
) -> bool {
    if depth > 32 {
        return false;
    }
    let d1 = depth + 1;
    let list = |a: hd_types::TyList,
                b: hd_types::TyList,
                ids: &mut dyn Iterator<Item = u32>,
                keys: &mut Vec<hd_types::ParamRef>,
                out: &mut Vec<u32>| {
        let (a, b) = (pool.list_items(a), pool.list_items(b));
        a.len() == b.len()
            && a.iter()
                .zip(b)
                .all(|(&x, &y)| line_up(pool, x, y, ids, keys, out, d1))
    };
    match (pool.get(d), pool.get(t)) {
        (TyData::Param(p), _) => {
            let k = keys.iter().position(|x| *x == p).unwrap_or_else(|| {
                keys.push(p);
                keys.len() - 1
            });
            let n = dep_slots(pool, t).min(255);
            out.extend((0..n).map(|s| TVAR + (w(k.min(255)) << 8) + w(s)));
            true
        }
        // A function value's one capture slot (`fn.capture.local-parameter`).
        (TyData::Fn { .. }, TyData::Fn { .. }) => {
            let Some(id) = ids.next() else { return false };
            out.push(id);
            true
        }
        // Each slot of an opaque type's hidden type, or of its instance,
        // takes the opaque type's lifetime (`types.opaque.borrows`).
        (TyData::Opaque { borrows, .. }, _) => {
            let id = if borrows {
                let Some(id) = ids.next() else { return false };
                id
            } else {
                NO_SOURCE
            };
            out.extend(std::iter::repeat_n(id, dep_slots(pool, t)));
            true
        }
        (TyData::Mut(a), TyData::Mut(b)) => line_up(pool, a, b, ids, keys, out, d1),
        (TyData::Mut(a), _) => line_up(pool, a, t, ids, keys, out, d1),
        (_, TyData::Mut(b)) => line_up(pool, d, b, ids, keys, out, d1),
        (
            TyData::Ref {
                counted: false,
                inner: a,
                ..
            },
            TyData::Ref {
                counted: false,
                inner: b,
                ..
            },
        ) => {
            let Some(id) = ids.next() else { return false };
            out.push(id);
            line_up(pool, a, b, ids, keys, out, d1)
        }
        (TyData::Ref { counted: true, .. }, TyData::Ref { counted: true, .. }) => true,
        (TyData::Option(a), TyData::Option(b)) => line_up(pool, a, b, ids, keys, out, d1),
        (TyData::Tuple { elems: a, rest: ra }, TyData::Tuple { elems: b, rest: rb }) => {
            list(a, b, ids, keys, out)
                && match (ra, rb) {
                    (Some(x), Some(y)) => line_up(pool, x, y, ids, keys, out, d1),
                    (None, None) => true,
                    _ => false,
                }
        }
        (TyData::Adt { def: x, args: a }, TyData::Adt { def: y, args: b })
        | (
            TyData::TraitValue {
                def: x, args: a, ..
            },
            TyData::TraitValue {
                def: y, args: b, ..
            },
        ) if x == y => list(a, b, ids, keys, out),
        _ => dep_slots(pool, d) == 0 && dep_slots(pool, t) == 0,
    }
}

/// `t` without its `mut`.
fn referent_fn(pool: Types<'_>, t: Ty) -> Ty {
    match pool.get(t) {
        TyData::Mut(i) => referent_fn(pool, i),
        _ => t,
    }
}

/// Whether `t` is a direct function value, under `mut`. Such a value has
/// one checker-only slot for the origins of local references captured by a
/// scoped closure parameter.
fn capture_slot(pool: Types<'_>, t: Ty) -> bool {
    matches!(pool.get(referent_fn(pool, t)), TyData::Fn { .. })
}

/// Whether slot type `t` is a function type's capture slot.
fn is_fn(pool: Types<'_>, t: Ty) -> bool {
    matches!(pool.get(t), TyData::Fn { .. })
}

/// The origin slots of `t` ([`slots_of`]): its local-reference slots and
/// a capture slot for each function type in it.
fn dep_slots(pool: Types<'_>, t: Ty) -> usize {
    if pool.has_fn(t) {
        slots_of(pool, t).len()
    } else {
        pool.ref_slots(t)
    }
}

/// Whether `t` has a function type's capture slot (`ref.generic.outlives.instance`).
fn has_fn_slot(pool: Types<'_>, t: Ty) -> bool {
    pool.has_fn(t) && slots_of(pool, t).iter().any(|&(s, _)| is_fn(pool, s))
}

/// Per slot of declared type `t`, whether it is a function type written
/// inside another type, which holds no captured origins
/// (`ref.generic.outlives.written`): not `t` itself nor the referent of
/// one of its references.
fn written_fns(pool: Types<'_>, t: Ty) -> Vec<bool> {
    let slots = slots_of(pool, t);
    (0..slots.len())
        .map(|k| {
            is_fn(pool, slots[k].0)
                && !(k == 0 && capture_slot(pool, t))
                && !(k > 0 && capture_slot(pool, referent(pool, slots[k - 1].0)))
        })
        .collect()
}

/// Each origin slot of `t` in pre-order: each reference type outside
/// function types and counted references, as in [`Types::ref_slots`],
/// and each function type outside another function type, whose slot
/// holds what its closure captures. Each comes with whether it is at the
/// top, outside another reference.
fn slots_of(pool: Types<'_>, t: Ty) -> Vec<(Ty, bool)> {
    fn walk(pool: Types<'_>, t: Ty, top: bool, depth: u32, out: &mut Vec<(Ty, bool)>) {
        if depth > 32 || !(pool.has_ref(t) || pool.has_fn(t)) {
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
            TyData::Opaque { borrows: true, .. } | TyData::Fn { .. } => out.push((t, top)),
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

/// Each top slot of `t`'s run of reference slots, itself and the slots
/// behind it, as `[start, end)`.
fn top_runs(pool: Types<'_>, t: Ty) -> Vec<(usize, usize)> {
    let tp = tops(pool, t);
    let n = dep_slots(pool, t);
    (0..tp.len())
        .map(|k| (tp[k].0, tp.get(k + 1).map_or(n, |x| x.0)))
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
    holds_local_ref(pool, t) || is_dyn(pool, t) || has_fn_slot(pool, t)
}

/// Where a call's parameter keeps a capture slot of its argument.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FnDest {
    /// A direct `fn(...)` parameter, or a type parameter that may hold
    /// captured origins: the call may only use it or pass it on.
    Scoped,
    /// A type parameter the call keeps under `'static`.
    Static,
    /// A type parameter with no position for captured origins, which a
    /// function value that captures only parameters' origins may reach.
    Forward,
    /// A function type written inside another type, which holds none.
    Kept,
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
    /// The generic calls that may store through a `&mut` argument
    /// (`ref.generic.store`).
    stores: &'a HashMap<u32, DepSig>,
    /// Each parameter's type, which `Origin::Param` slots index.
    params: Vec<Ty>,
    /// The function's own map, whose parameters' lifetimes decide what a
    /// place the caller owns may take (`ref.lifetime.store`).
    own: Option<&'a DepSig>,
    /// Stores of a local-holding value into a place the caller owns.
    out: HashSet<u32>,
    /// The references `box(e)` makes.
    boxes: &'a HashSet<u32>,
    /// Locals whose closure capture is already `local-reference-escapes`
    /// (`ref.local.escape.capture-stored`): they add no capture origins.
    reported: HashSet<hd_base::LocalId>,
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
                // `ref.generic.outlives.written`
                let scoped = capture_slot(self.pool, inner);
                ts.iter()
                    .map(|&(r, st)| {
                        if is_fn(self.pool, st) && !scoped {
                            vec![]
                        } else {
                            vec![Origin::Param(p, s + 1 + w(r))]
                        }
                    })
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
        // A part's slots inside `t`.
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
                let item = self.cx.lookup.item(def)?;
                let fields = match (&item.data, variant) {
                    (ItemData::Data(fs), None) => fs,
                    (ItemData::Enum { shared, variants }, Some(k)) if shared.is_empty() => {
                        &variants.get(k as usize)?.fields
                    }
                    _ => return None,
                };
                let field = fields.get(j as usize)?;
                let f = field.ty;
                let argv = pool.list_items(args);
                let child = subst_owner(pool, def, argv, f);
                let at = |a: usize| (argv[..a].iter().map(|&x| n(x)).sum(), n(argv[a]));
                let range = match pool.get(f) {
                    // `ref.generic.outlives.position`: a field of a type
                    // parameter bounded by a lifetime lies in that
                    // lifetime's position.
                    TyData::Param(_)
                        if let Some(l) =
                            crate::local_refs::outlives_field(pool, def, &item.generics, f) =>
                    {
                        let mut lts = item.generics.iter().enumerate().filter(|(_, g)| g.lifetime);
                        lts.nth(usize::from(l) - 1)
                            .map(|(g, _)| g)
                            .filter(|&g| g < argv.len())
                            .map(at)
                    }
                    TyData::Param(p) if p.owner == def && (p.index as usize) < argv.len() => {
                        Some(at(p.index as usize))
                    }
                    // `ref.lifetime.struct.read`: a field whose top slots
                    // all name one lifetime holds that lifetime's position.
                    _ => {
                        let slots = slots_of(pool, f);
                        let mut ids = slots
                            .iter()
                            .zip(&field.lifetimes)
                            .filter(|((_, top), _)| *top)
                            .map(|(_, &l)| l);
                        let l = ids.next().filter(|&l| l != 0 && ids.all(|x| x == l));
                        let g = l.and_then(|l| {
                            let mut lts =
                                item.generics.iter().enumerate().filter(|(_, g)| g.lifetime);
                            lts.nth(usize::from(l) - 1).map(|(g, _)| g)
                        });
                        let aligned = slots.len() == field.lifetimes.len()
                            && tops(pool, f).len() == tops(pool, child).len();
                        g.filter(|&g| aligned && g < argv.len()).map(at)
                    }
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
        self.range(self.b.local_ty[l as usize], proj)
    }

    /// The run of the top slots of a value of type `t` that the field
    /// projections `proj` reach, when that is known.
    fn range(&self, mut t: Ty, proj: &[Proj]) -> Option<(usize, usize)> {
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
            // An element's value has the collection's element origins
            // (`ref.lifetime.store.element`).
            PlaceRoot::Value(v) if matches!(p.proj.first(), Some(Proj::Elem(_))) => {
                let held = match self.ty(v.0) {
                    Some(t) => self.project(self.of(v.0).to_vec(), t, &p.proj, want),
                    None => empty,
                };
                let mut v = v.0;
                while v & hd_tir::ir::Ref::CONST_BIT == 0 && self.b.tags[v as usize] == Tag::Coerce
                {
                    v = self.b.data[v as usize][0];
                }
                if v & hd_tir::ir::Ref::CONST_BIT == 0 && self.b.tags[v as usize] == Tag::Read {
                    (self.place(v, self.b.data[v as usize][0], 0).0, held)
                } else {
                    (vec![Origin::Hidden(i)], held)
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

    /// The local and the run of its slots that the argument `a` refers
    /// to, when it is a reference to a local's place reached by field
    /// projections.
    fn arg_range(&self, a: u32) -> Option<(u32, (usize, usize))> {
        let b = self.b;
        let mut v = a;
        while v & hd_tir::ir::Ref::CONST_BIT == 0 && b.tags[v as usize] == Tag::Coerce {
            v = b.data[v as usize][0];
        }
        if v & hd_tir::ir::Ref::CONST_BIT != 0 || b.tags[v as usize] != Tag::Ref {
            return None;
        }
        let p = b.decode_place(b.data[v as usize][0])?;
        let PlaceRoot::Local(l) = p.root else {
            return None;
        };
        Some((l.raw(), self.local_range(l.raw(), &p.proj)?))
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
            return if dep_slots(pool, inner) > 0 {
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
        // `ref.generic.call.unaligned`: everything every argument reaches.
        if id == ANY {
            for p in 0..sig.params.len() {
                let Some(a) = source_arg(b, i, p) else {
                    continue;
                };
                let Some(at) = self.ty(a) else { continue };
                let set = if b.tags[a as usize] == Tag::Ref || may_hold(self.pool, at) {
                    self.reach(flat(self.of(a)))
                } else {
                    self.lent(a)
                };
                union(&mut out, &set);
            }
            return out;
        }
        if id == ANY_FN {
            for p in 0..sig.params.len() {
                let Some(a) = source_arg(b, i, p) else {
                    continue;
                };
                let Some(at) = self.ty(a) else { continue };
                // A counted receiver lends its place's value.
                let (cur, at) = match local_ref(self.pool, at) {
                    Some((true, false)) => {
                        let inner = referent(self.pool, at);
                        let mut h = vec![Set::new(); width(self.pool, inner)];
                        for o in self.lent(a) {
                            union_into(&mut h, &self.held(o));
                        }
                        (h, inner)
                    }
                    _ if may_hold(self.pool, at) => (self.of(a).to_vec(), at),
                    _ => continue,
                };
                for (s, &(st, _)) in slots_of(self.pool, at).iter().enumerate() {
                    if is_fn(self.pool, st) {
                        union(&mut out, &self.slot_in(&cur, at, at, s, 0));
                    }
                }
            }
            return out;
        }
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

    /// `ref.lifetime.store`: whether `value` may land in what slot `s` of
    /// parameter `q` refers to, a place the caller owns: the reference
    /// positions of that place's type all have one lifetime, and each
    /// origin of `value` is one a result of that lifetime may depend on.
    /// A place that the field projections of `deref` reach from a referent
    /// of that type checks each top slot of `value` against its own
    /// positions only (`ref.lifetime.store.field`).
    fn caller_place_takes(
        &self,
        q: u32,
        s: u32,
        deref: Option<(Ty, &[Proj])>,
        value: &[Set],
    ) -> bool {
        let pool = self.pool;
        let Some(own) = self.own else { return false };
        let Some((t, ids)) = own.params.get(q as usize) else {
            return false;
        };
        let s = s as usize;
        let Some(&(rt, _)) = slots_of(pool, *t).get(s) else {
            return false;
        };
        let inner = referent(pool, rt);
        let runs = top_runs(pool, inner);
        let takes = |lo: usize, hi: usize, v: &[Origin]| {
            let Some(ls @ [l, ..]) = ids.get(s + 1 + lo..s + 1 + hi) else {
                return false;
            };
            ls.iter().all(|x| x == l) && v.iter().all(|&o| own.allows(pool, o, *l))
        };
        let range = deref
            .filter(|&(t, _)| referent_fn(pool, t) == referent_fn(pool, inner))
            .and_then(|(_, proj)| self.range(inner, proj));
        match range {
            Some((a, k)) if value.len() == k && a + k <= runs.len() => (0..k).all(|j| {
                let (lo, hi) = runs[a + j];
                value[j].is_empty() || takes(lo, hi, &value[j])
            }),
            _ => takes(0, dep_slots(pool, inner), &flat(value)),
        }
    }

    /// `ref.generic.store`: call `i` may store, through each `&mut`
    /// argument, what the type parameters under that `&mut` stand for, so
    /// the argument's places take what those parameters' argument
    /// positions depend on, or a written lifetime's (`ref.lifetime.store.call`).
    /// A place the caller owns may take only what it already holds, or
    /// values of its lifetime (`ref.lifetime.store`).
    fn generic_store(&mut self, i: usize, sig: &DepSig) {
        let pool = self.pool;
        for (p, (pt, ids)) in sig.params.iter().enumerate() {
            let Some(a) = source_arg(self.b, i, p) else {
                continue;
            };
            let slots = slots_of(pool, *pt);
            let mut top_k = 0;
            for (s, &(rt, top)) in slots.iter().enumerate() {
                if !top {
                    continue;
                }
                let k = top_k;
                top_k += 1;
                let mutable = matches!(pool.get(rt), TyData::Ref { mutable: true, .. });
                let inner = dep_slots(pool, referent(pool, rt));
                if !mutable || inner == 0 {
                    continue;
                }
                // What each top slot of the referent may take: the
                // arguments of the lifetimes in its own positions
                // (`ref.lifetime.store.field`).
                let per: Vec<Set> = top_runs(pool, referent(pool, rt))
                    .into_iter()
                    .map(|(lo, hi)| {
                        let mut v = Set::new();
                        let n = ids.len();
                        for &id in &ids[(s + 1 + lo).min(n)..(s + 1 + hi).min(n)] {
                            if (TVAR..TVAR_END).contains(&id)
                                || id == ANY
                                || id == ANY_FN
                                || id < ANON
                            {
                                union(&mut v, &self.lifetime_args(i, sig, id));
                            }
                        }
                        v
                    })
                    .collect();
                let value = flat(&per);
                if value.is_empty() {
                    continue;
                }
                let place = self.arg_range(a).filter(|&(_, (_, n))| n == per.len());
                let roots = self.of(a).get(k).cloned().unwrap_or_default();
                for o in roots {
                    match o {
                        // Each position of the argument's own place.
                        Origin::Local(l) if place.is_some_and(|(x, _)| x == l) => {
                            self.hold(o, &per, place.map(|(_, r)| r));
                        }
                        Origin::Local(_) | Origin::Hidden(_) => {
                            self.hold(o, std::slice::from_ref(&value), None);
                        }
                        Origin::Param(q, s0) => {
                            if value
                                .iter()
                                .any(|x| !matches!(x, Origin::Param(r, _) if *r == q))
                                && !self.caller_place_takes(
                                    q,
                                    s0,
                                    Some((referent(pool, rt), &[])),
                                    &per,
                                )
                            {
                                self.out.insert(w(i));
                            }
                        }
                        Origin::Global(_) => {}
                    }
                }
            }
        }
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

    /// The values of struct literal `i` for fields of a type parameter
    /// bounded by a lifetime (`ref.generic.outlives.field`).
    fn outlives_fields(&self, i: usize) -> Vec<u32> {
        let b = self.b;
        if b.tags[i] != Tag::NewStruct {
            return Vec::new();
        }
        let TyData::Adt { def, .. } = self.pool.get(b.ty[i]) else {
            return Vec::new();
        };
        let Some(item) = self.cx.lookup.item(def) else {
            return Vec::new();
        };
        let ItemData::Data(fields) = &item.data else {
            return Vec::new();
        };
        b.record(b.data[i][1])
            .iter()
            .zip(fields)
            .filter(|(_, f)| {
                crate::local_refs::outlives_field(self.pool, def, &item.generics, f.ty).is_some()
            })
            .map(|(&v, _)| v)
            .collect()
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

    /// Store `i` of `value` into the place `word`: a place of the body
    /// takes its origins, and one the caller owns only values of its
    /// lifetime (`ref.dep.store`). `holds` is whether the value's type
    /// holds a local reference.
    fn store(&mut self, i: usize, word: u32, value: &[Set], holds: bool, captures: bool) {
        if value.iter().all(Vec::is_empty) {
            return;
        }
        let p = self.b.decode_place(word);
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
        // The referent's type and the projections past the dereference.
        let deref = match &p {
            Some(p) if through => match p.root {
                PlaceRoot::Value(v) => self.ty(v.0).map(|t| (referent(self.pool, t), &p.proj[1..])),
                _ => None,
            },
            _ => None,
        };
        let (storage, _) = self.place(w(i), word, 0);
        for s in storage {
            match s {
                Origin::Param(q, k) if through => {
                    // A capture-bearing function value never lands in a
                    // caller's place (`fn.capture.local-store`).
                    if captures || (holds && !self.caller_place_takes(q, k, deref, value)) {
                        self.out.insert(w(i));
                    }
                }
                Origin::Local(_) | Origin::Hidden(_) => self.hold(s, value, range),
                Origin::Global(_) if captures => {
                    self.out.insert(w(i));
                }
                _ => {}
            }
        }
    }

    /// A list push or a map store keeps its other operands in the
    /// collection its first operand reads (`ref.dep.store`).
    fn intrinsic_store(&mut self, i: usize) {
        let b = self.b;
        let ops = intrinsic_ops(b, i);
        let keeps = matches!(
            IntrinsicOp::from_u32(b.data[i][0]),
            Some(IntrinsicOp::ListPush | IntrinsicOp::MapSet)
        );
        let Some((&coll, kept)) = ops.split_first() else {
            return;
        };
        let holds = self.ty(coll).is_some_and(|t| holds_local_ref(self.pool, t));
        if !keeps || !holds || b.tags[coll as usize] != Tag::Read {
            return;
        }
        let mut value = Set::new();
        for &v in kept {
            if self.ty(v).is_some_and(|t| holds_local_ref(self.pool, t)) {
                union(&mut value, &flat(self.of(v)));
            }
        }
        self.store(i, b.data[coll as usize][0], &[value], true, false);
    }

    fn step(&mut self, i: usize) {
        let b = self.b;
        let tag = b.tags[i];
        if tag == Tag::Store {
            let [word, v] = b.data[i];
            let value = self.of(v).to_vec();
            let t = b.ty.get(v as usize).copied().unwrap_or(Ty::VOID);
            let captures = self.fn_origins(&value, t);
            self.store(i, word, &value, holds_local_ref(self.pool, t), captures);
            return;
        }
        if tag == Tag::Intrinsic {
            self.intrinsic_store(i);
        }
        if let Some(sig) = self.stores.get(&w(i)) {
            self.generic_store(i, sig);
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
            Tag::Call | Tag::Await | Tag::CallDyn | Tag::CallValue => match self.deps.get(&w(i)) {
                Some(sig) => self.call(i, sig, n),
                None => return,
            },
            Tag::Closure if !b.is_instantiation(i) => {
                let rec = b.record(b.data[i][1]);
                let mut captured = Set::new();
                if let [start, len, ..] = rec {
                    for c in *start..start.saturating_add(*len) {
                        let Some(local) = b.cap_local.get(c as usize) else {
                            continue;
                        };
                        if self.reported.contains(local) {
                            continue;
                        }
                        union(&mut captured, &flat(&self.held(Origin::Local(local.raw()))));
                    }
                }
                vec![captured]
            }
            Tag::Coerce => self.of(b.data[i][0]).to_vec(),
            // A collection intrinsic's result, as a map lookup's, holds
            // what its operands hold.
            Tag::Intrinsic => {
                let mut s = Set::new();
                for &v in intrinsic_ops(b, i) {
                    union(&mut s, &flat(self.of(v)));
                }
                vec![s; n]
            }
            Tag::Prim | Tag::CallHost | Tag::Return => return,
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

    /// Whether value `v` holds captured origins in a function type's
    /// capture slot at its top.
    fn captured_fn(&self, v: u32) -> bool {
        self.ty(v).is_some_and(|t| self.fn_origins(self.of(v), t))
    }

    /// Whether `value`, of type `t`, holds origins in a capture slot at
    /// the top of `t` (`ref.generic.outlives.instance`).
    fn fn_origins(&self, value: &[Set], t: Ty) -> bool {
        let pool = self.pool;
        if !pool.has_fn(t) {
            return false;
        }
        let tp = tops(pool, t);
        if value.len() == tp.len() {
            tp.iter()
                .zip(value)
                .any(|(&(_, st), set)| is_fn(pool, st) && !set.is_empty())
        } else {
            tp.iter().any(|&(_, st)| is_fn(pool, st)) && value.iter().any(|s| !s.is_empty())
        }
    }

    /// For each parameter of call `i`: its instantiated type, and for each
    /// capture slot of that type, where a scoped function value there may
    /// go. `Ok` for a direct `fn(...)` parameter or the referent of a
    /// reference to one, and for a type parameter that may hold captured
    /// origins (`ref.generic.outlives.instance`); for a type parameter
    /// that `'static` does not bound, whether it only forwards a
    /// parameter's (`Err(true)`). Else the message of the error.
    fn call_params(&self, i: usize) -> Vec<(Ty, Vec<(usize, FnDest)>)> {
        let b = self.b;
        let pool = self.pool;
        let static_bound = self.cx.names.known.static_bound;
        let instantiated = |def: DefId| {
            let sig = self.cx.lookup.item(def)?.sig()?;
            let names = &self.cx.names;
            let params = DepSig::new(
                pool,
                &sig.params,
                |s| names.text(s) == "self",
                sig.ret,
                &sig.lifetimes,
            )
            .instantiate(pool, b, i, None)
            .params;
            let dest = |p: hd_types::ParamRef| {
                let generics = if p.owner == def {
                    Some(&sig.generics)
                } else {
                    self.cx.lookup.item(p.owner).map(|it| &it.generics)
                };
                let g = generics.and_then(|g| g.get(usize::from(p.index)));
                if g.is_some_and(|g| crate::local_refs::static_bounded(pool, static_bound, g)) {
                    FnDest::Static
                } else if crate::local_refs::scoped_param(pool, sig, p) {
                    FnDest::Scoped
                } else {
                    FnDest::Forward
                }
            };
            Some(
                sig.params
                    .iter()
                    .zip(params)
                    .map(|(&(_, decl), (t, _))| {
                        let written = written_fns(pool, decl);
                        let n = dep_slots(pool, decl);
                        let mut keys = Vec::new();
                        let mut out = Vec::new();
                        let mut it = (0..w(n)).chain(std::iter::repeat(NO_SOURCE));
                        let aligned = line_up(pool, decl, t, &mut it, &mut keys, &mut out, 0);
                        let slots = slots_of(pool, t);
                        let dests = slots
                            .iter()
                            .enumerate()
                            .filter(|(_, (st, _))| is_fn(pool, *st))
                            .map(|(s, _)| {
                                let d = match out.get(s) {
                                    _ if !aligned => FnDest::Kept,
                                    Some(&id) if (TVAR..TVAR_END).contains(&id) => {
                                        let k = ((id - TVAR) >> 8) as usize;
                                        keys.get(k).map_or(FnDest::Kept, |&p| dest(p))
                                    }
                                    Some(&id) if (id as usize) < n => {
                                        if written[id as usize] {
                                            FnDest::Kept
                                        } else {
                                            FnDest::Scoped
                                        }
                                    }
                                    _ => FnDest::Kept,
                                };
                                (s, d)
                            })
                            .collect();
                        (t, dests)
                    })
                    .collect(),
            )
        };
        match b.tags[i] {
            Tag::Call | Tag::Await => Callee::from_words(b.record(b.data[i][0]))
                .map(|callee| match callee {
                    Callee::Item { def, .. } => def,
                    Callee::TraitMethod { method, .. } => method,
                })
                .and_then(instantiated)
                .unwrap_or_default(),
            Tag::CallDyn => b
                .record(b.data[i][1])
                .first()
                .copied()
                .map(DefId::from_raw)
                .and_then(instantiated)
                .unwrap_or_default(),
            Tag::CallValue => self
                .ty(b.data[i][0])
                .and_then(|t| match pool.get(referent_fn(pool, t)) {
                    TyData::Fn { params, .. } => Some(
                        pool.list_items(params)
                            .iter()
                            .map(|&p| {
                                let written = written_fns(pool, p);
                                let dests = slots_of(pool, p)
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, (st, _))| is_fn(pool, *st))
                                    .map(|(s, _)| {
                                        (
                                            s,
                                            if written[s] {
                                                FnDest::Kept
                                            } else {
                                                FnDest::Scoped
                                            },
                                        )
                                    })
                                    .collect();
                                (p, dests)
                            })
                            .collect(),
                    ),
                    _ => None,
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// Uses of scoped function values that keep them somewhere other than
    /// another direct `fn(...)` parameter (`fn.capture.local-store`).
    fn captured_fn_escapes(&self) -> Vec<(NodeIdx, &'static str)> {
        let b = self.b;
        let mut bad = Vec::new();
        let at = |v: u32, i: usize| {
            if v & hd_tir::ir::Ref::CONST_BIT == 0 {
                b.syn
                    .get(v as usize)
                    .copied()
                    .filter(|n| *n != NodeIdx::NONE)
                    .unwrap_or(b.syn[i])
            } else {
                b.syn[i]
            }
        };
        for i in 0..b.tags.len() {
            match b.tags[i] {
                // `box(f)` (`ref.generic.outlives.box`): a counted
                // reference to a new object, which holds a function value
                // that captures. A method receiver's or a loop cursor's
                // address is counted too, and stays in the body.
                Tag::Ref
                    if matches!(self.pool.get(b.ty[i]), TyData::Ref { counted: true, inner, .. }
                    if has_fn_slot(self.pool, inner)
                        && self.boxes.contains(&w(i))
                        && self.fn_origins(
                            &self.place(w(i), b.data[i][0], width(self.pool, inner)).1,
                            inner,
                        )) =>
                {
                    bad.push((
                        b.syn[i],
                        "a local reference captured by this function value never escapes to a box",
                    ));
                }
                Tag::Store => {
                    let [place, value] = b.data[i];
                    if !self.captured_fn(value) {
                        continue;
                    }
                    let local = b.decode_place(place).is_some_and(|p| {
                        matches!(p.root, PlaceRoot::Local(_)) && p.proj.is_empty()
                    });
                    if !local {
                        bad.push((
                            at(value, i),
                            "a local reference captured by this function value never escapes to a field, a top-level binding, or a place the caller owns",
                        ));
                    }
                }
                Tag::Call | Tag::Await | Tag::CallDyn | Tag::CallValue => {
                    for (p, (t, dests)) in self.call_params(i).iter().enumerate() {
                        let Some(value) = source_arg(b, i, p) else {
                            continue;
                        };
                        let Some(vt) = self.ty(value) else { continue };
                        if dests.is_empty() || !may_hold(self.pool, vt) {
                            continue;
                        }
                        for &(s, dest) in dests {
                            let set = self.slot_in(self.of(value), vt, *t, s, 0);
                            if set.is_empty() {
                                continue;
                            }
                            let forwards = set.iter().all(|o| matches!(o, Origin::Param(_, _)));
                            let msg = match dest {
                                FnDest::Scoped => continue,
                                FnDest::Forward if forwards => continue,
                                FnDest::Static => {
                                    "a local reference captured by this function value never escapes to a type parameter bounded by `'static`"
                                }
                                FnDest::Forward | FnDest::Kept => {
                                    "a local reference captured by this function value may be passed only to a `fn(...)` parameter"
                                }
                            };
                            bad.push((at(value, i), msg));
                            break;
                        }
                    }
                }
                Tag::NewStruct
                | Tag::CopyStruct
                | Tag::NewVariant
                | Tag::NewTuple
                | Tag::NewList
                | Tag::NewMap => {
                    let kept = self.outlives_fields(i);
                    slots(b, i, |_, value, word| {
                        if matches!(word, Word::Value(Role::Consume))
                            && self.captured_fn(value)
                            && !kept.contains(&value)
                        {
                            bad.push((
                                at(value, i),
                                "a local reference captured by this function value never escapes to a field or collection element",
                            ));
                        }
                    });
                }
                Tag::Intrinsic => {
                    slots(b, i, |_, value, word| {
                        if matches!(word, Word::Value(Role::Consume)) && self.captured_fn(value) {
                            bad.push((
                                at(value, i),
                                "a local reference captured by this function value never escapes to a collection element",
                            ));
                        }
                    });
                }
                Tag::With | Tag::ContextNew => {
                    slots(b, i, |_, value, word| {
                        if word == Word::Provider && self.captured_fn(value) {
                            bad.push((
                                at(value, i),
                                "a local reference captured by this function value never escapes to a provider",
                            ));
                        }
                    });
                }
                Tag::CallHost => {
                    slots(b, i, |_, value, word| {
                        if matches!(word, Word::Value(_)) && self.captured_fn(value) {
                            bad.push((
                                at(value, i),
                                "a local reference captured by this function value never escapes through a host call",
                            ));
                        }
                    });
                }
                _ => {}
            }
        }
        bad
    }
}

/// Intrinsic `i`'s operands.
fn intrinsic_ops(b: &Body, i: usize) -> &[u32] {
    match b.data[i][1] {
        NONE => &[],
        r => b.record(r),
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
    /// Whether a function value's dependency map is at least as general
    /// as the expected map: each result may borrow from a subset of the
    /// inputs the expected result permits (`fn.type.lifetime.subtype`).
    pub(crate) fn fn_deps_fit(&self, got: hd_types::FnDeps, want: hd_types::FnDeps) -> bool {
        if got == want {
            return true;
        }
        let pool = self.pool();
        let (Some((got_ins, got_outs)), Some((want_ins, want_outs))) =
            (pool.deps_of(got), pool.deps_of(want))
        else {
            return false;
        };
        got_ins.len() == want_ins.len()
            && got_outs.len() == want_outs.len()
            && got_outs.iter().zip(want_outs).all(|(&got_out, &want_out)| {
                got_ins
                    .iter()
                    .zip(want_ins)
                    .all(|(&got_in, &want_in)| got_in != got_out || want_in == want_out)
            })
    }

    /// The map a function value of `sig` carries, its parameters from
    /// `skip` on (`fn.type.lifetime.value`); also what a trait implementation's
    /// method must fit against the trait's (`trait.impl.lifetime.subtype`).
    pub(crate) fn sig_fn_deps(&self, sig: &hd_resolve::FnSig, skip: usize) -> hd_types::FnDeps {
        let pool = self.pool();
        if pool.ref_slots(sig.ret) == 0 {
            return hd_types::FnDeps::NONE;
        }
        let names = &self.cx.names;
        let is_self = |s| names.text(s) == "self";
        DepSig::new(pool, &sig.params, is_self, sig.ret, &sig.lifetimes).to_fn(pool, skip)
    }

    /// The map a trait implementation's method `sig` must share with the
    /// trait's `tsig` (`trait.impl.lifetime`): the trait's, with its opaque
    /// types opened over the implementation's result, which may take
    /// their place (`trait.opaque.impl-result`).
    pub(crate) fn trait_fn_deps(
        &self,
        tsig: &hd_resolve::FnSig,
        sig: &hd_resolve::FnSig,
    ) -> hd_types::FnDeps {
        let pool = self.pool();
        if !pool.has_opaque(tsig.ret) || pool.ref_slots(tsig.ret) == 0 {
            return self.sig_fn_deps(tsig, 0);
        }
        let names = &self.cx.names;
        let is_self = |s| names.text(s) == "self";
        DepSig::new(pool, &tsig.params, is_self, tsig.ret, &tsig.lifetimes)
            .open(pool, sig.ret)
            .to_fn(pool, 0)
    }

    /// The dependency map of a callee's signature (`ref.dep.call`,
    /// `ref.dep.trait`): a trait method's from the trait's signature.
    /// At call `i` of `b`, the map is over the types the call instantiates
    /// (`ref.generic.call`).
    fn callee_dep(&self, def: DefId, b: &Body, i: usize) -> Option<DepSig> {
        let sig = self.sig_of(def).ok()?;
        let pool = self.pool();
        let names = &self.cx.names;
        let dep = DepSig::new(
            pool,
            &sig.params,
            |s| names.text(s) == "self",
            sig.ret,
            &sig.lifetimes,
        )
        .instantiate(pool, b, i, Some((def, &sig.generics)));
        dep.depends().then_some(dep)
    }

    /// The map of call `i`, when it passes a local-holding value and may
    /// store one through a `&mut` argument (`ref.generic.store`,
    /// `ref.lifetime.store.call`).
    fn store_dep(&self, b: &Body, i: usize) -> Option<DepSig> {
        let pool = self.pool();
        let mut holding = false;
        slots(b, i, |_, v, word| {
            if matches!(word, Word::Value(Role::Arg(_)))
                && v & hd_tir::ir::Ref::CONST_BIT == 0
                && b.ty.get(v as usize).is_some_and(|&t| may_hold(pool, t))
            {
                holding = true;
            }
        });
        if !holding {
            return None;
        }
        let def = match Callee::from_words(b.record(b.data[i][0]))? {
            Callee::Item { def, .. } => def,
            Callee::TraitMethod { method, .. } => method,
        };
        let sig = self.sig_of(def).ok()?;
        let writes = sig.params.iter().any(|&(_, t)| {
            slots_of(pool, t).iter().any(|&(rt, top)| {
                top && matches!(pool.get(rt), TyData::Ref { mutable: true, inner, .. }
                    if pool.has_param(inner) || pool.ref_slots(inner) > 0)
            })
        });
        if !writes {
            return None;
        }
        let names = &self.cx.names;
        let dep = DepSig::new(
            pool,
            &sig.params,
            |s| names.text(s) == "self",
            sig.ret,
            &sig.lifetimes,
        )
        .instantiate(pool, b, i, Some((def, &sig.generics)));
        // A slot of a type parameter, or of a written lifetime
        // (`ref.lifetime.store.call`).
        let shared = dep.params.iter().any(|(_, ids)| {
            ids.iter()
                .any(|&id| (TVAR..TVAR_END).contains(&id) || id < ANON)
        });
        shared.then_some(dep)
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
        let fn_body = body.kind == BodyKind::Fn && fun.is_some() && may_hold(pool, ret);
        let mut opaque = false;
        let own_sig = fn_body
            .then(|| {
                if self.result_omitted {
                    let mut sig = self.cx.lookup.item(body.item)?.sig()?.clone();
                    sig.ret = ret;
                    Some(sig)
                } else {
                    self.sig_of(body.item).ok()
                }
            })
            .flatten();
        let own = match own_sig {
            Some(sig) => {
                let names = &self.cx.names;
                // A hidden result is checked against its opaque types'
                // lifetimes (`types.opaque.borrows.body`).
                opaque = pool.has_opaque(sig.ret);
                let dep = DepSig::new(
                    pool,
                    &sig.params,
                    |s| names.text(s) == "self",
                    if opaque { sig.ret } else { ret },
                    &sig.lifetimes,
                );
                Some(if opaque { dep.open(pool, ret) } else { dep })
            }
            _ => None,
        };
        // The parameters' lifetimes, which decide what a place the caller
        // owns may take (`ref.lifetime.store`), from the declared
        // signature: an omitted result is no part of it.
        let declared = (body.kind == BodyKind::Fn && fun.is_some())
            .then(|| self.cx.lookup.item(body.item).and_then(|i| i.sig()))
            .flatten();
        let mine = declared.map(|sig| {
            let names = &self.cx.names;
            let is_self = |s| names.text(s) == "self";
            DepSig::new(pool, &sig.params, is_self, Ty::VOID, &sig.lifetimes)
        });
        let mut deps: HashMap<u32, DepSig> = HashMap::default();
        let mut stores: HashMap<u32, DepSig> = HashMap::default();
        let mut closures: Vec<(usize, DepSig)> = Vec::new();
        let mut through = body.local_ty.iter().enumerate().any(|(l, &t)| {
            body.local_flags[l] & hd_tir::ir::local_flags::PARAM != 0 && capture_slot(pool, t)
        });
        for i in 0..body.tags.len() {
            if matches!(body.tags[i], Tag::Call | Tag::Await)
                && let Some(sig) = self.store_dep(body, i)
            {
                stores.insert(w(i), sig);
                through = true;
            }
            match body.tags[i] {
                // A block's or a loop's value may leave a local's scope.
                Tag::Block | Tag::Loop if w(i) != root.0 && holds_local_ref(pool, body.ty[i]) => {
                    through = true;
                }
                Tag::Call | Tag::Await | Tag::CallDyn if may_hold(pool, body.ty[i]) => {
                    let def = match body.tags[i] {
                        Tag::CallDyn => body.record(body.data[i][1]).first().copied(),
                        _ => match Callee::from_words(body.record(body.data[i][0])) {
                            Some(Callee::Item { def, .. }) => Some(def.raw()),
                            Some(Callee::TraitMethod { method, .. }) => Some(method.raw()),
                            None => None,
                        },
                    };
                    if let Some(dep) =
                        def.and_then(|d| self.callee_dep(DefId::from_raw(d), body, i))
                    {
                        deps.insert(w(i), dep);
                    }
                }
                // A function value's call takes the map its type carries
                // (`fn.type.lifetime.call`).
                Tag::CallValue if may_hold(pool, body.ty[i]) => {
                    let f = body.data[i][0];
                    if let Some(dep) = body
                        .ty
                        .get(f as usize)
                        .and_then(|&t| DepSig::of_fn(pool, t))
                    {
                        deps.insert(w(i), dep);
                    }
                }
                // A closure's returned values depend only on its own
                // parameters (`fn.closure.lifetime.return`); a result slot
                // with no source may depend on nothing.
                Tag::Closure if !body.is_instantiation(i) => {
                    let sub = body.data[i][0] as usize;
                    let rec = body.record(body.data[i][1]);
                    if let [start, len, ..] = rec {
                        through |= (*start..start.saturating_add(*len)).any(|c| {
                            body.cap_local
                                .get(c as usize)
                                .is_some_and(|l| may_hold(pool, body.local_ty[l.idx()]))
                        });
                    }
                    let dep = DepSig::of_fn(pool, body.ty[i]);
                    match pool.get(referent_fn(pool, body.ty[i])) {
                        // A local function's hidden result is checked
                        // against its opaque types' lifetimes
                        // (`types.opaque.borrows.body`).
                        TyData::Fn { params, result, .. } if pool.has_opaque(result) => {
                            let dep = dep.unwrap_or_else(|| DepSig {
                                params: pool
                                    .list_items(params)
                                    .iter()
                                    .map(|&p| (p, vec![]))
                                    .collect(),
                                outs: vec![],
                                ret: result,
                                elided: Elided::Missing,
                                capture_elided: Elided::Missing,
                            });
                            let hidden = body.sub_root.get(sub).map_or(result, |&r| {
                                body.ty.get(r as usize).copied().unwrap_or(result)
                            });
                            closures.push((sub, dep.open(pool, hidden)));
                        }
                        _ => {
                            if let Some(dep) = dep.filter(|d| !d.outs.is_empty()) {
                                closures.push((sub, dep));
                            }
                        }
                    }
                }
                Tag::Store => {
                    let [_, v] = body.data[i];
                    through |= v & hd_tir::ir::Ref::CONST_BIT == 0
                        && body.ty.get(v as usize).is_some_and(|&t| may_hold(pool, t));
                }
                // A list push or a map store (`Solve::intrinsic_store`).
                Tag::Intrinsic => {
                    through |= intrinsic_ops(body, i).iter().any(|&v| {
                        v & hd_tir::ir::Ref::CONST_BIT == 0
                            && body
                                .ty
                                .get(v as usize)
                                .is_some_and(|&t| holds_local_ref(pool, t))
                    });
                }
                _ => {}
            }
        }
        let elided = own.as_ref().map(|o| o.elided);
        // A function value's hidden capture slot may be empty: returning a
        // capture-free function needs no reference source. Only ordinary
        // local-reference result slots require an elided source.
        let needs_source = own.as_ref().is_some_and(|o| pool.ref_slots(o.ret) > 0);
        if let (Some(Elided::Missing | Elided::Ambiguous), Some(f)) = (elided, fun)
            && self.result_omitted
            && needs_source
        {
            self.source_error(elided == Some(Elided::Missing), f);
        }
        // A function value in the result may carry captures with no source
        // (`ref.generic.outlives.read`) when no `fn(...)` parameter gives one.
        let own = own.filter(|o| {
            opaque
                || o.depends()
                || o.capture_elided == Elided::Missing
                    && tops(pool, o.ret).iter().any(|&(_, t)| is_fn(pool, t))
        });
        if own.is_none() && !through && closures.is_empty() {
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
            stores: &stores,
            params,
            own: mine.as_ref(),
            out: HashSet::default(),
            reported: self.ref_captured.clone(),
            boxes: &self.boxes,
            changed: true,
        };
        for l in 0..body.local_ty.len() {
            let lt = body.local_ty[l];
            if body.local_flags[l] & hd_tir::ir::local_flags::PARAM != 0 && may_hold(pool, lt) {
                let pt = s.params[l];
                let tp = tops(pool, pt);
                // A function type written inside another type holds no
                // captures (`ref.generic.outlives.written`).
                let written = written_fns(pool, pt);
                let mine: Vec<Set> = tp
                    .iter()
                    .map(|&(f, _)| {
                        if written[f] {
                            vec![]
                        } else {
                            vec![Origin::Param(w(l), w(f))]
                        }
                    })
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
        let capture_bad = s.captured_fn_escapes();
        let mut stores: Vec<NodeIdx> = s.out.iter().map(|&i| body.syn[i as usize]).collect();
        stores.sort_stable_by_key(|n| n.raw());
        let mut bad: Vec<(NodeIdx, Origin, u32)> = Vec::new();
        if let Some(own) = &own {
            let ids = own.result_ids(pool, ret);
            // A function value's capture slot with no source, as a
            // capturing closure read from a field (`ref.generic.outlives.read`).
            let capture: Vec<bool> = tops(pool, ret)
                .iter()
                .map(|&(_, t)| own.capture_elided == Elided::Missing && is_fn(pool, t))
                .collect();
            for r in returned(body, root.0) {
                let v = fit(s.of(r), ids.len());
                for (k, (set, ids)) in v.iter().zip(&ids).enumerate() {
                    // A slot of an opaque type that does not borrow may
                    // depend on nothing.
                    let id = match ids.first() {
                        Some(&id) => id,
                        None if opaque || capture.get(k) == Some(&true) => NO_SOURCE,
                        None => continue,
                    };
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
        let mut closure_bad: Vec<(NodeIdx, String)> = Vec::new();
        for (sub, dep) in &closures {
            let r = body.sub_params[*sub];
            let locals: Vec<u32> = (0..r.len)
                .map(|k| body.extra[(r.start + 1 + k) as usize])
                .collect();
            let ids = dep.result_ids(pool, dep.ret);
            for v in returned(body, body.sub_root[*sub]) {
                let set = fit(s.of(v), ids.len());
                let ok = |o: Origin, ids: &[u32]| match o {
                    Origin::Param(p, k) => locals.iter().position(|&l| l == p).is_some_and(|at| {
                        ids.iter()
                            .any(|&id| dep.allows(pool, Origin::Param(w(at), k), id))
                    }),
                    _ => false,
                };
                if let Some(o) = set
                    .iter()
                    .zip(&ids)
                    .find_map(|(set, ids)| set.iter().copied().find(|&o| !ok(o, ids)))
                {
                    closure_bad.push((site(&s, v, o, 0), closure_message(body, self.cx, o)));
                }
            }
        }
        // A returned value's origins are `ref.dep.return`'s.
        let mut skip: HashSet<u32> = HashSet::default();
        if own.is_some() {
            skip.extend(returned(body, root.0));
        }
        for (sub, _) in &closures {
            skip.extend(returned(body, body.sub_root[*sub]));
        }
        skip.insert(root.0);
        let outlives = scoped_uses(&s, &skip, self.cx);
        let mut seen = HashSet::default();
        for at in stores {
            if at == NodeIdx::NONE || !seen.insert(at) {
                continue;
            }
            let node = self.cx.src.parse.tree.node(at);
            self.err(
                Code::LocalReferenceEscapes,
                node,
                "a local reference never escapes to a place the caller owns, unless the type of that place names the reference's lifetime",
            );
        }
        for (at, msg) in capture_bad {
            if at == NodeIdx::NONE || !seen.insert(at) {
                continue;
            }
            let node = self.cx.src.parse.tree.node(at);
            self.err(Code::LocalReferenceEscapes, node, msg);
        }
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
        for (at, msg) in closure_bad {
            if at == NodeIdx::NONE || !seen.insert(at) {
                continue;
            }
            let node = self.cx.src.parse.tree.node(at);
            self.err(Code::LocalReferenceEscapes, node, &msg);
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
                .is_some_and(|(_, ids)| ids.contains(&id) || id == ANY && !ids.is_empty())
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
            [] => String::new(),
            [one] => one.clone(),
            [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        };
        match lifetime(id) {
            _ if sources.is_empty() => format!("{what}; the result may depend on no parameter"),
            Some(l) => format!("{what}; a `{l}` reference of the result depends only on {from}"),
            None => format!("{what}; the result depends only on {from}"),
        }
    }
}

/// `fn.closure.lifetime`: the message for a closure's returned value that
/// depends on `o`.
fn closure_message(b: &Body, cx: &BodyCx<'_>, o: Origin) -> String {
    let name = |l: u32| cx.names.text(b.local_name[l as usize]).to_owned();
    let what = match o {
        Origin::Param(p, _) | Origin::Local(p)
            if b.local_flags[p as usize] & hd_tir::ir::local_flags::PARAM != 0 =>
        {
            format!("the value borrows from `{}`", name(p))
        }
        Origin::Local(l) if !name(l).starts_with('$') => format!("`{}` is a local", name(l)),
        Origin::Global(_) => "the value refers to a top-level binding".to_owned(),
        _ => "the value refers to a temporary".to_owned(),
    };
    format!(
        "{what}; a closure's reference result depends only on the closure's reference parameters of its lifetime"
    )
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
