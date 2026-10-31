//! Dependent results (spec/lang/13-values-and-references.md, "Dependent
//! Results"): a function's result may hold a local reference, which
//! depends on the parameters whose reference slots share its lifetime
//! ([`DepSig`]): a written lifetime variable, or the one its signature
//! elides.
//!
//! Once a body is checked, [`Ck::check_dependent_results`] gives each
//! slot of its values and locals a region (`ref.dep.slot`), relates them
//! by outlives constraints over the body's TIR and solves them once, as
//! Rust's NLL does (`ref.dep.origin.*`, [`constraints`]). A region's value
//! is a set of origins, each a loan. An origin is a storage root a local
//! reference may point into: what a parameter's reference slot refers to,
//! a local's or a parameter's own storage, the hidden local of `&e` on a
//! value, or a top-level binding.
//! A returned value's slot may depend only on the parameter slots of its
//! lifetime (`ref.dep.return`), a call's result slot depends on the
//! argument slots of its lifetime (`ref.dep.call`), a store through a
//! reference lands in a place of the body (`ref.dep.store`). An element
//! reference takes the origin of its collection's root place
//! (`ref.dep.origin.element`).
//! A suspension holds what its call's arguments hold in a slot of its own
//! (`ref.dep.suspension`).
//!
//! A value's slots are its reference slots and its function types'
//! capture slots at the top, outside another reference ([`Layout`]), and
//! the slots behind a reference are its referent's. Where a projection or
//! a construction cannot be lined up with the slots, each slot takes the
//! union of all of them through one region, which only adds origins.

use std::cell::RefCell;
use std::rc::Rc;

use hd_base::SliceSort as _;
use hd_base::{DefId, HashMap, HashSet, NodeIdx, Symbol};
use hd_diag::Code;
use hd_resolve::{ItemData, Src};
use hd_syntax::{GreenTree, NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{Body, BodyKind, Callee, ChoiceKind, NONE, Role, Tag, Word, slots};
use hd_types::{Ty, TyData, Types};

use crate::body::{BodyCx, Ck};
use crate::call::subst_owner;
use crate::local_refs::{holds_local_ref, outlives_field, scoped_param, static_bounded};
use crate::regions::Category;

mod constraints;

/// The lifetime of an input slot that writes none: its own, `ANON` plus
/// the slot's index. A written lifetime is its 1-based place in scope.
const ANON: u32 = 1 << 16;

/// A result slot that depends on every parameter (`types.opaque.borrows.source`,
/// `ref.dep.suspension.result`).
const ANY: u32 = u32::MAX;

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
/// origin slot of its parameters and of its result, in [`Layout`] order
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
        ls: &Layouts<'_>,
        params: &[(Symbol, Ty)],
        is_self: impl Fn(Symbol) -> bool,
        ret: Ty,
        written: &[u16],
    ) -> DepSig {
        let pool = ls.pool;
        let ref_ins: usize = params.iter().map(|&(_, t)| pool.ref_slots(t)).sum();
        let ref_outs = pool.ref_slots(ret);
        let trust_in = written.len() >= ref_ins;
        let trust_out = written.len() >= ref_ins + ref_outs;
        let mut written_at = 0;
        let mut dep_at = 0;
        // A suspension's written lifetime follows the reference slots'
        // (`ref.dep.suspension.lifetime`).
        let held_at = std::cell::Cell::new(ref_ins + ref_outs);
        let next_held = || {
            held_at.set(held_at.get() + 1);
            (written.get(held_at.get() - 1).copied())
                .filter(|&l| l != 0)
                .map(u32::from)
        };
        let ps: Vec<(Ty, Vec<u32>)> = params
            .iter()
            .map(|&(_, t)| {
                let at = (&mut written_at, &mut dep_at);
                let ids = slot_ids(ls, t, at, &next_held, |k| {
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
            Some((t, ids)) if receiver && !ref_ids_of(ls, *t, ids).is_empty() => {
                Elided::Lifetime(ref_ids_of(ls, *t, ids)[0])
            }
            _ => one_id(
                params
                    .iter()
                    .zip(&ps)
                    .filter(|((s, _), _)| !is_self(*s))
                    .flat_map(|(_, (t, ids))| ref_ids_of(ls, *t, ids)),
            ),
        };
        let capture_elided = one_id(
            ps.iter()
                .filter(|(t, _)| capture_slot(pool, *t))
                .filter_map(|(_, ids)| ids.first().copied()),
        );
        let held = hold_id(&ps);
        let mut r = 0;
        let outs = (ls.of(ret).slots.iter())
            .map(|s| {
                let &Slot {
                    ty: st, fun, hold, ..
                } = s;
                if hold {
                    return next_held().or(held);
                }
                if fun {
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
    fn of_fn(ls: &Layouts<'_>, t: Ty) -> Option<DepSig> {
        let pool = ls.pool;
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
                let at = (&mut ref_at, &mut dep_at);
                let ids = slot_ids(ls, p, at, &|| None, |k| ins.get(k).copied());
                (p, ids)
            })
            .collect::<Vec<_>>();
        let elided = one_id(params.iter().flat_map(|(p, ids)| ref_ids_of(ls, *p, ids)));
        let capture_elided = one_id(
            params
                .iter()
                .filter(|(p, _)| capture_slot(pool, *p))
                .filter_map(|(_, ids)| ids.first().copied()),
        );
        // A type that writes no map has no reference result slots.
        let held = hold_id(&params);
        let mut r = 0;
        let outs = (ls.of(result).slots.iter())
            .filter_map(|s| {
                let &Slot {
                    ty: st, fun, hold, ..
                } = s;
                if hold {
                    return Some(held);
                }
                if fun {
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
    fn to_fn(&self, ls: &Layouts<'_>, skip: usize) -> hd_types::FnDeps {
        let ins: Vec<u32> = self.params[skip.min(self.params.len())..]
            .iter()
            .flat_map(|(t, ids)| ref_ids_of(ls, *t, ids))
            .collect();
        let outs: Option<Vec<u32>> = ref_ids_of(ls, self.ret, &self.outs)
            .into_iter()
            .map(|l| l.filter(|l| ins.contains(l)))
            .collect();
        outs.map_or(hd_types::FnDeps::NONE, |outs| ls.pool.fn_deps(&ins, &outs))
    }

    /// The map of a body whose declared result has opaque types, over
    /// its hidden result `hidden`: each slot of a hidden type takes its
    /// opaque type's lifetime, and none for an opaque type that does not
    /// borrow (`types.opaque.borrows.body`).
    fn open(self, ls: &Layouts<'_>, hidden: Ty) -> DepSig {
        let n = ls.of(hidden).slots.len();
        let outs = line_up(ls, self.ret, hidden)
            .and_then(|keys| {
                (keys.into_iter())
                    .map(|k| match k {
                        Key::Decl(j) => self.outs.get(j).copied(),
                        _ => Some(None),
                    })
                    .collect()
            })
            .unwrap_or_else(|| vec![None; n]);
        DepSig {
            outs,
            ret: hidden,
            ..self
        }
    }

    /// This trait method's map over an implementation method's concrete
    /// `Self` shape (`ref.lifetime.trait.impl-target`). Trait lifetime `k` takes
    /// the implementation lifetime written as its `k`th argument. Slots
    /// which the trait's abstract `Self` gains from the implementation
    /// target take that target slot's lifetime. A result slot of a
    /// projection of `Self`, as `Self::Out`, takes the implementation
    /// result's lifetime `impl_outs` when the target has it: the impl's
    /// associated type may name the impl's lifetimes (`ref.dep.trait`).
    fn align_trait_impl(
        self,
        ls: &Layouts<'_>,
        params: &[(hd_base::Symbol, Ty)],
        (ret, impl_outs): (Ty, &[Option<u32>]),
        trait_: DefId,
        trait_lifetimes: &[u16],
        self_lifetimes: &[u16],
        impl_lifetime_count: usize,
    ) -> DepSig {
        let remap = |id: u32| {
            if id == 0 || id >= ANON {
                return id;
            }
            let at = id as usize - 1;
            match trait_lifetimes.get(at) {
                Some(&l) => u32::from(l),
                None => u32::try_from(impl_lifetime_count + at + 1 - trait_lifetimes.len())
                    .unwrap_or(id),
            }
        };
        // A slot a type parameter adds takes its own lifetime, past the
        // parameters' own ones, but the trait's `Self` that of the target.
        let own = ANON + w(self.params.iter().map(|(_, ids)| ids.len()).sum());
        let mut fresh: Vec<Key> = Vec::new();
        let is_self = |p: hd_types::ParamRef| p.owner == trait_ && p.index == 0;
        let of_self = |id: u32| self_lifetimes.iter().any(|&l| l != 0 && u32::from(l) == id);
        let mut align = |d: Ty,
                         t: Ty,
                         ids: &[Option<u32>],
                         outs: Option<&[Option<u32>]>|
         -> Option<Vec<Option<u32>>> {
            let target = |p: hd_types::ParamRef, s: usize| {
                let l = *self_lifetimes.get(s).filter(|_| is_self(p))?;
                (l != 0).then_some(u32::from(l))
            };
            let keys = line_up_in(ls, d, t, outs.is_some())?;
            (keys.into_iter().enumerate())
                .map(|(at, k)| match k {
                    Key::Decl(j) => Some(ids.get(j).copied()?.map(remap)),
                    Key::AllOf(p) if is_self(p) => {
                        let out = outs.and_then(|o| o.get(at).copied().flatten());
                        Some(out.filter(|&id| of_self(id)))
                    }
                    Key::AllOf(_) if outs.is_some() => None,
                    Key::None | Key::AllOf(_) => Some(None),
                    Key::Param(p, s) => Some(Some(target(p, s).unwrap_or_else(|| {
                        let at = fresh.iter().position(|&x| x == k).unwrap_or_else(|| {
                            fresh.push(k);
                            fresh.len() - 1
                        });
                        own + w(at)
                    }))),
                })
                .collect()
        };
        let params = (self.params.into_iter().zip(params))
            .map(|((decl, ids), &(_, actual))| {
                let ids: Vec<Option<u32>> = ids.into_iter().map(Some).collect();
                match align(decl, actual, &ids, None) {
                    Some(out) => (
                        actual,
                        out.into_iter().map(|id| id.unwrap_or(NO_SOURCE)).collect(),
                    ),
                    None => (decl, Vec::new()),
                }
            })
            .collect();
        let outs = align(self.ret, ret, &self.outs, Some(impl_outs)).unwrap_or_default();
        DepSig {
            params,
            outs,
            ret,
            ..self
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

    /// The lifetimes of the result's top slots ([`Layout`]), each a set:
    /// one lifetime per slot, or for another shape every lifetime per
    /// slot of `t`.
    fn result_ids(&self, ls: &Layouts<'_>, t: Ty) -> Vec<Vec<u32>> {
        let (have, want) = (ls.of(t), ls.of(self.ret));
        if have.tops.len() == want.tops.len() && !have.tops.is_empty() {
            return (want.tops.iter())
                .map(|&r| self.outs.get(r).copied().flatten().into_iter().collect())
                .collect();
        }
        vec![self.all_ids(); have.width]
    }

    /// Whether the result slot of lifetime `id` may depend on `o`: a
    /// parameter slot of that lifetime, or a reference slot behind one,
    /// whose referent outlives it (`ref.dep.return`).
    fn allows(&self, ls: &Layouts<'_>, o: Origin, id: u32) -> bool {
        let Origin::Param(p, s) = o else {
            return false;
        };
        let Some((t, ids)) = self.params.get(p as usize) else {
            return false;
        };
        if id == ANY {
            return true;
        }
        let s = s as usize;
        if ids.get(s) == Some(&id) {
            return true;
        }
        let all = ls.of(*t);
        (0..s.min(all.slots.len())).any(|s0| ids.get(s0) == Some(&id) && s < all.slots[s0].end)
    }
}

/// The lifetimes of the slots of a declared type `t`
/// ([`Layout`]): a reference slot's from `written` by its place among
/// the reference slots, counted by `ref_at`, or its own `ANON` one by its
/// place among all slots, counted by `dep_at`; a capture slot's its own.
fn slot_ids(
    ls: &Layouts<'_>,
    t: Ty,
    (ref_at, dep_at): (&mut usize, &mut usize),
    held: &dyn Fn() -> Option<u32>,
    written: impl Fn(usize) -> Option<u32>,
) -> Vec<u32> {
    let ids = (ls.of(t).slots.iter().enumerate())
        .map(|(k, s)| {
            let own = ANON + w(*dep_at + k);
            if s.hold {
                return held().unwrap_or(own);
            }
            if s.fun {
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
fn ref_ids_of<T: Copy>(ls: &Layouts<'_>, t: Ty, ids: &[T]) -> Vec<T> {
    (ls.of(t).slots.iter())
        .zip(ids)
        .filter(|(s, _)| !s.fun)
        .map(|(_, &id)| id)
        .collect()
}

/// What a suspension in a result depends on (`ref.dep.suspension.result`):
/// the parameters' one lifetime, or every parameter under two or more.
fn hold_id(params: &[(Ty, Vec<u32>)]) -> Option<u32> {
    match one_id(params.iter().flat_map(|(_, ids)| ids.iter().copied())) {
        Elided::Lifetime(id) => Some(id),
        Elided::Missing => None,
        Elided::Ambiguous => Some(ANY),
    }
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

/// Which slot of a declared type a slot of its instance is ([`line_up`]).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Key {
    /// The declared type's slot `k`.
    Decl(usize),
    /// Slot `s` of what type parameter `p` stands for, a function type's
    /// capture slot included.
    Param(hd_types::ParamRef, usize),
    /// A slot of a projection of type parameter `p`, as `P::Out`: any slot
    /// of what `p` stands for ([`line_up_call`] only).
    AllOf(hd_types::ParamRef),
    /// A slot of an opaque type that does not borrow.
    None,
}

/// Lines the instantiated type `t` up with the declared `d`: the [`Key`]
/// of each slot of `t`, in [`Layout`] order. `None` where the shapes differ.
fn line_up(ls: &Layouts<'_>, d: Ty, t: Ty) -> Option<Vec<Key>> {
    line_up_in(ls, d, t, false)
}

/// [`line_up`] for a call's argument or result (`ref.generic.call`): a
/// projection of `T`, as `T::Out`, takes [`Key::AllOf`].
fn line_up_call(ls: &Layouts<'_>, d: Ty, t: Ty) -> Option<Vec<Key>> {
    line_up_in(ls, d, t, true)
}

fn line_up_in(ls: &Layouts<'_>, d: Ty, t: Ty, call: bool) -> Option<Vec<Key>> {
    /// The next declared slot, and the keys so far.
    type St = (usize, Vec<Key>, bool);
    fn walk(ls: &Layouts<'_>, d: Ty, t: Ty, st: &mut St, depth: u32) -> bool {
        let (pool, d1) = (ls.pool, depth + 1);
        let list = |a: hd_types::TyList, b: hd_types::TyList, st: &mut St| {
            let (a, b) = (pool.list_items(a), pool.list_items(b));
            a.len() == b.len() && a.iter().zip(b).all(|(&x, &y)| walk(ls, x, y, st, d1))
        };
        let param = |t: Ty| match pool.get(referent_fn(pool, t)) {
            TyData::Param(p) => Some(p),
            _ => None,
        };
        let decl = |st: &mut St, n: usize| {
            st.1.extend(std::iter::repeat_n(Key::Decl(st.0), n));
            st.0 += 1;
        };
        match (pool.get(d), pool.get(t)) {
            _ if depth > 32 => false,
            (TyData::Param(p), _) => {
                st.1.extend((0..ls.of(t).slots.len()).map(|s| Key::Param(p, s)));
                true
            }
            // A function value's one capture slot (`fn.capture.local-parameter`).
            (TyData::Fn { .. }, TyData::Fn { .. }) => {
                decl(st, 1);
                true
            }
            // Each slot of an opaque type's hidden type, or of its instance,
            // takes the opaque type's lifetime (`types.opaque.borrows`).
            (TyData::Opaque { borrows: true, .. }, _) => {
                decl(st, ls.of(t).slots.len());
                true
            }
            (TyData::Opaque { .. }, _) => {
                st.1.extend(std::iter::repeat_n(Key::None, ls.of(t).slots.len()));
                true
            }
            (TyData::Assoc { self_ty, .. }, _) if st.2 && param(self_ty).is_some() => {
                let p = param(self_ty).expect("a type parameter");
                st.1.extend(std::iter::repeat_n(Key::AllOf(p), ls.of(t).slots.len()));
                true
            }
            (TyData::Mut(a), TyData::Mut(b)) => walk(ls, a, b, st, d1),
            (TyData::Mut(a), _) => walk(ls, a, t, st, d1),
            (_, TyData::Mut(b)) => walk(ls, d, b, st, d1),
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
                decl(st, 1);
                walk(ls, a, b, st, d1)
            }
            (TyData::Ref { counted: true, .. }, TyData::Ref { counted: true, .. }) => true,
            (TyData::Option(a), TyData::Option(b)) => walk(ls, a, b, st, d1),
            (TyData::Tuple { elems: a, rest: ra }, TyData::Tuple { elems: b, rest: rb }) => {
                list(a, b, st)
                    && match (ra, rb) {
                        (Some(x), Some(y)) => walk(ls, x, y, st, d1),
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
            ) if x == y => list(a, b, st),
            _ => ls.of(d).slots.is_empty() && ls.of(t).slots.is_empty(),
        }
    }
    let mut st = (0, Vec::new(), call);
    walk(ls, d, t, &mut st, 0).then_some(st.1)
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

/// One origin slot of a type ([`Layout`]).
#[derive(Clone, Copy)]
struct Slot {
    /// A local reference type, a borrowing opaque type, or a function
    /// type, whose slot holds what its closure captures.
    ty: Ty,
    /// Whether it is a function type's capture slot, or a suspension's.
    fun: bool,
    /// Whether it is a suspension's slot, which holds what its call's
    /// arguments hold (`ref.dep.suspension`).
    hold: bool,
    /// Whether it is at the top, outside another reference.
    top: bool,
    /// One past the last slot behind it.
    end: usize,
    /// Whether it is a function type written inside another type, which
    /// holds no captured origins (`ref.generic.outlives.written`): not the
    /// type itself nor the referent of one of its references.
    written: bool,
}

/// The origin slots of a type in pre-order (`ref.dep.slot`): each
/// reference type outside function types and counted references, as in
/// [`Types::ref_slots`], each borrowing opaque type, each function type
/// outside another function type, and each suspension after its
/// result's slots, as a struct's lifetime. [`Layouts`] makes each once.
struct Layout {
    slots: Vec<Slot>,
    /// The top slots.
    tops: Vec<usize>,
    /// How many origin sets a value of the type has: one per top slot, or
    /// one for a trait value with none; 0 when it may hold no origins.
    width: usize,
}

impl Layout {
    fn new(pool: Types<'_>, suspend: DefId, t: Ty) -> Layout {
        let mut slots = Vec::new();
        Self::walk(pool, suspend, t, true, 0, &mut slots);
        for k in 0..slots.len() {
            let fun = slots[k].fun && !slots[k].hold;
            slots[k].written = fun
                && !(k == 0 && capture_slot(pool, t))
                && !(k > 0 && capture_slot(pool, referent(pool, slots[k - 1].ty)));
        }
        let tops: Vec<usize> = (0..slots.len()).filter(|&k| slots[k].top).collect();
        // A trait value may hold a local reference (`trait.dyn.ref.convert`).
        let holds = holds_local_ref(pool, t) || is_dyn(pool, t);
        let width = if tops.is_empty() {
            usize::from(holds)
        } else {
            tops.len()
        };
        Layout { slots, tops, width }
    }

    /// Pushes the slots of `t` onto `out`. Any type may hold a suspension,
    /// so only depth stops the walk.
    fn walk(pool: Types<'_>, suspend: DefId, t: Ty, top: bool, depth: u32, out: &mut Vec<Slot>) {
        if depth > 32 {
            return;
        }
        let d = depth + 1;
        let walk = |t: Ty, top: bool, out: &mut Vec<Slot>| {
            Self::walk(pool, suspend, t, top, d, out);
        };
        let hold = matches!(pool.get(t), TyData::Adt { def, .. } if def == suspend);
        let slot = Slot {
            ty: t,
            fun: hold || matches!(pool.get(t), TyData::Fn { .. }),
            hold,
            top,
            end: out.len() + 1,
            written: false,
        };
        match pool.get(t) {
            TyData::Ref {
                counted: false,
                inner,
                ..
            } => {
                let k = out.len();
                out.push(slot);
                walk(inner, false, out);
                out[k].end = out.len();
            }
            TyData::Opaque { borrows: true, .. } | TyData::Fn { .. } => out.push(slot),
            TyData::Mut(i) | TyData::Option(i) => walk(i, top, out),
            TyData::Tuple { elems, rest } => {
                for &e in pool.list_items(elems) {
                    walk(e, top, out);
                }
                if let Some(r) = rest {
                    walk(r, top, out);
                }
            }
            TyData::Adt { args, .. } | TyData::TraitValue { args, .. } => {
                for &a in pool.list_items(args) {
                    walk(a, top, out);
                }
                if hold {
                    out.push(Slot {
                        end: out.len() + 1,
                        ..slot
                    });
                }
            }
            _ => {}
        }
    }

    /// Each top slot's run: itself and the slots behind it, `[start, end)`.
    fn runs(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.tops.iter().map(|&k| (k, self.slots[k].end))
    }
}

/// A field's slot that is no one slot of its instance ([`FieldMap`]).
const ALL_SLOTS: usize = usize::MAX;

/// Where a field's slots lie in an instance's: its type, and for each of
/// its slots the instance's slot it is, a type argument's or the one of
/// the lifetime it names, or [`ALL_SLOTS`]. `None` where its top slots do
/// not line up with the instance's, and each takes all of them.
struct FieldMap {
    ty: Ty,
    at: Option<Vec<usize>>,
}

/// The slot layouts of the types one check meets, and the slot maps of
/// the fields it projects, each made once.
struct Layouts<'a> {
    pool: Types<'a>,
    /// `Suspend`'s definition.
    suspend: DefId,
    types: RefCell<HashMap<Ty, Rc<Layout>>>,
    fields: RefCell<HashMap<(Ty, u32, u32), Option<FieldMap>>>,
}

impl<'a> Layouts<'a> {
    fn new(pool: Types<'a>, suspend: DefId) -> Self {
        Layouts {
            pool,
            suspend,
            types: RefCell::default(),
            fields: RefCell::default(),
        }
    }

    fn of(&self, t: Ty) -> Rc<Layout> {
        if let Some(l) = self.types.borrow().get(&t) {
            return Rc::clone(l);
        }
        let l = Rc::new(Layout::new(self.pool, self.suspend, t));
        self.types.borrow_mut().insert(t, Rc::clone(&l));
        l
    }

    /// Whether a value of type `t` may have origins: it holds a local
    /// reference or a function type's capture slot, or it is a trait value.
    fn holds(&self, t: Ty) -> bool {
        self.of(t).width > 0
    }

    /// Field `j`'s type and its [`FieldMap`] slots, made once.
    fn field_slots(
        &self,
        cx: &BodyCx<'_>,
        t: Ty,
        variant: Option<u32>,
        j: u32,
    ) -> Option<(Ty, Option<Vec<usize>>)> {
        let key = (t, variant.unwrap_or(NONE), j);
        if !self.fields.borrow().contains_key(&key) {
            let m = (self.field_map(cx, t, variant, j)).map(|(ty, at)| FieldMap { ty, at });
            self.fields.borrow_mut().insert(key, m);
        }
        let fields = self.fields.borrow();
        let m = fields.get(&key)?.as_ref()?;
        Some((m.ty, m.at.clone()))
    }

    /// The type of field `j` and where its slots lie ([`FieldMap`]).
    /// A field's `lifetimes` by slot of `f`, a suspension's from past the
    /// reference slots' (`ref.dep.suspension.lifetime`); as written when
    /// it names none or `f` has a capture slot.
    fn slot_lifetimes(&self, f: Ty, lifetimes: &[u16]) -> Vec<u16> {
        let (refs, decl) = (self.pool.ref_slots(f), self.of(f));
        if lifetimes.len() <= refs || decl.slots.iter().any(|s| s.fun && !s.hold) {
            return lifetimes.to_vec();
        }
        let (mut r, mut h) = (0, refs);
        let next = |k: &mut usize| {
            *k += 1;
            lifetimes.get(*k - 1).copied().unwrap_or(0)
        };
        (decl.slots.iter())
            .map(|s| if s.hold { next(&mut h) } else { next(&mut r) })
            .collect()
    }

    fn field_map(
        &self,
        cx: &BodyCx<'_>,
        t: Ty,
        variant: Option<u32>,
        j: u32,
    ) -> Option<(Ty, Option<Vec<usize>>)> {
        let pool = self.pool;
        let len = |t: Ty| self.of(t).slots.len();
        let run = |start: usize, t: Ty| Some((start..start + len(t)).collect());
        match (pool.get(t), variant) {
            (TyData::Mut(i), _) => self.field_map(cx, i, variant, j),
            (TyData::Tuple { elems, .. }, None) => {
                let es = pool.list_items(elems);
                let e = *es.get(j as usize)?;
                let start = es[..j as usize].iter().map(|&x| len(x)).sum();
                Some((e, run(start, e)))
            }
            (TyData::Option(i), Some(1)) if j == 0 => Some((i, run(0, i))),
            (TyData::Adt { def, args }, _) => {
                let item = cx.lookup.item(def)?;
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
                let ty = subst_owner(pool, def, argv, f);
                let start = |a: usize| argv[..a].iter().map(|&x| len(x)).sum::<usize>();
                let lts: Vec<usize> = (item.generics.iter().enumerate())
                    .filter(|(_, g)| g.lifetime)
                    .map(|(g, _)| g)
                    .collect();
                // The slot of the `l`th lifetime: its argument's one top slot.
                let lifetime = |l: u16| {
                    let g = *lts.get(usize::from(l).checked_sub(1)?)?;
                    match self.of(*argv.get(g)?).tops.as_slice() {
                        [one] => Some(start(g) + one),
                        _ => None,
                    }
                };
                let at = match pool.get(f) {
                    // `ref.generic.outlives.position`: a field of a type
                    // parameter bounded by a lifetime lies in that
                    // lifetime's position.
                    TyData::Param(_)
                        if let Some(l) = outlives_field(pool, def, &item.generics, f) =>
                    {
                        lifetime(l).map(|s| vec![s; len(ty)])
                    }
                    TyData::Param(p) if p.owner == def && (p.index as usize) < argv.len() => {
                        run(start(p.index as usize), ty)
                    }
                    // `ref.lifetime.struct.read`: each slot of a field lies
                    // in the position of the lifetime it names, so a field
                    // of several lifetimes, such as a nested `Two['b, 'a]`,
                    // maps each slot on its own (`ref.lifetime.store.field`).
                    // A slot a type argument adds behind a reference is
                    // no one slot.
                    _ => {
                        let (decl, inst) = (self.of(f), self.of(ty));
                        let lts = self.slot_lifetimes(f, &field.lifetimes);
                        let mut at = vec![ALL_SLOTS; inst.slots.len()];
                        let aligned = decl.slots.len() == lts.len()
                            && decl.tops.len() == inst.tops.len()
                            && !decl.tops.is_empty();
                        if aligned && decl.slots.len() == inst.slots.len() {
                            for (k, &l) in lts.iter().enumerate() {
                                at[k] = lifetime(l).unwrap_or(ALL_SLOTS);
                            }
                        }
                        let mut known = aligned;
                        for (&d, &k) in decl.tops.iter().zip(&inst.tops) {
                            match lifetime(lts.get(d).copied().unwrap_or(0)) {
                                Some(s) => at[k] = s,
                                None => known = false,
                            }
                        }
                        known.then_some(at)
                    }
                };
                Some((ty, at))
            }
            _ => None,
        }
    }
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

// The messages of `local-reference-escapes` for a store into a place the
// caller owns and for each destination of a captured function value.
const BOX_MSG: &str = "a local reference captured by this function value never escapes to a box";
const STORE_FN_MSG: &str = "a local reference captured by this function value never escapes to a field, a top-level binding, or a place the caller owns";
const STATIC_MSG: &str = "a local reference captured by this function value never escapes to a type parameter bounded by `'static`";
const FN_PARAM_MSG: &str =
    "a local reference captured by this function value may be passed only to a `fn(...)` parameter";
const FIELD_MSG: &str = "a local reference captured by this function value never escapes to a field or collection element";
const ELEM_MSG: &str =
    "a local reference captured by this function value never escapes to a collection element";
const PROVIDER_MSG: &str =
    "a local reference captured by this function value never escapes to a provider";
const HOST_MSG: &str =
    "a local reference captured by this function value never escapes through a host call";
const HELD_MSG: &str = "a suspension that holds a local reference never escapes to a box, a provider, a host call, a top-level binding, or a field that names no lifetime for it, as `Suspend[R, 'a]` does";
const CALLER_PLACE_MSG: &str = "a local reference never escapes to a place the caller owns, unless the type of that place names the reference's lifetime";

/// A callee variable of a call (`ref.dep.call`): a callee lifetime, a slot
/// of what a type parameter stands for (`ref.generic.call`), or what a
/// suspension holds: every argument but a counted reference the call
/// lends (`ref.dep.suspension`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Var {
    Life(u32),
    Param(hd_types::ParamRef, usize),
    Held,
}

/// An argument slot's variable, and its check if the callee may keep it.
type ArgSlot = (Var, Option<Category>);

/// A call's map over the types it instantiates (`ref.generic.call`).
struct CallSig {
    /// Each parameter's type, its argument's where the two line up, and
    /// for each of its slots the variable it takes and, for a capture slot
    /// the call may keep, the category of its check.
    params: Vec<(Ty, Vec<ArgSlot>)>,
    /// Each result slot's variables.
    outs: Vec<Vec<Var>>,
    /// The elided lifetimes, for a result with no slots.
    elided: Vec<Var>,
    /// Whether the result takes the variables.
    result: bool,
    /// Whether the call may store through a `&mut` argument
    /// (`ref.generic.store`, `ref.lifetime.store.call`).
    store: bool,
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
    /// The shared `Ref` at an instruction of a place a local's path
    /// reaches, for the conflict check only (`ref.excl.shared`).
    Site(u32),
}

/// An instruction's, a local's or a parameter's index as a word.
fn w(i: usize) -> u32 {
    u32::try_from(i).expect("an index")
}

/// An origin set, sorted.
type Set = Vec<Origin>;

/// What the constraint generator reads of one body besides its TIR.
struct Inputs<'a> {
    b: &'a Body,
    pool: Types<'a>,
    ls: &'a Layouts<'a>,
    cx: &'a BodyCx<'a>,
    /// The maps of the calls that pass or return origins.
    calls: &'a HashMap<u32, CallSig>,
    /// Each parameter's type, which `Origin::Param` slots index.
    params: Vec<Ty>,
    /// The function's own map, whose parameters' lifetimes decide what a
    /// place the caller owns may take (`ref.lifetime.store`).
    own: Option<&'a DepSig>,
    /// The references `box(e)` makes.
    boxes: &'a HashSet<u32>,
    /// Locals whose closure capture is already `local-reference-escapes`
    /// (`ref.local.escape.capture-stored`): they add no capture origins.
    reported: &'a HashSet<hd_base::LocalId>,
}

impl Inputs<'_> {
    fn ty(&self, r: u32) -> Option<Ty> {
        (r & hd_tir::ir::Ref::CONST_BIT == 0)
            .then(|| self.b.ty.get(r as usize).copied())
            .flatten()
    }
}

/// The type of argument `a` as a source argument. The address a call
/// takes for a `&self` or `&mut self` receiver is counted, but the call
/// lends it as a local reference, so it has the type of the explicit `&x`
/// or `&mut x` it stands for (`fn.method.auto-address`,
/// `ref.dep.origin.place`).
fn arg_ty(pool: Types<'_>, b: &Body, a: u32) -> Option<Ty> {
    if a & hd_tir::ir::Ref::CONST_BIT != 0 {
        return None;
    }
    let t = *b.ty.get(a as usize)?;
    if b.tags[a as usize] == Tag::Ref
        && b.data[a as usize][1] & hd_tir::ir::ref_word::LOCAL != 0
        && let TyData::Ref {
            inner,
            mutable,
            counted: true,
        } = pool.get(t)
    {
        return Some(pool.intern_ty(&TyData::Ref {
            inner,
            mutable,
            counted: false,
        }));
    }
    Some(t)
}

/// The type of argument `a` for a parameter of type `decl`: its source
/// type, but the counted address a `&self` call takes of a temporary is
/// the local reference `decl` names, as a place's is (`ref.dep.temporary`),
/// and a trait value given for `&Self` or `&T` is lent as a reference to
/// it, its own slot and its lifetime arguments' apart (`ref.dep.trait`).
fn call_arg_ty(pool: Types<'_>, b: &Body, a: u32, decl: Ty) -> Option<Ty> {
    let t = arg_ty(pool, b, a)?;
    if let TyData::Ref {
        counted: false,
        mutable,
        ..
    } = pool.get(referent_fn(pool, decl))
        && is_dyn(pool, t)
    {
        return Some(pool.intern_ty(&TyData::Ref {
            inner: t,
            mutable,
            counted: false,
        }));
    }
    if b.tags[a as usize] == Tag::Ref
        && let TyData::Ref {
            inner,
            mutable,
            counted: true,
        } = pool.get(t)
        && let TyData::Ref { counted: false, .. } = pool.get(referent_fn(pool, decl))
    {
        return Some(pool.intern_ty(&TyData::Ref {
            inner,
            mutable,
            counted: false,
        }));
    }
    Some(t)
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
        let ls = &Layouts::new(pool, self.cx.names.known.suspend);
        DepSig::new(ls, &sig.params, is_self, sig.ret, &sig.lifetimes).to_fn(ls, skip)
    }

    /// The map a trait implementation's method `sig` must share with the
    /// trait's `tsig` (`trait.impl.lifetime`): the trait's, with its opaque
    /// types opened over the implementation's result, which may take
    /// their place (`trait.opaque.impl-result`).
    pub(crate) fn trait_fn_deps(
        &self,
        tsig: &hd_resolve::FnSig,
        sig: &hd_resolve::FnSig,
        trait_: DefId,
        trait_lifetimes: &[u16],
        self_lifetimes: &[u16],
        impl_lifetime_count: usize,
    ) -> hd_types::FnDeps {
        let pool = self.pool();
        let names = &self.cx.names;
        let is_self = |s| names.text(s) == "self";
        let ls = &Layouts::new(pool, self.cx.names.known.suspend);
        let dep = DepSig::new(ls, &tsig.params, is_self, tsig.ret, &tsig.lifetimes);
        let dep = if pool.has_opaque(tsig.ret) && pool.ref_slots(tsig.ret) != 0 {
            dep.open(ls, sig.ret)
        } else {
            dep
        };
        let impl_outs = DepSig::new(ls, &sig.params, is_self, sig.ret, &sig.lifetimes).outs;
        dep.align_trait_impl(
            ls,
            &sig.params,
            (sig.ret, &impl_outs),
            trait_,
            trait_lifetimes,
            self_lifetimes,
            impl_lifetime_count,
        )
        .to_fn(ls, 0)
    }

    /// The map of call `i` of `b` when it passes or returns a value that
    /// may hold origins ([`CallSig`]): from its callee's signature, a trait
    /// method's from the trait's (`ref.dep.trait`), or the map a function
    /// value's type carries (`fn.type.lifetime.call`). A parameter of a
    /// bare type parameter bounded by a lifetime puts its slots in that
    /// lifetime (`ref.generic.outlives.argument`). A result slot a type
    /// parameter adds takes what that parameter's argument slots took, and
    /// nothing where no argument slot has it (`ref.generic.call`).
    fn call_sig(&self, ls: &Layouts<'_>, b: &Body, i: usize) -> Option<CallSig> {
        let (pool, tag, names) = (ls.pool, b.tags[i], &self.cx.names);
        let (mut holds, mut held) = (ls.holds(b.ty[i]), false);
        let receiver = (tag == Tag::CallDyn).then(|| b.data[i][0]);
        slots(b, i, |_, v, word| {
            let arg = matches!(word, Word::Value(Role::Arg(_)));
            if (arg || receiver == Some(v)) && arg_ty(pool, b, v).is_some_and(|t| ls.holds(t)) {
                holds = true;
                held |= arg && ls.holds(b.ty[v as usize]);
            }
        });
        if !holds {
            return None;
        }
        let (def, trait_, self_slots) = match tag {
            Tag::Call | Tag::Await => match Callee::from_words(b.record(b.data[i][0]))? {
                Callee::Item { def, .. } => (Some(def), None, Vec::new()),
                Callee::TraitMethod {
                    trait_,
                    method,
                    self_ty,
                    choice,
                    ..
                } => {
                    let slots = match choice {
                        (ChoiceKind::Impl, imp) => self.impl_trait_self_slots(DefId::from_raw(imp)),
                        (ChoiceKind::TraitValue, _) => {
                            self.trait_value_self_slots(ls, trait_, self_ty)
                        }
                        _ => Vec::new(),
                    };
                    (Some(method), Some(trait_), slots)
                }
            },
            Tag::CallDyn => {
                let method = DefId::from_raw(*b.record(b.data[i][1]).first()?);
                let Some(ItemData::Method { owner, .. }) =
                    self.cx.lookup.item(method).map(|m| &m.data)
                else {
                    return None;
                };
                let t = source_arg(b, i, 0).and_then(|a| arg_ty(pool, b, a))?;
                (
                    Some(method),
                    Some(*owner),
                    self.trait_value_self_slots(ls, *owner, t),
                )
            }
            Tag::CallValue => (None, None, Vec::new()),
            _ => return None,
        };
        let sig = match def {
            Some(def) => Some(self.sig_of(def).ok()?),
            None => None,
        };
        let dep = match &sig {
            Some(sig) => {
                let is_self = |s| names.text(s) == "self";
                DepSig::new(ls, &sig.params, is_self, sig.ret, &sig.lifetimes)
            }
            None => DepSig::of_fn(ls, *b.ty.get(b.data[i][0] as usize)?)?,
        };
        // A slot of the trait's `Self` that the implementation target's
        // lifetime fills (`ref.lifetime.trait`).
        let target = |p: hd_types::ParamRef, s: usize| {
            trait_.filter(|&tr| p.owner == tr && p.index == 0)?;
            self_slots.get(s).copied().flatten()
        };
        // Where a capture slot a type parameter adds is kept.
        let declared = def.and_then(|d| Some((d, self.cx.lookup.item(d)?.sig()?)));
        let static_bound = names.known.static_bound;
        let dest = |p: hd_types::ParamRef| {
            let (def, declared) = declared?;
            let generics = if p.owner == def {
                Some(&declared.generics)
            } else {
                self.cx.lookup.item(p.owner).map(|it| &it.generics)
            };
            let g = generics.and_then(|g| g.get(usize::from(p.index)));
            if g.is_some_and(|g| static_bounded(pool, static_bound, g)) {
                Some(Category::StaticParam)
            } else if scoped_param(pool, declared, p) {
                None
            } else {
                Some(Category::Forward)
            }
        };
        // The variables each slot a type parameter adds to the parameters
        // took, which its result slots take (`ref.generic.call`).
        let mut took: Vec<(Key, Var)> = Vec::new();
        let mut params = Vec::with_capacity(dep.params.len());
        for (p, (decl, ids)) in dep.params.iter().enumerate() {
            let at = source_arg(b, i, p).and_then(|a| call_arg_ty(pool, b, a, *decl));
            let (t, keys) = at
                .and_then(|t| Some((t, line_up_call(ls, *decl, t)?)))
                .unwrap_or_else(|| (*decl, (0..ids.len()).map(Key::Decl).collect()));
            let outlives = (def.zip(sig.as_ref())).and_then(|(d, sig)| {
                outlives_field(pool, d, &sig.generics, referent_fn(pool, *decl))
            });
            let written = ls.of(*decl);
            let slots = (keys.into_iter().zip(&ls.of(t).slots))
                .map(|(k, s)| {
                    let var = match k {
                        Key::Decl(j) => Var::Life(ids.get(j).copied().unwrap_or(NO_SOURCE)),
                        Key::None | Key::AllOf(_) => Var::Life(NO_SOURCE),
                        Key::Param(q, s) => (target(q, s).or(outlives.map(u32::from)))
                            .map_or(Var::Param(q, s), Var::Life),
                    };
                    if let Key::Param(..) = k {
                        took.push((k, var));
                    }
                    let cat = match k {
                        _ if !s.fun || s.hold => None,
                        Key::Param(q, _) => dest(q),
                        Key::Decl(j) if written.slots.get(j).is_some_and(|x| !x.written) => None,
                        _ => Some(Category::FieldCapture),
                    };
                    (var, cat)
                })
                .collect();
            params.push((t, slots));
        }
        // A result slot a type parameter adds takes what that parameter's
        // argument slots took, and a projection of it all of them.
        let from = |want: &dyn Fn(Key) -> bool| -> Vec<Var> {
            let mut vs: Vec<Var> = (took.iter()).filter(|x| want(x.0)).map(|x| x.1).collect();
            vs.dedup();
            vs
        };
        let out = |k: Key| match k {
            Key::Decl(j) => (dep.outs.get(j).copied().flatten())
                .map(Var::Life)
                .into_iter()
                .collect(),
            Key::None => Vec::new(),
            Key::Param(q, j) => match target(q, j) {
                Some(l) => vec![Var::Life(l)],
                None => from(&|x| x == k),
            },
            Key::AllOf(q) => from(&|x| matches!(x, Key::Param(p, _) if p == q)),
        };
        // A suspension, the result of a bang call made without `!`, holds
        // what its arguments hold in a slot of its own, and its result's
        // slots depend as the call's with `!` would (`ref.dep.suspension`).
        let output = match pool.get(referent_fn(pool, b.ty[i])) {
            TyData::Adt { def, args } if def == ls.suspend => {
                pool.list_items(args).first().copied()
            }
            _ => None,
        };
        let lined = (line_up_call(ls, dep.ret, b.ty[i]).map(|keys| (None, keys)))
            .or_else(|| Some((Some(vec![Var::Held]), line_up_call(ls, dep.ret, output?)?)));
        let outs: Vec<Vec<Var>> = match lined {
            Some((held, keys)) => keys.into_iter().map(out).chain(held).collect(),
            None => vec![vec![Var::Held]; ls.of(b.ty[i]).slots.len()],
        };
        let lifetime = |e: Elided| match e {
            Elided::Lifetime(l) => Some(Var::Life(l)),
            _ => None,
        };
        let depends = outs.iter().any(|o| !o.is_empty()) || lifetime(dep.elided).is_some();
        // A call may store an argument's origins through a `&mut` argument
        // of a type parameter or of a written lifetime.
        let writes = sig.as_ref().is_some_and(|sig| {
            sig.params.iter().any(|&(_, t)| {
                ls.of(t).slots.iter().any(|x| {
                    x.top
                        && matches!(pool.get(x.ty), TyData::Ref { mutable: true, inner, .. }
                        if pool.has_param(inner) || pool.ref_slots(inner) > 0)
                })
            })
        });
        let shared = (params.iter().flat_map(|(_, x)| x)).any(|&(v, _)| match v {
            Var::Life(l) => l < ANON,
            _ => true,
        });
        Some(CallSig {
            store: matches!(tag, Tag::Call | Tag::Await) && held && writes && shared,
            result: ls.holds(b.ty[i]) && (tag == Tag::CallValue || depends),
            elided: [dep.elided, dep.capture_elided]
                .map(lifetime)
                .into_iter()
                .flatten()
                .collect(),
            params,
            outs,
        })
    }

    /// For each reference slot an implementation gives the trait's
    /// abstract `Self`, the trait lifetime it instantiates there.
    fn impl_trait_self_slots(&self, imp: DefId) -> Vec<Option<u32>> {
        let Some(ItemData::Impl {
            trait_lifetimes: t,
            self_lifetimes,
            ..
        }) = self.cx.lookup.item(imp).map(|i| &i.data)
        else {
            return Vec::new();
        };
        let at = |l: &u16| t.iter().position(|x| x == l && *l != 0).map(|k| w(k + 1));
        self_lifetimes.iter().map(at).collect()
    }

    /// The hidden lifetime-argument slots of a trait value, numbered as
    /// the trait declares them (`ref.lifetime.trait`).
    fn trait_value_self_slots(&self, ls: &Layouts<'_>, tr: DefId, mut t: Ty) -> Vec<Option<u32>> {
        let pool = ls.pool;
        while let TyData::Ref { inner, .. } | TyData::Mut(inner) = pool.get(t) {
            t = inner;
        }
        let (TyData::TraitValue { def, args, .. }, Some(item)) =
            (pool.get(t), self.cx.lookup.item(tr))
        else {
            return Vec::new();
        };
        let mut lifetime = 0_u32;
        let slots = (pool.list_items(args).iter().zip(&item.generics)).flat_map(|(&arg, g)| {
            lifetime += u32::from(g.lifetime);
            std::iter::repeat_n(g.lifetime.then_some(lifetime), ls.of(arg).slots.len())
        });
        slots.filter(|_| def == tr).collect()
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
        let ls = &Layouts::new(pool, self.cx.names.known.suspend);
        let body = self.b.body();
        // The body's own map: parameter k is local k.
        let fn_body = body.kind == BodyKind::Fn && fun.is_some() && ls.holds(ret);
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
                    ls,
                    &sig.params,
                    |s| names.text(s) == "self",
                    if opaque { sig.ret } else { ret },
                    &sig.lifetimes,
                );
                Some(if opaque { dep.open(ls, ret) } else { dep })
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
            DepSig::new(ls, &sig.params, is_self, Ty::VOID, &sig.lifetimes)
        });
        let mut calls: HashMap<u32, CallSig> = HashMap::default();
        let mut closures: Vec<(usize, DepSig)> = Vec::new();
        let mut through = body.local_ty.iter().enumerate().any(|(l, &t)| {
            body.local_flags[l] & hd_tir::ir::local_flags::PARAM != 0 && capture_slot(pool, t)
        });
        for i in 0..body.tags.len() {
            if matches!(
                body.tags[i],
                Tag::Call | Tag::Await | Tag::CallDyn | Tag::CallValue
            ) && let Some(sig) = self.call_sig(ls, body, i)
            {
                // A suspension that holds an argument may be kept
                // (`ref.dep.suspension.local`).
                let held = sig.result && ls.of(body.ty[i]).slots.iter().any(|s| s.hold);
                through |= sig.store || held;
                calls.insert(w(i), sig);
            }
            match body.tags[i] {
                // A block's or a loop's value may leave a local's scope.
                Tag::Block | Tag::Loop if w(i) != root.0 && holds_local_ref(pool, body.ty[i]) => {
                    through = true;
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
                                .is_some_and(|l| ls.holds(body.local_ty[l.idx()]))
                        });
                    }
                    let dep = DepSig::of_fn(ls, body.ty[i]);
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
                            closures.push((sub, dep.open(ls, hidden)));
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
                        && body.ty.get(v as usize).is_some_and(|&t| ls.holds(t));
                }
                // A list push or a map store (`ref.dep.store`).
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
        // A direct function result with two or more `fn(...)` parameters
        // has no one capture source (`ref.dep.capture-elide.ambiguous`).
        if let (Some(o), Some(f)) = (&own, fun)
            && o.capture_elided == Elided::Ambiguous
            && (ls.of(o.ret).slots.iter()).any(|s| s.fun && !s.hold && !s.written)
        {
            self.capture_source_error(self.b.body().item, f);
        }
        // A function value in the result may carry captures with no source
        // (`ref.generic.outlives.read`) when no `fn(...)` parameter gives one.
        let own = own.filter(|o| {
            opaque
                || o.depends()
                || o.capture_elided == Elided::Missing
                    && ls.of(o.ret).tops.iter().any(|&k| ls.of(o.ret).slots[k].fun)
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
        let s = Inputs {
            b: body,
            pool,
            ls,
            cx: self.cx,
            calls: &calls,
            params,
            own: mine.as_ref(),
            boxes: &self.boxes,
            reported: &self.ref_captured,
        };
        let slots = own.as_ref().map(|o| result_slots(ls, o, ret, opaque));
        // A returned value's origins are `ref.dep.return`'s.
        let mut skip: HashSet<u32> = HashSet::default();
        if own.is_some() {
            skip.extend(returned(body, root.0));
        }
        for (sub, _) in &closures {
            skip.extend(returned(body, body.sub_root[*sub]));
        }
        skip.insert(root.0);
        let returns = constraints::Returns {
            own: own.as_ref().zip(slots.as_deref()),
            root: root.0,
            closures: &closures,
            skip: &skip,
        };
        let verdicts = constraints::solve(&s, &returns);
        for (at, code, msg) in self.verdict_list(verdicts, own.as_ref(), fun) {
            let node = self.cx.src.parse.tree.node(at);
            self.err(code, node, &msg);
        }
    }

    /// The diagnostics of `v`, one per node, in today's order: stores into
    /// a place the caller owns, captured function values, returned values,
    /// closures' returned values, then the scope check.
    fn verdict_list(
        &self,
        v: Verdicts,
        own: Option<&DepSig>,
        fun: Option<NodeRef<'_>>,
    ) -> Vec<(NodeIdx, Code, String)> {
        let body = self.b.body();
        let mut stores = v.stores;
        stores.sort_stable_by_key(|n| n.raw());
        let mut seen = HashSet::default();
        let mut out = Vec::new();
        let mut push = |at: NodeIdx, msg: &dyn Fn() -> String| {
            if at != NodeIdx::NONE && seen.insert(at) {
                out.push((at, Code::LocalReferenceEscapes, msg()));
            }
        };
        for at in stores {
            push(at, &|| CALLER_PLACE_MSG.to_owned());
        }
        for (at, msg) in v.capture {
            push(at, &|| msg.to_owned());
        }
        if let Some(own) = own {
            for (at, o, id) in v.results {
                push(at, &|| self.origin_message(own, o, id, fun));
            }
        }
        for (at, o) in v.closures {
            push(at, &|| closure_message(body, self.cx, o));
        }
        let scope = v.scope.into_iter();
        out.extend(scope.map(|(at, msg)| (at, Code::LocalReferenceOutlivesLocal, msg)));
        let conflicts = v.conflicts.into_iter();
        out.extend(conflicts.map(|(at, msg)| (at, Code::ConflictingAccess, msg)));
        out
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

    /// `ambiguous-reference-source` at the result type of `f`, or at `f`
    /// when it omits one, naming the first two `fn(...)` parameters.
    fn capture_source_error(&mut self, item: DefId, f: NodeRef<'_>) {
        let pool = self.pool();
        let sig = self.cx.lookup.item(item).and_then(|i| i.sig());
        let names: Vec<&str> = sig
            .map_or(&[][..], |s| &s.params[..])
            .iter()
            .filter(|&&(_, t)| capture_slot(pool, t))
            .map(|&(s, _)| self.cx.names.text(s))
            .take(2)
            .collect();
        let msg = match names.as_slice() {
            [a, b] => format!(
                "the result may hold what `{a}` or `{b}` captures; take them as type parameters bounded by `'static`"
            ),
            _ => "the result may hold what more than one function parameter captures; take them as type parameters bounded by `'static`".to_owned(),
        };
        self.err(
            Code::AmbiguousReferenceSource,
            Src::type_child(f).unwrap_or(f),
            &msg,
        );
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
            Origin::Hidden(_) | Origin::Site(_) => {
                format!("the value refers to a temporary of {f}")
            }
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

/// The diagnostics one solver gives a body, before their messages.
#[derive(Default)]
struct Verdicts {
    /// Stores into a place the caller owns (`ref.dep.store`).
    stores: Vec<NodeIdx>,
    /// Captured function values kept where they may not go.
    capture: Vec<(NodeIdx, &'static str)>,
    /// Returned values: the origin they may not depend on, the lifetime.
    results: Vec<(NodeIdx, Origin, u32)>,
    /// Closures' returned values and the origin.
    closures: Vec<(NodeIdx, Origin)>,
    /// `local-reference-outlives-local`.
    scope: Vec<(NodeIdx, String)>,
    /// `conflicting-access` (`ref.excl.shared`).
    conflicts: Vec<(NodeIdx, String)>,
}

/// What each top slot of a returned value of type `ret` may depend on
/// (`ref.dep.return`): its result slot's lifetimes, none for a slot of an
/// opaque type that does not borrow or a capture slot with no source
/// (`ref.generic.outlives.read`), and `None` for a slot that is not checked.
fn result_slots(ls: &Layouts<'_>, own: &DepSig, ret: Ty, opaque: bool) -> Vec<Option<Vec<u32>>> {
    let rl = ls.of(ret);
    let capture = |k: usize| {
        own.capture_elided == Elided::Missing && rl.tops.get(k).is_some_and(|&t| rl.slots[t].fun)
    };
    (own.result_ids(ls, ret).into_iter().enumerate())
        .map(|(k, ids)| (!ids.is_empty() || opaque || capture(k)).then_some(ids))
        .collect()
}

/// Whether a returned slot of lifetimes `ids` of closure `sub` may depend
/// on `o`: a slot of its own parameter of that lifetime
/// (`fn.closure.lifetime.return`).
fn closure_allows(
    ls: &Layouts<'_>,
    b: &Body,
    sub: usize,
    dep: &DepSig,
    o: Origin,
    ids: &[u32],
) -> bool {
    let Origin::Param(p, k) = o else { return false };
    let r = b.sub_params[sub];
    (0..r.len)
        .position(|j| b.extra[(r.start + 1 + j) as usize] == p)
        .is_some_and(|at| (ids.iter()).any(|&id| dep.allows(ls, Origin::Param(w(at), k), id)))
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

/// The innermost part of value `v` whose origins hold `o`, as `has`
/// says, for the diagnostic's position: an arm, a block's value, a
/// construction's part.
fn site(b: &Body, has: &dyn Fn(u32, Origin) -> bool, v: u32, o: Origin, depth: u32) -> NodeIdx {
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
                && has(w, o)
            {
                inner = Some(w);
            }
        });
    }
    match inner {
        Some(w) => {
            let at = site(b, has, w, o, depth + 1);
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
/// that outlives the iteration. `uses` gives an instruction's origins,
/// each with the local or hidden local it is read from, if any.
fn scoped_uses(
    b: &Body,
    cx: &BodyCx<'_>,
    skip: &HashSet<u32>,
    uses: &dyn Fn(u32) -> Vec<(Option<Origin>, Origin)>,
) -> Vec<(NodeIdx, String)> {
    let tree = &cx.src.parse.tree;
    let mut scopes: HashMap<Origin, Option<Scope>> = HashMap::default();
    let mut bad: Vec<(NodeIdx, Origin, Option<Origin>, bool, u32)> = Vec::new();
    for i in 0..b.tags.len() {
        let at = b.syn[i];
        if at == NodeIdx::NONE || skip.contains(&w(i)) {
            continue;
        }
        for (st, o) in uses(w(i)) {
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
