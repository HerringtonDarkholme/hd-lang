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
        let is_self = |p: hd_types::ParamRef| p.owner == trait_ && p.index == 0;
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
            let target = |p: hd_types::ParamRef, s: usize| {
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
                    let p = hd_types::ParamRef {
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
/// ([`own_proj`]).
fn wholes(ls: &Layouts<'_>, t: Ty) -> (Vec<Ty>, bool) {
    let (pool, mut out, mut other) = (ls.pool, Vec::new(), false);
    if !ls.of(t).slots.is_empty() || !pool.has_assoc(t) {
        return (out, other);
    }
    let mut stack = vec![t];
    while let Some(t) = stack.pop() {
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
fn bare_parts(pool: Types<'_>, t: Ty) -> (Vec<hd_types::ParamRef>, bool) {
    let (mut out, mut other) = (Vec::new(), false);
    let mut stack = vec![t];
    while let Some(t) = stack.pop() {
        match pool.get(t) {
            TyData::TraitValue { .. } | TyData::Opaque { .. } => other = true,
            _ if !pool.has_param(t) => {}
            TyData::Param(p) if !out.contains(&p) => out.push(p),
            TyData::Param(_) | TyData::Assoc { .. } => {}
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
fn proj_holds(pool: Types<'_>, proj: Ty, p: hd_types::ParamRef) -> bool {
    let mut stack = vec![proj];
    while let Some(t) = stack.pop() {
        match pool.get(t) {
            _ if !pool.has_param(t) => {}
            TyData::Param(x) if x == p => return true,
            TyData::Assoc {
                trait_, self_ty, ..
            } if p.owner == trait_
                && pool.get(self_ty)
                    == TyData::Param(hd_types::ParamRef {
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
    Param(hd_types::ParamRef, usize),
    /// A slot of projection `t` of type parameter `p`, as `P::Out`: any
    /// slot of what `p` or a type parameter in its trait's arguments
    /// stands for ([`proj_holds`]), and any lifetime the projection's
    /// bounds write ([`line_up_call`] only).
    AllOf(hd_types::ParamRef, Ty),
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
        let param = |t: Ty| match pool.get(t) {
            TyData::Param(p) => Some(p),
            _ => None,
        };
        let decl = |st: &mut St, n: usize| {
            st.1.extend(std::iter::repeat_n(Key::Decl(st.0), n));
            st.0 += 1;
        };
        match (pool.get(d), pool.get(t)) {
            _ if depth > hd_base::limits::TYPE_DEPTH => false,
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
                st.1.extend(std::iter::repeat_n(Key::AllOf(p, d), ls.of(t).slots.len()));
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
                decl(st, 1);
                let ok = walk(ls, a, b, st, d1);
                // The whole slot of each side's referent ([`Slot::whole`]):
                // the declared one, or what a type parameter there stands
                // for whole, its slot 0, as a call passes it
                // ([`Ck::call_sig`]).
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
                match (ok, whole(d), whole(t)) {
                    (false, ..) | (_, false, false) => {}
                    (_, true, true) => decl(st, 1),
                    (_, false, true) if let Some(p) = bare() => st.1.push(Key::Param(p, 0)),
                    (_, false, true)
                        if st.2
                            && let Some((p, proj)) = param_proj(a) =>
                    {
                        st.1.push(Key::AllOf(p, proj));
                    }
                    (_, true, false) => st.0 += 1,
                    (_, false, true) => st.1.push(Key::None),
                }
                ok
            }
            (TyData::Ref { counted: true, .. }, TyData::Ref { counted: true, .. }) => true,
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
            ) if x == y => {
                if !list(a, b, st) {
                    return false;
                }
                // Layout::walk puts the suspension's retained origins after
                // its result slots, including inside another type argument.
                if x == ls.suspend {
                    decl(st, 1);
                }
                true
            }
            _ => ls.of(d).slots.is_empty() && ls.of(t).slots.is_empty(),
        }
    }
    let mut st = (0, Vec::new(), call);
    walk(ls, d, t, &mut st, 0).then_some(st.1)
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
/// ([`Slot::whole`]). [`Layouts`] makes each once.
struct Layout {
    slots: Vec<Slot>,
    /// The top slots.
    tops: Vec<usize>,
    /// How many origin sets a value of the type has: one per top slot, or
    /// one for a trait value with none; 0 when it may hold no origins.
    width: usize,
}

impl Layout {
    fn new(pool: Types<'_>, suspend: DefId, lifetimed: Lifetimed<'_>, t: Ty) -> Layout {
        let mut slots = Vec::new();
        Self::walk(pool, suspend, lifetimed, t, true, 0, &mut slots);
        for k in 0..slots.len() {
            let fun = slots[k].fun && !slots[k].hold;
            slots[k].written = fun
                && !(k == 0 && capture_slot(pool, t))
                && !(k > 0 && capture_slot(pool, referent(pool, slots[k - 1].ty)));
        }
        let tops: Vec<usize> = (0..slots.len()).filter(|&k| slots[k].top).collect();
        // A trait value may hold a local reference (`trait.dyn.ref.convert`),
        // and so may a trait's own projection where the trait has lifetimes.
        let holds = holds_local_ref(pool, t) || is_dyn(pool, t) || own_proj(pool, lifetimed, t);
        let width = if tops.is_empty() {
            usize::from(holds)
        } else {
            tops.len()
        };
        Layout { slots, tops, width }
    }

    /// Pushes the slots of `t` onto `out`. Any type may hold a suspension,
    /// so only depth stops the walk.
    fn walk(
        pool: Types<'_>,
        suspend: DefId,
        lifetimed: Lifetimed<'_>,
        t: Ty,
        top: bool,
        depth: u32,
        out: &mut Vec<Slot>,
    ) {
        if depth > hd_base::limits::TYPE_DEPTH {
            return;
        }
        let d = depth + 1;
        let walk = |t: Ty, top: bool, out: &mut Vec<Slot>| {
            Self::walk(pool, suspend, lifetimed, t, top, d, out);
        };
        let hold = matches!(pool.get(t), TyData::Adt { def, .. } if def == suspend);
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
        match pool.get(t) {
            TyData::Ref {
                counted: false,
                inner,
                ..
            } => {
                let k = out.len();
                out.push(slot);
                walk(inner, false, out);
                // A referent with no slot of its own that holds origins is
                // one place, which a store fills whole.
                if out.len() == k + 1 && whole_referent(pool, lifetimed, inner) {
                    out.push(Slot {
                        ty: inner,
                        top: false,
                        end: k + 2,
                        whole: true,
                        ..slot
                    });
                }
                out[k].end = out.len();
            }
            TyData::Opaque { borrows: true, .. } | TyData::Fn { .. } => out.push(slot),
            // A counted reference holds its referent's slots, as a
            // generic instance does (`ref.local.holding.counted`).
            TyData::Ref { inner, mutable, .. } => {
                let k = out.len();
                walk(inner, top, out);
                if mutable {
                    out[k..].iter_mut().for_each(|s| s.shared_mut = true);
                }
            }
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
    lifetimed: Lifetimed<'a>,
    types: RefCell<HashMap<Ty, Rc<Layout>>>,
    fields: RefCell<HashMap<(Ty, u32, u32), Option<FieldMap>>>,
}

impl<'a> Layouts<'a> {
    fn new(pool: Types<'a>, suspend: DefId, lifetimed: Lifetimed<'a>) -> Self {
        Layouts {
            pool,
            suspend,
            lifetimed,
            types: RefCell::default(),
            fields: RefCell::default(),
        }
    }

    fn of(&self, t: Ty) -> Rc<Layout> {
        if let Some(l) = self.types.borrow().get(&t) {
            return Rc::clone(l);
        }
        let l = Rc::new(Layout::new(self.pool, self.suspend, self.lifetimed, t));
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

    /// The slot of an instance with arguments `argv` of a type with
    /// `generics` that its `l`th lifetime, 1-based, lies in: that
    /// argument's one top slot.
    fn lifetime_slot(
        &self,
        generics: &[hd_resolve::Generic],
        argv: &[Ty],
        l: u16,
    ) -> Option<usize> {
        let mut lts = (generics.iter().enumerate()).filter(|(_, g)| g.lifetime);
        let g = lts.nth(usize::from(l).checked_sub(1)?)?.0;
        let start: usize = argv.get(..g)?.iter().map(|&x| self.of(x).slots.len()).sum();
        match self.of(*argv.get(g)?).tops.as_slice() {
            [one] => Some(start + one),
            _ => None,
        }
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
            (TyData::Tuple { elems, .. }, None) => {
                let es = pool.list_items(elems);
                let e = *es.get(j as usize)?;
                let start = es[..j as usize].iter().map(|&x| len(x)).sum();
                Some((e, run(start, e)))
            }
            (TyData::Adt { def, args }, _) => {
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
                let argv = pool.list_items(args);
                let ty = subst_owner(pool, def, argv, f);
                let start = |a: usize| argv[..a].iter().map(|&x| len(x)).sum::<usize>();
                let lifetime = |l: u16| self.lifetime_slot(&item.generics, argv, l);
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

/// Whether trait `def` has a lifetime parameter ([`own_proj`]).
type Lifetimed<'f> = &'f dyn Fn(DefId) -> bool;

/// Whether `t` holds a trait's own projection, as `Self::Item` in
/// `trait Shelf['b]`'s bodies, of a trait with a lifetime parameter. It lists
/// no trait arguments, so [`holds_local_ref`] says no to it, but it holds
/// what the trait's lifetimes name, as `S::Item` with `S < Shelf['a]` does
/// (`ref.lifetime.trait.bound-argument`).
fn own_proj(pool: Types<'_>, lifetimed: Lifetimed<'_>, t: Ty) -> bool {
    if !pool.has_assoc(t) {
        return false;
    }
    let mut stack = vec![t];
    while let Some(t) = stack.pop() {
        if !pool.has_assoc(t) {
            continue;
        }
        match pool.get(t) {
            TyData::Assoc {
                trait_, self_ty, ..
            } if pool.get(self_ty)
                == TyData::Param(hd_types::ParamRef {
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
fn lifetimed<'l>(lookup: &'l hd_resolve::Lookup<'_>) -> impl Fn(DefId) -> bool + 'l {
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
fn whole_referent(pool: Types<'_>, lifetimed: Lifetimed<'_>, t: Ty) -> bool {
    pool.has_assoc(t)
        && !is_dyn(pool, t)
        && (holds_local_ref(pool, t) || own_proj(pool, lifetimed, t))
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
}

impl CallSig {
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

/// What [`Ck::written_inputs`] reads of one body: the body's own map,
/// whose parameter slots give a field's written lifetime, and the stores
/// into each local, indexed once on first use.
struct FnValues<'a> {
    b: &'a Body,
    mine: Option<&'a DepSig>,
    stores: std::cell::OnceCell<HashMap<u32, Vec<u32>>>,
}

impl<'a> FnValues<'a> {
    fn new(b: &'a Body, mine: Option<&'a DepSig>) -> Self {
        FnValues {
            b,
            mine,
            stores: std::cell::OnceCell::new(),
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
    /// A mark that the value may point into result path `k`'s parameter
    /// outside that path (`ref.result.path.callee`).
    Outside(u32),
    /// The shared `Ref` at an instruction of a place a local's path
    /// reaches, or a `&mut` binding a call reads as `&`, for the conflict
    /// check only (`ref.excl.shared`, `ref.excl.call`).
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
}

impl Inputs<'_> {
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
    ) -> Option<CallSig> {
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
                            bound_ids = self.bound_ids(ls, self_ty, trait_, 0);
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
            Some(sig) => {
                let is_self = |s| names.text(s) == "self";
                let dep = DepSig::new(ls, &sig.params, is_self, sig.ret, &sig.lifetimes)
                    .with_paths(ls, &sig.paths);
                self.with_wholes(ls, dep)
            }
            None => DepSig::of_fn(ls, *b.ty.get(b.data[i][0] as usize)?)?,
        };
        let ids = dep.all_ids();
        let held_args = (dep.params.iter().enumerate())
            .filter(|(_, (t, s))| {
                local_ref(pool, *t).is_some_and(|(c, _)| !c)
                    && (ref_ids_of(ls, *t, s).first())
                        .is_some_and(|id| ids.contains(id) || ids.contains(&ANY))
            })
            .filter_map(|(p, _)| source_arg(b, i, p))
            .collect();
        let is_trait_self =
            |p: hd_types::ParamRef| trait_.is_some_and(|tr| p.owner == tr && p.index == 0);
        // A slot of the trait's `Self` that the implementation target's
        // lifetime fills (`ref.lifetime.trait`).
        // A slot of a trait argument whose bound writes the caller's
        // lifetime (`ref.lifetime.trait.bound-argument`).
        let own = |p: hd_types::ParamRef, s: usize| {
            let k = usize::from(p.index).checked_sub(1)?;
            let at = bound_ids.get(k)?.get(s).copied();
            at.filter(|&l| l != 0 && trait_ == Some(p.owner))
                .map(Var::Own)
        };
        let target = |p: hd_types::ParamRef, s: usize| {
            let k = usize::from(p.index);
            (trait_ == Some(p.owner))
                .then(|| self_slots.get(k)?.get(s).copied().flatten())
                .flatten()
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
        let mut fn_held: Vec<(hd_types::ParamRef, u32)> = Vec::new();
        if let Some(sig) = &sig {
            let n = sig.params.len();
            for (p, lts) in sig.fn_lifetimes.iter().take(n).enumerate() {
                let TyData::Fn { result, .. } = pool.get(sig.params[p].1) else {
                    continue;
                };
                for q in bare_parts(pool, result).0 {
                    for &l in lts.iter().filter(|&&l| l != 0) {
                        if !fn_held.contains(&(q, u32::from(l))) {
                            fn_held.push((q, u32::from(l)));
                        }
                    }
                }
            }
        }
        let held_by = |q: hd_types::ParamRef| {
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
        let mut by_value = false;
        if tag == Tag::CallValue {
            let ins: Vec<u32> = (dep.params.iter())
                .flat_map(|(t, ids)| ref_ids_of(ls, *t, ids))
                .collect();
            for (k, var) in self.written_inputs(ls, b, b.data[i][0], values) {
                if let Some(&id) = ins.get(k) {
                    links.push((Var::Life(id), var));
                    by_value = true;
                }
            }
        } else if let Some(sig) = &sig {
            let n = sig.params.len();
            for (p, lts) in sig.fn_lifetimes.iter().take(n).enumerate() {
                let Some(a) = (lts.iter().any(|&l| l != 0))
                    .then(|| source_arg(b, i, p))
                    .flatten()
                else {
                    continue;
                };
                for (k, var) in self.written_inputs(ls, b, a, values) {
                    if let Some(&l) = lts.get(k).filter(|&&l| l != 0) {
                        links.push((Var::Life(u32::from(l)), var));
                        by_value = true;
                    }
                }
            }
        }
        Some(CallSig {
            links,
            by_value,
            paths,
            store: matches!(tag, Tag::Call | Tag::Await) && held && writes && shared,
            result: ls.holds(b.ty[i]) && (tag == Tag::CallValue || depends),
            bare: ([dep.elided, dep.capture_elided].map(lifetime).into_iter())
                .flatten()
                .filter(|_| elides)
                .chain(bare)
                .collect(),
            params,
            outs,
            held: held_args,
        })
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
        v: u32,
        values: &FnValues<'_>,
    ) -> Vec<(usize, Var)> {
        let mut out = Vec::new();
        let mut seen = HashSet::default();
        self.trace_value(ls, b, values, (v, &[], 0), &mut seen, &mut out);
        out
    }

    /// [`Ck::written_inputs`] of the part `path` of value `v`.
    fn trace_value(
        &self,
        ls: &Layouts<'_>,
        b: &Body,
        values: &FnValues<'_>,
        (v, path, depth): (u32, &[Proj], u32),
        seen: &mut HashSet<(u32, Vec<Proj>)>,
        out: &mut Vec<(usize, Var)>,
    ) {
        let i = v as usize;
        if depth > 32 || v & hd_tir::ir::Ref::CONST_BIT != 0 || i >= b.tags.len() {
            return;
        }
        let d = depth + 1;
        let mut push = |k: usize, var: Var| {
            if !out.contains(&(k, var)) {
                out.push((k, var));
            }
        };
        // A named function's value writes no lifetime of the body's in its
        // inputs, nor does a value of no function type. A closure literal
        // that writes one, and a function type nested in another type, as
        // a call through a value returns, are not traced: no record keeps
        // their written lifetimes.
        let tag = b.tags[i];
        if tag == Tag::Coerce {
            self.trace_value(ls, b, values, (b.data[i][0], path, d), seen, out);
        } else if tag == Tag::Read
            && let Some(place) = b.decode_place(b.data[i][0])
        {
            let full: Vec<Proj> = place.proj.iter().chain(path).copied().collect();
            match place.root {
                PlaceRoot::Local(l) => {
                    self.trace_local(ls, b, values, (l.raw(), &full, d), seen, out);
                }
                PlaceRoot::Value(r) => {
                    self.trace_value(ls, b, values, (r.0, &full, d), seen, out);
                }
                // A top-level binding's type names no lifetime of the
                // body's, and the frame and pointer roots hold no
                // function value the checker built.
                PlaceRoot::Global(..) | PlaceRoot::Frame(_) | PlaceRoot::Ptr(_) => {}
            }
        } else if tag == Tag::NewStruct
            && let [Proj::Field(j), rest @ ..] = path
            && let Some(&f) = b.record(b.data[i][1]).get(*j as usize)
        {
            // A struct literal's field is the value it was given.
            self.trace_value(ls, b, values, (f, rest, d), seen, out);
        } else if matches!(tag, Tag::Call | Tag::Await)
            && path.is_empty()
            && let Some(sig) = (Callee::from_words(b.record(b.data[i][0])))
                .map(|c| match c {
                    Callee::Item { def, .. } => def,
                    Callee::TraitMethod { method, .. } => method,
                })
                .and_then(|def| self.cx.lookup.item(def)?.sig())
        {
            // A call's result is at the call's own lifetimes.
            let ret = sig.fn_lifetimes.get(sig.params.len());
            for (k, &l) in ret.into_iter().flatten().enumerate() {
                if l != 0 {
                    push(k, Var::At(v, u32::from(l)));
                }
            }
        }
    }

    /// [`Ck::written_inputs`] of the part `path` of local `l`: a
    /// parameter's by its declared type, a binding's by what each store
    /// into it, or into the part read, puts there.
    fn trace_local(
        &self,
        ls: &Layouts<'_>,
        b: &Body,
        values: &FnValues<'_>,
        (l, path, depth): (u32, &[Proj], u32),
        seen: &mut HashSet<(u32, Vec<Proj>)>,
        out: &mut Vec<(usize, Var)>,
    ) {
        if !seen.insert((l, path.to_vec())) {
            return;
        }
        let param = b.kind == BodyKind::Fn
            && (b.local_flags.get(l as usize))
                .is_some_and(|f| f & hd_tir::ir::local_flags::PARAM != 0);
        let sig = self.cx.lookup.item(b.item).and_then(|it| it.sig());
        if param && let Some(sig) = sig.filter(|sig| (l as usize) < sig.params.len()) {
            self.trace_param(ls, sig, values, (l as usize, path), out);
            return;
        }
        for &s in values.stores(l) {
            let place = b.decode_place(b.data[s as usize][0]);
            if let Some(rest) = place.and_then(|p| path.strip_prefix(p.proj.as_slice())) {
                self.trace_value(
                    ls,
                    b,
                    values,
                    (b.data[s as usize][1], rest, depth),
                    seen,
                    out,
                );
            }
        }
    }

    /// [`Ck::written_inputs`] of the part `path` of parameter `p`: its own
    /// function type's ([`hd_resolve::FnSig::fn_lifetimes`]), or a
    /// function-typed field's ([`hd_resolve::Field::fn_lifetimes`]) at the
    /// lifetime the parameter's type gives that struct's lifetime.
    fn trace_param(
        &self,
        ls: &Layouts<'_>,
        sig: &hd_resolve::FnSig,
        values: &FnValues<'_>,
        (p, path): (usize, &[Proj]),
        out: &mut Vec<(usize, Var)>,
    ) -> Option<()> {
        let pool = ls.pool;
        let mut push = |k: usize, var: Var| {
            if !out.contains(&(k, var)) {
                out.push((k, var));
            }
        };
        let Some((&Proj::Field(j), steps)) = path.split_last() else {
            let lts = sig.fn_lifetimes.get(p).into_iter().flatten();
            for (k, &l) in lts.enumerate().filter(|x| *x.1 != 0) {
                push(k, Var::Own(u32::from(l)));
            }
            return Some(());
        };
        let &(mut ty, ref ids) = values.mine?.params.get(p)?;
        // The parameter's slots the part reached so far lies in, as
        // [`constraints::Gen::project`] walks them.
        let mut at: Vec<usize> = (0..ls.of(ty).slots.len()).collect();
        let mut variant = None;
        for step in steps {
            match *step {
                Proj::Deref => {
                    let (counted, _) = local_ref(pool, ty)?;
                    if !counted {
                        at = at.get(1..)?.to_vec();
                    }
                    ty = referent(pool, ty);
                }
                Proj::Variant(k) => variant = Some(k),
                Proj::Field(j) => {
                    let (child, map) = ls.field_slots(self.cx, ty, variant.take(), j)?;
                    at = map?
                        .iter()
                        .map(|&k| at.get(k).copied())
                        .collect::<Option<_>>()?;
                    ty = child;
                }
                Proj::Elem(_) => return None,
            }
        }
        // The function-typed field, at its struct's lifetimes.
        let TyData::Adt { def, args } = pool.get(ty) else {
            return None;
        };
        let item = self.cx.lookup.item(def)?;
        let fields = match (&item.data, variant) {
            (ItemData::Struct(fs, _), None) => fs,
            (ItemData::Enum { variants, .. }, Some(k)) => &variants.get(k as usize)?.fields,
            _ => return None,
        };
        let field = fields.get(j as usize)?;
        let argv = pool.list_items(args);
        for (k, &l) in field.fn_lifetimes.iter().enumerate().filter(|x| *x.1 != 0) {
            let s = ls.lifetime_slot(&item.generics, argv, l)?;
            push(k, Var::Own(*ids.get(*at.get(s)?)?));
        }
        Some(())
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

    /// [`Ck::proj_lifetimes`] unsorted. The base is a smaller type, so the
    /// walk ends.
    fn proj_lifetimes_into(&self, ls: &Layouts<'_>, proj: Ty, out: &mut Vec<u32>) {
        let pool = ls.pool;
        let TyData::Assoc {
            assoc,
            trait_: owner,
            self_ty: base,
            ..
        } = pool.get(proj)
        else {
            return;
        };
        out.extend(self.bound_ids(ls, base, owner, 0).concat());
        self.proj_lifetimes_into(ls, base, out);
        let Some(ItemData::AssocType { bounds, .. }) =
            self.cx.lookup.item(assoc).map(|it| &it.data)
        else {
            return;
        };
        (bounds.iter())
            .filter_map(|&b| pool.trait_value(b))
            .for_each(|(tr, _)| out.extend(self.bound_ids(ls, proj, tr, 0).concat()));
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
    fn bound_ids(&self, ls: &Layouts<'_>, self_ty: Ty, trait_: DefId, depth: u32) -> Vec<Vec<u32>> {
        if depth > 8 {
            return Vec::new();
        }
        match ls.pool.get(self_ty) {
            TyData::Param(p) => self.param_bound_ids(ls, p, trait_, depth),
            TyData::Assoc {
                assoc,
                trait_: owner,
                self_ty: base,
                args,
            } => self.assoc_bound_ids(ls, assoc, (owner, base, args), trait_, depth),
            _ => Vec::new(),
        }
    }

    /// [`Ck::bound_ids`] of type parameter `p`.
    fn param_bound_ids(
        &self,
        ls: &Layouts<'_>,
        p: hd_types::ParamRef,
        trait_: DefId,
        depth: u32,
    ) -> Vec<Vec<u32>> {
        let Some(it) = self.cx.lookup.item(p.owner) else {
            return Vec::new();
        };
        // A trait's parameters follow its `Self`, which is bounded by the
        // trait over its own parameters, lifetimes 1 and on in its scope.
        let k = usize::from(p.index);
        let k = match it.data {
            ItemData::Trait(_) if k == 0 => return self.self_bound_ids(ls, it, trait_, depth),
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
            if let Some(found) = self.reach(ls, (def, args), ids, trait_, depth) {
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
        depth: u32,
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
                let t = pool.intern_ty(&TyData::Param(hd_types::ParamRef {
                    owner: tr.def,
                    index,
                }));
                ids.push(vec![0; ls.of(t).slots.len()]);
                args.push(t);
            }
        }
        self.reach(ls, (tr.def, &args), ids, trait_, depth)
            .unwrap_or_default()
    }

    /// [`Ck::bound_ids`] of the projection of `assoc` on `owner[args]`
    /// over `base`: each bound of `assoc`, written in `owner`'s scope, over
    /// the lifetimes `base`'s bounds give `owner`'s arguments.
    fn assoc_bound_ids(
        &self,
        ls: &Layouts<'_>,
        assoc: DefId,
        (owner, base, args): (DefId, Ty, hd_types::TyList),
        trait_: DefId,
        depth: u32,
    ) -> Vec<Vec<u32>> {
        let Some(ItemData::AssocType {
            bounds,
            bound_lifetimes,
            ..
        }) = self.cx.lookup.item(assoc).map(|it| &it.data)
        else {
            return Vec::new();
        };
        let ids = self.bound_ids(ls, base, owner, depth + 1);
        if ids.iter().flatten().all(|&l| l == 0) {
            return Vec::new();
        }
        let Some(it) = self.cx.lookup.item(owner) else {
            return Vec::new();
        };
        let (args, lifetimes) = (
            ls.pool.list_items(args),
            trait_lifetimes(&it.generics, &ids),
        );
        let none = Vec::new();
        for (k, &b) in bounds.iter().enumerate() {
            let lts = bound_lifetimes.get(k).unwrap_or(&none);
            let Some((def, bids, bargs)) =
                Self::bound_in(ls, (owner, args), &ids, &lifetimes, b, lts)
            else {
                continue;
            };
            if let Some(found) = self.reach(ls, (def, &bargs), bids, trait_, depth + 1) {
                return found;
            }
        }
        Vec::new()
    }

    /// The slot lifetimes of `target`'s arguments, reached from trait
    /// `def` whose arguments' slots have lifetimes `ids`: `def` itself, or
    /// one of its supertraits.
    fn reach(
        &self,
        ls: &Layouts<'_>,
        (def, args): (DefId, &[Ty]),
        ids: Vec<Vec<u32>>,
        target: DefId,
        depth: u32,
    ) -> Option<Vec<Vec<u32>>> {
        if def == target {
            return Some(ids);
        }
        let it = self.cx.lookup.item(def)?;
        let ItemData::Trait(td) = &it.data else {
            return None;
        };
        if depth > 8 || td.super_lifetimes.is_empty() && ids.iter().flatten().all(|&l| l == 0) {
            return None;
        }
        let lifetimes = trait_lifetimes(&it.generics, &ids);
        let none = Vec::new();
        for (k, &sup) in td.supers.iter().enumerate() {
            let lts = td.super_lifetimes.get(k).unwrap_or(&none);
            let Some((sdef, sids, sargs)) =
                Self::bound_in(ls, (def, args), &ids, &lifetimes, sup, lts)
            else {
                continue;
            };
            if let Some(found) = self.reach(ls, (sdef, &sargs), sids, target, depth + 1) {
                return Some(found);
            }
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
            pool.subst(t, &|p: hd_types::ParamRef| {
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
            let q = hd_types::ParamRef { owner, index };
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
                let mine = self.bound_ids(ls, t, tr, 0);
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
            trait_slots,
            ..
        }) = self.cx.lookup.item(imp).map(|i| &i.data)
        else {
            return Vec::new();
        };
        let pool = ls.pool;
        let mut lts: Vec<Vec<u32>> = vec![self_lifetimes.iter().map(|&l| u32::from(l)).collect()];
        if !trait_slots.is_empty() {
            let args = pool.list_items(*trait_args);
            let written: Vec<u32> = trait_slots.iter().map(|&l| u32::from(l)).collect();
            for (&a, ids) in args.iter().zip(arg_ids(ls, args, &written)) {
                let inst = targs.get(lts.len() - 1).copied().unwrap_or(a);
                let keys = line_up(ls, a, inst).unwrap_or_default();
                let mine = keys.into_iter().map(|k| match k {
                    Key::Decl(j) => ids.get(j).copied().unwrap_or(0),
                    _ => 0,
                });
                lts.push(mine.collect());
            }
        }
        let at = |l: u32| {
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
                    hd_types::ParamRef {
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
    ) {
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
        let mut calls: HashMap<u32, CallSig> = HashMap::default();
        let mut closures: Vec<(usize, DepSig)> = Vec::new();
        let mut through = body.local_ty.iter().enumerate().any(|(l, &t)| {
            body.local_flags[l] & hd_tir::ir::local_flags::PARAM != 0 && capture_slot(pool, t)
        });
        for i in 0..body.tags.len() {
            if matches!(
                body.tags[i],
                Tag::Call | Tag::Await | Tag::CallDyn | Tag::CallValue
            ) && let Some(sig) = self.call_sig(ls, body, i, &values)
            {
                // A suspension that holds an argument may be kept
                // (`ref.dep.suspension.local`).
                let held = sig.result && ls.of(body.ty[i]).slots.iter().any(|s| s.hold);
                through |= sig.store || held || sig.passes_own();
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
        // The arguments a call's result holds, for the backend's run-time
        // loans (`ref.excl.alias.loan`): its holders are the bindings the
        // result reaches, directly or through a match on it.
        let mut held_args = Vec::new();
        for (&v, sig) in &calls {
            held_args.extend(sig.held.iter().map(|&a| (v, a)));
        }
        held_args.sort_stable();
        held_args.dedup();
        self.b.body_mut().held_args = held_args;
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
            reborrows: &facts.reborrows,
            moves: &facts.moves,
            reported: &self.ref_captured,
            paths: &paths,
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
        let verdicts = constraints::solve(&s, &returns);
        // One error for a conflict `ref.elem.conflict` reported.
        let tree = &self.cx.src.parse.tree;
        let elem: Vec<_> = (self.conflicts.iter())
            .map(|c| range(tree.node(c.1)))
            .collect();
        let sigs = (own.as_ref(), mine.as_ref());
        for (at, code, msg) in self.verdict_list(verdicts, sigs, fun, &paths) {
            let node = self.cx.src.parse.tree.node(at);
            let (lo, hi) = range(node);
            if code == Code::ConflictingAccess
                && elem
                    .iter()
                    .any(|&(a, b)| a <= lo && hi <= b || lo <= a && b <= hi)
            {
                continue;
            }
            self.err(code, node, &msg);
        }
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
            TyData::Adt { args, .. } => pool.list_items(args).to_vec(),
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
        for (at, msg) in v.capture {
            push(at, &|| msg.to_owned());
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
            Origin::Hidden(_) | Origin::Site(_) | Origin::Outside(_) => {
                format!("the value refers to a temporary of {f}")
            }
            Origin::Global(_) => "the value refers to a top-level binding".to_owned(),
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
    /// Call arguments passed at one of the body's own lifetimes that a
    /// bound or a function value's type writes: the origin that does not
    /// outlive it, the lifetime, and whether the call is of a function
    /// value (`ref.lifetime.trait.bound-argument`, `fn.type.lifetime.call.written`).
    bounds: Vec<(NodeIdx, Origin, u32, bool)>,
    /// Closures' returned values and the origin.
    closures: Vec<(NodeIdx, Origin)>,
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
