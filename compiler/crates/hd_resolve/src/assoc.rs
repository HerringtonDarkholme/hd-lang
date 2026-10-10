//! Which trait declares an associated type (trait-solver.md §4.1): a
//! projection and a binding name the declaring trait's item, which a trait
//! may reach through its supertraits (trait.assoc.projection,
//! trait.binding.name-reach). Header lowering and the body checker both
//! read traits through these, each with its own view of the declarations.

use std::collections::HashSet;

use hd_base::DefId;
use hd_types::{ParamRef, Ty, TyData, TyList, Types};

use crate::iface::{Names, show_ty};

/// How the two callers see traits: each trait's direct supertraits, as
/// trait-value types over its `Self` (parameter 0) and its parameters, and
/// the items a trait itself declares as associated types.
pub trait TraitView {
    fn supers(&self, trait_: DefId) -> Vec<Ty>;
    fn own_assoc(&self, trait_: DefId, name: &str) -> Option<DefId>;
    fn own_assocs(&self, trait_: DefId) -> Vec<DefId>;
}

/// Every associated type `trait_` declares or reaches through its
/// supertraits.
#[must_use]
pub fn reached_assocs(pool: Types<'_>, view: &dyn TraitView, trait_: DefId) -> Vec<DefId> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut todo = vec![trait_];
    while let Some(t) = todo.pop() {
        if !seen.insert(t) {
            continue;
        }
        out.extend(view.own_assocs(t));
        for s in view.supers(t) {
            if let TyData::TraitValue { def, .. } = pool.get(s) {
                todo.push(def);
            }
        }
    }
    out
}

/// The declarations of the associated type `name` that `trait_[args]`
/// declares or reaches through its supertraits: each declaring trait at
/// its arguments, `Self` read as `self_ty` (kept when `None`), and the
/// associated item. Arguments left implicit (`Self::X` in a trait) stay
/// implicit. A declaration reached twice, as through a diamond, is listed
/// once.
#[must_use]
pub fn assoc_decls(
    pool: Types<'_>,
    view: &dyn TraitView,
    trait_: DefId,
    args: TyList,
    self_ty: Option<Ty>,
    name: &str,
) -> Vec<(DefId, TyList, DefId)> {
    let mut out: Vec<(DefId, TyList, DefId)> = Vec::new();
    let mut seen = HashSet::new();
    let mut todo = vec![(trait_, args)];
    while let Some((t, a)) = todo.pop() {
        if !seen.insert((t, a)) {
            continue;
        }
        if let Some(d) = view.own_assoc(t, name) {
            if !out.iter().any(|x| x.2 == d) {
                out.push((t, a, d));
            }
            continue;
        }
        let known = pool.list_items(a);
        for s in view.supers(t).into_iter().rev() {
            let s = pool.subst(s, &|p: ParamRef| {
                if p.owner != t {
                    None
                } else if p.index == 0 {
                    self_ty
                } else {
                    known.get(p.index as usize - 1).copied()
                }
            });
            if let TyData::TraitValue { def, args, .. } = pool.get(s) {
                todo.push((def, args));
            }
        }
    }
    out
}

/// A written binding whose name reaches no associated type, or two
/// (`unknown-associated-type`, `ambiguous-associated-type`).
pub struct BadBinding {
    pub name: String,
    pub ambiguous: bool,
}

/// The written bindings of `trait_[args]`, each keyed by the associated
/// type its name reaches (trait.binding.name-reach). After a bad name,
/// every associated type left unbound is bound to poison, so the uses of
/// its projections report nothing more.
#[must_use]
pub fn resolve_bindings(
    pool: Types<'_>,
    view: &dyn TraitView,
    (trait_, args, self_ty): (DefId, TyList, Option<Ty>),
    written: Vec<(String, Ty)>,
) -> (Vec<(DefId, Ty)>, Vec<BadBinding>) {
    let mut bindings = Vec::new();
    let mut bad = Vec::new();
    for (name, t) in written {
        match assoc_decls(pool, view, trait_, args, self_ty, &name)[..] {
            [(_, _, assoc)] => bindings.push((assoc, t)),
            ref decls => bad.push(BadBinding {
                name,
                ambiguous: !decls.is_empty(),
            }),
        }
    }
    if !bad.is_empty() {
        for a in reached_assocs(pool, view, trait_) {
            if !bindings.iter().any(|(x, _)| *x == a) {
                bindings.push((a, Ty::POISON));
            }
        }
    }
    (bindings, bad)
}

/// An associated type that a bound fixes, the type it gets, and a flag:
/// for [`super_fixed`] whether the type names a `Self` on the way, for
/// [`bound_clashes`] whether the binding is written.
type Fixed = (DefId, Ty, bool);

/// The bindings the supertrait lists above `trait_[args]` fix, each with
/// whether it names the `Self` of a trait on the way (trait.binding.super.meaning).
fn super_fixed(
    pool: Types<'_>,
    view: &dyn TraitView,
    (trait_, args, self_ty): (DefId, TyList, Option<Ty>),
) -> Vec<Fixed> {
    let mut out: Vec<Fixed> = Vec::new();
    let mut seen: HashSet<(DefId, TyList)> = HashSet::new();
    let mut todo = vec![(trait_, args)];
    while let Some((tr, a)) = todo.pop() {
        if !seen.insert((tr, a)) {
            continue;
        }
        let known = pool.list_items(a);
        for s in view.supers(tr) {
            let s = pool.subst(s, &|p: ParamRef| {
                if p.owner != tr {
                    None
                } else if p.index == 0 {
                    self_ty
                } else {
                    known.get(p.index as usize - 1).copied()
                }
            });
            let TyData::TraitValue {
                def: sd,
                args: sa,
                bindings: sb,
            } = pool.get(s)
            else {
                continue;
            };
            for (assoc, b) in sb {
                // `Self` of a trait on the way is parameter 0 of its owner.
                let names_self = std::cell::Cell::new(false);
                let _ = pool.subst(b, &|p: ParamRef| {
                    if p.index == 0 && seen.iter().any(|x| x.0 == p.owner) {
                        names_self.set(true);
                    }
                    None
                });
                if !out.iter().any(|x| x.0 == assoc) {
                    out.push((assoc, b, names_self.get()));
                }
            }
            todo.push((sd, sa));
        }
    }
    out
}

/// A `dyn` type with the bindings its trait's supertrait lists fix added
/// (trait.binding.super.meaning): every implementation of the trait binds
/// them so, so `dyn PriceFeed` under `PriceFeed < Feed[Item = i32]` is
/// `dyn PriceFeed[Item = i32]` (trait.dyn.binding.identity). A binding
/// that names `Self` is not one a value can carry, and stays a projection.
#[must_use]
pub fn with_super_bindings(pool: Types<'_>, view: &dyn TraitView, t: Ty) -> Ty {
    let TyData::TraitValue {
        def,
        args,
        mut bindings,
    } = pool.get(t)
    else {
        return t;
    };
    for (assoc, b, names_self) in super_fixed(pool, view, (def, args, None)) {
        if !names_self && !bindings.iter().any(|(x, _)| *x == assoc) {
            bindings.push((assoc, b));
        }
    }
    pool.intern_ty(&TyData::TraitValue {
        def,
        args,
        bindings,
    })
}

/// The associated types a trait value type leaves unbound: those of its
/// trait and its supertraits that neither it nor a supertrait list binds
/// (trait.dyn.binding.complete, req.key.binding.complete). A binding that
/// names `Self` still fixes its type.
#[must_use]
pub fn unbound_assocs(pool: Types<'_>, view: &dyn TraitView, t: Ty) -> Vec<DefId> {
    let TyData::TraitValue {
        def,
        args,
        bindings,
    } = pool.get(t)
    else {
        return Vec::new();
    };
    let fixed = super_fixed(pool, view, (def, args, None));
    reached_assocs(pool, view, def)
        .into_iter()
        .filter(|a| !bindings.iter().any(|(x, _)| x == a) && !fixed.iter().any(|(x, _, _)| x == a))
        .collect()
}

/// The error for a trait value type that leaves associated types unbound,
/// or `None` when it binds them all.
#[must_use]
pub fn incomplete_message(names: &Names<'_>, view: &dyn TraitView, t: Ty) -> Option<String> {
    let unbound = unbound_assocs(names.pool.types(), view, t);
    if unbound.is_empty() {
        return None;
    }
    let list: Vec<&str> = unbound.iter().map(|a| names.display_name(*a)).collect();
    Some(format!(
        "`{}` leaves `{}` unbound, so it is not dynamically safe",
        show_ty(names, t),
        list.join("`, `")
    ))
}

/// One associated type that two bounds of a list fix, in a generic
/// parameter's bounds or a trait's supertrait list.
pub struct BoundClash {
    pub assoc: DefId,
    /// The two bounds' traits, in list order, and the types they give it.
    pub first: (DefId, Ty),
    pub second: (DefId, Ty),
    /// Both bindings are written (`trait.binding.once.error`); otherwise
    /// two supertrait paths bind it to different types
    /// (`trait.binding.super.conflict`).
    pub written: bool,
}

/// The associated types that two bounds of one list both fix, `self_ty`
/// being the bounded type (`Self` of a trait for its supertrait list). A
/// written binding repeated in another bound is a clash even when it
/// names the same type; bindings that arrive through supertraits merge
/// when they name the same type and clash when they do not
/// (`trait.binding.once.elaborated`, `trait.binding.super.merge`).
#[must_use]
pub fn bound_clashes(
    pool: Types<'_>,
    view: &dyn TraitView,
    self_ty: Ty,
    bounds: &[Ty],
) -> Vec<BoundClash> {
    let fixed: Vec<(DefId, Vec<Fixed>)> = bounds
        .iter()
        .filter_map(|b| {
            let TyData::TraitValue {
                def,
                args,
                bindings,
            } = pool.get(*b)
            else {
                return None;
            };
            let mut all: Vec<Fixed> = bindings.into_iter().map(|(a, t)| (a, t, true)).collect();
            for (a, t, _) in super_fixed(pool, view, (def, args, Some(self_ty))) {
                if !all.iter().any(|x| x.0 == a) {
                    all.push((a, t, false));
                }
            }
            Some((def, all))
        })
        .collect();
    let mut out: Vec<BoundClash> = Vec::new();
    for (i, (d1, f1)) in fixed.iter().enumerate() {
        for (d2, f2) in &fixed[i + 1..] {
            for &(assoc, t1, w1) in f1 {
                let Some(&(_, t2, w2)) = f2.iter().find(|x| x.0 == assoc) else {
                    continue;
                };
                let same = t1 == t2;
                if pool.has_poison(t1)
                    || pool.has_poison(t2)
                    || (same && !(w1 && w2))
                    || out.iter().any(|c| c.assoc == assoc)
                {
                    continue;
                }
                out.push(BoundClash {
                    assoc,
                    first: (*d1, t1),
                    second: (*d2, t2),
                    written: w1 && w2,
                });
            }
        }
    }
    out
}

/// The error for a [`BoundClash`], naming both bounds.
#[must_use]
pub fn clash_message(names: &Names<'_>, c: &BoundClash) -> String {
    let name = names.display_name(c.assoc);
    let (a, b) = (
        names.display_name(c.first.0),
        names.display_name(c.second.0),
    );
    if c.written {
        format!("`{name}` is bound in both `{a}` and `{b}`, and a projection is bound once")
    } else {
        format!(
            "`{name}` is `{}` through `{a}` and `{}` through `{b}`",
            show_ty(names, c.first.1),
            show_ty(names, c.second.1)
        )
    }
}

/// Traits as a body sees them: through its module's items and the
/// interfaces it reads.
pub struct LookupView<'a> {
    pub lookup: &'a crate::Lookup<'a>,
    pub names: &'a crate::Names<'a>,
}

impl TraitView for LookupView<'_> {
    fn supers(&self, trait_: DefId) -> Vec<Ty> {
        match self.lookup.item(trait_).map(|i| &i.data) {
            Some(crate::ItemData::Trait(t)) => t.supers.clone(),
            _ => Vec::new(),
        }
    }
    fn own_assoc(&self, trait_: DefId, name: &str) -> Option<DefId> {
        match self.lookup.item(trait_).map(|i| &i.data) {
            Some(crate::ItemData::Trait(t)) => t
                .assoc
                .iter()
                .find(|(s, _)| self.names.text(*s) == name)
                .map(|(_, d)| *d),
            _ => None,
        }
    }
    fn own_assocs(&self, trait_: DefId) -> Vec<DefId> {
        match self.lookup.item(trait_).map(|i| &i.data) {
            Some(crate::ItemData::Trait(t)) => t.assoc.iter().map(|(_, d)| *d).collect(),
            _ => Vec::new(),
        }
    }
}
