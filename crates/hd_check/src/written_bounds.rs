//! Goals for the declared bounds of written type arguments. Header and body
//! checking ask the same goals through their own solver contexts.

use hd_base::DefId;
use hd_resolve::Generic;
use hd_types::solver::TraitRef;
use hd_types::{ParamRef, Ty, TyData, Types};

/// The non-default arguments of `def`, paired with each substituted declared
/// bound: its goal, and the bound itself, whose bindings the argument's
/// projections must equal (`trait.bound.no-implied.positions`). Callers ask
/// the goals in order and stop after the first miss for an argument
/// (`types.generic.bound`, `types.generic.default.bound`).
///
/// `self_ty` is `Some` when `def` is a trait (`trait.bound.unsatisfied`):
/// its parameter 0 is `Self`, so `generics[i]` is parameter `i + 1`, and a
/// bound naming `Self` reads the implementing type. Each goal is one
/// substitution of one declared bound: linear in the bounds written.
pub(crate) fn goals(
    pool: Types<'_>,
    def: DefId,
    generics: &[Generic],
    self_ty: Option<Ty>,
    args: &[Ty],
) -> Vec<(usize, Ty, TraitRef, Ty)> {
    let subst = |t| {
        pool.subst(t, &|p: ParamRef| {
            if p.owner != def {
                return None;
            }
            match (self_ty, usize::from(p.index)) {
                (None, i) => args.get(i).copied(),
                (Some(s), 0) => Some(s),
                (Some(_), i) => args.get(i - 1).copied(),
            }
        })
    };
    let mut goals = Vec::new();
    for (i, g) in generics.iter().enumerate() {
        let Some(&arg) = args.get(i) else { continue };
        if g.default.is_some_and(|default| arg == subst(default)) {
            continue;
        }
        for &bound in &g.bounds {
            let bound = subst(bound);
            let TyData::TraitValue {
                def: trait_, args, ..
            } = pool.get(bound)
            else {
                continue;
            };
            goals.push((
                i,
                arg,
                TraitRef {
                    trait_,
                    self_ty: arg,
                    args,
                },
                bound,
            ));
        }
    }
    goals
}

/// [`goals`]' form of goals already substituted: (argument position,
/// argument, bound) each, as [`hd_resolve::AliasUse::goals`] keeps them.
pub(crate) fn of_bounds(
    pool: Types<'_>,
    bounds: &[(u16, Ty, Ty)],
) -> Vec<(usize, Ty, TraitRef, Ty)> {
    bounds
        .iter()
        .filter_map(|&(i, arg, bound)| {
            let TyData::TraitValue {
                def: trait_, args, ..
            } = pool.get(bound)
            else {
                return None;
            };
            let tref = TraitRef {
                trait_,
                self_ty: arg,
                args,
            };
            Some((usize::from(i), arg, tref, bound))
        })
        .collect()
}
