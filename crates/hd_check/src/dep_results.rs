//! Dependent results (spec/lang/13-values-and-references.md, "Dependent
//! Results"): a function's result may hold a local reference, which
//! depends on the parameters whose reference slots share its lifetime
//! ([`DepSig`]): a written lifetime variable, or the one its signature
//! elides.
//!
//! Once a body is checked, [`Ck::check_dependent_results`] gives each
//! slot of its values and locals a region (`ref.dep.position`), relates them
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
//! the slots behind a reference are its referent's, or one for the whole
//! of a referent with none that still holds origins, as `S::Item`. Where a projection or
//! a construction cannot be lined up with the slots, each slot takes the
//! union of all of them through one region, which only adds origins.

use hd_types::ANON_SUM;
use hd_types::AdtId;
use std::cell::RefCell;
use std::rc::Rc;

use hd_base::SliceSort as _;
use hd_base::{DefId, HashMap, HashSet, NodeIdx, Symbol};
use hd_diag::Code;
use hd_resolve::{ItemData, Src};
use hd_syntax::{GreenTree, NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{
    Body, BodyKind, Callee, ChoiceKind, NONE, PlaceRoot, Proj, Role, Tag, Word, slots,
};
use hd_types::solver::{Impls, PlanStep, RegionObligations, TraitRef};
use hd_types::{ParamRef, Ty, TyData, Types};

use crate::body::{BodyCx, Ck, RegionSource};
use crate::call::subst_owner;
use crate::local_refs::{holds_local_ref, outlives_field, scoped_param, static_bounded};
use crate::regions::Category;

mod constraints;
mod fn_inputs;
mod loan_shapes;

/// The lifetime of an input slot that writes none: its own, `ANON` plus
/// the slot's index. A written lifetime is its 1-based place in scope.
const ANON: u32 = 1 << 16;

/// A result slot that depends on every parameter (`types.opaque.borrows.source`,
/// `ref.dep.suspension.result`).
const ANY: u32 = u32::MAX;

/// A declared result slot with no source, while lining up.
const NO_SOURCE: u32 = u32::MAX - 1;

/// The static lifetime of a slot written `&'static T` (`ref.static.form`):
/// what flows into it must be a static source (`ref.static.source.error`).
const STATIC: u32 = hd_resolve::STATIC_LIFETIME as u32;
const STATIC_U16: u16 = hd_resolve::STATIC_LIFETIME;

/// The mark of a returned slot that names result path `k`: `PATH + k`
/// beside its lifetime (`ref.result.path.callee`).
const PATH: u32 = 1 << 30;

/// A result path of the body's own signature, resolved
/// (`ref.result.path.form`): its parameter, its fields' indices, the result's
/// layout slot, and its text as written.
pub(super) struct OwnPath {
    param: u32,
    /// The place steps from the parameter's referent: each field, and a
    /// dereference after a `*T` or `*mut T` field (`ref.result.path.counted`).
    steps: Vec<Proj>,
    slot: usize,
    text: String,
}

/// The layout slot of each result reference slot of `ret` that names a
/// field path, and its parameter (`ref.result.path.form`).
fn path_slots(ls: &Layouts<'_>, ret: Ty, paths: &[hd_resolve::ResultPath]) -> Vec<(usize, u32)> {
    let mut j = 0;
    let mut out = Vec::new();
    for (r, s) in ls.of(ret).slots.iter().enumerate() {
        if s.hold || s.fun {
            continue;
        }
        if let Some(p) = paths.iter().find(|p| usize::from(p.slot) == j) {
            out.push((r, u32::from(p.param)));
        }
        j += 1;
    }
    out
}

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

/// A signature's dependency map (`ref.dep.position`): the lifetime of each
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
    /// The lifetimes a result with no slot of its own may depend on as a
    /// whole: its projections' ([`Ck::whole_lifetimes`]).
    whole: Vec<u32>,
}

impl DepSig {
    /// A crossing parameter's closure lends its captured references for
    /// the lifetime its marker bound names (`ref.frozen.lifetime`): its
    /// capture slot takes that lifetime, as a written reference slot does.
    fn with_crossing(mut self, ls: &Layouts<'_>, crossing: &[hd_resolve::Crossing]) -> DepSig {
        for c in crossing.iter().filter(|c| c.lifetime != 0) {
            if let Some((t, ids)) = self.params.get_mut(usize::from(c.param))
                && let Some(k) = ls.of(*t).slots.iter().position(|s| s.fun)
                && let Some(id) = ids.get_mut(k)
            {
                *id = u32::from(c.lifetime);
            }
        }
        self
    }

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
                    ty: st,
                    fun,
                    hold,
                    whole,
                    ..
                } = s;
                if hold {
                    return next_held().or(held);
                }
                if whole {
                    return None;
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
            whole: Vec::new(),
        }
    }

    /// `ref.result.path.lifetime`: a result slot that names a field path
    /// takes its parameter's lifetime.
    fn with_paths(mut self, ls: &Layouts<'_>, paths: &[hd_resolve::ResultPath]) -> DepSig {
        for (r, p) in path_slots(ls, self.ret, paths) {
            let id = self
                .params
                .get(p as usize)
                .and_then(|x| x.1.first().copied());
            if let (Some(o), Some(id)) = (self.outs.get_mut(r), id) {
                *o = Some(id);
            }
        }
        self
    }

    /// The map a function type carries (`fn.type.lifetime`), for a call
    /// through a function value.
    fn of_fn(ls: &Layouts<'_>, t: Ty) -> Option<DepSig> {
        let pool = ls.pool;
        let TyData::Fn { params, result, .. } = pool.get(t) else {
            return None;
        };
        // An open map's inputs mark which write a lifetime, not which
        // share one ([`hd_types::FnDeps`]).
        let deps = pool.fn_ty_deps(t).filter(|(_, outs)| !outs.is_empty());
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
                    ty: st,
                    fun,
                    hold,
                    whole,
                    ..
                } = s;
                if hold {
                    return Some(held);
                }
                if whole {
                    return Some(None);
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
            whole: Vec::new(),
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
    /// target take that target slot's lifetime. A result slot the
    /// implementation fills in, from a trait type argument or an
    /// associated type of `Self` as `Self::Out`, is the implementation's
    /// type substituted there, so it takes the implementation result's
    /// lifetime `impl_outs` when the target has it
    /// (`ref.lifetime.impl-target.assoc`), or for a projection of `Self`
    /// when it is one of the lifetimes `proj` gives that projection
    /// (`ref.lifetime.trait.bound-argument`). A slot of a trait type
    /// argument takes the lifetime the implementation's header writes there,
    /// `arg_lifetimes` by argument and slot, as `Self`'s takes the target's
    /// (`ref.lifetime.trait.impl-argument`), and a result slot it fills
    /// may take that lifetime, or for a projection of `Self` that of a
    /// trait argument it holds.
    fn align_trait_impl(
        self,
        ls: &Layouts<'_>,
        params: &[(hd_base::Symbol, Ty)],
        (ret, impl_outs, proj): (Ty, &[Option<u32>], ProjLifetimes<'_>),
        trait_: DefId,
        (trait_lifetimes, self_lifetimes, arg_lifetimes): (&[u16], &[u16], &[Vec<u32>]),
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
        let is_self = |p: ParamRef| p.owner == trait_ && p.index == 0;
        let filled = |k: Key| match k {
            Key::AllOf(p, _) => is_self(p),
            Key::Param(p, _) => p.owner == trait_ && p.index > 0,
            _ => false,
        };
        let of_self = |id: u32| self_lifetimes.iter().any(|&l| l != 0 && u32::from(l) == id);
        let mut align = |d: Ty,
                         t: Ty,
                         ids: &[Option<u32>],
                         outs: Option<&[Option<u32>]>|
         -> Option<Vec<Option<u32>>> {
            let target = |p: ParamRef, s: usize| {
                let l = match usize::from(p.index).checked_sub(1) {
                    _ if p.owner != trait_ => return None,
                    None => u32::from(*self_lifetimes.get(s)?),
                    Some(k) => *arg_lifetimes.get(k)?.get(s)?,
                };
                (l != 0).then_some(l)
            };
            // A lifetime the header writes in a trait argument a
            // projection of `Self` holds ([`proj_holds`]).
            let held = |t: Ty, id: u32| {
                (arg_lifetimes.iter().enumerate()).any(|(k, lts)| {
                    let index = u16::try_from(k + 1).unwrap_or(u16::MAX);
                    let p = ParamRef {
                        owner: trait_,
                        index,
                    };
                    lts.contains(&id) && proj_holds(ls.pool, t, p)
                })
            };
            let keys = line_up_in(ls, d, t, outs.is_some())?;
            (keys.into_iter().enumerate())
                .map(|(at, k)| match k {
                    Key::Decl(j) => Some(ids.get(j).copied()?.map(remap)),
                    _ if outs.is_some() && filled(k) => {
                        let out = outs.and_then(|o| o.get(at).copied().flatten());
                        let named = |id: u32| match k {
                            Key::AllOf(_, t) => {
                                proj(t).into_iter().any(|l| remap(l) == id) || held(t, id)
                            }
                            Key::Param(p, s) => target(p, s) == Some(id),
                            _ => false,
                        };
                        Some(out.filter(|&id| of_self(id) || named(id)))
                    }
                    Key::AllOf(..) if outs.is_some() => None,
                    Key::None | Key::AllOf(..) => Some(None),
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
        self.outs.iter().any(Option::is_some)
            || matches!(self.elided, Elided::Lifetime(_))
            || !self.whole.is_empty()
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
        ids.extend_from_slice(&self.whole);
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
            // A whole slot writes no lifetime: [`Ck::with_wholes`] gives
            // it its referent's.
            if s.fun || s.whole {
                return own;
            }
            *ref_at += 1;
            written(*ref_at - 1).unwrap_or(own)
        })
        .collect::<Vec<_>>();
    *dep_at += ids.len();
    ids
}

/// Splits `lts`, one lifetime per reference slot of `args` in
/// [`Types::ref_slots`] order, into each argument's [`Layout`] slots; 0
/// for each slot of an argument whose layout has other slots too.
fn arg_ids(ls: &Layouts<'_>, args: &[Ty], lts: &[u32]) -> Vec<Vec<u32>> {
    let pool = ls.pool;
    let mut at = 0;
    (args.iter())
        .map(|&a| {
            let (n, lay) = (pool.ref_slots(a), ls.of(a));
            let mine = lts.get(at..at + n);
            at += n;
            let plain = lay.slots.len() == n && lay.slots.iter().all(|s| !s.fun);
            match mine {
                Some(m) if plain => m.to_vec(),
                _ => vec![0; lay.slots.len()],
            }
        })
        .collect()
}

/// A trait's `k`-th lifetime parameter's lifetime, where `ids` are the
/// slot lifetimes of its arguments: the one its argument's slot names.
fn trait_lifetimes(generics: &[hd_resolve::Generic], ids: &[Vec<u32>]) -> Vec<u32> {
    (generics.iter().enumerate())
        .filter(|(_, g)| g.lifetime)
        .map(|(k, _)| ids.get(k).and_then(|x| x.first()).copied().unwrap_or(0))
        .collect()
}

/// The lifetimes of a projection ([`Ck::proj_lifetimes`]).
type ProjLifetimes<'f> = &'f dyn Fn(Ty) -> Vec<u32>;

/// The projections a value of type `t` holds where its layout has no slot
/// for them, as `S::Item` or `List[S::Item]`, each outermost one, and whether
/// it holds a trait value or an opaque type besides. None for a type with slots of its own.
/// A trait value's and an opaque type's arguments are no part of what they
/// hold (`trait.dyn.ref.convert`, `types.opaque.borrows`). A trait's own
/// `Self::Item` lists no trait arguments, so [`holds_local_ref`] says no to
/// it, but it holds what the trait's lifetimes name all the same
/// ([`own_proj`]). Each distinct part is entered once, so each projection
/// is listed once, and a part the pool shares costs its distinct nodes,
/// not its paths (#1984); [`bare_parts`] and [`proj_holds`] walk the same
/// way.
fn wholes(ls: &Layouts<'_>, t: Ty) -> (Vec<Ty>, bool) {
    let (pool, mut out, mut other) = (ls.pool, Vec::new(), false);
    if !ls.of(t).slots.is_empty() || !pool.has_assoc(t) {
        return (out, other);
    }
    let mut seen: HashSet<Ty> = HashSet::default();
    let mut stack = vec![t];
    while let Some(t) = stack.pop() {
        if !seen.insert(t) {
            continue;
        }
        crate::work::slot_count_step();
        match pool.get(t) {
            TyData::Assoc { .. } => out.push(t),
            TyData::TraitValue { .. } | TyData::Opaque { .. } => other = true,
            _ => pool.children(t, &mut stack),
        }
    }
    (out, other)
}

/// The type parameters `t` holds outside a projection, which a type
/// argument with no slot of its own passes whole ([`Ck::call_sig`]), and
/// whether it holds a trait value or an opaque type, whose own origins a
/// result may hold besides. A projection's parameters are in what the
/// projection holds ([`proj_holds`]), and a trait value's and an opaque
/// type's arguments are no part of what they hold (`trait.dyn.ref.convert`,
/// `types.opaque.borrows`).
fn bare_parts(pool: Types<'_>, t: Ty) -> (Vec<ParamRef>, bool) {
    let (mut out, mut other) = (Vec::new(), false);
    let mut seen: HashSet<Ty> = HashSet::default();
    let mut stack = vec![t];
    while let Some(t) = stack.pop() {
        if !seen.insert(t) {
            continue;
        }
        crate::work::slot_count_step();
        match pool.get(t) {
            TyData::TraitValue { .. } | TyData::Opaque { .. } => other = true,
            _ if !pool.has_param(t) => {}
            // Each distinct parameter type is entered once, so listed once.
            TyData::Param(p) => out.push(p),
            TyData::Assoc { .. } => {}
            _ => pool.children(t, &mut stack),
        }
    }
    (out, other)
}

/// Whether a slot of what type parameter `p` stands for is part of what
/// projection `proj` holds: `p` is its base or is in its trait's
/// arguments, as Rust's projection outlives rule has it. A trait's own
/// `Self::Item` lists no arguments, and holds each of the trait's
/// parameters.
fn proj_holds(pool: Types<'_>, proj: Ty, p: ParamRef) -> bool {
    let mut seen: HashSet<Ty> = HashSet::default();
    let mut stack = vec![proj];
    while let Some(t) = stack.pop() {
        if !seen.insert(t) {
            continue;
        }
        crate::work::slot_count_step();
        match pool.get(t) {
            _ if !pool.has_param(t) => {}
            TyData::Param(x) if x == p => return true,
            TyData::Assoc {
                trait_, self_ty, ..
            } if p.owner == trait_
                && pool.get(self_ty)
                    == TyData::Param(ParamRef {
                        owner: trait_,
                        index: 0,
                    }) =>
            {
                return true;
            }
            _ => pool.children(t, &mut stack),
        }
    }
    false
}

/// The entries of `ids`, one per slot of `t`, at its reference slots.
fn ref_ids_of<T: Copy>(ls: &Layouts<'_>, t: Ty, ids: &[T]) -> Vec<T> {
    (ls.of(t).slots.iter())
        .zip(ids)
        .filter(|(s, _)| !s.fun && !s.whole)
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
    /// capture slot included; slot 0 also for the whole of one with no
    /// slot of its own, as [`Ck::call_sig`] passes it.
    Param(ParamRef, usize),
    /// A slot of projection `t` of type parameter `p`, as `P::Out`: any
    /// slot of what `p` or a type parameter in its trait's arguments
    /// stands for ([`proj_holds`]), and any lifetime the projection's
    /// bounds write ([`line_up_call`] only).
    AllOf(ParamRef, Ty),
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

/// Parts are pushed in reverse ([`hd_base::stack::all`]), so keys append
/// in order; a part that keys a slot after its own parts (a local
/// reference's whole referent, a suspension's retained origins) pushes
/// that step under them. Each distinct pair is checked once; repeated pairs
/// replay their keys with declared slots rebased to the current position.
fn line_up_in(ls: &Layouts<'_>, d: Ty, t: Ty, call: bool) -> Option<Vec<Key>> {
    line_up_with(ls, d, t, call, || {})
}

fn line_up_with(
    ls: &Layouts<'_>,
    d: Ty,
    t: Ty,
    call: bool,
    mut work: impl FnMut(),
) -> Option<Vec<Key>> {
    /// A pair to line up at a depth, or what a part does after its parts.
    enum Step {
        Pair(Ty, Ty, u32),
        Done(Ty, Ty, usize, usize, u32, u32),
        /// The whole referent of a local reference, declared and
        /// instantiated.
        Whole(Ty, Ty),
        /// A suspension's slot of retained origins.
        Retained,
    }
    // Completed pairs record an output slice, declared-slot advance and
    // subtree height. The mode is fixed for this walk. Replaying preserves
    // distinct reference positions and the existing language depth limit.
    let mut memo = HashMap::<(Ty, Ty), (usize, usize, usize, usize, u32)>::default();
    let mut deepest = 0;
    let pool = ls.pool;
    // The next declared slot, and the keys so far.
    let (mut next, mut keys) = (0, Vec::new());
    let param = |t: Ty| match pool.get(t) {
        TyData::Param(p) => Some(p),
        _ => None,
    };
    let decl = |next: &mut usize, keys: &mut Vec<Key>, n: usize| {
        keys.extend(std::iter::repeat_n(Key::Decl(*next), n));
        *next += 1;
    };
    let lined = hd_base::stack::all(Step::Pair(d, t, 0), |step, stack| {
        crate::work::scan_work(1);
        let (d, t, depth) = match step {
            Step::Pair(d, t, depth) => (d, t, depth),
            Step::Done(d, t, start, base, depth, previous) => {
                memo.insert(
                    (d, t),
                    (start, keys.len(), base, next - base, deepest - depth),
                );
                deepest = deepest.max(previous);
                return true;
            }
            Step::Retained => {
                decl(&mut next, &mut keys, 1);
                return true;
            }
            // The whole slot of each side's referent ([`Slot::whole`]):
            // the declared one, or what a type parameter there stands for
            // whole, its slot 0, as a call passes it ([`Ck::call_sig`]).
            Step::Whole(d, t) => {
                let TyData::Ref { inner: a, .. } = pool.get(d) else {
                    return true;
                };
                let whole = |r: Ty| ls.of(r).slots.get(1).is_some_and(|s| s.whole);
                let bare = || bare_parts(pool, a).0.first().copied();
                // The first projection of a type parameter there, as
                // `Self::Item` of a trait with no lifetimes.
                let param_proj = |a: Ty| {
                    let mut stack = vec![a];
                    let mut each = std::iter::from_fn(|| {
                        let x = stack.pop()?;
                        pool.children(x, &mut stack);
                        Some(x)
                    });
                    each.find_map(|x| match pool.get(x) {
                        TyData::Assoc { self_ty, .. } => Some((param(self_ty)?, x)),
                        _ => None,
                    })
                };
                match (whole(d), whole(t)) {
                    (false, false) => {}
                    (true, true) => decl(&mut next, &mut keys, 1),
                    (false, true) if let Some(p) = bare() => keys.push(Key::Param(p, 0)),
                    (false, true) if call && let Some((p, proj)) = param_proj(a) => {
                        keys.push(Key::AllOf(p, proj));
                    }
                    (true, false) => next += 1,
                    (false, true) => keys.push(Key::None),
                }
                return true;
            }
        };
        if depth > hd_base::limits::TYPE_DEPTH {
            return false;
        }
        if let Some(&(start, end, base, advance, height)) = memo.get(&(d, t)) {
            deepest = deepest.max(depth + height);
            if deepest > hd_base::limits::TYPE_DEPTH {
                return false;
            }
            for k in start..end {
                keys.push(match keys[k] {
                    Key::Decl(slot) => Key::Decl(next + (slot - base)),
                    key => key,
                });
            }
            next += advance;
            return true;
        }
        work();
        stack.push(Step::Done(d, t, keys.len(), next, depth, deepest));
        deepest = depth;
        let d1 = depth + 1;
        let list = |stack: &mut Vec<Step>, a: hd_types::TyList, b: hd_types::TyList| {
            let (a, b) = (pool.list_items(a), pool.list_items(b));
            stack.extend(a.iter().zip(b).rev().map(|(&x, &y)| Step::Pair(x, y, d1)));
            a.len() == b.len()
        };
        match (pool.get(d), pool.get(t)) {
            (TyData::Param(p), _) => {
                keys.extend((0..ls.of(t).slots.len()).map(|s| Key::Param(p, s)));
                true
            }
            // A function value's one capture slot (`fn.capture.local-parameter`).
            (TyData::Fn { .. }, TyData::Fn { .. }) => {
                decl(&mut next, &mut keys, 1);
                true
            }
            // Each slot of an opaque type's hidden type, or of its instance,
            // takes the opaque type's lifetime (`types.opaque.borrows`).
            (TyData::Opaque { borrows: true, .. }, _) => {
                decl(&mut next, &mut keys, ls.of(t).slots.len());
                true
            }
            (TyData::Opaque { .. }, _) => {
                keys.extend(std::iter::repeat_n(Key::None, ls.of(t).slots.len()));
                true
            }
            (TyData::Assoc { self_ty, .. }, _) if call && let Some(p) = param(self_ty) => {
                keys.extend(std::iter::repeat_n(Key::AllOf(p, d), ls.of(t).slots.len()));
                true
            }
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
                decl(&mut next, &mut keys, 1);
                stack.push(Step::Whole(d, t));
                stack.push(Step::Pair(a, b, d1));
                true
            }
            (
                TyData::Ref {
                    counted: true,
                    inner: a,
                    ..
                },
                TyData::Ref {
                    counted: true,
                    inner: b,
                    ..
                },
            ) => {
                stack.push(Step::Pair(a, b, d1));
                true
            }
            // A rest that does not line up fails the whole walk, so it
            // fails before the elements are entered.
            (TyData::Adt(x, a), TyData::Adt(y, b)) if x == y => {
                // Layout::walk puts the suspension's retained origins after
                // its result slots, including inside another type argument.
                if x == AdtId::Declared(ls.suspend) {
                    stack.push(Step::Retained);
                }
                list(stack, a, b)
            }
            (
                TyData::TraitValue {
                    def: x, args: a, ..
                },
                TyData::TraitValue {
                    def: y, args: b, ..
                },
            ) if x == y => list(stack, a, b),
            _ => ls.of(d).slots.is_empty() && ls.of(t).slots.is_empty(),
        }
    });
    lined.then_some(keys)
}

/// Whether `t` is a direct function value. Such a value has
/// one checker-only slot for the origins of local references captured by a
/// scoped closure parameter.
fn capture_slot(pool: Types<'_>, t: Ty) -> bool {
    matches!(pool.get(t), TyData::Fn { .. })
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
    /// Whether a `*mut` holds it: every copy of the counted reference
    /// shares it, so it is related both ways, as behind a `&mut`.
    shared_mut: bool,
    /// Whether it is the one origin set of a referent with no slot of its
    /// own that holds origins, as `S::Item` behind `&mut S::Item`
    /// ([`whole_referent`]): a place the referent's value is stored into
    /// whole, and no reference slot.
    whole: bool,
}

/// The origin slots of a type in pre-order (`ref.dep.position`): each
/// local reference type outside function types, through counted
/// references, as in [`Types::ref_slots`], each borrowing opaque type, each function type
/// outside another function type, each suspension after its
/// result's slots, as a struct's lifetime, and one whole slot for a
/// referent with no slot of its own that holds origins
/// ([`Slot::whole`]). [`Layouts`] makes each once, and walks a part that
/// has no slots once per check ([`Layouts::slotless`]).
struct Layout {
    slots: Vec<Slot>,
    /// The top slots.
    tops: Vec<usize>,
    /// How many origin sets a value of the type has: one per top slot, or
    /// one for a trait value with none; 0 when it may hold no origins.
    width: usize,
}

impl Layout {
    fn new(ls: &Layouts<'_>, t: Ty) -> Layout {
        let pool = ls.pool;
        let mut slots = Vec::new();
        Self::walk(ls, t, true, 0, &mut slots);
        for k in 0..slots.len() {
            let fun = slots[k].fun && !slots[k].hold;
            slots[k].written = fun
                && !(k == 0 && capture_slot(pool, t))
                && !(k > 0 && capture_slot(pool, referent(pool, slots[k - 1].ty)));
        }
        let tops: Vec<usize> = (0..slots.len()).filter(|&k| slots[k].top).collect();
        // A trait value may hold a local reference (`trait.dyn.ref.convert`),
        // and so may a trait's own projection where the trait has lifetimes.
        let holds = holds_local_ref(pool, t) || is_dyn(pool, t) || ls.own_proj(t);
        let width = if tops.is_empty() {
            usize::from(holds)
        } else {
            tops.len()
        };
        Layout { slots, tops, width }
    }

    /// Pushes the slots of `t` onto `out`. Any type may hold a suspension,
    /// so only depth stops the walk, and a part found to have no slots,
    /// which the pool shares between its uses, is not entered again
    /// ([`Layouts::slotless`]): a type of n doubled levels,
    /// `((s, s), (s, s))`, costs n steps, not 2^n. A part with slots is
    /// entered once per use, so a layout's work is its slots' paths. The
    /// walk keeps its own stack, so a type nested to the depth limit
    /// takes no native frame per level: each entry is a part to enter
    /// (`None`), or one to finish, with where its slots start.
    fn walk(ls: &Layouts<'_>, t: Ty, top: bool, depth: u32, out: &mut Vec<Slot>) {
        let (pool, suspend) = (ls.pool, ls.suspend);
        let mut stack: Vec<(Ty, bool, u32, Option<usize>)> = vec![(t, top, depth, None)];
        while let Some((t, top, depth, entered)) = stack.pop() {
            let hold =
                matches!(pool.get(t), TyData::Adt(AdtId::Declared(def), _) if def == suspend);
            let slot = Slot {
                ty: t,
                fun: hold || matches!(pool.get(t), TyData::Fn { .. }),
                hold,
                top,
                end: out.len() + 1,
                written: false,
                shared_mut: false,
                whole: false,
            };
            if let Some(start) = entered {
                // A referent with no slot of its own that holds origins
                // is one place, which a store fills whole.
                if let TyData::Ref {
                    counted: false,
                    inner,
                    ..
                } = pool.get(t)
                {
                    if out.len() == start + 1 && whole_referent(ls, inner) {
                        out.push(Slot {
                            ty: inner,
                            top: false,
                            end: start + 2,
                            whole: true,
                            ..slot
                        });
                    }
                    out[start].end = out.len();
                } else if matches!(pool.get(t), TyData::Ref { mutable: true, .. }) {
                    out[start..].iter_mut().for_each(|s| s.shared_mut = true);
                } else if hold {
                    out.push(slot);
                }
                // Only a walk the depth limit cut nowhere is the type's
                // whole answer.
                if out.len() == start && depth + pool.depth(t) <= hd_base::limits::TYPE_DEPTH {
                    ls.slotless.borrow_mut().insert(t);
                }
                continue;
            }
            if depth > hd_base::limits::TYPE_DEPTH || ls.slotless.borrow().contains(&t) {
                continue;
            }
            crate::work::layout_step();
            stack.push((t, top, depth, Some(out.len())));
            let (first, d) = (stack.len(), depth + 1);
            match pool.get(t) {
                TyData::Ref {
                    counted: false,
                    inner,
                    ..
                } => {
                    out.push(slot);
                    stack.push((inner, false, d, None));
                }
                TyData::Opaque { borrows: true, .. } | TyData::Fn { .. } => out.push(slot),
                // A counted reference holds its referent's slots, as a
                // generic instance does (`ref.local.holding.counted`).
                TyData::Ref { inner, .. } => stack.push((inner, top, d, None)),

                TyData::Adt(_, args) | TyData::TraitValue { args, .. } => {
                    stack.extend(pool.list_items(args).iter().map(|&a| (a, top, d, None)));
                }
                TyData::Prim(_)
                | TyData::Never
                | TyData::Poison
                | TyData::Opaque { borrows: false, .. }
                | TyData::Param(_)
                | TyData::Const(_)
                | TyData::Assoc { .. }
                | TyData::Infer(_)
                | TyData::Canon(_)
                | TyData::Row(_)
                | TyData::Context(_) => {}
            }
            // Parts are entered in order, the first on top.
            stack[first..].reverse();
        }
    }

    /// Each top slot's run: itself and the slots behind it, `[start, end)`.
    fn runs(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.tops.iter().map(|&k| (k, self.slots[k].end))
    }
}

/// The origin slots of types, counted without laying them out
/// (`types.limit.references`): each `&T` and `&mut T` outside function
/// types, through counted references, each borrowing opaque type, each
/// function type outside another, and each suspension, a part counted
/// at each of its uses. That is [`Layout::walk`]'s slots less its whole
/// slots, which are at most one per reference, so a type within
/// [`hd_base::limits::REFERENCES`] lays out in at most twice that many.
/// Each distinct part is counted once ([`Self::memo`]), and a count
/// saturates just past the limit, so asking every type of a body costs
/// its distinct type nodes.
pub(crate) struct SlotCounts {
    suspend: DefId,
    memo: HashMap<Ty, u32>,
}

impl SlotCounts {
    pub(crate) fn new(suspend: DefId) -> Self {
        SlotCounts {
            suspend,
            memo: HashMap::default(),
        }
    }

    /// Whether `t` holds more origin slots than the limit allows.
    pub(crate) fn over(&mut self, pool: Types<'_>, t: Ty) -> bool {
        self.count(pool, t) > hd_base::limits::REFERENCES
    }

    /// The parts of `t` whose slots it holds, and the slots it adds itself.
    fn parts(&self, pool: Types<'_>, t: Ty, out: &mut Vec<Ty>) -> u32 {
        match pool.get(t) {
            TyData::Ref { counted, inner, .. } => {
                out.push(inner);
                u32::from(!counted)
            }
            TyData::Opaque { borrows: true, .. } | TyData::Fn { .. } => 1,

            TyData::TraitValue { args, .. } => {
                out.extend(pool.list_items(args));
                0
            }
            TyData::Adt(def, args) => {
                out.extend(pool.list_items(args));
                u32::from(def.get().is_some_and(|def| def == self.suspend))
            }
            _ => 0,
        }
    }

    /// `t`'s count, after its parts', with an explicit stack: a type
    /// nested past [`hd_base::limits::TYPE_DEPTH`] is counted too, and
    /// no native frame is taken per level.
    fn count(&mut self, pool: Types<'_>, t: Ty) -> u32 {
        let cap = hd_base::limits::REFERENCES + 1;
        let mut stack = vec![(t, false)];
        let mut parts = Vec::new();
        while let Some((t, done)) = stack.pop() {
            if self.memo.contains_key(&t) {
                continue;
            }
            parts.clear();
            let own = self.parts(pool, t, &mut parts);
            if done {
                let n = (parts.iter())
                    .fold(own, |n, p| n.saturating_add(self.memo[p]))
                    .min(cap);
                self.memo.insert(t, n);
            } else {
                crate::work::slot_count_step();
                stack.push((t, true));
                stack.extend(parts.iter().map(|&p| (p, false)));
            }
        }
        self.memo[&t]
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
    /// The capture slots of the function types the declaration writes,
    /// which hold no captured origins (`ref.generic.outlives.written`):
    /// a read of the field takes none from the instance.
    written: Vec<usize>,
    /// The field is written `&'static T`: what is stored into it must be
    /// a static source (`ref.static.not-local`, `ref.static.source.error`).
    statik: bool,
}

/// The slot layouts of the types one check meets, and the slot maps of
/// the fields it projects, each made once.
pub(crate) struct Layouts<'a> {
    pool: Types<'a>,
    /// `Suspend`'s definition.
    suspend: DefId,
    lifetimed: Lifetimed<'a>,
    types: RefCell<HashMap<Ty, Rc<Layout>>>,
    /// The types whose walk pushed no slot ([`Layout::walk`]).
    slotless: RefCell<HashSet<Ty>>,
    /// Each type's [`own_proj`] answer under `lifetimed`, kept for every
    /// part a search enters ([`Layouts::own_proj`]).
    own_projs: RefCell<HashMap<Ty, bool>>,
    fields: RefCell<HashMap<(Ty, u32, u32), Option<FieldMap>>>,
    loan_shapes: RefCell<loan_shapes::Shapes>,
}

impl<'a> Layouts<'a> {
    pub(crate) fn new(pool: Types<'a>, suspend: DefId, lifetimed: Lifetimed<'a>) -> Self {
        Layouts {
            pool,
            suspend,
            lifetimed,
            types: RefCell::default(),
            slotless: RefCell::default(),
            own_projs: RefCell::default(),
            fields: RefCell::default(),
            loan_shapes: RefCell::default(),
        }
    }

    fn of(&self, t: Ty) -> Rc<Layout> {
        if let Some(l) = self.types.borrow().get(&t) {
            return Rc::clone(l);
        }
        let l = Rc::new(Layout::new(self, t));
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
        self.field_entry(cx, t, variant, j, |m| (m.ty, m.at.clone()))
    }

    /// Field `j`'s [`FieldMap::written`] slots.
    fn field_written(&self, cx: &BodyCx<'_>, t: Ty, variant: Option<u32>, j: u32) -> Vec<usize> {
        (self.field_entry(cx, t, variant, j, |m| m.written.clone())).unwrap_or_default()
    }

    /// `f` of field `j`'s [`FieldMap`], made once.
    /// Whether field `j` is written `&'static T` ([`FieldMap::statik`]).
    fn field_static(&self, cx: &BodyCx<'_>, t: Ty, variant: Option<u32>, j: u32) -> bool {
        self.field_entry(cx, t, variant, j, |m| m.statik) == Some(true)
    }

    fn field_entry<R>(
        &self,
        cx: &BodyCx<'_>,
        t: Ty,
        variant: Option<u32>,
        j: u32,
        f: impl FnOnce(&FieldMap) -> R,
    ) -> Option<R> {
        let key = (t, variant.unwrap_or(NONE), j);
        if !self.fields.borrow().contains_key(&key) {
            let m = self.field_map(cx, t, variant, j);
            self.fields.borrow_mut().insert(key, m);
        }
        let fields = self.fields.borrow();
        Some(f(fields.get(&key)?.as_ref()?))
    }

    /// The type of field `j` and where its slots lie ([`FieldMap`]).
    /// Written lifetimes by origin slot, inserting zero for capture and
    /// whole slots. A suspension's lifetime follows the reference slots
    /// (`ref.dep.suspension.lifetime`).
    fn slot_lifetimes(&self, f: Ty, lifetimes: &[u16]) -> Vec<u16> {
        let (mut r, mut h) = (0, self.pool.ref_slots(f));
        let next = |k: &mut usize| {
            *k += 1;
            lifetimes.get(*k - 1).copied().unwrap_or(0)
        };
        (self.of(f).slots.iter())
            .map(|s| {
                if s.whole || s.fun && !s.hold {
                    0
                } else if s.hold {
                    next(&mut h)
                } else {
                    next(&mut r)
                }
            })
            .collect()
    }

    /// The slot of an instance with arguments `type_items` of a type with
    /// `generics` that its `l`th lifetime, 1-based, lies in: that
    /// argument's one top slot.
    fn lifetime_slot(
        &self,
        generics: &[hd_resolve::Generic],
        type_items: &[Ty],
        l: u16,
    ) -> Option<usize> {
        let mut lts = (generics.iter().enumerate()).filter(|(_, g)| g.lifetime);
        let g = lts.nth(usize::from(l).checked_sub(1)?)?.0;
        let start: usize = type_items
            .get(..g)?
            .iter()
            .map(|&x| self.of(x).slots.len())
            .sum();
        match self.of(*type_items.get(g)?).tops.as_slice() {
            [one] => Some(start + one),
            _ => None,
        }
    }

    fn field_map(&self, cx: &BodyCx<'_>, t: Ty, variant: Option<u32>, j: u32) -> Option<FieldMap> {
        let pool = self.pool;
        let len = |t: Ty| self.of(t).slots.len();
        let run = |start: usize, t: Ty| Some((start..start + len(t)).collect());
        match (pool.get(t), variant) {
            (TyData::Adt(AdtId::Product { .. }, _), None) => {
                let (es, _) = pool.product_parts(t)?;
                let e = *es.get(j as usize)?;
                let start = es[..j as usize].iter().map(|&x| len(x)).sum();
                Some(FieldMap {
                    ty: e,
                    at: run(start, e),
                    written: Vec::new(),
                    statik: false,
                })
            }
            (TyData::Adt(AdtId::Declared(def), args), _) if def.get().is_some() => {
                let item = cx.lookup.item(def)?;
                let fields = match (&item.data, variant) {
                    (ItemData::Struct(fs, _), None) => fs,
                    (
                        ItemData::Enum {
                            shared, variants, ..
                        },
                        Some(k),
                    ) if shared.is_empty() => &variants.get(k as usize)?.fields,
                    _ => return None,
                };
                let field = fields.get(j as usize)?;
                let f = field.ty;
                let type_items = pool.list_items(args);
                let ty = subst_owner(pool, def, type_items, f);
                let start = |a: usize| type_items[..a].iter().map(|&x| len(x)).sum::<usize>();
                let lifetime = |l: u16| self.lifetime_slot(&item.generics, type_items, l);
                let at = match pool.get(f) {
                    // `ref.generic.outlives.position`: a field of a type
                    // parameter bounded by a lifetime lies in that
                    // lifetime's position.
                    TyData::Param(_)
                        if let Some(l) = outlives_field(pool, def, &item.generics, f) =>
                    {
                        lifetime(l).map(|s| vec![s; len(ty)])
                    }
                    TyData::Param(p) if p.owner == def && (p.index as usize) < type_items.len() => {
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
                // A capture slot of a function type the field's type
                // writes, not one a type argument brings.
                let decl = self.of(f);
                let written = (line_up(self, f, ty).unwrap_or_default().iter().enumerate())
                    .filter(|(_, k)| {
                        matches!(k, Key::Decl(d) if decl.slots.get(*d).is_some_and(|s| s.fun && !s.hold))
                    })
                    .map(|(k, _)| k)
                    .collect();
                let statik = field.lifetimes.first() == Some(&hd_resolve::STATIC_LIFETIME)
                    && matches!(pool.get(f), TyData::Ref { counted: false, .. });
                Some(FieldMap {
                    ty,
                    at,
                    written,
                    statik,
                })
            }
            _ => None,
        }
    }
}

/// Whether trait `def` has a lifetime parameter ([`own_proj`]).
type Lifetimed<'f> = &'f dyn Fn(DefId) -> bool;

/// Whether `t` holds a trait's own projection, as `Self::Item` in
/// `trait Shelf['b]`'s bodies, of a trait with a lifetime parameter. It lists
/// no trait arguments, so [`holds_local_ref`] says no to it, but it holds
/// what the trait's lifetimes name, as `S::Item` with `S < Shelf['a]` does
/// (`ref.lifetime.trait.bound-argument`). Each distinct part is entered
/// once: a part the pool shares, as `(A[S], A[S])` through a chain of
/// aliases, costs its distinct nodes, not its paths (#1984).
fn own_proj(pool: Types<'_>, lifetimed: Lifetimed<'_>, t: Ty) -> bool {
    if !pool.has_assoc(t) {
        return false;
    }
    let mut seen: HashSet<Ty> = HashSet::default();
    let mut stack = vec![t];
    while let Some(t) = stack.pop() {
        if !pool.has_assoc(t) || !seen.insert(t) {
            continue;
        }
        crate::work::slot_count_step();
        match pool.get(t) {
            TyData::Assoc {
                trait_, self_ty, ..
            } if pool.get(self_ty)
                == TyData::Param(ParamRef {
                    owner: trait_,
                    index: 0,
                })
                && lifetimed(trait_) =>
            {
                return true;
            }
            _ => pool.children(t, &mut stack),
        }
    }
    false
}

/// [`Lifetimed`] by `lookup`.
pub(crate) fn lifetimed<'l>(lookup: &'l hd_resolve::Lookup<'_>) -> impl Fn(DefId) -> bool + 'l {
    move |d| {
        lookup
            .item(d)
            .is_some_and(|it| it.generics.iter().any(|g| g.lifetime))
    }
}

/// Whether a referent `t` with no slot of its own still holds origins
/// as a whole, so a reference to it has one slot for them: a projection,
/// or a type holding one, as `S::Item` or `(S::Item, i32)` with
/// `S < Shelf['a]` (`ref.lifetime.trait.bound-argument`), a trait's own
/// projection where the trait has lifetimes ([`own_proj`]) included. A
/// trait value's origins are its reference's (`ref.dep.trait`).
fn whole_referent(ls: &Layouts<'_>, t: Ty) -> bool {
    let pool = ls.pool;
    pool.has_assoc(t) && !is_dyn(pool, t) && (holds_local_ref(pool, t) || ls.own_proj(t))
}

impl Layouts<'_> {
    /// [`own_proj`] under this check's `lifetimed`, answered for each
    /// distinct part once over the whole check: a part's answer is its
    /// own projection's or any of its parts', so the types a check lays
    /// out, each a part of the next as a chain of aliases makes them, cost
    /// their distinct nodes together, not each its own subtree (#1984).
    fn own_proj(&self, t: Ty) -> bool {
        let pool = self.pool;
        let mut memo = self.own_projs.borrow_mut();
        let mut stack = vec![(t, false)];
        let mut parts = Vec::new();
        while let Some((n, entered)) = stack.pop() {
            if memo.contains_key(&n) {
                continue;
            }
            if !pool.has_assoc(n) {
                memo.insert(n, false);
                continue;
            }
            parts.clear();
            let own = match pool.get(n) {
                TyData::Assoc {
                    trait_, self_ty, ..
                } if pool.get(self_ty)
                    == TyData::Param(ParamRef {
                        owner: trait_,
                        index: 0,
                    })
                    && (self.lifetimed)(trait_) =>
                {
                    true
                }
                _ => {
                    pool.children(n, &mut parts);
                    false
                }
            };
            if own || entered || parts.is_empty() {
                crate::work::slot_count_step();
                let any = own || parts.iter().any(|p| memo.get(p) == Some(&true));
                memo.insert(n, any);
            } else {
                stack.push((n, true));
                stack.extend(parts.iter().map(|&p| (p, false)));
            }
        }
        memo[&t]
    }
}

/// What reference type `t` refers to.
fn referent(pool: Types<'_>, t: Ty) -> Ty {
    match pool.get(t) {
        TyData::Ref { inner, .. } => inner,
        _ => Ty::VOID,
    }
}

/// `Some((counted, to a trait value))` when `t` is a reference type,
/// under any `mut`.
fn local_ref(pool: Types<'_>, t: Ty) -> Option<(bool, bool)> {
    match pool.get(t) {
        TyData::Ref { counted, inner, .. } => Some((counted, is_dyn(pool, inner))),
        _ => None,
    }
}

fn is_dyn(pool: Types<'_>, t: Ty) -> bool {
    matches!(pool.get(t), TyData::TraitValue { .. })
}

// The messages of `local-reference-escapes` for a store into a place the
// caller owns and for each destination of a captured function value.
/// The start of each message of a captured function value but
/// [`HELD_MSG`]'s.
const CAPTURED: &str = "a local reference captured by this function value";
const BOX_MSG: &str = "a local reference captured by this function value never escapes to a box";
const STORE_FN_MSG: &str = "a local reference captured by this function value never escapes to a field, a top-level binding, or a place the caller owns";
const PROOF_MSG: &str = "a reference does not outlive the lifetime required by its trait bound";
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
    Param(ParamRef, usize),
    Held,
    /// The calling body's own lifetime, as a bound writes it for a trait
    /// argument (`ref.lifetime.trait.bound-argument`): what flows in must
    /// be what a result of that lifetime may depend on.
    Own(u32),
    /// Callee lifetime `l` of the call at instruction `c`, which wrote it
    /// into an input of the function value it returned
    /// (`fn.type.lifetime.call.written`): the variable that call made.
    At(u32, u32),
}

/// An argument slot's variable, and its check if the callee may keep it.
type ArgSlot = (Var, Option<Category>);

/// Equivalence classes of the existing call-slot variables, indexed once.
/// Polonius-style origin sets remain distinct unless the checker links them.
struct CallOrigins {
    classes: HashMap<Var, u32>,
    returned: HashSet<Var>,
}

/// O(input/output slots + links), with an iterative component worklist.
fn call_origins(
    inputs: &[Var],
    outs: &[Vec<Var>],
    bare: &[Var],
    links: &[(Var, Var)],
    mut work: impl FnMut(),
) -> CallOrigins {
    let mut neighbors: HashMap<Var, Vec<Var>> = HashMap::default();
    for &(a, b) in links {
        work();
        neighbors.entry(a).or_default().push(b);
        neighbors.entry(b).or_default().push(a);
    }
    let mut classes = HashMap::default();
    let mut pending = Vec::new();
    for variable in inputs
        .iter()
        .copied()
        .chain(outs.iter().flatten().copied())
        .chain(bare.iter().copied())
        .chain(links.iter().flat_map(|&(a, b)| [a, b]))
    {
        work();
        if classes.contains_key(&variable) {
            continue;
        }
        let class = w(classes.len());
        classes.insert(variable, class);
        pending.push(variable);
        while let Some(variable) = pending.pop() {
            work();
            for &next in neighbors.get(&variable).into_iter().flatten() {
                work();
                classes.entry(next).or_insert_with(|| {
                    pending.push(next);
                    class
                });
            }
        }
    }
    let returned_classes: HashSet<_> = outs
        .iter()
        .flatten()
        .chain(bare)
        .filter_map(|v| {
            work();
            classes.get(v).copied()
        })
        .collect();
    let all = [Var::Held, Var::Life(ANY)]
        .iter()
        .any(|v| classes.get(v).is_some_and(|c| returned_classes.contains(c)));
    let mut returned = HashSet::default();
    for (&variable, class) in &mut classes {
        work();
        if all || returned_classes.contains(class) {
            returned.insert(variable);
        }
        if all {
            *class = 0;
        }
    }
    CallOrigins { classes, returned }
}

#[cfg(test)]
mod returned_variable_tests {
    use super::*;

    #[test]
    fn linked_origins_are_indexed_in_linear_width_and_depth() {
        for depth in [1u32, 20] {
            let mut small = 0;
            for width in [1_000u32, 10_000] {
                let mut links = Vec::new();
                for branch in 0..width {
                    let mut previous = Var::Life(0);
                    for level in 0..depth {
                        let next = Var::Life(1 + branch * depth + level);
                        links.push((previous, next));
                        previous = next;
                    }
                }
                let mut work = 0;
                let result =
                    call_origins(&[], &[vec![Var::Life(0)]], &[], &links, || work += 1).returned;
                assert_eq!(result.len(), (width * depth + 1) as usize);
                assert!(!result.contains(&Var::Held));
                if width == 1_000 {
                    small = work;
                } else {
                    assert!(work <= 10 * small);
                }
            }
        }
    }
}

#[cfg(test)]
mod line_up_tests {
    use super::*;
    use hd_types::{InternPool, ParamRef};

    fn tuple(pool: &InternPool, elems: &[Ty]) -> Ty {
        pool.product(pool.list(elems), None)
    }

    #[test]
    fn doubling_pairs_are_checked_once() {
        let pool = InternPool::new();
        let lifetimed = |_: DefId| false;
        let ls = Layouts::new(pool.types(), DefId::from_raw(2), &lifetimed);
        for depth in [13, 26] {
            let mut pair = [Ty::I32, Ty::BOOL];
            for _ in 0..depth {
                pair = pair.map(|t| tuple(&pool, &[t, t]));
            }
            for call in [false, true] {
                let mut work = 0;
                let keys = line_up_with(&ls, pair[0], pair[1], call, || work += 1);
                assert_eq!(keys, Some(Vec::new()));
                assert_eq!(work, depth + 1);
            }
            let mismatch = tuple(&pool, &[pair[1], pair[1], Ty::I32]);
            assert!(line_up(&ls, pair[0], mismatch).is_none());
        }
    }

    #[test]
    fn repeated_pairs_rebase_declared_slots_but_keep_parameter_keys() {
        let pool = InternPool::new();
        let p = ParamRef {
            owner: DefId::from_raw(1),
            index: 0,
        };
        let param = pool.intern_ty(&TyData::Param(p));
        let r = pool.intern_ty(&TyData::Ref {
            inner: Ty::I32,
            mutable: false,
            counted: false,
        });
        let d = tuple(&pool, &[r, param]);
        let t = tuple(&pool, &[r, r]);
        let lifetimed = |_: DefId| false;
        let ls = Layouts::new(pool.types(), DefId::from_raw(2), &lifetimed);
        assert_eq!(
            line_up(&ls, tuple(&pool, &[d, d]), tuple(&pool, &[t, t])),
            Some(vec![
                Key::Decl(0),
                Key::Param(p, 0),
                Key::Decl(1),
                Key::Param(p, 0)
            ])
        );
        let wide = tuple(&pool, &vec![r; 1_000]);
        assert_eq!(
            line_up(&ls, wide, wide),
            Some((0..1_000).map(Key::Decl).collect())
        );
    }

    #[test]
    fn a_cached_pair_still_obeys_the_type_depth_limit() {
        let pool = InternPool::new();
        let shallow = tuple(&pool, &[Ty::I32]);
        let mut deep = shallow;
        for _ in 0..hd_base::limits::TYPE_DEPTH {
            deep = tuple(&pool, &[deep]);
        }
        let root = tuple(&pool, &[shallow, deep]);
        let lifetimed = |_: DefId| false;
        let ls = Layouts::new(pool.types(), DefId::from_raw(2), &lifetimed);
        assert!(line_up(&ls, root, root).is_none());
    }

    /// [`line_up`] keys a type on its own stack: a declared type 1,000
    /// levels deep, a local reference beside the next level at each, with
    /// a type parameter at the bottom, lines up with its instance on a
    /// 96 KiB thread.
    #[test]
    fn a_line_up_at_the_type_depth_fits_a_small_stack() {
        std::thread::Builder::new()
            // cost: one 96 KiB thread for this test.
            .stack_size(96 << 10)
            .spawn(|| {
                let pool = InternPool::new();
                let r = pool.intern_ty(&TyData::Ref {
                    inner: Ty::I32,
                    mutable: false,
                    counted: false,
                });
                let p = ParamRef {
                    owner: DefId::from_raw(1),
                    index: 0,
                };
                let mut deep = (pool.intern_ty(&TyData::Param(p)), r);
                for _ in 1..hd_base::limits::TYPE_DEPTH {
                    let pair = |x| pool.product(pool.list(&[r, x]), None);
                    deep = (pair(deep.0), pair(deep.1));
                }
                let lifetimed = |_: DefId| false;
                let ls = Layouts::new(pool.types(), DefId::from_raw(2), &lifetimed);
                let keys = line_up(&ls, deep.0, deep.1).expect("the two line up");
                let n = hd_base::limits::TYPE_DEPTH as usize;
                assert_eq!(keys.len(), n);
                assert_eq!(keys[n - 2], Key::Decl(n - 2));
                assert_eq!(keys[n - 1], Key::Param(p, 0));
            })
            .expect("thread")
            .join()
            .expect("no stack overflow");
    }
}

#[cfg(test)]
mod projection_lifetime_tests {
    use super::*;
    use hd_resolve::{Generic, Item, TraitData};
    use hd_types::{ParamRef, TyList};

    #[test]
    fn a_thousand_projection_lifetimes_share_bound_queries_on_a_small_stack() {
        std::thread::Builder::new()
            .stack_size(96 << 10)
            // cost: one 96 KiB thread for this generated projection-chain test.
            .spawn(|| {
                for lifetime in [0, 7] {
                    crate::body::with_checker_items(
                        |names| {
                            let p = names.pool.types();
                            let shelf = names.item("app.main", "Shelf");
                            let out = names.member(shelf, hd_intern::PathKind::Member, "Out");
                            let holder = names.item("app.main", "Holder");
                            let bound = p.intern_ty(&TyData::TraitValue {
                                def: shelf,
                                args: p.list(&[hd_resolve::lifetime_arg(p)]),
                                bindings: Vec::new(),
                            });
                            let mut life = Generic::plain(names.syms.intern("a"));
                            life.lifetime = true;
                            let mut tr = Item::new(
                                shelf,
                                names.syms.intern("Shelf"),
                                true,
                                ItemData::Trait(TraitData::default()),
                            );
                            tr.generics.push(life);
                            let associated = Item::new(
                                out,
                                names.syms.intern("Out"),
                                true,
                                ItemData::AssocType {
                                    owner: shelf,
                                    bounds: vec![bound],
                                    bound_lifetimes: vec![vec![1]],
                                },
                            );
                            let mut t = Generic::plain(names.syms.intern("T"));
                            t.bounds.push(bound);
                            t.bound_lifetimes.push(vec![lifetime]);
                            let mut root = Item::new(
                                holder,
                                names.syms.intern("Holder"),
                                true,
                                ItemData::Builtin,
                            );
                            root.generics.push(t);
                            vec![tr, associated, root]
                        },
                        |ck| {
                            let p = ck.pool();
                            let shelf = ck.cx.names.item("app.main", "Shelf");
                            let assoc =
                                ck.cx
                                    .names
                                    .member(shelf, hd_intern::PathKind::Member, "Out");
                            let owner = ck.cx.names.item("app.main", "Holder");
                            let other = ck.cx.names.item("app.main", "Other");
                            let lifetimed = |_: DefId| false;
                            let ls = Layouts::new(p, ck.cx.names.known.suspend, &lifetimed);
                            for depth in [100, 1000] {
                                let mut root =
                                    p.intern_ty(&TyData::Param(ParamRef { owner, index: 0 }));
                                for _ in 0..depth {
                                    root = p.intern_ty(&TyData::Assoc {
                                        assoc,
                                        trait_: shelf,
                                        self_ty: root,
                                        args: p.list(&[hd_resolve::lifetime_arg(p)]),
                                    });
                                }
                                let wanted = if lifetime == 0 {
                                    Vec::new()
                                } else {
                                    vec![u32::from(lifetime)]
                                };
                                assert_eq!(
                                    hd_types::work::count_type_nodes(
                                        || ck.proj_lifetimes(&ls, root)
                                    ),
                                    (wanted, 2 * depth + 1)
                                );
                                let mut memo = HashMap::default();
                                let ids = if lifetime == 0 {
                                    Vec::new()
                                } else {
                                    vec![vec![u32::from(lifetime)]]
                                };
                                assert_eq!(
                                    hd_types::work::count_type_nodes(
                                        || ck.bound_ids_in(&ls, root, shelf, &mut memo)
                                    ),
                                    (ids.clone(), depth + 1)
                                );
                                assert_eq!(
                                    hd_types::work::count_type_nodes(
                                        || ck.bound_ids_in(&ls, root, shelf, &mut memo)
                                    ),
                                    (ids, 0)
                                );
                                assert_eq!(
                                    hd_types::work::count_type_nodes(
                                        || ck.bound_ids_in(&ls, root, other, &mut memo)
                                    ),
                                    (Vec::<Vec<u32>>::new(), 1)
                                );
                                let scalar = p.product(TyList::EMPTY, None);
                                assert!(ck.proj_lifetimes(&ls, scalar).is_empty());
                            }
                        },
                    );
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }
}

/// A call's map over the types it instantiates (`ref.generic.call`).
struct CallSig {
    /// Each parameter's type, its argument's where the two line up, and
    /// for each of its slots the variable it takes and, for a capture slot
    /// the call may keep, the category of its check.
    params: Vec<(Ty, Vec<ArgSlot>)>,
    /// Each result slot's variables.
    outs: Vec<Vec<Var>>,
    /// What a result with no slots takes: the elided lifetimes, and what
    /// the projections and type parameters it holds hold.
    bare: Vec<Var>,
    /// Whether the result takes the variables.
    result: bool,
    /// Whether the call may store through a `&mut` argument
    /// (`ref.generic.store`, `ref.lifetime.store.call`).
    store: bool,
    /// The arguments for `&` or `&mut` parameters whose reference the
    /// result holds (`ref.excl.call`).
    held: Vec<u32>,
    loan_slots: Vec<hd_tir::ir::LoanSlot>,
    /// Each argument the result borrows only through result paths, and the
    /// place steps those paths share (`ref.result.path.caller`).
    paths: Vec<(u32, Vec<Proj>)>,
    /// Variables a bound's written lifetimes make one
    /// (`ref.lifetime.trait.bound-argument`): a type parameter's slots and
    /// the lifetimes its bounds write, and those and the caller's own
    /// where the parameter stands for a parameter of the caller's.
    links: Vec<(Var, Var)>,
    /// Whether a function value's type wrote some of `links`: a call
    /// of a function value, or a call passing one
    /// (`fn.type.lifetime.call.written`).
    by_value: bool,
    /// Each closure literal argument whose parameter's function type
    /// writes some of the callee's lifetimes at its inputs, and the
    /// lifetime of each input slot, 0 where none (`fn.closure.lifetime.call`).
    closures: Vec<(u32, Vec<u16>)>,
}

fn canonical_impl_head(pool: Types<'_>, t: Ty, declaration: &hd_resolve::Item) -> Ty {
    pool.subst(t, &|p: ParamRef| {
        (p.owner == declaration.def).then(|| {
            if declaration
                .generics
                .get(usize::from(p.index))
                .is_some_and(|g| g.row)
            {
                let row = pool.row(&hd_types::RowData {
                    keys: Vec::new(),
                    params: vec![hd_types::RowParamRef {
                        owner: DefId::NONE,
                        index: p.index,
                    }],
                });
                pool.intern_ty(&TyData::Row(row))
            } else {
                pool.intern_ty(&TyData::Param(ParamRef {
                    owner: DefId::NONE,
                    ..p
                }))
            }
        })
    })
}

/// Per slot of a goal, the origins it holds.
pub(crate) type ProofSlots<V> = Rc<Vec<Vec<V>>>;
type ProofBindings<V = Var> = Rc<HashMap<ParamRef, ProofSlots<V>>>;
type BoundOrigins = HashMap<(Ty, usize, Vec<u16>), ProofSlots<Var>>;

type StaticProofCache =
    HashMap<(Vec<(usize, RegionSource, TraitRef)>, Vec<(Key, Var)>), HashSet<Var>>;

/// The origins a proof's static requirements reach (`ref.generic.static.impl`),
/// from the origins of the reference slots of each root goal, its subject
/// then its trait arguments as one tuple ([`goal_tuple`]): a body's call
/// variables, or a header's written lifetimes. Each static leaf takes its
/// subject's top slots; each impl step lines its head, self then trait
/// arguments, up with the goal and passes on the slots of the bound's
/// parameter and of its trait arguments. Nothing is emitted.
///
/// Cost: per distinct (proof node, subject, origins) one head line-up,
/// memoized per head; O(proof nodes + slots) in all.
pub(crate) fn proof_statics<'o, V: Copy + Eq + std::hash::Hash>(
    ls: &Layouts<'_>,
    (lookup, impls): (&hd_resolve::Lookup<'_>, Impls<'_>),
    resolve: &dyn Fn(Ty) -> Ty,
    roots: impl IntoIterator<Item = (&'o RegionObligations, Ty, ProofSlots<V>)>,
) -> hd_base::StageResult<HashSet<V>> {
    let bug = |message| hd_base::NotImplemented::bug(hd_base::Stage::Body, message);
    let pool = ls.pool;
    let mut heads = HashMap::<(Ty, Ty, usize), (ProofBindings<V>, ProofSlots<V>)>::default();
    let mut identity = HashMap::<Ty, Option<ParamRef>>::default();
    let mut counts = SlotCounts::new(ls.suspend);
    let slot_type = pool.intern_ty(&TyData::Ref {
        inner: Ty::VOID,
        mutable: false,
        counted: false,
    });
    let mut canonical = HashMap::default();
    let mut statics = HashSet::default();
    // Retain origin vectors while their pointer identities key queries.
    let mut seen = HashMap::default();
    for (proof, actual, origins) in roots {
        let mut work = vec![(proof, actual, origins)];
        while let Some((proof, actual, origins)) = work.pop() {
            crate::work::scan_work(1);
            if seen
                .insert(
                    (proof.key(), actual, Rc::as_ptr(&origins) as usize),
                    Rc::clone(&origins),
                )
                .is_some()
            {
                continue;
            }
            let Some((row, args, steps)) = proof.parts() else {
                continue;
            };
            let Some(row) = row else {
                for &slot in &ls.of(actual).tops {
                    statics.extend(origins.get(slot).into_iter().flatten().copied());
                }
                continue;
            };
            let table = impls
                .table(row.module)
                .ok_or_else(|| bug("an outlives proof lost its selected impl table"))?;
            let r = row.row as usize;
            let owner = table.def[r];
            let declaration = lookup
                .item(owner)
                .ok_or_else(|| bug("an outlives proof lost its impl declaration"))?;
            let is_type = |index: u16| {
                declaration
                    .generics
                    .get(usize::from(index))
                    .is_some_and(|g| !g.row && !g.lifetime && g.const_type.is_none())
            };
            let canon = |t| canonical_impl_head(pool, t, declaration);
            let head = goal_tuple(pool, table.head_self[r], table.head_args[r]);
            let pattern = *canonical.entry(head).or_insert_with(|| canon(head));
            let key = (pattern, actual, Rc::as_ptr(&origins) as usize);
            let (bindings, declared) = if let Some((bindings, declared)) = heads.get(&key) {
                (Rc::clone(bindings), Rc::clone(declared))
            } else {
                let mut bindings = HashMap::<ParamRef, ProofSlots<V>>::default();
                let mut declared = Vec::<Vec<V>>::new();
                // A sole parameter occurrence with no own slots maps
                // its input vector identically, even under many wrappers.
                let direct = *identity.entry(pattern).or_insert_with(|| {
                    if counts.count(pool, pattern) != 0 {
                        return None;
                    }
                    let marked = pool.subst(pattern, &|p: ParamRef| {
                        (p.owner == DefId::NONE && is_type(p.index)).then_some(slot_type)
                    });
                    if counts.count(pool, marked) != 1 {
                        return None;
                    }
                    match line_up(ls, pattern, marked)?.as_slice() {
                        [Key::Param(p, 0)] => Some(*p),
                        _ => None,
                    }
                });
                if let Some(p) = direct {
                    bindings.insert(p, Rc::clone(&origins));
                } else {
                    let keys = line_up_goal(ls, pattern, actual);
                    crate::work::scan_work(keys.len());
                    // The lifetime the impl writes at each written slot of
                    // its head: its target's, then its trait arguments'.
                    let written = match &declaration.data {
                        ItemData::Impl {
                            self_lifetimes,
                            trait_slots,
                            ..
                        } => written_slots(ls, pattern, &[self_lifetimes, trait_slots]),
                        _ => Vec::new(),
                    };
                    for (key, from) in keys.into_iter().zip(origins.iter()) {
                        if let Key::Decl(slot) = key {
                            declared.resize_with(declared.len().max(slot + 1), Vec::new);
                            declared[slot].extend(from);
                        }
                        // An impl lifetime is bound like a parameter: by
                        // what the goal holds where the head writes it.
                        let key = match key {
                            Key::Decl(d) => match written.get(d).copied().unwrap_or(0) {
                                0 | STATIC_U16 => continue,
                                l => Key::Param(ParamRef { owner, index: l }, 0),
                            },
                            key => key,
                        };
                        if let Key::Param(p, slot) = key {
                            let into = Rc::make_mut(bindings.entry(p).or_default());
                            into.resize_with(into.len().max(slot + 1), Vec::new);
                            for &v in from {
                                if !into[slot].contains(&v) {
                                    into[slot].push(v);
                                }
                            }
                        }
                    }
                }
                let bindings = Rc::new(bindings);
                let declared = Rc::new(declared);
                heads.insert(key, (Rc::clone(&bindings), Rc::clone(&declared)));
                (bindings, declared)
            };
            if let ItemData::Impl {
                self_lifetimes,
                trait_slots,
                ..
            } = &declaration.data
                && (self_lifetimes.contains(&hd_resolve::STATIC_LIFETIME)
                    || trait_slots.contains(&hd_resolve::STATIC_LIFETIME))
            {
                let mut lifetimes = ls.slot_lifetimes(table.head_self[r], self_lifetimes);
                let mut at = 0;
                for &arg in pool.list_items(table.head_args[r]) {
                    let end = at + pool.ref_slots(arg);
                    lifetimes
                        .extend(ls.slot_lifetimes(arg, trait_slots.get(at..end).unwrap_or(&[])));
                    at = end;
                }
                for (slot, &lifetime) in lifetimes.iter().enumerate() {
                    if u32::from(lifetime) == STATIC {
                        statics.extend(declared.get(slot).into_iter().flatten().copied());
                    }
                }
            }
            for (step, child) in steps {
                // A template's member step (`annot.walker.obligation`):
                // the member's slots of the target's.
                if usize::from(*step) >= table.plan[r].len() {
                    let member = member_origins(ls, impls, (table, r), args, *step, &bindings);
                    let (actual, origins) =
                        member.ok_or_else(|| bug("an outlives proof lost its member step"))?;
                    work.push((child, resolve(actual), origins));
                    continue;
                }
                let Some(&PlanStep::Bound {
                    param,
                    trait_: bound_trait,
                    args: ref bound_args,
                    ..
                }) = table.plan[r].get(usize::from(*step))
                else {
                    return Err(bug("an outlives proof lost its bound plan step"));
                };
                let param = ParamRef {
                    owner: DefId::NONE,
                    index: u16::from(param),
                };
                // The bound's goal in the impl's parameters, then as this
                // proof instantiates them.
                let declared = goal_tuple(pool, pool.intern_ty(&TyData::Param(param)), *bound_args);
                let declared = canon(declared);
                let actual = resolve(pool.subst(declared, &|p: ParamRef| {
                    (p.owner == DefId::NONE)
                        .then(|| pool.list_items(args).get(usize::from(p.index)).copied())?
                }));
                let origins = if pool.list_items(*bound_args).is_empty() {
                    bindings.get(&param).cloned().unwrap_or_default()
                } else {
                    let keys = line_up_goal(ls, declared, actual);
                    crate::work::scan_work(keys.len());
                    // The impl lifetime the bound writes at each of its
                    // own slots (`Generic::bound_lifetimes`).
                    let g = declaration.generics.get(usize::from(param.index));
                    let own = g
                        .and_then(|g| {
                            let at = g.bounds.iter().position(|&b| {
                                pool.trait_value(b) == Some((bound_trait, *bound_args))
                            })?;
                            g.bound_lifetimes.get(at)
                        })
                        .map_or(&[][..], Vec::as_slice);
                    let written = written_slots(ls, declared, &[&[], own]);
                    let of = |key| match key {
                        Key::Param(p, slot) => bindings.get(&p).and_then(|b| b.get(slot)).cloned(),
                        Key::Decl(d) => match written.get(d).copied().unwrap_or(0) {
                            0 | STATIC_U16 => None,
                            l => (bindings.get(&ParamRef { owner, index: l }))
                                .and_then(|b| b.first())
                                .cloned(),
                        },
                        _ => None,
                    };
                    Rc::new(
                        keys.into_iter()
                            .map(|k| of(k).unwrap_or_default())
                            .collect(),
                    )
                };
                work.push((child, actual, origins));
            }
        }
    }
    Ok(statics)
}

/// The goal of a template impl's member step `step` (row `r` at `args`),
/// subject then trait arguments ([`goal_tuple`]), and its origins: the
/// member's slots of what the target `T` holds, in the order
/// [`hd_types::solver::row_goals`] asks the members (a tuple's elements,
/// its rest item or rest list; a union's members), each part's slots
/// consecutive in the target's layout. The trait arguments carry none.
fn member_origins<V: Clone>(
    ls: &Layouts<'_>,
    impls: Impls<'_>,
    (table, r): (&hd_types::solver::ImplTable, usize),
    args: hd_types::TyList,
    step: u16,
    bindings: &ProofBindings<V>,
) -> Option<(Ty, ProofSlots<V>)> {
    let pool = ls.pool;
    let args = pool.list_items(args);
    let goals = hd_types::solver::row_goals(pool, impls, table, r, args);
    let sub = goals.iter().find(|g| g.0 == step)?.1;
    let target = *args.first()?;
    // Each part of the target, with its first slot.
    let mut at = 0;
    let mut parts: Vec<(Ty, usize)> = Vec::new();
    let mut part = |t: Ty, parts: &mut Vec<(Ty, usize)>| {
        parts.push((t, at));
        at += ls.of(t).slots.len();
    };
    let (members, rest) = match pool.get(target) {
        TyData::Adt(ANON_SUM, args) => Some((pool.list_items(args), None)),
        _ => pool.product_parts(target),
    }?;
    for &e in members {
        part(e, &mut parts);
    }
    if let Some(r) = rest {
        part(r, &mut parts);
    }
    // The goals ask parts in order; a rest item stands at its list.
    let first = table.plan[r].len();
    let k = usize::from(step).checked_sub(first)?;
    let plan = impls.tuple_plan(table.def[r])?;
    let mut asked: Vec<usize> = Vec::new();
    for s in plan.iter() {
        match s.on {
            hd_types::solver::MemberOn::Elements => {
                asked.extend(0..members.len());
            }
            hd_types::solver::MemberOn::RestItem | hd_types::solver::MemberOn::RestList => {
                asked.extend(rest.map(|_| parts.len() - 1));
            }
        }
    }
    let (_, from) = *parts.get(*asked.get(k)?)?;
    let n = ls.of(sub.self_ty).slots.len();
    let holds = bindings.get(&ParamRef {
        owner: DefId::NONE,
        index: 0,
    });
    let mut origins: Vec<Vec<V>> = (from..from + n)
        .map(|s| holds.and_then(|h| h.get(s)).cloned().unwrap_or_default())
        .collect();
    let goal = goal_tuple(pool, sub.self_ty, sub.args);
    origins.resize_with(ls.of(goal).slots.len(), Vec::new);
    Some((goal, Rc::new(origins)))
}

/// [`line_up`] of a declared goal with an actual one ([`goal_tuple`]),
/// part by part: the subject, then each trait argument. A part the
/// declared side does not place keys each of its slots [`Key::None`]: an
/// argument the solver normalized to prove the goal, as a projection
/// where an impl's head writes a function type, holds no origin the walk
/// can follow there, and the rest of the goal lines up as written.
fn line_up_goal(ls: &Layouts<'_>, declared: Ty, actual: Ty) -> Vec<Key> {
    let pool = ls.pool;
    let none = |t: Ty| std::iter::repeat_n(Key::None, ls.of(t).slots.len());
    let (
        TyData::Adt(AdtId::Product { rest: false }, d),
        TyData::Adt(AdtId::Product { rest: false }, a),
    ) = (pool.get(declared), pool.get(actual))
    else {
        return line_up(ls, declared, actual).unwrap_or_else(|| none(actual).collect());
    };
    let (d, a) = (pool.list_items(d), pool.list_items(a));
    if d.len() != a.len() {
        return none(actual).collect();
    }
    let mut out = Vec::new();
    let mut from = 0;
    for (&dp, &ap) in d.iter().zip(a) {
        match line_up(ls, dp, ap) {
            Some(keys) => out.extend(keys.into_iter().map(|k| match k {
                Key::Decl(x) => Key::Decl(x + from),
                k => k,
            })),
            None => out.extend(none(ap)),
        }
        from += ls.of(dp).slots.len();
    }
    out
}

/// The written lifetime at each layout slot of goal tuple `t`: its
/// elements' local reference slots take `lists` in turn, the first list
/// the subject's and the last the rest's, each in [`Types::ref_slots`]
/// order; a function's or suspension's slot writes none.
fn written_slots(ls: &Layouts<'_>, t: Ty, lists: &[&[u16]]) -> Vec<u16> {
    let pool = ls.pool;
    let Some((elems, _)) = pool.product_parts(t) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut list = lists.iter().map(|l| l.iter());
    let (mut first, mut rest) = (list.next(), list.next());
    for (k, &e) in elems.iter().enumerate() {
        let ids = if k == 0 { &mut first } else { &mut rest };
        for slot in &ls.of(e).slots {
            let id = match ids {
                Some(ids) if !slot.fun && !slot.hold => ids.next().copied(),
                _ => None,
            };
            out.push(id.unwrap_or(0));
        }
    }
    out
}

/// Where a header's goal slot writes its lifetime: slot `k` its declared
/// goal writes after the subject's, or slot `s` of the trait parameter
/// `p` stands for.
pub(crate) enum Written {
    Declared(usize),
    Param(ParamRef, usize),
}

/// Whether the static requirements of a header's proof reach a written
/// lifetime that is not `'static` (`ref.generic.static.impl`): the goal's
/// subject's local reference slots write `subject`, in
/// [`Types::ref_slots`] order, then each suspension slot the lifetime its
/// `Suspend` names (a function's slot writes none), and `written` names
/// each other slot's lifetime (0: none written, which nothing requires). One line-up of `declared` with
/// `goal`, then [`proof_statics`].
pub(crate) fn written_escapes(
    (lookup, impls, suspend): (&hd_resolve::Lookup<'_>, Impls<'_>, DefId),
    pool: Types<'_>,
    proof: &RegionObligations,
    (declared, goal): (Ty, Ty),
    (subject, written): (&[u16], &dyn Fn(Written) -> u32),
) -> hd_base::StageResult<bool> {
    let lifetimed = lifetimed(lookup);
    let ls = Layouts::new(pool, suspend, &lifetimed);
    let Some(keys) = line_up(&ls, declared, goal) else {
        return Ok(false);
    };
    // The subject's lifetime at each of its layout slots.
    let first = match pool.get(goal) {
        TyData::Adt(AdtId::Product { .. }, _) => pool
            .product_parts(goal)
            .and_then(|(es, _)| es.first().copied()),
        _ => None,
    };
    let own: Vec<u32> = first.map_or_else(Vec::new, |t| {
        let (refs, held) = subject.split_at(pool.ref_slots(t).min(subject.len()));
        let (mut refs, mut held) = (refs.iter(), held.iter());
        (ls.of(t).slots.iter())
            .map(|s| {
                let next = if s.hold {
                    held.next()
                } else if s.fun {
                    None
                } else {
                    refs.next()
                };
                next.map_or(0, |&l| u32::from(l))
            })
            .collect()
    });
    // The declared subject's slots, which its written lifetimes key
    // before the trait arguments'.
    let n = match pool.get(declared) {
        TyData::Adt(AdtId::Product { .. }, _) => pool
            .product_parts(declared)
            .and_then(|(es, _)| es.first())
            .map_or(0, |&t| ls.of(t).slots.len()),
        _ => 0,
    };
    let origins = (keys.into_iter().enumerate()).map(|(k, key)| {
        let id = match key {
            Key::Decl(d) if d < n => own.get(k).copied().unwrap_or(0),
            Key::Decl(d) => written(Written::Declared(d - n)),
            Key::Param(p, s) => written(Written::Param(p, s)),
            _ => 0,
        };
        if id == 0 || id == STATIC {
            Vec::new()
        } else {
            vec![id]
        }
    });
    let roots = [(proof, goal, Rc::new(origins.collect()))];
    Ok(!proof_statics(&ls, (lookup, impls), &|t| t, roots)?.is_empty())
}

/// A goal as one type, its subject then its trait arguments, so that one
/// line-up pairs a head's or a bound's slots with a goal's in [`Layout`]
/// order.
pub(crate) fn goal_tuple(pool: Types<'_>, subject: Ty, args: hd_types::TyList) -> Ty {
    let mut elems = vec![subject];
    elems.extend_from_slice(pool.list_items(args));
    pool.product(pool.list(&elems), None)
}

impl CallSig {
    /// Whether the call passes an argument at a parameter slot written
    /// `&'static T`, which is checked at the call (`ref.static.source.error`).
    fn passes_static(&self) -> bool {
        (self.params.iter()).any(|p| {
            p.1.iter().any(|x| {
                x.0 == Var::Life(STATIC)
                    || matches!(x.1, Some(Category::StaticParam | Category::StaticProof))
            })
        }) || (self.links.iter()).any(|&(_, y)| y == Var::Life(STATIC))
    }

    /// Whether the call passes an argument at one of the body's own
    /// lifetimes, or at one a function value it calls writes, which it
    /// checks there (`ref.lifetime.trait.bound-argument`,
    /// `fn.type.lifetime.call.written`).
    fn passes_own(&self) -> bool {
        let own = |v: &Var| matches!(v, Var::Own(_) | Var::At(..));
        (self.params.iter()).any(|p| p.1.iter().any(|x| own(&x.0)))
            || self.links.iter().any(|(x, y)| own(x) || own(y))
    }
}

/// What [`Ck::written_inputs`] gives: each written input slot and its
/// variable.
type WrittenInputs = Vec<(usize, Var)>;

/// What [`Ck::written_inputs`] reads of one body: the body's own map,
/// whose parameter slots give a field's written lifetime, and the stores
/// into each local, indexed once on first use.
struct FnValues<'a> {
    b: &'a Body,
    mine: Option<&'a DepSig>,
    stores: std::cell::OnceCell<HashMap<u32, Vec<u32>>>,
    /// [`Ck::written_inputs`] of each local's part a value reads whole,
    /// traced once however many uses read it.
    inputs: RefCell<HashMap<(u32, Vec<Part>), WrittenInputs>>,
}

impl<'a> FnValues<'a> {
    fn new(b: &'a Body, mine: Option<&'a DepSig>) -> Self {
        FnValues {
            b,
            mine,
            stores: std::cell::OnceCell::new(),
            inputs: RefCell::default(),
        }
    }

    /// The `Store`s whose place is rooted at local `l`.
    fn stores(&self, l: u32) -> &[u32] {
        let b = self.b;
        let all = self.stores.get_or_init(|| {
            let mut m: HashMap<u32, Vec<u32>> = HashMap::default();
            for s in (0..b.tags.len()).filter(|&s| b.tags[s] == Tag::Store) {
                if let Some(PlaceRoot::Local(l)) = b.decode_place(b.data[s][0]).map(|p| p.root) {
                    m.entry(l.raw()).or_default().push(w(s));
                }
            }
            m
        });
        all.get(&l).map_or(&[], Vec::as_slice)
    }
}

/// A body's run-time loan sites, and each holder of one with the site's
/// index ([`hd_tir::own::loans::place`]).
pub(crate) type RunLoans = (
    Vec<hd_tir::own::loans::Site>,
    Vec<(hd_tir::own::loans::Holder, u32)>,
);

/// Per instruction, whether it may take a run-time loan
/// (`ref.excl.alias.loan`): a `Ref`, or a `Read` of a pointer or through
/// one, whose place may cross an object a pointer, a shared variable's cell
/// or a top-level binding's slot holds: a dereference on its path or on the
/// path of the read its root value is, a global root, or a captured local's
/// root. A dereference of a known local Ref reuses that address's crossing
/// fact; it does not introduce another object. Unknown references and later
/// dereferences remain conservative. The backend takes loans of counted
/// crossings. Cost: O(instructions + place projections + captures), one
/// forward pass, as a root comes before the place on it.
pub(crate) fn runtime_sites(b: &Body, pool: Types<'_>) -> Vec<bool> {
    let mut captured = vec![false; b.local_ty.len()];
    for l in &b.cap_local {
        captured[l.idx()] = true;
    }
    let mut crosses = vec![false; b.len()];
    let mut sites = vec![false; b.len()];
    for i in 0..b.len() {
        crate::work::scan_work(1);
        if !matches!(b.tags[i], Tag::Ref | Tag::Read) {
            continue;
        }
        let p = b.view(b.data[i][0]);
        // A local reference made by Ref keeps its place's provenance.
        // Dereferencing that known address adds no new counted crossing
        // (rustc instsimplify's &*a identity). Further dereferences, or an
        // unknown/loaded reference, still need the conservative loan.
        let mut steps = p.steps(b);
        let known = match p.root {
            hd_tir::ir::Root::Value(r)
                if (r as usize) < i
                    && b.tags[r as usize] == Tag::Ref
                    && matches!(
                        pool.get(b.ty[r as usize]),
                        TyData::Ref { counted: false, .. }
                    )
                    && steps.next() == Some(Proj::Deref) =>
            {
                Some(r as usize)
            }
            _ => None,
        };
        crosses[i] = match known {
            Some(r) => crosses[r] || steps.any(|s| matches!(s, Proj::Deref)),
            None => {
                p.steps(b).any(|s| matches!(s, Proj::Deref))
                    || match p.root {
                        hd_tir::ir::Root::Global(..) => true,
                        hd_tir::ir::Root::Local(l) => captured[l as usize],
                        hd_tir::ir::Root::Value(r) => {
                            (r as usize) < i
                                && b.tags[r as usize] == Tag::Read
                                && crosses[r as usize]
                        }
                        _ => false,
                    }
            }
        };
        sites[i] = crosses[i]
            || b.tags[i] == Tag::Read
                && matches!(pool.get(b.ty[i]), TyData::Ref { counted: true, .. });
    }
    sites
}

/// Whether instruction `i`, through same-value coercions (`*mut T` as
/// `*T`), is a run-time loan site of `sites` ([`runtime_sites`]).
pub(crate) fn runtime_site(b: &Body, sites: &[bool], i: u32) -> bool {
    b.direct_value(i)
        .is_some_and(|v| sites.get(v).copied().unwrap_or(false))
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
    /// What a static reference may refer to: a top-level binding bound
    /// without `mut`, reached through no reference, by its definition
    /// (`ref.static.place`), or [`LITERAL`] for a constant literal's
    /// static data (`ref.static.literal`). It is a top-level binding
    /// everywhere else.
    Static(u32),
    /// A mark that the value may point into result path `k`'s parameter
    /// outside that path (`ref.result.path.callee`).
    Outside(u32),
    /// The shared `Ref` at an instruction of a place a local's path
    /// reaches, or a `&mut` binding a call reads as `&`, for the conflict
    /// check only (`ref.excl.shared`, `ref.excl.call`).
    Site(u32),
    /// A `Ref`, or a call's argument, through a pointer that no conflict
    /// check needs: a run-time loan only, which the regions carry so that
    /// it lives while a value holding it does (`ref.excl.alias.loan`).
    /// Never a diagnostic.
    Lent(u32),
}

/// The definition word of [`Origin::Static`] for a constant literal, which
/// no top-level binding's raw id reaches.
const LITERAL: u32 = u32::MAX;

/// The slot [`Ck::field_inputs`] gives a function type's input written
/// `&'static T` in a field: no slot of the struct, but `'static` itself.
const STATIC_SLOT: usize = usize::MAX;

/// An instruction's, a local's or a parameter's index as a word.
fn w(i: usize) -> u32 {
    hd_tir::ir::u32_of(i)
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
    /// The reads of a `&mut` that a typed `let`, an assignment or a `&mut`
    /// field reborrows (`ref.move.reborrow.excl`).
    reborrows: &'a HashSet<u32>,
    /// The reads that move a binding or a part of it (`ref.excl.move`).
    moves: &'a HashSet<u32>,
    /// Locals whose closure capture is already `local-reference-escapes`
    /// (`ref.local.escape.capture-stored`): they add no capture origins.
    reported: &'a HashSet<hd_base::LocalId>,
    /// The body's own result paths (`ref.result.path.callee`).
    paths: &'a [OwnPath],
    /// The lifetime each slot of a closure parameter writes, by its
    /// local, where the closure's annotation writes one
    /// ([`Ck::annotated_params`]), 0 elsewhere.
    annotated: HashMap<u32, Vec<u32>>,
    /// The values bound to a binding written `&'static T`
    /// ([`Ck::static_values`]).
    statics: &'a [(hd_tir::ir::Ref, Box<[u32]>, Category)],
    /// Conditional marker queries against the solution, never constraints.
    markers: &'a [(hd_tir::ir::Ref, Box<[u32]>, NodeIdx, String)],
    /// Each argument a call lends to a local-reference parameter, whether
    /// as `&mut`, and the call when it is a `&mut self` receiver, shared
    /// until the call starts ([`Ck::lent_args`]).
    lent: Vec<(u32, bool, u32)>,
    /// Per instruction, whether it may take a run-time loan
    /// ([`runtime_sites`]).
    runtime: Vec<bool>,
    /// The locals a crossing closure captures: a reference one holds is a
    /// frozen loan, whose element stores conflict too (`ref.frozen.write`).
    frozen: Vec<u32>,
}

impl Inputs<'_> {
    /// Whether origin `o` lives as long as the body's own lifetime `l`:
    /// what a parameter slot of `l` refers to, or one behind it, of the
    /// function's own parameters or of a closure parameter whose
    /// annotation writes `l` (`fn.type.lifetime.call.written.closure`).
    fn allows(&self, o: Origin, l: u32) -> bool {
        if self.own.is_some_and(|own| own.allows(self.ls, o, l)) {
            return true;
        }
        let Origin::Param(p, s) = o else {
            return false;
        };
        let Some(ids) = self.annotated.get(&p) else {
            return false;
        };
        let s = s as usize;
        let lay = self.ls.of(self.b.local_ty[p as usize]);
        ids.get(s) == Some(&l)
            || (0..s.min(lay.slots.len()))
                .any(|s0| ids.get(s0) == Some(&l) && s < lay.slots[s0].end)
    }

    /// The body's own lifetime of parameter slot `o`, which [`Self::allows`]
    /// reads: the function's map's, or what a closure parameter's
    /// annotation writes there. `None` for another origin, or a slot no
    /// lifetime of the body's names.
    fn lifetime_of(&self, o: Origin) -> Option<u32> {
        let Origin::Param(p, s) = o else {
            return None;
        };
        let own = (self.own.and_then(|own| own.params.get(p as usize)))
            .and_then(|x| x.1.get(s as usize).copied());
        own.or_else(|| (self.annotated.get(&p)?.get(s as usize).copied()).filter(|&l| l != 0))
    }

    fn ty(&self, r: u32) -> Option<Ty> {
        (r & hd_tir::ir::Ref::CONST_BIT == 0)
            .then(|| self.b.ty.get(r as usize).copied())
            .flatten()
    }
}

/// The type of argument `a` as a source argument. The address a call
/// takes for a `&self` or `&mut self` receiver is already the local
/// reference the explicit `&x` or `&mut x` would be
/// (`fn.method.auto-address`, `ref.dep.origin.place`).
fn arg_ty(b: &Body, a: u32) -> Option<Ty> {
    if a & hd_tir::ir::Ref::CONST_BIT != 0 {
        return None;
    }
    b.ty.get(a as usize).copied()
}

/// The type of argument `a` for a parameter of type `decl`: its source
/// type, but the counted address a `&self` call takes of a temporary is
/// the local reference `decl` names, as a place's is (`ref.dep.temporary`),
/// and a trait value given for `&Self` or `&T` is lent as a reference to
/// it, its own slot and its lifetime arguments' apart (`ref.dep.trait`).
fn call_arg_ty(pool: Types<'_>, b: &Body, a: u32, decl: Ty) -> Option<Ty> {
    let t = arg_ty(b, a)?;
    if let TyData::Ref {
        counted: false,
        mutable,
        ..
    } = pool.get(decl)
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
        && let TyData::Ref { counted: false, .. } = pool.get(decl)
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
        // `NONE`, or an open map: no result slot yet.
        let open = |d| pool.deps_of(d).is_none_or(|(_, outs)| outs.is_empty());
        if open(got) && open(want) {
            return true;
        }
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
        let lifetimed = lifetimed(self.cx.lookup);
        let ls = &Layouts::new(pool, self.cx.names.known.suspend, &lifetimed);
        DepSig::new(ls, &sig.params, is_self, sig.ret, &sig.lifetimes)
            .with_paths(ls, &sig.paths)
            .to_fn(ls, skip)
    }

    /// The map a trait implementation's method `sig` must share with the
    /// trait's `tsig` (`trait.impl.lifetime`): the trait's, with its opaque
    /// types opened over the implementation's result, which may take
    /// their place (`trait.opaque.impl-result`).
    pub(crate) fn trait_fn_deps(
        &self,
        tsig: &hd_resolve::FnSig,
        sig: &hd_resolve::FnSig,
        (trait_, trait_args): (DefId, hd_types::TyList),
        (trait_lifetimes, self_lifetimes, trait_slots): (&[u16], &[u16], &[u16]),
        impl_lifetime_count: usize,
    ) -> hd_types::FnDeps {
        let pool = self.pool();
        let names = &self.cx.names;
        let is_self = |s| names.text(s) == "self";
        let lifetimed = lifetimed(self.cx.lookup);
        let ls = &Layouts::new(pool, self.cx.names.known.suspend, &lifetimed);
        let dep = DepSig::new(ls, &tsig.params, is_self, tsig.ret, &tsig.lifetimes);
        let dep = if pool.has_opaque(tsig.ret) && pool.ref_slots(tsig.ret) != 0 {
            dep.open(ls, sig.ret)
        } else {
            dep
        };
        let impl_outs = DepSig::new(ls, &sig.params, is_self, sig.ret, &sig.lifetimes)
            .with_paths(ls, &sig.paths)
            .outs;
        let slots: Vec<u32> = trait_slots.iter().map(|&l| u32::from(l)).collect();
        let arg_lifetimes = arg_ids(ls, pool.list_items(trait_args), &slots);
        dep.align_trait_impl(
            ls,
            &sig.params,
            (sig.ret, &impl_outs, &|t| self.proj_lifetimes(ls, t)),
            trait_,
            (trait_lifetimes, self_lifetimes, &arg_lifetimes),
            impl_lifetime_count,
        )
        .to_fn(ls, 0)
    }

    /// The variables of call `i`'s type-parameter slots that an explicit
    /// type argument writes `'static` ([`Ck::static_targs`]): what the call
    /// passes there takes only a static source (`ref.static.form`). The
    /// written slots are reference slots; the call's are its layout's,
    /// which also hold function and suspension slots.
    fn written_static_targs(
        &self,
        ls: &Layouts<'_>,
        b: &Body,
        i: usize,
        took: &[(Key, Var)],
    ) -> HashSet<Var> {
        let mut out = HashSet::default();
        if self.static_targs.is_empty() || !matches!(b.tags[i], Tag::Call | Tag::Await) {
            return out;
        }
        let Some((def, targs)) = Callee::from_words(b.record(b.data[i][0])).map(|c| match c {
            Callee::Item { def, targs } => (def, targs),
            Callee::TraitMethod { method, targs, .. } => (method, targs),
        }) else {
            return out;
        };
        let Some(sig) = self.cx.lookup.item(def).and_then(|it| it.sig()) else {
            return out;
        };
        let tree = &self.cx.src.parse.tree;
        // The written positions name the generics an annotation may
        // write, in order: not an `impl Trait` parameter's.
        let written: Vec<usize> = (sig.generics.iter().enumerate())
            .filter(|(_, g)| !g.anonymous)
            .map(|(k, _)| k)
            .collect();
        let targs = ls.pool.list_items(targs);
        for (&(owner, position), statics) in &self.static_targs {
            // The call the callee's node is written in.
            let call = std::iter::successors(tree.parent(owner), |&n| tree.parent(n))
                .find(|&n| tree.node(n).kind() == SyntaxKind::CallExpr);
            if call != Some(b.syn[i]) {
                continue;
            }
            let Some(&index) = written.get(usize::from(position)) else {
                continue;
            };
            let Some(&t) = targs.get(targs.len().saturating_sub(sig.generics.len()) + index) else {
                continue;
            };
            let refs: Vec<usize> = (ls.of(self.infer.resolve(ls.pool, t)).slots.iter())
                .enumerate()
                .filter(|(_, s)| !s.fun && !s.hold)
                .map(|(k, _)| k)
                .collect();
            for &(key, var) in took {
                if let Key::Param(p, slot) = key
                    && usize::from(p.index) == index
                    && statics.iter().any(|&k| refs.get(k as usize) == Some(&slot))
                {
                    out.insert(var);
                }
            }
        }
        out
    }

    /// Lower proof requirements to existing call variables
    /// ([`proof_statics`]). This emits no graph: call's ordinary
    /// `StaticParam` edges go to `regions.rs` unchanged.
    fn call_proof_statics(
        &self,
        ls: &Layouts<'_>,
        uses: &[crate::body::RegionUse],
        took: &[(Key, Var)],
        cache: &mut StaticProofCache,
    ) -> hd_base::StageResult<HashSet<Var>> {
        if uses.is_empty() {
            return Ok(HashSet::default());
        }
        let pool = ls.pool;
        // Inference stays fixed during this pass; original goal IDs and
        // the call variables determine the resolved correspondence.
        let key = (
            uses.iter().map(|u| (u.3.key(), u.1, u.2)).collect(),
            took.to_vec(),
        );
        if let Some(statics) = cache.get(&key) {
            return Ok(statics.clone());
        }
        let mut parameters = HashMap::<ParamRef, Vec<Vec<Var>>>::default();
        for &(key, var) in took {
            if let Key::Param(p, slot) = key {
                let slots = parameters.entry(p).or_default();
                slots.resize_with(slots.len().max(slot + 1), Vec::new);
                if !slots[slot].contains(&var) {
                    slots[slot].push(var);
                }
            }
        }
        let parameters: HashMap<_, ProofSlots<Var>> = parameters
            .into_iter()
            .map(|(p, s)| (p, Rc::new(s)))
            .collect();
        let empty = ProofSlots::default();
        let mut origins_cache = BoundOrigins::default();
        let roots = uses.iter().filter_map(|&(_, source, tref, ref proof)| {
            let RegionSource::Bound(param, bound) = source else {
                return None;
            };
            let goal = self
                .infer
                .resolve(pool, goal_tuple(pool, tref.self_ty, tref.args));
            let subject = parameters.get(&param).unwrap_or(&empty);
            let origins =
                self.bound_origins(ls, (param, bound), goal, Some(subject), &mut origins_cache);
            Some((proof, goal, origins))
        });
        let tables = (self.cx.lookup, Impls::Owned(self.cx.impls));
        let mut statics = proof_statics(ls, tables, &|t| self.infer.resolve(pool, t), roots)?;
        self.head_argument_statics(ls, uses, &parameters, &mut statics)?;
        cache.insert(key, statics.clone());
        Ok(statics)
    }

    /// The origins of each slot of a bound's goal, subject then trait
    /// arguments ([`goal_tuple`]): the call variables of the type
    /// parameter's slots, then the callee lifetime each of the bound's own
    /// reference slots writes (`ref.lifetime.trait.impl-argument`,
    /// `Generic::bound_lifetimes`), whose call variable the call made.
    /// `'static` and an unwritten slot have none; a function's or
    /// suspension's slot holds no written reference.
    fn bound_origins(
        &self,
        ls: &Layouts<'_>,
        (param, bound): (ParamRef, usize),
        goal: Ty,
        subject: Option<&ProofSlots<Var>>,
        cache: &mut BoundOrigins,
    ) -> ProofSlots<Var> {
        let pool = ls.pool;
        let Some((elems, _)) = pool.product_parts(goal) else {
            return subject.cloned().unwrap_or_default();
        };
        let generics = self.cx.lookup.item(param.owner).map(|it| match &it.data {
            ItemData::Fn(sig, _) | ItemData::Method { sig, .. } => &sig.generics,
            _ => &it.generics,
        });
        let written = (generics.and_then(|g| g.get(usize::from(param.index))))
            .and_then(|g| g.bound_lifetimes.get(bound))
            .map_or(&[][..], Vec::as_slice);
        if written.is_empty() {
            return subject.cloned().unwrap_or_default();
        }
        // Parameters and the shared empty source retain these pointer keys
        // throughout projection. Equal written goals share one origin query.
        let key = (
            goal,
            subject.map_or(0, |s| Rc::as_ptr(s) as usize),
            written.to_vec(),
        );
        if let Some(origins) = cache.get(&key) {
            return Rc::clone(origins);
        }
        let n = elems.first().map_or(0, |&t| ls.of(t).slots.len());
        let mut out: Vec<Vec<Var>> = subject.map_or_else(Vec::new, |s| s.to_vec());
        out.resize_with(n, Vec::new);
        let mut refs = written.iter();
        for &arg in &elems[1..] {
            for slot in &ls.of(arg).slots {
                let id = if slot.fun || slot.hold {
                    None
                } else {
                    refs.next()
                };
                out.push(match id.map(|&l| u32::from(l)) {
                    Some(l) if l != 0 && l != STATIC => vec![Var::Life(l)],
                    _ => Vec::new(),
                });
            }
        }
        let origins = Rc::new(out);
        cache.insert(key, Rc::clone(&origins));
        origins
    }

    /// A committed bound's written argument positions use the same call
    /// variables as its value operands (`ref.lifetime.trait.bound-argument`).
    fn head_argument_statics(
        &self,
        ls: &Layouts<'_>,
        uses: &[crate::body::RegionUse],
        parameters: &HashMap<ParamRef, ProofSlots<Var>>,
        statics: &mut HashSet<Var>,
    ) -> hd_base::StageResult<()> {
        let pool = ls.pool;
        let mut correspondences = HashMap::default();
        let mut canonical = HashMap::default();
        let bug = |message| hd_base::NotImplemented::bug(hd_base::Stage::Body, message);
        for (_, source, tref, proof) in uses {
            let RegionSource::Bound(param, bound) = *source else {
                continue;
            };
            let Some((Some(row), _, _)) = proof.parts() else {
                continue;
            };
            let table = self
                .cx
                .impl_table(row.module)
                .ok_or_else(|| bug("an outlives head lost its impl table"))?;
            let declaration = self
                .cx
                .lookup
                .item(table.def[row.row as usize])
                .ok_or_else(|| bug("an outlives head lost its declaration"))?;
            let ItemData::Impl { trait_slots, .. } = &declaration.data else {
                return Err(bug("an outlives head lost its declaration"));
            };
            if !trait_slots.contains(&hd_resolve::STATIC_LIFETIME) {
                continue;
            }
            let Some(generic) = self
                .declared_sig(param.owner)
                .and_then(|s| s.generics.get(usize::from(param.index)))
            else {
                return Err(bug("an outlives head lost its source parameter"));
            };
            let Some((_, formal)) = generic.bounds.get(bound).and_then(|&b| pool.trait_value(b))
            else {
                return Err(bug("an outlives head lost its source bound"));
            };
            let written = generic
                .bound_lifetimes
                .get(bound)
                .map_or(&[][..], Vec::as_slice);
            let (mut f_at, mut h_at) = (0, 0);
            for ((&formal, &actual), &head) in pool
                .list_items(formal)
                .iter()
                .zip(pool.list_items(tref.args))
                .zip(pool.list_items(table.head_args[row.row as usize]))
            {
                let actual = self.infer.resolve(pool, actual);
                let head = *canonical
                    .entry(head)
                    .or_insert_with(|| canonical_impl_head(pool, head, declaration));
                let f_end = f_at + pool.ref_slots(formal);
                let f_lts = ls.slot_lifetimes(formal, written.get(f_at..f_end).unwrap_or(&[]));
                f_at = f_end;
                let h_end = h_at + pool.ref_slots(head);
                let h_lts = ls.slot_lifetimes(head, trait_slots.get(h_at..h_end).unwrap_or(&[]));
                h_at = h_end;
                for pair in [(formal, actual), (head, actual)] {
                    correspondences
                        .entry(pair)
                        .or_insert_with(|| line_up(ls, pair.0, pair.1));
                }
                let from = correspondences[&(formal, actual)]
                    .as_ref()
                    .ok_or_else(|| bug("a bound argument lost its source correspondence"))?;
                let into = correspondences[&(head, actual)]
                    .as_ref()
                    .ok_or_else(|| bug("an impl argument lost its head correspondence"))?;
                crate::work::scan_work(from.len() + into.len());
                for (&from, &into) in from.iter().zip(into) {
                    if !matches!(into, Key::Decl(k) if h_lts.get(k) == Some(&hd_resolve::STATIC_LIFETIME))
                    {
                        continue;
                    }
                    match from {
                        Key::Decl(k) => {
                            if let Some(&l) = f_lts.get(k).filter(|&&l| l != 0) {
                                statics.insert(Var::Life(u32::from(l)));
                            }
                            // ruled out: an unnamed reference in a bound
                            // already fails ref.lifetime.trait.bound-unnamed.
                        }
                        Key::Param(p, slot) => statics.extend(
                            parameters
                                .get(&p)
                                .and_then(|s| s.get(slot))
                                .into_iter()
                                .flatten()
                                .copied(),
                        ),
                        Key::AllOf(p, _) => statics.extend(
                            parameters
                                .get(&p)
                                .into_iter()
                                .flat_map(|s| s.iter().flatten())
                                .copied(),
                        ),
                        // A non-borrowing opaque slot has no origin.
                        Key::None => {}
                    }
                }
            }
        }
        Ok(())
    }

    /// The map of call `i` of `b` when it passes or returns a value that
    /// may hold origins ([`CallSig`]): from its callee's signature, a trait
    /// method's from the trait's (`ref.dep.trait`), or the map a function
    /// value's type carries (`fn.type.lifetime.call`). A parameter of a
    /// bare type parameter bounded by a lifetime puts its slots in that
    /// lifetime (`ref.generic.outlives.argument`). A result slot a type
    /// parameter adds takes what that parameter's argument slots took, and
    /// nothing where no argument slot has it (`ref.generic.call`).
    fn call_sig(
        &self,
        ls: &Layouts<'_>,
        b: &Body,
        i: usize,
        values: &FnValues<'_>,
    ) -> Option<(CallSig, Vec<(Key, Var)>)> {
        let (pool, tag, names) = (ls.pool, b.tags[i], &self.cx.names);
        let (mut holds, mut held) = (ls.holds(b.ty[i]), false);
        let receiver = (tag == Tag::CallDyn).then(|| b.data[i][0]);
        // The caller's own lifetime of each slot of each trait argument, by
        // the bound a call through a type parameter's or an associated
        // type's bound uses.
        let (mut bound_ids, mut through_bound): (Vec<Vec<u32>>, bool) = (Vec::new(), false);
        slots(b, i, |_, v, word| {
            let arg = matches!(word, Word::Value(Role::Arg(_)));
            if (arg || receiver == Some(v)) && arg_ty(b, v).is_some_and(|t| ls.holds(t)) {
                holds = true;
                held |= arg && ls.holds(b.ty[v as usize]);
            }
        });
        if !holds {
            return None;
        }
        // Links of the slots of one implementation lifetime.
        let mut impl_links: Vec<(Var, Var)> = Vec::new();
        let (def, trait_, self_slots) = match tag {
            Tag::Call | Tag::Await => match Callee::from_words(b.record(b.data[i][0]))? {
                Callee::Item { def, .. } => (Some(def), None, Vec::new()),
                Callee::TraitMethod {
                    trait_,
                    method,
                    self_ty,
                    targs,
                    choice,
                } => {
                    let slots = match choice {
                        (ChoiceKind::Impl, imp) => {
                            let targs = pool.list_items(targs);
                            let imp = DefId::from_raw(imp);
                            self.impl_trait_slots(ls, imp, targs, &mut impl_links)
                        }
                        (ChoiceKind::TraitValue, _) => {
                            vec![self.trait_value_self_slots(ls, trait_, self_ty)]
                        }
                        (ChoiceKind::Bound, _) => {
                            bound_ids = self.bound_ids(ls, self_ty, trait_);
                            through_bound = true;
                            Vec::new()
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
                let t = source_arg(b, i, 0).and_then(|a| arg_ty(b, a))?;
                (
                    Some(method),
                    Some(*owner),
                    vec![self.trait_value_self_slots(ls, *owner, t)],
                )
            }
            Tag::CallValue => (None, None, Vec::new()),
            _ => return None,
        };
        let sig = match def {
            Some(def) => Some(self.sig_of(def)?),
            None => None,
        };
        let dep = match &sig {
            Some(sig) => self.with_wholes(ls, self.call_dep(ls, sig)),
            None => DepSig::of_fn(ls, *b.ty.get(b.data[i][0] as usize)?)?,
        };
        let is_trait_self = |p: ParamRef| trait_.is_some_and(|tr| p.owner == tr && p.index == 0);
        // A slot of the trait's `Self` that the implementation target's
        // lifetime fills (`ref.lifetime.trait`).
        // A slot of a trait argument whose bound writes the caller's
        // lifetime (`ref.lifetime.trait.bound-argument`).
        let own = |p: ParamRef, s: usize| {
            let k = usize::from(p.index).checked_sub(1)?;
            let at = bound_ids.get(k)?.get(s).copied();
            at.filter(|&l| l != 0 && trait_ == Some(p.owner))
                .map(Var::Own)
        };
        let target = |p: ParamRef, s: usize| {
            let k = usize::from(p.index);
            (trait_ == Some(p.owner))
                .then(|| self_slots.get(k)?.get(s).copied().flatten())
                .flatten()
        };
        // Where a capture slot a type parameter adds is kept.
        let declared = def.and_then(|d| Some((d, self.cx.lookup.item(d)?.sig()?)));
        let static_bound = names.known.static_bound;
        let dest = |p: ParamRef| {
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
        // Each projection's first lifetime its bounds write, and the
        // variables it makes one with the others.
        let mut projs: Vec<(Ty, Option<u32>)> = Vec::new();
        let mut proj_links: Vec<(Var, Var)> = Vec::new();
        let mut params = Vec::with_capacity(dep.params.len());
        for (p, (decl, ids)) in dep.params.iter().enumerate() {
            let at = source_arg(b, i, p).and_then(|a| call_arg_ty(pool, b, a, *decl));
            let (t, keys) = at
                .and_then(|t| Some((t, line_up_call(ls, *decl, t)?)))
                .unwrap_or_else(|| (*decl, (0..ids.len()).map(Key::Decl).collect()));
            let outlives = (def.zip(sig.as_ref()))
                .and_then(|(d, sig)| outlives_field(pool, d, &sig.generics, *decl));
            let written = ls.of(*decl);
            // Through a bound, the trait's own projection, as `Self::Item`
            // of `Mk[V]` with `S < Mk[&'a Point]`, is at the lifetimes of
            // the caller's projection it stands for, the caller's own
            // (`ref.lifetime.trait.bound-argument`).
            let own_whole =
                |actual: Ty| (self.whole_lifetimes(ls, actual).first().copied()).map(Var::Own);
            let slots = (keys.into_iter().zip(&ls.of(t).slots))
                .map(|(k, s)| {
                    let var = match k {
                        Key::Decl(j) => Var::Life(ids.get(j).copied().unwrap_or(NO_SOURCE)),
                        Key::None => Var::Life(NO_SOURCE),
                        Key::AllOf(p, _)
                            if s.whole
                                && through_bound
                                && trait_ == Some(p.owner)
                                && let Some(v) = own_whole(s.ty) =>
                        {
                            v
                        }
                        // Each slot of a projection takes each lifetime its
                        // bounds write (`ref.lifetime.trait.bound-argument`).
                        Key::AllOf(_, proj) => {
                            let known = projs.iter().find(|x| x.0 == proj).map(|x| x.1);
                            let first = known.unwrap_or_else(|| {
                                let lts = self.proj_lifetimes(ls, proj);
                                let first = lts.first().copied();
                                proj_links.extend(first.into_iter().flat_map(|a| {
                                    (lts[1..].iter()).map(move |&l| (Var::Life(a), Var::Life(l)))
                                }));
                                projs.push((proj, first));
                                first
                            });
                            Var::Life(first.unwrap_or(NO_SOURCE))
                        }
                        Key::Param(q, s) => own(q, s).unwrap_or_else(|| {
                            (target(q, s).or(outlives.map(u32::from)))
                                .map_or(Var::Param(q, s), Var::Life)
                        }),
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
                .collect::<Vec<_>>();
            // An argument with no slot of its own is passed as one origin
            // set: at the lifetimes of the projections its parameter holds,
            // as each slot of a projection is
            // (`ref.lifetime.trait.bound-argument`), and as the whole of
            // what each type parameter there stands for, slot 0 of a
            // type argument with none, which a result of that parameter
            // takes (`ref.generic.call`). All of them are one.
            let mut vs: Vec<Var> = Vec::new();
            if slots.is_empty() && ls.holds(t) {
                vs.extend(self.whole_lifetimes(ls, *decl).into_iter().map(Var::Life));
                if through_bound
                    && let Some(tr) = trait_
                    && own_proj(pool, &|d| d == tr, *decl)
                {
                    vs.extend(own_whole(t));
                }
                for q in bare_parts(pool, *decl).0 {
                    let v = own(q, 0).unwrap_or(Var::Param(q, 0));
                    took.push((Key::Param(q, 0), v));
                    vs.push(v);
                    // A trait argument the bound fixes, as `T::Item` of
                    // `S < Put[T::Item]`, is at that type's lifetimes, the
                    // caller's own.
                    if through_bound && trait_ == Some(q.owner) && q.index > 0 {
                        let lts = self.whole_lifetimes(ls, t);
                        proj_links.extend(lts.into_iter().map(|l| (v, Var::Own(l))));
                    }
                }
            }
            let slots = match vs.split_first() {
                Some((&a, rest)) => {
                    proj_links.extend(rest.iter().map(|&v| (a, v)));
                    vec![(a, None)]
                }
                None => slots,
            };
            params.push((t, slots));
        }
        // A result slot a type parameter adds takes what that parameter's
        // argument slots took, and a projection all those it holds.
        let from = |want: &dyn Fn(Key) -> bool| -> Vec<Var> {
            let mut vs: Vec<Var> = (took.iter()).filter(|x| want(x.0)).map(|x| x.1).collect();
            vs.dedup();
            vs
        };
        // A type parameter in the result of a function-typed parameter
        // whose inputs write lifetimes may hold those lifetimes, so a
        // result of that parameter takes them: `good(ident, p)` with
        // `f: fn(&'a Point) -> T` returns what `p` holds
        // (`fn.type.lifetime.result.param`).
        let mut fn_held: Vec<(ParamRef, u32)> = Vec::new();
        if let Some(sig) = &sig {
            let n = sig.params.len();
            for (p, lts) in sig.fn_lifetimes.iter().take(n).enumerate() {
                let TyData::Fn { result, .. } = pool.get(sig.params[p].1) else {
                    continue;
                };
                let lts = own_inputs(pool, sig.params[p].1, lts);
                for q in bare_parts(pool, result).0 {
                    for &l in lts.iter().filter(|&&l| l != 0) {
                        if !fn_held.contains(&(q, u32::from(l))) {
                            fn_held.push((q, u32::from(l)));
                        }
                    }
                }
            }
        }
        let held_by = |q: ParamRef| {
            (fn_held.iter())
                .filter(move |x| x.0 == q)
                .map(|x| Var::Life(x.1))
        };
        let out = |k: Key| match k {
            Key::Decl(j) => (dep.outs.get(j).copied().flatten())
                .map(Var::Life)
                .into_iter()
                .collect(),
            Key::None => Vec::new(),
            Key::Param(q, j) if let Some(v) = own(q, j) => vec![v],
            Key::Param(q, j) => {
                let mut vs = match target(q, j) {
                    Some(l) => vec![Var::Life(l)],
                    // A trait type argument the implementation fills in may
                    // name `Self`'s lifetimes (`ref.lifetime.impl-target.assoc`).
                    None if trait_.is_some_and(|tr| q.owner == tr && q.index > 0) => {
                        from(&|x| x == k || matches!(x, Key::Param(p, _) if is_trait_self(p)))
                    }
                    None => from(&|x| x == k),
                };
                vs.extend(held_by(q).filter(|v| !vs.contains(v)).collect::<Vec<_>>());
                vs
            }
            Key::AllOf(_, proj) => {
                let mut vs = from(&|x| matches!(x, Key::Param(p, _) if proj_holds(pool, proj, p)));
                vs.extend(self.proj_lifetimes(ls, proj).into_iter().map(Var::Life));
                vs
            }
        };
        // A suspension, the result of a bang call made without `!`, holds
        // what its arguments hold in a slot of its own, and its result's
        // slots depend as the call's with `!` would (`ref.dep.suspension`).
        let output = match pool.get(b.ty[i]) {
            TyData::Adt(AdtId::Declared(def), args) if def == ls.suspend => {
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
        // A result with no slot of its own that holds projections holds
        // what each of them holds: what its base and its trait's type
        // arguments stand for, and its lifetimes ([`Key::AllOf`]), not
        // the elided lifetime (`ref.dep.elide`).
        let mut bare: Vec<Var> = dep.whole.iter().map(|&l| Var::Life(l)).collect();
        for proj in wholes(ls, dep.ret).0 {
            for v in from(&|x| matches!(x, Key::Param(p, _) if proj_holds(pool, proj, p))) {
                if !bare.contains(&v) {
                    bare.push(v);
                }
            }
        }
        // And what each type parameter it holds stood for, which writes no
        // reference either: no lifetime elides to a result of `T` that its
        // instance gives no slot (`ref.generic.call`, `ref.dep.elide`).
        let (ret_params, ret_other) = bare_parts(pool, dep.ret);
        for &q in &ret_params {
            for v in out(Key::Param(q, 0)) {
                if !bare.contains(&v) {
                    bare.push(v);
                }
            }
        }
        let elides = ret_params.is_empty()
            || ret_other
            || !ls.of(dep.ret).slots.is_empty()
            || bare_parts(pool, b.ty[i]).1;
        let depends = outs.iter().any(|o| !o.is_empty())
            || lifetime(dep.elided).is_some()
            || !bare.is_empty();
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
        let paths = match &sig {
            Some(sig) if !sig.paths.is_empty() => self.call_paths(ls, b, i, sig, &dep),
            _ => Vec::new(),
        };
        let mut links = match (tag, &sig) {
            (Tag::Call | Tag::Await, Some(sig)) => self.call_links(ls, b, i, sig),
            _ => Vec::new(),
        };
        links.append(&mut proj_links);
        links.append(&mut impl_links);
        // A call through a bound passes at the trait's lifetimes the ones
        // the bound writes, the caller's own: `s.put(&q)` with
        // `S < Store['a]` stores at `'a` (`ref.lifetime.trait.bound-argument`).
        // A trait method numbers its trait's lifetimes first.
        if let Some(tr) = trait_.filter(|_| !bound_ids.is_empty())
            && let Some(generics) = self.cx.lookup.item(tr).map(|it| &it.generics)
        {
            let lts = trait_lifetimes(generics, &bound_ids);
            let pairs = (lts.into_iter().enumerate()).filter(|x| x.1 != 0);
            links.extend(pairs.map(|(k, l)| (Var::Life(w(k + 1)), Var::Own(l))));
        }
        // A function value's type writes lifetimes the type does not keep:
        // `f(&q)` with `f: fn(&'a Point) -> T` passes `&q` at `'a`, the
        // caller's or a call's, wherever `f` comes from; a call passing
        // `f` for a parameter whose type writes `'b` makes `'b` that one
        // (`fn.type.lifetime.call.written`).
        let (mut by_value, mut closures) = (false, Vec::new());
        if tag == Tag::CallValue {
            let ins: Vec<u32> = (dep.params.iter())
                .flat_map(|(t, ids)| ref_ids_of(ls, *t, ids))
                .collect();
            for (k, var) in self.written_inputs(ls, b, (b.data[i][0], Vec::new()), values) {
                if let Some(&id) = ins.get(k) {
                    links.push((Var::Life(id), var));
                    by_value = true;
                }
            }
        } else if let Some(sig) = &sig {
            let n = sig.params.len();
            for (p, lts) in sig.fn_lifetimes.iter().take(n).enumerate() {
                // A function type nested in the parameter's type is
                // matched to no part of the argument.
                let lts = own_inputs(ls.pool, sig.params[p].1, lts);
                let Some(a) = (lts.iter().any(|&l| l != 0))
                    .then(|| source_arg(b, i, p))
                    .flatten()
                else {
                    continue;
                };
                if b.tags[a as usize] == Tag::Closure && !b.is_instantiation(a as usize) {
                    closures.push((a, lts.to_vec()));
                }
                for (k, var) in self.written_inputs(ls, b, (a, Vec::new()), values) {
                    if let Some(&l) = lts.get(k).filter(|&&l| l != 0) {
                        links.push((Var::Life(u32::from(l)), var));
                        by_value = true;
                    }
                }
            }
            // A function type declared in a field of a parameter's struct
            // type writes the struct's lifetime, which the parameter's type
            // gives one of the callee's: the argument's function value
            // there makes that lifetime the value's, as a parameter of the
            // function type does. A call's result that holds the field's
            // value then passes its arguments at the caller's lifetime:
            // `relay(h, p).f(&q)` with `h: Holder['a, T]`.
            for (p, &(_, pt)) in sig.params.iter().enumerate() {
                let ids = dep.params.get(p).map_or(&[][..], |x| &x.1[..]);
                let arg = source_arg(b, i, p);
                for path in self.fn_field_paths(ls, pt) {
                    let found = self.field_inputs(ls, pt, &path, 0).unwrap_or_default();
                    if let Some(a) = arg.filter(|_| !found.is_empty()) {
                        let written = self.written_inputs(ls, b, (a, path), values);
                        for ((k, slot), &(k2, var)) in found
                            .iter()
                            .flat_map(|f| written.iter().map(move |x| (f, x)))
                        {
                            if let Some(&l) = ids.get(*slot).filter(|_| *k == k2) {
                                links.push((Var::Life(l), var));
                                by_value = true;
                            }
                        }
                    }
                }
            }
        }
        // Export runtime dependencies after lining up generic arguments,
        // not just the reference types spelled in the declaration. Keep
        // the existing lifetime-variable links, including trait bindings.
        let bare: Vec<_> = ([dep.elided, dep.capture_elided].map(lifetime).into_iter())
            .flatten()
            .filter(|_| elides)
            .chain(bare)
            .collect();
        let (held_args, loan_slots) = if ls.holds(b.ty[i]) {
            let input_vars: Vec<_> = params
                .iter()
                .flat_map(|(_, slots)| slots.iter().map(|s| s.0))
                .collect();
            let origins = call_origins(&input_vars, &outs, &bare, &links, || {});
            let held_args = params
                .iter()
                .enumerate()
                .filter(|(_, (_, slots))| slots.iter().any(|(v, _)| origins.returned.contains(v)))
                .filter_map(|(p, _)| source_arg(b, i, p))
                .collect();
            let loan_slots = self.call_loan_slots(ls, b, i, &params, &outs, &bare, &origins);
            (held_args, loan_slots)
        } else {
            (Vec::new(), Vec::new())
        };
        Some((
            CallSig {
                links,
                by_value,
                closures,
                paths,
                store: matches!(tag, Tag::Call | Tag::Await) && held && writes && shared,
                result: ls.holds(b.ty[i]) && (tag == Tag::CallValue || depends),
                bare,
                params,
                outs,
                held: held_args,
                loan_slots,
            },
            took,
        ))
    }

    fn call_loan_slots(
        &self,
        ls: &Layouts<'_>,
        b: &Body,
        call: usize,
        params: &[(Ty, Vec<ArgSlot>)],
        outs: &[Vec<Var>],
        bare: &[Var],
        origins: &CallOrigins,
    ) -> Vec<hd_tir::ir::LoanSlot> {
        let mut result = Vec::new();
        let mut seen = HashSet::default();
        let mut emit = |value, ty, slots: &[Vec<Var>], whole: &[Var]| {
            let shapes = ls.loan_shapes.borrow_mut().of(ls, self.cx, ty);
            for shape in shapes.iter() {
                let selected: Vec<_> = match &shape.slots {
                    Some(indices) => indices
                        .iter()
                        .flat_map(|&s| slots.get(s).into_iter().flatten())
                        .copied()
                        .collect(),
                    None => slots.iter().flatten().chain(whole).copied().collect(),
                };
                for var in selected {
                    if !origins.returned.contains(&var) {
                        continue;
                    }
                    let Some(&origin) = origins.classes.get(&var) else {
                        continue;
                    };
                    if seen.insert((value, shape.path.clone(), origin, shape.expand)) {
                        result.push(hd_tir::ir::LoanSlot {
                            call: w(call),
                            value,
                            path: shape.path.clone(),
                            origin,
                            expand: shape.expand,
                        });
                    }
                }
            }
        };
        for (parameter, (ty, slots)) in params.iter().enumerate() {
            if let Some(argument) = source_arg(b, call, parameter) {
                if hd_tir::ir::Ref(argument).as_inst().is_none() {
                    continue;
                }
                let slots: Vec<_> = slots.iter().map(|&(v, _)| vec![v]).collect();
                emit(argument, *ty, &slots, &[]);
            }
        }
        emit(w(call), b.ty[call], outs, bare);
        result
    }

    /// The input slots of function value `v` that write a lifetime, each
    /// with the variable an argument there is passed at
    /// (`fn.type.lifetime.call.written`). The type keeps no lifetime names
    /// (`fn.type.lifetime.identity`), so they come from where the value
    /// does: a parameter's or a field's declared function type, through
    /// copies, coercions, struct literals and closure captures, or the
    /// declared result of the call that returned it. Each slot is in
    /// [`Types::ref_slots`] order over the inputs.
    fn written_inputs(
        &self,
        ls: &Layouts<'_>,
        b: &Body,
        (v, path): (u32, Vec<Part>),
        values: &FnValues<'_>,
    ) -> Vec<(usize, Var)> {
        let local = (b.tags.get(v as usize) == Some(&Tag::Read))
            .then(|| b.decode_place(b.data[v as usize][0]))
            .flatten()
            .and_then(|p| match p.root {
                PlaceRoot::Local(l) if p.proj.is_empty() => Some((l.raw(), path.clone())),
                _ => None,
            });
        if let Some(found) = local
            .as_ref()
            .and_then(|k| values.inputs.borrow().get(k).cloned())
        {
            return found;
        }
        let mut out = Vec::new();
        let push = |out: &mut Vec<(usize, Var)>, k: usize, var: Var| {
            if !out.contains(&(k, var)) {
                out.push((k, var));
            }
        };
        // A work stack of the parts to trace, in the order a recursive
        // walk takes them: a body's value graph is as deep as its program,
        // so a depth cap traced nothing past it (#1777). Values lead to
        // earlier values, and each local's part is traced once (`seen`),
        // which ends a cycle through a binding.
        let mut seen: HashSet<(u32, Vec<Part>)> = HashSet::default();
        let mut stack: Vec<Trace> = vec![Trace::Value(v, path)];
        while let Some(t) = stack.pop() {
            crate::work::fn_input_trace();
            match t {
                Trace::Value(v, path) => {
                    self.trace_value(ls, b, (v, path), &mut stack, &mut |k, var| {
                        push(&mut out, k, var);
                    });
                }
                Trace::Local(l, path) => {
                    if !seen.contains(&(l, path.clone())) {
                        seen.insert((l, path.clone()));
                        self.trace_local(ls, b, values, (l, &path), &mut stack, &mut out);
                    }
                }
            }
        }
        if let Some(k) = local {
            values.inputs.borrow_mut().insert(k, out.clone());
        }
        out
    }

    /// [`Ck::written_inputs`] of the part `path` of value `v`: what it
    /// writes itself goes to `push`, the parts it is made of to `stack`.
    fn trace_value(
        &self,
        ls: &Layouts<'_>,
        b: &Body,
        (v, path): (u32, Vec<Part>),
        stack: &mut Vec<Trace>,
        push: &mut dyn FnMut(usize, Var),
    ) {
        let i = v as usize;
        if v & hd_tir::ir::Ref::CONST_BIT != 0 || i >= b.tags.len() {
            return;
        }
        // A named function's value writes no lifetime of the body's in its
        // inputs, nor does a value of no function type. A closure literal
        // writes those its parameter annotations do.
        let tag = b.tags[i];
        // A reference's referent is the place it was taken of: `&h` for a
        // `&Holder['a, T]` parameter.
        let read = match (tag, path.split_first()) {
            (Tag::Read, _) => Some(&path[..]),
            (Tag::Ref, Some((Part::Proj(Proj::Deref), rest))) => Some(rest),
            _ => None,
        };
        if tag == Tag::Coerce {
            // This traces written signature lifetimes, not value identity.
            // RowAdapt changes only the effect row: its parameters and result
            // retain the source callable's written lifetime relationships.
            stack.push(Trace::Value(b.data[i][0], path));
        } else if tag == Tag::ItemRef && path.is_empty() {
            // A named function's value needs what its signature writes
            // `'static` (`fn.type.lifetime.input`).
            let def = b.record(b.data[i][0]).first().copied().map(DefId::from_raw);
            if let Some(sig) = def.and_then(|d| self.cx.lookup.item(d)?.sig()) {
                let dep = self.call_dep(ls, sig);
                let ins = (dep.params.iter()).flat_map(|(t, ids)| ref_ids_of(ls, *t, ids));
                for (k, l) in ins.enumerate().filter(|x| x.1 == STATIC) {
                    push(k, Var::Own(l));
                }
            }
        } else if tag == Tag::Closure && path.is_empty() {
            let rec = b.syn.get(i).and_then(|n| self.closure_lifetimes.get(n));
            for (k, &l) in rec.into_iter().flatten().enumerate() {
                if l != 0 {
                    push(k, Var::Own(u32::from(l)));
                }
            }
        } else if let Some(rest) = read
            && let Some(place) = b.decode_place(b.data[i][0])
        {
            let full: Vec<Part> = (place.proj.iter().map(|&p| Part::Proj(p)))
                .chain(rest.iter().copied())
                .collect();
            match place.root {
                PlaceRoot::Local(l) => stack.push(Trace::Local(l.raw(), full)),
                PlaceRoot::Value(r) => stack.push(Trace::Value(r.0, full)),
                // A top-level binding's type names no lifetime of the
                // body's, and the frame and pointer roots hold no
                // function value the checker built.
                PlaceRoot::Global(..) | PlaceRoot::Frame(_) | PlaceRoot::Ptr(_) => {}
            }
        } else if matches!(tag, Tag::NewStruct | Tag::NewTuple)
            && let [Part::Proj(Proj::Field(j)), rest @ ..] = path.as_slice()
            && let Some(&f) = b.record(b.data[i][1]).get(*j as usize)
        {
            // A struct or tuple literal's field is the value it was given;
            // a tuple's rest part is traced no further.
            stack.push(Trace::Value(f, rest.to_vec()));
        } else if tag == Tag::NewVariant
            && let [
                Part::Proj(Proj::Variant(k)),
                Part::Proj(Proj::Field(j)),
                rest @ ..,
            ] = path.as_slice()
            && *k == b.data[i][0]
            && let Some(&f) = b.record(b.data[i][1]).get(*j as usize)
        {
            // A variant's field is the value it was given.
            stack.push(Trace::Value(f, rest.to_vec()));
        } else if tag == Tag::CallValue {
            // A field of the result is at the call's own lifetimes, which
            // the function value's type gives it, whatever function the
            // value holds: `g(p).f(&q)` with `g: fn(&Point) -> Holder[T]`
            // passes `&q` at `p`'s lifetime.
            let callee = b.data[i][0];
            if let Some(dep) = arg_ty(b, callee).and_then(|t| DepSig::of_fn(ls, t))
                && let Some(Spot::Field(at)) = fn_spot(self.cx, ls.pool, dep.ret, &path)
            {
                self.result_field_at(ls, (v, &dep), &path, at, push);
            }
            // A call through a function value returns part of that value's
            // type: `f(p)(&q)` calls the function type in `f`'s result.
            let full = std::iter::once(Part::Result).chain(path).collect();
            stack.push(Trace::Value(callee, full));
        } else if matches!(tag, Tag::Call | Tag::Await)
            && let Some(sig) = (Callee::from_words(b.record(b.data[i][0])))
                .map(|c| match c {
                    Callee::Item { def, .. } => def,
                    Callee::TraitMethod { method, .. } => method,
                })
                .and_then(|def| self.cx.lookup.item(def)?.sig())
        {
            // A call's result is at the call's own lifetimes.
            let rec = sig.fn_lifetimes.get(sig.params.len());
            match fn_spot(self.cx, ls.pool, sig.ret, &path) {
                Some(Spot::Fn(lo, n)) => {
                    let lts = rec.and_then(|r| r.get(lo..lo + n)).unwrap_or(&[]);
                    for (k, &l) in lts.iter().enumerate().filter(|x| *x.1 != 0) {
                        push(k, Var::At(v, u32::from(l)));
                    }
                }
                Some(Spot::Field(at)) => {
                    self.result_field_at(ls, (v, &self.call_dep(ls, sig)), &path, at, push);
                }
                Some(Spot::ResultField { from, at, lo, ty }) => {
                    let rec = rec.and_then(|r| r.get(lo..)).unwrap_or(&[]);
                    for (k, l) in self
                        .result_field_inputs(ls, (ty, &path[from..], at), rec)
                        .unwrap_or_default()
                    {
                        push(k, Var::At(v, u32::from(l)));
                    }
                }
                None => {}
            }
        }
    }

    /// The map a call of the function or method `sig` passes and returns
    /// its arguments' origins by (`ref.dep.position`): the receiver's
    /// lifetime elides, and a result path takes its parameter's.
    fn call_dep(&self, ls: &Layouts<'_>, sig: &hd_resolve::FnSig) -> DepSig {
        let is_self = |s| self.cx.names.text(s) == "self";
        DepSig::new(ls, &sig.params, is_self, sig.ret, &sig.lifetimes)
            .with_paths(ls, &sig.paths)
            .with_crossing(ls, &sig.crossing)
    }

    /// The input slots of the function type in the field that `path`
    /// reads at step `at` of call `v`'s result, by the call's map `dep`:
    /// each at the call's lifetime of the result slot that holds the
    /// struct's lifetime it writes ([`Var::At`]), elided or written.
    fn result_field_at(
        &self,
        ls: &Layouts<'_>,
        (v, dep): (u32, &DepSig),
        path: &[Part],
        at: usize,
        push: &mut dyn FnMut(usize, Var),
    ) {
        for (k, slot) in self.field_inputs(ls, dep.ret, path, at).unwrap_or_default() {
            if slot == STATIC_SLOT {
                push(k, Var::Life(STATIC));
            } else if let Some(Some(l)) = dep.outs.get(slot) {
                push(k, Var::At(v, *l));
            }
        }
    }

    /// The paths to each struct field of type `t` whose declared type is
    /// a function type that writes a lifetime, through a reference at the
    /// top and through fields of struct types with lifetimes. Only a
    /// struct with lifetimes has such a field, so another type costs one
    /// look.
    fn fn_field_paths(&self, ls: &Layouts<'_>, t: Ty) -> Vec<Vec<Part>> {
        let pool = ls.pool;
        let mut out = Vec::new();
        let top = match pool.get(t) {
            TyData::Ref {
                counted: false,
                inner,
                ..
            } => (inner, vec![Part::Proj(Proj::Deref)]),
            _ => (t, Vec::new()),
        };
        // Each struct type to look in, with its path and the structs it
        // lies in, which a struct held by value never repeats.
        let mut stack: Vec<(Ty, Vec<Part>, Vec<DefId>)> = vec![(top.0, top.1, Vec::new())];
        while let Some((t, path, mut outer)) = stack.pop() {
            let Some((def, _)) = pool.adt(t) else {
                continue;
            };
            if !(ls.lifetimed)(def) || outer.contains(&def) {
                continue;
            }
            let Some(ItemData::Struct(fields, _)) = self.cx.lookup.item(def).map(|it| &it.data)
            else {
                continue;
            };
            outer.push(def);
            for (j, f) in fields.iter().enumerate() {
                let Ok(j) = u32::try_from(j) else { break };
                let mut here = path.clone();
                here.push(Part::Proj(Proj::Field(j)));
                let written = f.fn_lifetimes.iter().any(|&l| l != 0);
                if written && matches!(pool.get(f.ty), TyData::Fn { .. }) {
                    out.push(here);
                } else if matches!(pool.get(f.ty), TyData::Adt(..)) {
                    stack.push((f.ty, here, outer.clone()));
                }
            }
        }
        out
    }

    /// The input slots of a function type declared in a struct's or
    /// variant's field, which a step of `path` at or after `from` reads
    /// in a value of type `ty`, each with the slot of `ty`'s layout that
    /// holds the struct's lifetime it writes (`Field::fn_lifetimes`): the
    /// field whose own declared type holds the function type the rest of
    /// the path reaches. `None` when a step before it leaves the slots
    /// `ty`'s layout tracks.
    fn field_inputs(
        &self,
        ls: &Layouts<'_>,
        mut ty: Ty,
        path: &[Part],
        from: usize,
    ) -> Option<Vec<(usize, usize)>> {
        let pool = ls.pool;
        // The slots of `ty` the part reached so far lies in, as
        // [`constraints::Gen::project`] walks them.
        let mut slots: Vec<usize> = (0..ls.of(ty).slots.len()).collect();
        let mut variant = None;
        for (i, step) in path.iter().enumerate() {
            match *step {
                Part::Proj(Proj::Deref) => {
                    let (counted, _) = local_ref(pool, ty)?;
                    if !counted {
                        slots = slots.get(1..)?.to_vec();
                    }
                    ty = referent(pool, ty);
                }
                Part::Proj(Proj::Variant(k)) => variant = Some(k),
                Part::Proj(Proj::Field(j)) => {
                    if i >= from
                        && let Some(found) = self.field_record(ls, ty, variant, j, &path[i + 1..])
                    {
                        return found
                            .into_iter()
                            .map(|(k, s)| match s {
                                STATIC_SLOT => Some((k, s)),
                                _ => Some((k, *slots.get(s)?)),
                            })
                            .collect();
                    }
                    let (child, map) = ls.field_slots(self.cx, ty, variant.take(), j)?;
                    slots = map?
                        .iter()
                        .map(|&k| slots.get(k).copied())
                        .collect::<Option<_>>()?;
                    ty = child;
                }
                // A list's slots are its element's, as for any generic
                // instance (`ref.local.holding`).
                Part::Proj(Proj::Elem(_)) => match pool.get(ty) {
                    TyData::Adt(AdtId::Declared(def), args) if def == self.cx.names.known.list => {
                        ty = *pool.list_items(args).first()?;
                    }
                    _ => return None,
                },
                Part::Result => return None,
            }
        }
        None
    }

    /// When field `j` of struct or enum type `ty` (variant `variant`)
    /// declares the function type `rest` reaches in it: each of that
    /// function type's input slots that writes a struct lifetime, with
    /// the slot of `ty`'s layout that lifetime is at.
    fn field_record(
        &self,
        ls: &Layouts<'_>,
        ty: Ty,
        variant: Option<u32>,
        j: u32,
        rest: &[Part],
    ) -> Option<Vec<(usize, usize)>> {
        let pool = ls.pool;
        let (def, types) = pool.adt(ty)?;
        let item = self.cx.lookup.item(def)?;
        let fields = match (&item.data, variant) {
            (ItemData::Struct(fs, _), None) => fs,
            (ItemData::Enum { variants, .. }, Some(k)) => &variants.get(k as usize)?.fields,
            _ => return None,
        };
        let field = fields.get(j as usize)?;
        let type_items = pool.list_items(types);
        let (lo, n) = match fn_spot(self.cx, pool, field.ty, rest)? {
            Spot::Fn(lo, n) => (lo, n),
            // A field of the field's function type's result, as
            // `(m.g)(p).f` with `g: fn(&'a Point) -> Holder['a, T]`, is
            // at the struct's lifetime that result writes. `from` is past
            // the path's last result step, so the walk below meets no
            // other and comes back here at most once.
            Spot::ResultField { from, at, lo, ty } => {
                let rec = field.fn_lifetimes.get(lo..)?;
                let found = self.result_field_inputs(ls, (ty, &rest[from..], at), rec)?;
                return found
                    .into_iter()
                    .map(|(k, l)| Some((k, ls.lifetime_slot(&item.generics, type_items, l)?)))
                    .collect();
            }
            Spot::Field(_) => return None,
        };
        let mut out = Vec::new();
        for (k, &l) in field.fn_lifetimes.get(lo..lo + n)?.iter().enumerate() {
            if l == hd_resolve::STATIC_LIFETIME {
                out.push((k, STATIC_SLOT));
            } else if l != 0 {
                out.push((k, ls.lifetime_slot(&item.generics, type_items, l)?));
            }
        }
        Some(out)
    }

    /// [`Ck::written_inputs`] of the part `path` of local `l`: a
    /// parameter's by its declared type, a binding's by what each store
    /// into it, or into the part read, puts there (to `stack`, first
    /// store on top).
    fn trace_local(
        &self,
        ls: &Layouts<'_>,
        b: &Body,
        values: &FnValues<'_>,
        (l, path): (u32, &[Part]),
        stack: &mut Vec<Trace>,
        out: &mut Vec<(usize, Var)>,
    ) {
        let param = b.kind == BodyKind::Fn
            && (b.local_flags.get(l as usize))
                .is_some_and(|f| f & hd_tir::ir::local_flags::PARAM != 0);
        let sig = self.cx.lookup.item(b.item).and_then(|it| it.sig());
        if param && let Some(sig) = sig.filter(|sig| (l as usize) < sig.params.len()) {
            self.trace_param(ls, sig, values, (l as usize, path), out);
            return;
        }
        let at = stack.len();
        for &s in values.stores(l) {
            let Some(place) = b.decode_place(b.data[s as usize][0]) else {
                continue;
            };
            let n = place.proj.len();
            let into =
                path.len() >= n && (place.proj.iter().zip(path)).all(|(&q, &p)| p == Part::Proj(q));
            if into {
                stack.push(Trace::Value(b.data[s as usize][1], path[n..].to_vec()));
            }
        }
        stack[at..].reverse();
    }

    /// [`Ck::written_inputs`] of the part `path` of parameter `p`: a
    /// function type in its declared type
    /// ([`hd_resolve::FnSig::fn_lifetimes`]), or one in a struct's field
    /// ([`hd_resolve::Field::fn_lifetimes`]) at the lifetime the
    /// parameter's type gives that struct's lifetime.
    fn trace_param(
        &self,
        ls: &Layouts<'_>,
        sig: &hd_resolve::FnSig,
        values: &FnValues<'_>,
        (p, path): (usize, &[Part]),
        out: &mut Vec<(usize, Var)>,
    ) -> Option<()> {
        let mut push = |k: usize, var: Var| {
            if !out.contains(&(k, var)) {
                out.push((k, var));
            }
        };
        match fn_spot(self.cx, ls.pool, sig.params.get(p)?.1, path)? {
            Spot::Fn(lo, n) => {
                let lts = sig.fn_lifetimes.get(p)?.get(lo..lo + n)?;
                for (k, &l) in lts.iter().enumerate().filter(|x| *x.1 != 0) {
                    push(k, Var::Own(u32::from(l)));
                }
            }
            Spot::Field(at) => {
                let &(ty, ref ids) = values.mine?.params.get(p)?;
                for (k, slot) in self.field_inputs(ls, ty, path, at)? {
                    match slot {
                        STATIC_SLOT => push(k, Var::Life(STATIC)),
                        _ => push(k, Var::Own(*ids.get(slot)?)),
                    }
                }
            }
            Spot::ResultField { from, at, lo, ty } => {
                let rec = sig.fn_lifetimes.get(p)?.get(lo..)?;
                for (k, l) in self.result_field_inputs(ls, (ty, &path[from..], at), rec)? {
                    push(k, Var::Own(u32::from(l)));
                }
            }
        }
        Some(())
    }

    /// [`Ck::field_inputs`] of part `path` of a function type's result of
    /// type `ty`, whose reference slots write `rec` in
    /// [`Types::ref_slots`] order: each input slot, with the lifetime the
    /// result writes at the slot of the struct's lifetime it writes, where
    /// one is written. `g(p).f(&q)` with
    /// `g: fn(&'a Point) -> Holder['a, T]` passes `&q` at `'a`
    /// (`fn.type.lifetime.call.written`).
    fn result_field_inputs(
        &self,
        ls: &Layouts<'_>,
        (ty, path, at): (Ty, &[Part], usize),
        rec: &[u16],
    ) -> Option<Vec<(usize, u16)>> {
        // Each layout slot's place among the reference slots, which a
        // function value's capture slot and a whole slot are not.
        let mut next = 0;
        let refs: Vec<Option<usize>> = (ls.of(ty).slots.iter())
            .map(|s| {
                (!s.fun && !s.whole).then(|| {
                    next += 1;
                    next - 1
                })
            })
            .collect();
        let found = self.field_inputs(ls, ty, path, at)?;
        Some(
            found
                .into_iter()
                .filter_map(|(k, slot)| Some((k, *rec.get((*refs.get(slot)?)?)?)))
                .filter(|&(_, l)| l != 0)
                .collect(),
        )
    }

    /// The lifetimes of projection `proj`, in its scope's numbering,
    /// sorted: those its bounds write, those its base's bounds give its
    /// trait's arguments, and its base's own when the base is a projection
    /// too. Each slot of what it stands for takes them all, as a type
    /// parameter's slots take its bounds' (`ref.lifetime.trait.bound-argument`).
    fn proj_lifetimes(&self, ls: &Layouts<'_>, proj: Ty) -> Vec<u32> {
        let mut out = Vec::new();
        self.proj_lifetimes_into(ls, proj, &mut out);
        out.retain(|&l| l != 0);
        out.sort_stable();
        out.dedup();
        out
    }

    /// [`Ck::proj_lifetimes`] unsorted. Base queries share their completed
    /// bound answers instead of rewalking the projection chain per level.
    fn proj_lifetimes_into(&self, ls: &Layouts<'_>, proj: Ty, out: &mut Vec<u32>) {
        let pool = ls.pool;
        let mut memo = HashMap::default();
        hd_base::stack::all((proj, false), |(proj, finish), stack| {
            let TyData::Assoc {
                assoc,
                trait_: owner,
                self_ty: base,
                ..
            } = pool.get(proj)
            else {
                return true;
            };
            if finish {
                let Some(ItemData::AssocType { bounds, .. }) =
                    self.cx.lookup.item(assoc).map(|it| &it.data)
                else {
                    return true;
                };
                for (tr, _) in bounds.iter().filter_map(|&b| pool.trait_value(b)) {
                    out.extend(self.bound_ids_in(ls, proj, tr, &mut memo).concat());
                }
            } else {
                hd_types::work::type_node();
                out.extend(self.bound_ids_in(ls, base, owner, &mut memo).concat());
                stack.push((proj, true));
                stack.push((base, false));
            }
            true
        });
    }

    /// The lifetimes a value of type `t` holds as a whole where its layout
    /// has no slot for them: those of each projection it holds, as
    /// `S::Item` or `List[S::Item]` with `S < Shelf['a]`. Empty for a type
    /// with slots of its own. A trait value's and an opaque type's
    /// arguments are no part of what they hold (`trait.dyn.ref.convert`,
    /// `types.opaque.borrows`).
    fn whole_lifetimes(&self, ls: &Layouts<'_>, t: Ty) -> Vec<u32> {
        let mut out: Vec<u32> = (wholes(ls, t).0.into_iter())
            .flat_map(|p| self.proj_lifetimes(ls, p))
            .collect();
        out.sort_stable();
        out.dedup();
        out
    }

    /// `dep` with each parameter whose type holds projections and has no
    /// slot at the first of their lifetimes, and its result's
    /// [`DepSig::whole`] theirs, so `-> S::Item: x` with `x: S::Item`
    /// returns what it was given (`ref.lifetime.return`). A caller links a
    /// projection's lifetimes as one ([`Key::AllOf`]), so the first stands
    /// for them all. A result that holds only projections writes no
    /// reference, so nothing elides to it (`ref.dep.elide`): it depends on
    /// its projections' lifetimes alone, as Rust's projection outlives rule
    /// has it, not on a receiver it was not built from.
    fn with_wholes(&self, ls: &Layouts<'_>, mut dep: DepSig) -> DepSig {
        // The first of a whole slot's lifetimes ([`Slot::whole`]).
        let first = |s: &Slot| (s.whole).then(|| self.whole_lifetimes(ls, s.ty).first().copied());
        for (t, ids) in &mut dep.params {
            if ids.is_empty()
                && let Some(&l) = self.whole_lifetimes(ls, *t).first()
            {
                ids.push(l);
            }
            for (id, s) in ids.iter_mut().zip(&ls.of(*t).slots) {
                if let Some(Some(l)) = first(s) {
                    *id = l;
                }
            }
        }
        for (o, s) in dep.outs.iter_mut().zip(&ls.of(dep.ret).slots) {
            if let Some(l) = first(s) {
                *o = l;
            }
        }
        let (projs, other) = wholes(ls, dep.ret);
        if !projs.is_empty() && !other {
            dep.elided = Elided::Missing;
            dep.capture_elided = Elided::Missing;
        }
        dep.whole = self.whole_lifetimes(ls, dep.ret);
        dep
    }

    /// The generics of `owner`: a function's or a method's signature's,
    /// or a type's, a trait's or an implementation's own.
    fn generics_of(&self, owner: DefId) -> Option<&[hd_resolve::Generic]> {
        let it = self.cx.lookup.item(owner)?;
        Some(it.sig().map_or(&it.generics[..], |s| &s.generics[..]))
    }

    /// For a call through a bound of `self_ty` on `trait_`, the lifetime
    /// the bound writes for each slot of each of the trait's arguments, in
    /// the numbering of the calling body's scope; 0 where it writes none
    /// (`ref.lifetime.trait.bound-argument`). The bound is a type
    /// parameter's, or an associated type's, whose trait's lifetimes are
    /// the ones the bounds of its projection's base give that trait
    /// (`ref.lifetime.trait.bound-unnamed`). Through a supertrait, its
    /// written lifetimes name the bound's lifetime arguments and its type
    /// parameters the bound's arguments. Empty where no bound writes one.
    fn bound_ids(&self, ls: &Layouts<'_>, self_ty: Ty, trait_: DefId) -> Vec<Vec<u32>> {
        match ls.pool.get(self_ty) {
            TyData::Param(p) => self.param_bound_ids(ls, p, trait_),
            TyData::Assoc { .. } => self.bound_ids_in(ls, self_ty, trait_, &mut HashMap::default()),
            _ => Vec::new(),
        }
    }

    fn bound_ids_in(
        &self,
        ls: &Layouts<'_>,
        self_ty: Ty,
        trait_: DefId,
        memo: &mut HashMap<(Ty, DefId), Vec<Vec<u32>>>,
    ) -> Vec<Vec<u32>> {
        enum Work {
            Enter(Ty, DefId, usize),
            Finish {
                key: (Ty, DefId),
                assoc: DefId,
                owner: DefId,
                args: hd_types::TyList,
                child: usize,
                out: usize,
            },
        }
        if let Some(done) = memo.get(&(self_ty, trait_)) {
            return done.clone();
        }
        let mut values = vec![Vec::new()];
        hd_base::stack::all(Work::Enter(self_ty, trait_, 0), |work, stack| {
            let (key, out, done) = match work {
                Work::Enter(ty, trait_, out) => {
                    if let Some(done) = memo.get(&(ty, trait_)) {
                        values[out].clone_from(done);
                        return true;
                    }
                    hd_types::work::type_node();
                    let done = match ls.pool.get(ty) {
                        TyData::Param(p) => self.param_bound_ids(ls, p, trait_),
                        TyData::Assoc {
                            assoc,
                            trait_: owner,
                            self_ty: base,
                            args,
                        } if matches!(
                            self.cx.lookup.item(assoc).map(|it| &it.data),
                            Some(ItemData::AssocType { .. })
                        ) =>
                        {
                            let child = values.len();
                            values.push(Vec::new());
                            stack.push(Work::Finish {
                                key: (ty, trait_),
                                assoc,
                                owner,
                                args,
                                child,
                                out,
                            });
                            stack.push(Work::Enter(base, owner, child));
                            return true;
                        }
                        _ => Vec::new(),
                    };
                    ((ty, trait_), out, done)
                }
                Work::Finish {
                    key,
                    assoc,
                    owner,
                    args,
                    child,
                    out,
                } => {
                    let ids = std::mem::take(&mut values[child]);
                    (
                        key,
                        out,
                        self.assoc_bound_ids(ls, assoc, (owner, args), key.1, &ids),
                    )
                }
            };
            memo.insert(key, done.clone());
            values[out] = done;
            true
        });
        std::mem::take(&mut values[0])
    }

    /// [`Ck::bound_ids`] of type parameter `p`.
    fn param_bound_ids(&self, ls: &Layouts<'_>, p: ParamRef, trait_: DefId) -> Vec<Vec<u32>> {
        let Some(it) = self.cx.lookup.item(p.owner) else {
            return Vec::new();
        };
        // A trait's parameters follow its `Self`, which is bounded by the
        // trait over its own parameters, lifetimes 1 and on in its scope.
        let k = usize::from(p.index);
        let k = match it.data {
            ItemData::Trait(_) if k == 0 => return self.self_bound_ids(ls, it, trait_),
            ItemData::Trait(_) => k - 1,
            _ => k,
        };
        let Some(g) = it
            .sig()
            .map_or(&it.generics[..], |s| &s.generics[..])
            .get(k)
        else {
            return Vec::new();
        };
        if g.bound_lifetimes.is_empty() {
            return Vec::new();
        }
        for (b, lts) in g.bounds.iter().zip(&g.bound_lifetimes) {
            let Some((def, args)) = ls.pool.trait_value(*b) else {
                continue;
            };
            let args = ls.pool.list_items(args);
            let ids = arg_ids(
                ls,
                args,
                &lts.iter().map(|&l| u32::from(l)).collect::<Vec<_>>(),
            );
            if let Some(found) = self.reach(ls, (def, args), ids, trait_) {
                return found;
            }
        }
        Vec::new()
    }

    /// [`Ck::bound_ids`] of trait `tr`'s `Self`: `tr` over its own
    /// parameters, whose lifetimes are the first of its scope.
    fn self_bound_ids(
        &self,
        ls: &Layouts<'_>,
        tr: &hd_resolve::Item,
        trait_: DefId,
    ) -> Vec<Vec<u32>> {
        if !tr.generics.iter().any(|g| g.lifetime) {
            return Vec::new();
        }
        let pool = ls.pool;
        let mut n = 0;
        let (mut args, mut ids) = (Vec::new(), Vec::new());
        for (k, g) in tr.generics.iter().enumerate() {
            if g.lifetime {
                n += 1;
                args.push(hd_resolve::lifetime_arg(pool));
                ids.push(vec![n]);
            } else {
                let index = u16::try_from(k + 1).unwrap_or(u16::MAX);
                let t = pool.intern_ty(&TyData::Param(ParamRef {
                    owner: tr.def,
                    index,
                }));
                ids.push(vec![0; ls.of(t).slots.len()]);
                args.push(t);
            }
        }
        self.reach(ls, (tr.def, &args), ids, trait_)
            .unwrap_or_default()
    }

    /// [`Ck::bound_ids`] of the projection of `assoc` on `owner[args]`
    /// over `base`: each bound of `assoc`, written in `owner`'s scope, over
    /// the lifetimes `base`'s bounds give `owner`'s arguments.
    fn assoc_bound_ids(
        &self,
        ls: &Layouts<'_>,
        assoc: DefId,
        (owner, args): (DefId, hd_types::TyList),
        trait_: DefId,
        ids: &[Vec<u32>],
    ) -> Vec<Vec<u32>> {
        let Some(ItemData::AssocType {
            bounds,
            bound_lifetimes,
            ..
        }) = self.cx.lookup.item(assoc).map(|it| &it.data)
        else {
            return Vec::new();
        };
        if ids.iter().flatten().all(|&l| l == 0) {
            return Vec::new();
        }
        let Some(it) = self.cx.lookup.item(owner) else {
            return Vec::new();
        };
        let (args, lifetimes) = (ls.pool.list_items(args), trait_lifetimes(&it.generics, ids));
        let none = Vec::new();
        for (k, &b) in bounds.iter().enumerate() {
            let lts = bound_lifetimes.get(k).unwrap_or(&none);
            let Some((def, bids, bargs)) =
                Self::bound_in(ls, (owner, args), ids, &lifetimes, b, lts)
            else {
                continue;
            };
            if let Some(found) = self.reach(ls, (def, &bargs), bids, trait_) {
                return found;
            }
        }
        Vec::new()
    }

    /// The slot lifetimes of `target`'s arguments, reached from trait
    /// `def` whose arguments' slots have lifetimes `ids`: `def` itself, or
    /// one of its supertraits, the first a left-to-right depth-first
    /// search reaches.
    ///
    /// A work stack, each trait entered once: a supertrait chain is as
    /// long as the program's, so a depth cap of 8 answered "no lifetimes"
    /// past it (#1777). Whether `target` lies above a trait does not
    /// depend on the lifetimes it is reached with, so a trait entered
    /// once and left without it is not entered again. One that writes
    /// no lifetime, reached with none, is skipped and not marked: all it
    /// could give is no lifetimes.
    fn reach(
        &self,
        ls: &Layouts<'_>,
        (def, args): (DefId, &[Ty]),
        ids: Vec<Vec<u32>>,
        target: DefId,
    ) -> Option<Vec<Vec<u32>>> {
        let mut stack = vec![(def, args.to_vec(), ids)];
        let mut entered: HashSet<DefId> = HashSet::default();
        let none = Vec::new();
        while let Some((def, args, ids)) = stack.pop() {
            if def == target {
                return Some(ids);
            }
            let Some(ItemData::Trait(td)) = self.cx.lookup.item(def).map(|it| &it.data) else {
                continue;
            };
            if td.super_lifetimes.is_empty() && ids.iter().flatten().all(|&l| l == 0)
                || !entered.insert(def)
            {
                continue;
            }
            let Some(it) = self.cx.lookup.item(def) else {
                continue;
            };
            let lifetimes = trait_lifetimes(&it.generics, &ids);
            let at = stack.len();
            for (k, &sup) in td.supers.iter().enumerate() {
                let lts = td.super_lifetimes.get(k).unwrap_or(&none);
                if let Some((sdef, sids, sargs)) =
                    Self::bound_in(ls, (def, &args), &ids, &lifetimes, sup, lts)
                {
                    stack.push((sdef, sargs, sids));
                }
            }
            stack[at..].reverse();
        }
        None
    }

    /// A bound `bound` written in trait `def`'s scope (a supertrait or an
    /// associated type's bound), over `def`'s arguments `args` whose slots
    /// have lifetimes `ids` and whose `k`-th lifetime is `lifetimes[k]`:
    /// its trait, the lifetime of each slot of its arguments, and its
    /// arguments. Each slot the bound writes takes its lifetime, and each
    /// slot of a type parameter of `def` the argument's (`lts`, one per
    /// reference slot, as [`hd_resolve::TraitData::super_lifetimes`]).
    fn bound_in(
        ls: &Layouts<'_>,
        (def, args): (DefId, &[Ty]),
        ids: &[Vec<u32>],
        lifetimes: &[u32],
        bound: Ty,
        lts: &[u16],
    ) -> Option<(DefId, Vec<Vec<u32>>, Vec<Ty>)> {
        let pool = ls.pool;
        let (sdef, sargs) = pool.trait_value(bound)?;
        let named = |l: &u16| {
            let l = usize::from(*l).checked_sub(1);
            l.and_then(|l| lifetimes.get(l)).copied().unwrap_or(0)
        };
        // The bound's argument over `def`'s arguments.
        let inst = |t: Ty| {
            pool.subst(t, &|p: ParamRef| {
                let k = usize::from(p.index).checked_sub(1)?;
                (p.owner == def).then(|| args.get(k).copied()).flatten()
            })
        };
        let (mut at, sargs) = (0, pool.list_items(sargs));
        let mut sids = Vec::with_capacity(sargs.len());
        let mut iargs = Vec::with_capacity(sargs.len());
        for &a in sargs {
            let n = pool.ref_slots(a);
            let written: Vec<u32> = (lts.get(at..at + n).unwrap_or_default().iter())
                .map(named)
                .collect();
            at += n;
            let decl = arg_ids(ls, &[a], &written).pop().unwrap_or_default();
            let t = inst(a);
            let x = match line_up(ls, a, t) {
                Some(keys) => (keys.into_iter())
                    .map(|key| match key {
                        Key::Decl(j) => decl.get(j).copied().unwrap_or(0),
                        Key::Param(q, s) if q.owner == def && q.index > 0 => {
                            let k = usize::from(q.index) - 1;
                            ids.get(k).and_then(|x| x.get(s)).copied().unwrap_or(0)
                        }
                        _ => 0,
                    })
                    .collect(),
                None => vec![0; ls.of(t).slots.len()],
            };
            sids.push(x);
            iargs.push(t);
        }
        Some((sdef, sids, iargs))
    }

    /// The variables the bounds that write lifetimes of call `i`'s callee
    /// make one ([`Ck::bound_links`]): a function's or a method's own
    /// type parameters, and an inherent method's implementation's.
    fn call_links(
        &self,
        ls: &Layouts<'_>,
        b: &Body,
        i: usize,
        sig: &hd_resolve::FnSig,
    ) -> Vec<(Var, Var)> {
        let pool = ls.pool;
        let mut links = Vec::new();
        let (def, targs, skip) = match Callee::from_words(b.record(b.data[i][0])) {
            Some(Callee::Item { def, targs }) => (def, pool.list_items(targs), 0),
            Some(Callee::TraitMethod {
                trait_,
                method,
                targs,
                ..
            }) => {
                let n = self.generics_of(trait_).map_or(0, <[_]>::len);
                (method, pool.list_items(targs), n)
            }
            None => return links,
        };
        let mut skip = skip;
        if let Some(ItemData::Method { owner, .. }) = self.cx.lookup.item(def).map(|it| &it.data)
            && let Some(it) = self.cx.lookup.item(*owner)
            && matches!(it.data, ItemData::Impl { .. })
        {
            self.bound_links(ls, *owner, &it.generics, targs, &mut links);
            skip = it.generics.len();
        }
        self.bound_links(
            ls,
            def,
            &sig.generics,
            targs.get(skip..).unwrap_or_default(),
            &mut links,
        );
        links
    }

    /// The variables a callee's bounds that write lifetimes make one
    /// (`ref.lifetime.trait.bound-argument`): each slot of what such a
    /// type parameter of `owner` stands for and each lifetime its bounds
    /// write, and where it stands for a type parameter or an associated
    /// type projection of the caller's with a bound on the same trait,
    /// each lifetime with the caller's own of the same slot.
    fn bound_links(
        &self,
        ls: &Layouts<'_>,
        owner: DefId,
        generics: &[hd_resolve::Generic],
        targs: &[Ty],
        links: &mut Vec<(Var, Var)>,
    ) {
        let pool = ls.pool;
        for (k, g) in generics.iter().enumerate() {
            if g.bound_lifetimes.is_empty() {
                continue;
            }
            let (Some(&t), Ok(index)) = (targs.get(k), u16::try_from(k)) else {
                continue;
            };
            let q = ParamRef { owner, index };
            let mut written: Vec<u32> = (g.bound_lifetimes.iter().flatten())
                .filter(|&&l| l != 0)
                .map(|&l| u32::from(l))
                .collect();
            written.sort_stable();
            written.dedup();
            for s in 0..ls.of(t).slots.len() {
                links.extend(written.iter().map(|&l| (Var::Param(q, s), Var::Life(l))));
            }
            if !matches!(pool.get(t), TyData::Param(_) | TyData::Assoc { .. }) {
                continue;
            }
            for (b, lts) in g.bounds.iter().zip(&g.bound_lifetimes) {
                let Some((tr, args)) = pool.trait_value(*b) else {
                    continue;
                };
                let lts: Vec<u32> = lts.iter().map(|&l| u32::from(l)).collect();
                let theirs = arg_ids(ls, pool.list_items(args), &lts);
                let mine = self.bound_ids(ls, t, tr);
                for (x, y) in theirs.iter().zip(&mine).filter(|(x, y)| x.len() == y.len()) {
                    let pairs = x.iter().zip(y).filter(|(a, b)| **a != 0 && **b != 0);
                    links.extend(pairs.map(|(&a, &b)| (Var::Life(a), Var::Own(b))));
                }
            }
        }
    }

    /// `ref.result.path.caller`: each argument of call `i` whose
    /// parameter's lifetime reaches only result slots that name a path of
    /// it, with the fields those paths share. Its result's loan covers only
    /// that part of the argument's place.
    fn call_paths(
        &self,
        ls: &Layouts<'_>,
        b: &Body,
        i: usize,
        sig: &hd_resolve::FnSig,
        dep: &DepSig,
    ) -> Vec<(u32, Vec<Proj>)> {
        let slots = path_slots(ls, sig.ret, &sig.paths);
        let mut out = Vec::new();
        for (p, (_, ids)) in dep.params.iter().enumerate() {
            let (Some(&id), Some(&(_, pty))) = (ids.first(), sig.params.get(p)) else {
                continue;
            };
            let mine: Vec<_> = (slots.iter().zip(&sig.paths))
                .filter(|(s, _)| s.1 == w(p))
                .collect();
            let only = |r: usize| mine.iter().any(|(s, _)| s.0 == r);
            let elsewhere = (dep.outs.iter().enumerate())
                .any(|(r, o)| *o == Some(ANY) || *o == Some(id) && !only(r))
                || (dep.params.iter().enumerate()).any(|(q, x)| q != p && x.1.contains(&id));
            if mine.is_empty() || elsewhere {
                continue;
            }
            // The steps every path of the parameter starts with.
            let mut shared: Option<Vec<Proj>> = None;
            for (_, path) in &mine {
                let steps = self.path_steps(pty, &path.fields, false);
                let fields = steps.unwrap_or_default();
                let s = shared.get_or_insert_with(|| fields.clone());
                let n = s.iter().zip(&fields).take_while(|(a, b)| a == b).count();
                s.truncate(n);
            }
            if let (Some(v), Some(fields)) = (source_arg(b, i, p), shared.filter(|f| !f.is_empty()))
            {
                out.push((v, fields));
            }
        }
        out
    }

    /// For the trait's abstract `Self` and then each trait argument, as a
    /// call through implementation `imp` instantiates them with `targs`,
    /// the trait lifetime each slot's implementation lifetime is
    /// (`ref.lifetime.trait`). The slots of one implementation lifetime
    /// that is no trait lifetime are one variable, pushed to `links`: with
    /// `impl['b] Put[&'b Point] for Cell['b]`, `put(&mut self, v: V)`
    /// stores `v` in the target's `'b` slot (`ref.lifetime.trait.impl-argument`).
    fn impl_trait_slots(
        &self,
        ls: &Layouts<'_>,
        imp: DefId,
        targs: &[Ty],
        links: &mut Vec<(Var, Var)>,
    ) -> Vec<Vec<Option<u32>>> {
        let Some(ItemData::Impl {
            trait_,
            trait_args,
            trait_lifetimes: t,
            self_lifetimes,
            self_ty,
            trait_slots,
            ..
        }) = self.cx.lookup.item(imp).map(|i| &i.data)
        else {
            return Vec::new();
        };
        let pool = ls.pool;
        let mut lts: Vec<Vec<u32>> = vec![
            ls.slot_lifetimes(*self_ty, self_lifetimes)
                .into_iter()
                .map(u32::from)
                .collect(),
        ];
        if !trait_slots.is_empty() {
            let args = pool.list_items(*trait_args);
            let mut at = 0;
            for &a in args {
                let end = at + pool.ref_slots(a);
                let ids = ls.slot_lifetimes(a, trait_slots.get(at..end).unwrap_or(&[]));
                at = end;
                let inst = targs.get(lts.len() - 1).copied().unwrap_or(a);
                let keys = line_up(ls, a, inst).unwrap_or_default();
                let mine = keys.into_iter().map(|k| match k {
                    Key::Decl(j) => ids.get(j).copied().map_or(0, u32::from),
                    _ => 0,
                });
                lts.push(mine.collect());
            }
        }
        let at = |l: u32| {
            if l == STATIC {
                return Some(STATIC);
            }
            t.iter()
                .position(|&x| u32::from(x) == l && l != 0)
                .map(|k| w(k + 1))
        };
        let mut first: Vec<(u32, Var)> = Vec::new();
        for (g, slots) in lts.iter().enumerate() {
            let index = u16::try_from(g).unwrap_or(u16::MAX);
            for (s, &l) in slots.iter().enumerate() {
                if l == 0 || at(l).is_some() {
                    continue;
                }
                let v = Var::Param(
                    ParamRef {
                        owner: *trait_,
                        index,
                    },
                    s,
                );
                match first.iter().find(|x| x.0 == l) {
                    Some(&(_, f)) => links.push((f, v)),
                    None => first.push((l, v)),
                }
            }
        }
        (lts.iter())
            .map(|slots| slots.iter().map(|&l| at(l)).collect())
            .collect()
    }

    /// The hidden lifetime-argument slots of a trait value, numbered as
    /// the trait declares them (`ref.lifetime.trait`).
    fn trait_value_self_slots(&self, ls: &Layouts<'_>, tr: DefId, mut t: Ty) -> Vec<Option<u32>> {
        let pool = ls.pool;
        while let TyData::Ref { inner, .. } = pool.get(t) {
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
        facts: &crate::moves::MoveFacts,
    ) -> hd_base::StageResult<()> {
        let pool = self.pool();
        let lifetimed = lifetimed(self.cx.lookup);
        let ls = &Layouts::new(pool, self.cx.names.known.suspend, &lifetimed);
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
                    self.sig_of(body.item)
                }
            })
            .flatten();
        let (mut paths, mut errs) = (Vec::new(), Vec::new());
        let own = match own_sig {
            Some(sig) => {
                if let Some(f) = fun {
                    paths = self.own_paths(ls, &sig, f, &mut errs);
                }
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
                )
                .with_paths(ls, &sig.paths);
                let dep = self.with_wholes(ls, dep);
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
            self.with_wholes(
                ls,
                DepSig::new(ls, &sig.params, is_self, Ty::VOID, &sig.lifetimes),
            )
        });
        let values = FnValues::new(body, mine.as_ref());
        self.input_fits(ls, body, &values, (root.0, fun), &mut errs);
        // Own the shared handles: hashbrown's destructor keeps borrowed
        // entries live past the mutable diagnostic calls below.
        let mut proofs = HashMap::<hd_base::NodeIdx, Vec<crate::body::RegionUse>>::default();
        // Region uses retain proof handles for this entire pass. Equal
        // proof/type/offset queries share projection, not value edges.
        let mut value_projections = HashMap::<(usize, Ty, usize), Box<[u32]>>::default();
        let mut markers = Vec::new();
        let mut statics: Vec<_> = self
            .static_values
            .iter()
            .map(|(r, slots)| (*r, slots.clone(), Category::Static))
            .collect();
        for (proof, message) in self
            .region_uses
            .iter()
            .map(|p| (p, None))
            .chain(self.marker_uses.iter().map(|(p, m)| (p, Some(m))))
        {
            if let RegionSource::Value(value, ty) = proof.1 {
                let ty = self.infer.resolve(pool, ty);
                let actual = self.infer.resolve(pool, proof.2.self_ty);
                let offset = match pool.get(ty) {
                    TyData::Ref { inner, counted, .. } if inner == actual => usize::from(!counted),
                    _ => 0,
                };
                let goal = self
                    .infer
                    .resolve(pool, goal_tuple(pool, actual, proof.2.args));
                let key = (proof.3.key(), goal, offset);
                let slots = if let Some(slots) = value_projections.get(&key) {
                    slots.clone()
                } else {
                    let slots = Rc::new(
                        (0..ls.of(actual).slots.len())
                            .map(|k| vec![w(k + offset)])
                            .collect(),
                    );
                    let roots = [(&proof.3, goal, slots)];
                    let tables = (self.cx.lookup, Impls::Owned(self.cx.impls));
                    let positions =
                        proof_statics(ls, tables, &|t| self.infer.resolve(pool, t), roots)?;
                    let slots: Box<[_]> = (0..ls.of(actual).slots.len())
                        .map(|k| w(k + offset))
                        .filter(|k| positions.contains(k))
                        .collect();
                    value_projections.insert(key, slots.clone());
                    slots
                };
                if let Some(message) = message {
                    markers.push((value, slots, proof.0, message.clone()));
                } else if !slots.is_empty() {
                    statics.push((value, slots, Category::StaticProof));
                }
            } else {
                proofs.entry(proof.0).or_default().push(proof.clone());
            }
        }
        let mut static_proofs = StaticProofCache::default();
        let mut calls: HashMap<u32, CallSig> = HashMap::default();
        let mut closures: Vec<(usize, DepSig)> = Vec::new();
        let mut through = !statics.is_empty()
            || !markers.is_empty()
            || body.local_ty.iter().enumerate().any(|(l, &t)| {
                body.local_flags[l] & hd_tir::ir::local_flags::PARAM != 0 && capture_slot(pool, t)
            });
        let (mut last_write, mut first_elem_ref) = (None, u32::MAX);
        for i in 0..body.tags.len() {
            if matches!(
                body.tags[i],
                Tag::Call | Tag::Await | Tag::CallDyn | Tag::CallValue
            ) && let Some((mut sig, took)) = self.call_sig(ls, body, i, &values)
            {
                let statics = self.call_proof_statics(
                    ls,
                    proofs.get(&body.syn[i]).map_or(&[], Vec::as_slice),
                    &took,
                    &mut static_proofs,
                )?;
                // A slot an explicit type argument writes `'static` is one
                // with `'static`, as a parameter written `&'static T` is.
                let written = self.written_static_targs(ls, body, i, &took);
                sig.links
                    .extend(written.into_iter().map(|v| (v, Var::Life(STATIC))));
                for (_, slots) in &mut sig.params {
                    for (var, category) in slots {
                        if statics.contains(var) && *category != Some(Category::StaticParam) {
                            *category = Some(Category::StaticProof);
                        }
                    }
                }
                // A suspension that holds an argument may be kept
                // (`ref.dep.suspension.local`).
                let held = sig.result && ls.of(body.ty[i]).slots.iter().any(|s| s.hold);
                through |= sig.store || held || sig.passes_own() || sig.passes_static();
                calls.insert(w(i), sig);
            }
            // A call that takes a reference beside another argument
            // (`ref.excl.args`, `ref.excl.args.shared`).
            if matches!(
                body.tags[i],
                Tag::Call | Tag::Await | Tag::CallDyn | Tag::CallValue
            ) {
                let (mut n, mut m) = (0, false);
                slots(body, i, |_, v, word| {
                    if matches!(word, Word::Value(_)) && (v as usize) < i {
                        n += 1;
                        m |= constraints::is_ref(pool, body.ty[v as usize]);
                    }
                });
                through |= m && n > 1;
            }
            // A write after an element reference, a wildcard loop's
            // included, or between a built-in call's element receiver and
            // the call (`ref.elem.conflict`).
            if let Some(recv) = constraints::elem_receiver(body, i) {
                through |= last_write.is_some_and(|x| x > recv);
            }
            if constraints::writes_place(pool, body, i) {
                through |= first_elem_ref < w(i);
                last_write = Some(w(i));
            }
            if constraints::elem_ref(body, i) {
                first_elem_ref = first_elem_ref.min(w(i));
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
                    match pool.get(body.ty[i]) {
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
                                whole: Vec::new(),
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
                // A reference stored into a field written `&'static T`
                // (`ref.static.not-local`): one field map lookup per part.
                tag @ (Tag::NewStruct | Tag::NewVariant) => {
                    let [a, d] = body.data[i];
                    let variant = (tag == Tag::NewVariant).then_some(a);
                    through |= body.record(d).iter().enumerate().any(|(j, &v)| {
                        v & hd_tir::ir::Ref::CONST_BIT == 0
                            && constraints::is_ref(pool, body.ty[v as usize])
                            && ls.field_static(self.cx, body.ty[i], variant, w(j))
                    });
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
                Tag::ItemRef
                | Tag::ProviderGet
                | Tag::Hole
                | Tag::Poison
                | Tag::Prim
                | Tag::And
                | Tag::Or
                | Tag::Call
                | Tag::CallValue
                | Tag::CallDyn
                | Tag::Is
                | Tag::CallHost
                | Tag::DefaultCall
                | Tag::Coerce
                | Tag::Await
                | Tag::AwaitValue
                | Tag::AwaitAll
                | Tag::AwaitRace
                | Tag::Hook
                | Tag::CopyStruct
                | Tag::NewTuple
                | Tag::NewList
                | Tag::NewMap
                | Tag::Closure
                | Tag::RefDyn
                | Tag::RefIdentity
                | Tag::Read
                | Tag::Ref
                | Tag::Drop
                | Tag::IsUnique
                | Tag::FlagSet
                | Tag::FlagClear
                | Tag::Box
                | Tag::With
                | Tag::ContextNew
                | Tag::ContextFor
                | Tag::Block
                | Tag::Scope
                | Tag::Defer
                | Tag::If
                | Tag::Loop
                | Tag::Break
                | Tag::Continue
                | Tag::Return
                | Tag::Unreachable
                | Tag::Match
                | Tag::SwitchTag
                | Tag::SwitchInt
                | Tag::SwitchChar
                | Tag::SwitchStr
                | Tag::Guard
                | Tag::ToArm
                | Tag::LoanEnd
                | Tag::Retain
                | Tag::Release
                | Tag::MakeUnique
                | Tag::LoanBegin
                | Tag::LoanClose
                | Tag::OwnLoans => {}
            }
        }
        // The arguments a call's result holds, for the backend's run-time
        // loans (`ref.excl.alias.loan`): its holders are the bindings the
        // result reaches, directly or through a match on it.
        let mut held_args = Vec::new();
        for (&v, sig) in &calls {
            held_args.extend(sig.held.iter().map(|&a| (v, a)));
        }
        held_args.sort_stable();
        held_args.dedup();
        let mut loan_slots: Vec<_> = calls
            .values()
            .flat_map(|sig| sig.loan_slots.iter().cloned())
            .collect();
        loan_slots.sort_stable_by_key(|row| (row.call, row.value, row.origin));
        self.b.body_mut().held_args = held_args;
        self.b.body_mut().loan_slots = loan_slots;
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
        for (at, code, msg) in errs {
            self.err(code, self.cx.src.parse.tree.node(at), &msg);
        }
        let own = own.filter(|o| {
            opaque
                || o.depends()
                || o.capture_elided == Elided::Missing
                    && ls.of(o.ret).tops.iter().any(|&k| ls.of(o.ret).slots[k].fun)
        });
        // A place through a pointer may take a run-time loan, whose holders
        // the regions give.
        // A run-time loan needs the solve only where one may begin: a `Ref`
        // site, or a `*T` a call lends; a read of a captured local alone
        // takes none.
        let runtime = runtime_sites(self.b.body(), pool);
        let lent = self.lent_args(self.b.body(), &runtime);
        let lends = !lent.is_empty()
            || (runtime.iter().enumerate())
                .any(|(i, &site)| site && self.b.body().tags[i] == Tag::Ref);
        if own.is_none() && !through && !lends && closures.is_empty() {
            return Ok(());
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
            reborrows: &facts.reborrows,
            moves: &facts.moves,
            reported: &self.ref_captured,
            paths: &paths,
            annotated: self.annotated_params(ls),
            statics: &statics,
            markers: &markers,
            lent,
            runtime,
            frozen: self.frozen_locals(),
        };
        let slots = own
            .as_ref()
            .map(|o| result_slots(ls, o, ret, opaque, &paths));
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
        let mut verdicts = constraints::solve(&s, &returns);
        let run_loans = std::mem::take(&mut verdicts.run_loans);
        let sigs = (own.as_ref(), mine.as_ref());
        for (at, code, msg) in self.verdict_list(verdicts, sigs, fun, &paths) {
            let node = self.cx.src.parse.tree.node(at);
            self.err(code, node, &msg);
        }
        if !run_loans.0.is_empty() {
            self.run_loans = Some(run_loans);
        }
        Ok(())
    }

    /// Each argument a call lends to a local-reference parameter of its
    /// callee's declared signature, and whether as `&mut`
    /// (`ref.excl.alias.loan`): a `*T` passed where `&T` is expected takes
    /// a run-time loan for the call, whether or not its result holds it.
    /// Cost: one pass over the calls, each callee's signature read once
    /// per call.
    fn lent_args(&self, b: &Body, sites: &[bool]) -> Vec<(u32, bool, u32)> {
        let pool = self.pool();
        let mut out = Vec::new();
        for i in 0..b.len() {
            let method = match b.tags[i] {
                Tag::Call | Tag::Await => match Callee::from_words(b.record(b.data[i][0])) {
                    Some(Callee::Item { def, .. }) => Some(def),
                    Some(Callee::TraitMethod { method, .. }) => Some(method),
                    _ => None,
                },
                Tag::CallDyn => (b.record(b.data[i][1]).first()).map(|&m| DefId::from_raw(m)),
                _ => None,
            };
            let names = &self.cx.names;
            let params: Vec<(bool, Ty)> = match (method, b.tags[i]) {
                (Some(def), _) => (self.sig_of(def).into_iter())
                    .flat_map(|s| {
                        s.params
                            .into_iter()
                            .map(|p| (names.text(p.0) == "self", p.1))
                    })
                    .collect(),
                (None, Tag::CallValue) => {
                    match b.ty.get(b.data[i][0] as usize).map(|&t| pool.get(t)) {
                        Some(TyData::Fn { params, .. }) => pool
                            .list_items(params)
                            .iter()
                            .map(|&t| (false, t))
                            .collect(),
                        _ => Vec::new(),
                    }
                }
                _ => Vec::new(),
            };
            for (p, &(is_self, t)) in params.iter().enumerate() {
                if !matches!(local_ref(pool, t), Some((false, _))) {
                    continue;
                }
                if let Some(a) = source_arg(b, i, p)
                    && runtime_site(b, sites, a)
                {
                    let excl = constraints::is_mut(pool, t);
                    // A `&mut self` receiver is shared until its call
                    // starts (two-phase, `ref.excl.alias.loan`).
                    let activate = if excl && is_self && p == 0 {
                        w(i)
                    } else {
                        NONE
                    };
                    out.push((a, excl, activate));
                }
            }
        }
        out
    }

    /// `ref.result.path.fields`: the body's result paths, each step a field
    /// of the struct reached, visible here, and public in a `pub`
    /// signature, and each on an outer reference of the result. Any other
    /// is an error, which `errs` takes, and is dropped.
    fn own_paths(
        &self,
        ls: &Layouts<'_>,
        sig: &hd_resolve::FnSig,
        f: NodeRef<'_>,
        errs: &mut Vec<(NodeIdx, Code, String)>,
    ) -> Vec<OwnPath> {
        let src = self.cx.src;
        let toks = &src.parse.tokens;
        let public = self
            .cx
            .lookup
            .item(self.b.body().item)
            .is_some_and(|i| i.public);
        let spelled = |n: NodeRef<'_>| -> String {
            (n.direct_tokens())
                .filter(|&t| !matches!(toks.kind(t), TokenKind::Amp | TokenKind::KwMut))
                .map(|t| src.text(t))
                .collect()
        };
        let nodes: Vec<NodeRef<'_>> = (f.descendants())
            .filter(|n| {
                n.kind() == SyntaxKind::RefType && n.direct_token(toks, TokenKind::Dot).is_some()
            })
            .collect();
        let slots = path_slots(ls, sig.ret, &sig.paths);
        let mut out = Vec::new();
        for (p, &(slot, param)) in sig.paths.iter().zip(&slots) {
            let names = &self.cx.names;
            let Some(&(pname, pty)) = sig.params.get(param as usize) else {
                continue;
            };
            let text = (std::iter::once(pname).chain(p.fields.iter().copied()))
                .map(|x| names.text(x))
                .collect::<Vec<_>>()
                .join(".");
            let Some(&node) = nodes.iter().find(|&&n| spelled(n) == format!("'{text}")) else {
                continue;
            };
            let walk = || {
                let steps = self.path_steps(pty, &p.fields, public)?;
                if !ls.of(sig.ret).tops.contains(&slot) {
                    let msg =
                        "a field path is checked only on an outer reference of the result so far";
                    return Err((Code::ReferencesNotYetChecked, msg.to_owned()));
                }
                Ok(steps)
            };
            match walk() {
                Err((code, msg)) => errs.push((node.index(), code, msg)),
                Ok(steps) => out.push(OwnPath {
                    param,
                    steps,
                    slot,
                    text,
                }),
            }
        }
        out
    }

    /// The place steps of result path `fields` from parameter type `pty`:
    /// each field, and a dereference where the next field is reached
    /// through a `*T` or `*mut T` (`ref.result.path.fields`,
    /// `ref.result.path.counted`).
    fn path_steps(
        &self,
        pty: Ty,
        fields: &[Symbol],
        public: bool,
    ) -> Result<Vec<Proj>, (Code, String)> {
        let pool = self.pool();
        let (mut t, mut steps) = (referent(pool, pty), Vec::new());
        for &name in fields {
            if let TyData::Ref {
                inner,
                counted: true,
                ..
            } = pool.get(self.resolved(t))
            {
                steps.push(Proj::Deref);
                t = inner;
            }
            let (j, next) = self.path_step(t, name, public)?;
            steps.push(Proj::Field(j));
            t = next;
        }
        Ok(steps)
    }

    /// One step of a result path from `t`: the field's index and type
    /// (`ref.result.path.fields`).
    fn path_step(&self, t: Ty, name: Symbol, public: bool) -> Result<(u32, Ty), (Code, String)> {
        let pool = self.pool();
        let (field, shown) = (self.cx.names.text(name), self.show(t));
        if matches!(pool.get(t), TyData::Ref { .. }) {
            let msg = format!("a field path through the reference `{shown}` is not checked yet");
            return Err((Code::ReferencesNotYetChecked, msg));
        }
        let no_field = || {
            (
                Code::UnknownField,
                format!("`{shown}` has no field `{field}`"),
            )
        };
        // A tuple element is a step as a field is (`ref.place.tuple-element`).
        if let Some(elem) = self.tuple_elem_of(t, field) {
            return Ok(elem);
        }
        let (def, fs) = self.data_decl(t).ok_or_else(no_field)?;
        let j = fs
            .iter()
            .position(|x| x.name == name)
            .ok_or_else(no_field)?;
        let open = fs[j].public || fs[j].embedded;
        if !self.member_visible(def, open) {
            let msg = format!("the field `{field}` of `{shown}` is private to its module");
            return Err((Code::PrivateMember, msg));
        }
        if public && !open {
            let msg = format!(
                "the field `{field}` of `{shown}` is private, so a public signature cannot name it"
            );
            return Err((Code::PrivateTypeLeak, msg));
        }
        let args = match pool.get(self.resolved(t)) {
            TyData::Adt(_, args) => pool.list_items(args).to_vec(),
            _ => Vec::new(),
        };
        Ok((w(j), subst_owner(pool, def, &args, fs[j].ty)))
    }

    /// The diagnostics of `v`, one per node, in today's order: stores into
    /// a place the caller owns, arguments passed at a bound's lifetime,
    /// captured function values, returned values, closures' returned
    /// values, then the scope check.
    fn verdict_list(
        &self,
        v: Verdicts,
        (own, mine): (Option<&DepSig>, Option<&DepSig>),
        fun: Option<NodeRef<'_>>,
        paths: &[OwnPath],
    ) -> Vec<(NodeIdx, Code, String)> {
        let body = self.b.body();
        let mut stores = v.stores;
        stores.sort_stable_by_key(|n| n.raw());
        let mut seen = HashSet::default();
        let mut out = Vec::new();
        let mut push = |at: NodeIdx, msg: &dyn Fn() -> String| {
            if at != NodeIdx::NONE
                && seen.insert(at)
                && !(self.bound_escapes)
                    .contains(&self.cx.src.span(self.cx.src.parse.tree.node(at)).lo)
            {
                out.push((at, Code::LocalReferenceEscapes, msg()));
            }
        };
        for at in stores {
            push(at, &|| CALLER_PLACE_MSG.to_owned());
        }
        if let Some(own) = own.or(mine) {
            for (at, o, l, value) in v.bounds {
                push(at, &|| self.passed_message(own, (o, l, value), fun));
            }
        }
        for (at, o, msg) in v.capture {
            push(at, &|| {
                if msg == PROOF_MSG {
                    msg.to_owned()
                } else {
                    capture_message(body, self.cx, o, msg)
                }
            });
        }
        if let Some(own) = own {
            for (at, o, id) in v.results {
                push(at, &|| match o {
                    Origin::Outside(k) => format!(
                        "the value may point outside `{}`, the only part of it the result may borrow",
                        paths[k as usize].text
                    ),
                    _ => self.origin_message(own, o, id, fun),
                });
            }
        }
        for (at, o) in v.closures {
            push(at, &|| closure_message(body, self.cx, o));
        }
        for (at, o) in v.statics {
            if at != NodeIdx::NONE && seen.insert(at) {
                out.push((at, Code::StaticReferenceSource, self.static_message(o)));
            }
        }
        let scope = v.scope.into_iter();
        out.extend(scope.map(|(at, msg)| (at, Code::LocalReferenceOutlivesLocal, msg)));
        let conflicts = v.conflicts.into_iter();
        out.extend(conflicts.map(|(at, msg)| (at, Code::ConflictingAccess, msg)));
        out.extend(
            v.markers
                .into_iter()
                .map(|(at, msg)| (at, Code::UnregisteredAutoClosing, msg)),
        );
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

    /// The lifetime each layout slot of each closure parameter writes, by
    /// its local, for the closures whose parameter annotations write one
    /// (`Ck::closure_lifetimes`, in [`Types::ref_slots`] order over the
    /// closure's inputs): 0 for a slot that writes none. One step per
    /// such closure's parameter slot.
    fn annotated_params(&self, ls: &Layouts<'_>) -> HashMap<u32, Vec<u32>> {
        let body = self.b.body();
        let mut out = HashMap::default();
        if self.closure_lifetimes.is_empty() {
            return out;
        }
        for i in (0..body.tags.len()).filter(|&i| body.tags[i] == Tag::Closure) {
            // A generic local function's instance names its declaration,
            // which keeps no record.
            let Some(rec) = body.syn.get(i).and_then(|n| self.closure_lifetimes.get(n)) else {
                continue;
            };
            let mut at = 0;
            for &p in body.sub_param_locals(body.data[i][0] as usize) {
                let ids = (ls.of(body.local_ty[p as usize]).slots.iter())
                    .map(|s| {
                        if s.fun || s.whole || s.hold {
                            return 0;
                        }
                        at += 1;
                        rec.get(at - 1).map_or(0, |&l| u32::from(l))
                    })
                    .collect();
                out.insert(p, ids);
            }
        }
        out
    }

    /// The lifetimes in scope at node `at` of a function body, an
    /// enclosing implementation's first, with their `'`: those of the
    /// outermost function declaration around it, the body's own, whose
    /// numbering [`DepSig`] and `FnSig::lifetimes` share. `None` outside
    /// a function's body.
    pub(crate) fn lifetimes_in_scope(&self, at: NodeIdx) -> Option<Vec<String>> {
        if self.b.body().kind != BodyKind::Fn {
            return None;
        }
        let x = at.raw();
        let mut node = self.cx.src.parse.tree.root();
        loop {
            node = node.children().find(|c| {
                let (lo, hi) = range(*c);
                lo < x && x < hi
            })?;
            if node.kind() == SyntaxKind::FnDecl {
                return Some(self.lifetime_names(node));
            }
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
                matches!(c.kind(), SyntaxKind::ImplDecl | SyntaxKind::TraitDecl) && lo < x && x < hi
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

    /// `ref.lifetime.trait.bound-argument`, `fn.type.lifetime.call.written`: an
    /// argument a call passes at the body's own lifetime `l`, as a bound
    /// or, for a call of a function value, its type writes it, refers to
    /// `o`, which does not outlive `l`.
    fn passed_message(
        &self,
        own: &DepSig,
        (o, l, value): (Origin, u32, bool),
        fun: Option<NodeRef<'_>>,
    ) -> String {
        let lifetimes = fun.map(|f| self.lifetime_names(f)).unwrap_or_default();
        let what = self.origin_what(own, o, l, fun, &lifetimes);
        let at = (l < ANON).then(|| lifetimes.get((l as usize).wrapping_sub(1)));
        let at = at
            .flatten()
            .map_or_else(String::new, |n| format!(" `{n}`,"));
        let writer = if value {
            "the function value's type"
        } else {
            "the bound"
        };
        format!(
            "{what}; the call passes it at{at} the lifetime {writer} writes, which it must outlive"
        )
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
        let carries = |p: u32| {
            own.params
                .get(p as usize)
                .is_some_and(|(_, ids)| ids.contains(&id) || id == ANY && !ids.is_empty())
        };
        let what = self.origin_what(own, o, id, fun, &lifetimes);
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

    /// What origin `o` is, for a message about lifetime `id`: a local, a
    /// parameter's own storage, a temporary, a top-level binding, or what
    /// a parameter refers to and at which of `lifetimes`.
    fn origin_what(
        &self,
        own: &DepSig,
        o: Origin,
        id: u32,
        fun: Option<NodeRef<'_>>,
        lifetimes: &[String],
    ) -> String {
        let body = self.b.body();
        let name = |l: u32| self.cx.names.text(body.local_name[l as usize]).to_owned();
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
        match o {
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
            Origin::Hidden(_) | Origin::Site(_) | Origin::Lent(_) | Origin::Outside(_) => {
                format!("the value refers to a temporary of {f}")
            }
            Origin::Global(_) | Origin::Static(_) => {
                "the value refers to a top-level binding".to_owned()
            }
        }
    }

    /// `ref.static.source.message`: what a static reference was made from,
    /// and the three sources it may be.
    fn static_message(&self, o: Origin) -> String {
        let body = self.b.body();
        let name = |l: u32| self.cx.names.text(body.local_name[l as usize]).to_owned();
        let param = |l: u32| body.local_flags[l as usize] & hd_tir::ir::local_flags::PARAM != 0;
        let what = match o {
            Origin::Param(p, _) => {
                format!("the value borrows from `{}`, a plain reference", name(p))
            }
            Origin::Local(l) if param(l) => format!("`{}` is a parameter", name(l)),
            Origin::Local(l) if !name(l).starts_with('$') => format!("`{}` is a local", name(l)),
            Origin::Global(_) => {
                "the value refers to a `let mut` top-level binding or through a reference"
                    .to_owned()
            }
            _ => "the value refers to a temporary".to_owned(),
        };
        format!(
            "{what}; a static reference is made only from a top-level binding bound without `mut`, a constant literal, or data the compiler places in static memory"
        )
    }
}

/// The message of a captured function value kept where `msg` says it may
/// not go, holding `o`. A function parameter's capture slot holds what a
/// closure the caller supplied for it captured, so the message names the
/// parameter (`fn.capture.local-store`).
fn capture_message(b: &Body, cx: &BodyCx<'_>, o: Origin, msg: &str) -> String {
    let fn_param = |p: u32| b.local_flags[p as usize] & hd_tir::ir::local_flags::PARAM != 0;
    match (o, msg.strip_prefix(CAPTURED)) {
        (Origin::Param(p, _), Some(rest)) if fn_param(p) => format!(
            "`{}` is a function parameter, which may hold a closure that captures the caller's local references; it{rest}",
            cx.names.text(b.local_name[p as usize])
        ),
        _ => msg.to_owned(),
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
        Origin::Global(_) | Origin::Static(_) => {
            "the value refers to a top-level binding".to_owned()
        }
        _ => "the value refers to a temporary".to_owned(),
    };
    format!(
        "{what}; a closure's reference result depends only on the closure's reference parameters of its lifetime"
    )
}

/// The diagnostics one solver gives a body, before their messages.
#[derive(Default)]
struct Verdicts {
    /// Conditional `AutoClosing` proofs established in the value context.
    markers: Vec<(NodeIdx, String)>,
    /// The run-time loan sites and their holders (`ref.excl.alias.loan`).
    run_loans: RunLoans,
    /// Stores into a place the caller owns (`ref.dep.store`).
    stores: Vec<NodeIdx>,
    /// Captured function values kept where they may not go: the origin
    /// that may not go there and the destination's message.
    capture: Vec<(NodeIdx, Origin, &'static str)>,
    /// Returned values: the origin they may not depend on, the lifetime.
    results: Vec<(NodeIdx, Origin, u32)>,
    /// Call arguments passed at one of the body's own lifetimes that a
    /// bound or a function value's type writes: the origin that does not
    /// outlive it, the lifetime, and whether the call is of a function
    /// value (`ref.lifetime.trait.bound-argument`, `fn.type.lifetime.call.written`).
    bounds: Vec<(NodeIdx, Origin, u32, bool)>,
    /// Closures' returned values and the origin.
    closures: Vec<(NodeIdx, Origin)>,
    /// Values where a static reference is expected, and the origin that
    /// is no static source (`ref.static.source.error`).
    statics: Vec<(NodeIdx, Origin)>,
    /// `local-reference-outlives-local`.
    scope: Vec<(NodeIdx, String)>,
    /// `conflicting-access` (`ref.excl.shared`, `ref.excl.mut`).
    conflicts: Vec<(NodeIdx, String)>,
}

/// What each top slot of a returned value of type `ret` may depend on
/// (`ref.dep.return`): its result slot's lifetimes, none for a slot of an
/// opaque type that does not borrow or a capture slot with no source
/// (`ref.generic.outlives.read`), and `None` for a slot that is not checked.
/// A slot that names result path `k` is marked `PATH + k`, every slot
/// where the shapes do not line up (`ref.result.path.callee`).
fn result_slots(
    ls: &Layouts<'_>,
    own: &DepSig,
    ret: Ty,
    opaque: bool,
    paths: &[OwnPath],
) -> Vec<Option<Vec<u32>>> {
    let rl = ls.of(ret);
    let capture = |k: usize| {
        own.capture_elided == Elided::Missing && rl.tops.get(k).is_some_and(|&t| rl.slots[t].fun)
    };
    let tops = &ls.of(own.ret).tops;
    let lined = tops.len() == rl.tops.len();
    (own.result_ids(ls, ret).into_iter().enumerate())
        .map(|(k, mut ids)| {
            for (i, p) in paths.iter().enumerate() {
                if !ids.is_empty() && (!lined || tops.get(k) == Some(&p.slot)) {
                    ids.push(PATH + w(i));
                }
            }
            (!ids.is_empty() || opaque || capture(k)).then_some(ids)
        })
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
    b.sub_param_locals(sub)
        .iter()
        .position(|&local| local == p)
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
            Tag::ItemRef
            | Tag::ProviderGet
            | Tag::Hole
            | Tag::Poison
            | Tag::Prim
            | Tag::And
            | Tag::Or
            | Tag::Call
            | Tag::CallValue
            | Tag::CallDyn
            | Tag::Is
            | Tag::CallHost
            | Tag::Intrinsic
            | Tag::DefaultCall
            | Tag::Coerce
            | Tag::Await
            | Tag::AwaitValue
            | Tag::AwaitAll
            | Tag::AwaitRace
            | Tag::Hook
            | Tag::NewStruct
            | Tag::CopyStruct
            | Tag::NewVariant
            | Tag::NewTuple
            | Tag::NewList
            | Tag::NewMap
            | Tag::RefDyn
            | Tag::RefIdentity
            | Tag::Read
            | Tag::Store
            | Tag::Ref
            | Tag::Drop
            | Tag::IsUnique
            | Tag::FlagSet
            | Tag::FlagClear
            | Tag::Box
            | Tag::With
            | Tag::ContextNew
            | Tag::ContextFor
            | Tag::Block
            | Tag::Scope
            | Tag::Defer
            | Tag::If
            | Tag::Loop
            | Tag::Break
            | Tag::Continue
            | Tag::Unreachable
            | Tag::Match
            | Tag::SwitchTag
            | Tag::SwitchInt
            | Tag::SwitchChar
            | Tag::SwitchStr
            | Tag::Guard
            | Tag::ToArm
            | Tag::LoanEnd
            | Tag::Retain
            | Tag::Release
            | Tag::MakeUnique
            | Tag::LoanBegin
            | Tag::LoanClose
            | Tag::OwnLoans => {}
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

/// A part [`Ck::written_inputs`] traces: part `path` of a value or of a
/// local.
enum Trace {
    Value(u32, Vec<Part>),
    Local(u32, Vec<Part>),
}

/// One step into a value [`Ck::written_inputs`] traces: a place
/// projection, or the result of calling a function value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Part {
    Proj(Proj),
    Result,
}

/// Where a part of a value lies in its declared type's record of
/// written input lifetimes (`FnSig::fn_lifetimes`), as [`fn_spot`]
/// finds it.
enum Spot {
    /// The function type whose inputs write the record's slots
    /// `lo..lo + n`.
    Fn(usize, usize),
    /// The part lies under the struct or variant field the path's step
    /// at this index reads, whose declared type is not a type parameter:
    /// the field's own record ([`hd_resolve::Field::fn_lifetimes`]) holds
    /// what its function types write.
    Field(usize),
    /// The part lies under a struct or variant field in the result of a
    /// function type, past the path's step at index `from`: the field
    /// the step at `from + at` reads, in a value of the result type `ty`,
    /// whose slots the record writes at `lo..` in [`Types::ref_slots`]
    /// order (`fn.type.lifetime.call.written`).
    ResultField {
        from: usize,
        at: usize,
        lo: usize,
        ty: Ty,
    },
}

/// The record slots of function type `t`'s own inputs, when `rec` is
/// the record of a whole type `t`: after those of the function types in
/// its inputs and result, and before those of its own result.
fn own_inputs<'r>(pool: Types<'_>, t: Ty, rec: &'r [u16]) -> &'r [u16] {
    let TyData::Fn { params, result, .. } = pool.get(t) else {
        return &[];
    };
    let n: usize = pool
        .list_items(params)
        .iter()
        .map(|&a| pool.ref_slots(a))
        .sum();
    let lo = rec.len().saturating_sub(n + pool.ref_slots(result));
    rec.get(lo..lo + n).unwrap_or(&[])
}

/// Where part `path` of a value of declared type `t` lies among the
/// written slots of `t`'s function types, in [`Types::fn_written_slots`]
/// order: a tuple element, a reference's referent, a list element, a
/// call's result, and a field whose declared type is one of its type's
/// parameters each step into the matching part of `t`. `None` for a part
/// no function type's inputs hold.
fn fn_spot(cx: &BodyCx<'_>, pool: Types<'_>, t: Ty, path: &[Part]) -> Option<Spot> {
    let slots = |ts: &[Ty]| ts.iter().map(|&a| pool.fn_written_slots(a)).sum::<usize>();
    let ref_slots = |ts: &[Ty]| ts.iter().map(|&a| pool.ref_slots(a)).sum::<usize>();
    let (mut ty, mut base, mut variant) = (t, 0, None);
    // The last function type's result the path stepped into: the index
    // of the step after it, where its record's result slots start, and
    // the result type.
    let mut in_result: Option<(usize, usize, Ty)> = None;
    for (i, part) in path.iter().enumerate() {
        match (*part, pool.get(ty)) {
            (Part::Proj(Proj::Deref), TyData::Ref { inner, .. }) => ty = inner,
            (Part::Proj(Proj::Variant(k)), _) => variant = Some(k),
            (Part::Proj(Proj::Elem(_)), TyData::Adt(AdtId::Declared(def), args))
                if def == cx.names.known.list =>
            {
                ty = *pool.list_items(args).first()?;
            }
            (Part::Result, TyData::Fn { params, result, .. }) => {
                let ps = pool.list_items(params);
                let lo = base + pool.fn_written_slots(ty) - pool.ref_slots(result);
                in_result = Some((i + 1, lo, result));
                base += slots(ps);
                ty = result;
            }
            (Part::Proj(Proj::Field(j)), TyData::Adt(AdtId::Product { .. }, args)) => {
                let items = pool.list_items(args);
                let j = j as usize;
                base += slots(items.get(..j)?);
                ty = *items.get(j)?;
            }
            (Part::Proj(Proj::Field(j)), TyData::Adt(AdtId::Declared(def), args))
                if def.get().is_some() =>
            {
                let fields = match (&cx.lookup.item(def)?.data, variant.take()) {
                    (ItemData::Struct(fs, _), None) => fs,
                    (ItemData::Enum { variants, .. }, Some(k)) => &variants.get(k as usize)?.fields,
                    _ => return None,
                };
                let TyData::Param(pr) = pool.get(fields.get(j as usize)?.ty) else {
                    return Some(match in_result {
                        Some((from, lo, ty)) => Spot::ResultField {
                            from,
                            at: i - from,
                            lo,
                            ty,
                        },
                        None => Spot::Field(i),
                    });
                };
                let type_items = pool.list_items(args);
                let k = usize::from(pr.index);
                if pr.owner != def {
                    return None;
                }
                base += slots(type_items.get(..k)?);
                ty = *type_items.get(k)?;
            }
            _ => return None,
        }
    }
    let TyData::Fn { params, result, .. } = pool.get(ty) else {
        return None;
    };
    let n = ref_slots(pool.list_items(params));
    let own = n + pool.ref_slots(result);
    Some(Spot::Fn(base + pool.fn_written_slots(ty) - own, n))
}

/// The innermost part of value `v` whose origins hold `o`, as `has`
/// says, for the diagnostic's position: an arm, a block's value, a
/// construction's part. A loop down the body's tree, which ends at a
/// leaf: each step goes to a part of the value before it.
fn site(b: &Body, has: &dyn Fn(u32, Origin) -> bool, v: u32, o: Origin) -> NodeIdx {
    let mut at = NodeIdx::NONE;
    let mut v = v;
    loop {
        let here = b.syn.get(v as usize).copied().unwrap_or(NodeIdx::NONE);
        if here != NodeIdx::NONE {
            at = here;
        }
        if v & hd_tir::ir::Ref::CONST_BIT != 0 {
            return at;
        }
        let mut inner = None;
        if matches!(b.tags[v as usize], Tag::Coerce) {
            // A wrapper's captured origins still diagnose at their source,
            // but its own storage origin must stop at the allocation.
            let source = b.data[v as usize][0];
            if b.row_adaptation(v as usize).is_none() || has(source, o) {
                inner = Some(source);
            }
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
            Some(w) => v = w,
            None => return at,
        }
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

/// The nearest ancestor of a kind a climb asks for, with its child on the
/// path; `None` when the climb reached the root.
type Above = Option<(NodeIdx, NodeIdx)>;

/// One body's scope questions ([`scope_of`]): what a climb from each node
/// found above it, memoized per node, as rustc's region `ScopeTree` gives
/// each node its parent scope once. A question climbs only the nodes no
/// earlier question of the body climbed, then reads the memo, so the
/// questions of a body cost the nodes between its bindings and their
/// scopes once, not each binding's depth.
struct ScopeIndex<'t> {
    tree: &'t GreenTree,
    /// The nearest node that may end a binding's scope search: a `let`,
    /// a binding expression, a `match` arm, a `for`, a comprehension's
    /// `for`, a block, a closure or a function.
    anchor: HashMap<u32, Above>,
    /// The nearest block, closure or function: a suite.
    suite: HashMap<u32, Above>,
    /// The loop whose every iteration runs the node, if no closure or
    /// function comes first.
    lp: HashMap<u32, Option<(u32, u32)>>,
}

/// The answer above node `n`: the memo's, or a climb up the parent links
/// until `stop` answers for a parent and its child on the path, `top` at
/// the root. Every node climbed takes the answer. Linear in the nodes
/// climbed, each once per memo.
fn climb<A: Copy>(
    tree: &GreenTree,
    memo: &mut HashMap<u32, A>,
    n: NodeIdx,
    top: A,
    stop: impl Fn(NodeRef<'_>, NodeRef<'_>) -> Option<A>,
) -> A {
    let mut climbed = Vec::new();
    let mut cur = n;
    let answer = loop {
        if let Some(&a) = memo.get(&cur.raw()) {
            break a;
        }
        crate::work::scan_work(1);
        climbed.push(cur.raw());
        let Some(up) = tree.parent(cur) else {
            break top;
        };
        if let Some(a) = stop(tree.node(up), tree.node(cur)) {
            break a;
        }
        cur = up;
    };
    memo.extend(climbed.into_iter().map(|c| (c, answer)));
    answer
}

impl<'t> ScopeIndex<'t> {
    fn new(tree: &'t GreenTree) -> Self {
        ScopeIndex {
            tree,
            anchor: HashMap::default(),
            suite: HashMap::default(),
            lp: HashMap::default(),
        }
    }

    fn anchor(&mut self, n: NodeIdx) -> Above {
        climb(self.tree, &mut self.anchor, n, None, |p, c| {
            matches!(
                p.kind(),
                SyntaxKind::LetStmt
                    | SyntaxKind::BindingExpr
                    | SyntaxKind::MatchArm
                    | SyntaxKind::ForExpr
                    | SyntaxKind::ComprehensionFor
                    | SyntaxKind::Block
                    | SyntaxKind::ClosureExpr
                    | SyntaxKind::FnDecl
            )
            .then_some(Some((p.index(), c.index())))
        })
    }

    fn suite(&mut self, n: NodeIdx) -> Above {
        climb(self.tree, &mut self.suite, n, None, |p, c| {
            matches!(
                p.kind(),
                SyntaxKind::Block | SyntaxKind::ClosureExpr | SyntaxKind::FnDecl
            )
            .then_some(Some((p.index(), c.index())))
        })
    }

    /// The loop whose every iteration runs node `n`: a `for` or `while`
    /// whose head or body holds it, or a comprehension; none past a
    /// closure or a function.
    fn lp(&mut self, n: NodeIdx) -> Option<(u32, u32)> {
        let own = self.tree.node(n);
        match own.kind() {
            SyntaxKind::ComprehensionExpr => return Some(range(own)),
            SyntaxKind::ClosureExpr | SyntaxKind::FnDecl => return None,
            // ruled out: any other node climbs on to its parent below; syntax kinds are an open
            // set.
            _ => {}
        }
        climb(self.tree, &mut self.lp, n, None, |p, c| match p.kind() {
            SyntaxKind::ForExpr | SyntaxKind::WhileExpr => {
                let head = p.children().next().map(NodeRef::index);
                let body = first_block(p).map(NodeRef::index);
                [head, body]
                    .contains(&Some(c.index()))
                    .then_some(Some(range(p)))
            }
            SyntaxKind::ComprehensionExpr => Some(Some(range(p))),
            SyntaxKind::ClosureExpr | SyntaxKind::FnDecl => Some(None),
            _ => None,
        })
    }
}

/// The scope of origin `o`, declared at node `n`. A `let`, `:=` or
/// pattern binding is in scope from the end of its statement or binding
/// to the end of its suite, a `match` arm's in its arm, a `for` pattern's
/// in the loop body, and a comprehension's in the comprehension. A
/// temporary or a compiler local is in scope from the start of its
/// statement to the end of its suite (`ref.dep.scope.temporary`).
///
/// From `n` itself, then the nearest node that can decide (`anchor`), at
/// most twice: a `let` sets the start, and its suite (`suite`) the end.
/// Memo reads after the first climb of each node ([`ScopeIndex`]).
fn scope_of(ix: &mut ScopeIndex<'_>, n: NodeIdx, temp: bool) -> Option<Scope> {
    let tree = ix.tree;
    let x = n.raw();
    let lp = ix.lp(n);
    let mut lo = temp.then_some(x);
    // The node asked and its child on the path from `n`.
    let mut at = (n, None);
    loop {
        let p = tree.node(at.0);
        let (pl, ph) = range(p);
        let region = match p.kind() {
            SyntaxKind::LetStmt | SyntaxKind::BindingExpr if lo.is_none() => {
                lo = Some(ph);
                None
            }
            SyntaxKind::MatchArm if lo.is_none() => Some((pl, ph)),
            SyntaxKind::ForExpr if lo.is_none() => first_block(p).map(range),
            SyntaxKind::ComprehensionFor if lo.is_none() => {
                tree.parent(p.index()).map(|u| range(tree.node(u)))
            }
            SyntaxKind::Block | SyntaxKind::ClosureExpr | SyntaxKind::FnDecl => {
                // A temporary's statement uses it from its start.
                let lo = lo?;
                let stmt = at.1.map(NodeIdx::raw);
                Some((if temp { stmt.unwrap_or(lo) } else { lo }, ph))
            }
            _ => None,
        };
        if let Some((lo, hi)) = region {
            return Some(Scope { lo, hi, lp });
        }
        let (up, child) = if lo.is_none() {
            ix.anchor(at.0)?
        } else {
            ix.suite(at.0)?
        };
        at = (up, Some(child));
    }
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
    let mut ix = ScopeIndex::new(&cx.src.parse.tree);
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
                .or_insert_with(|| decl_node(b, cx, o).and_then(|(n, t)| scope_of(&mut ix, n, t)))
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
