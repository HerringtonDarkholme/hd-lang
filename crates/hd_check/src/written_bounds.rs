//! Goals for the declared bounds of written type arguments. Header and body
//! checking ask the same goals through their own solver contexts.

use hd_base::DefId;
use hd_resolve::Generic;
use hd_types::solver::TraitRef;
use hd_types::{ParamRef, Ty, TyData, Types};

/// The non-default arguments of `def`, paired with each substituted declared
/// bound. Callers ask the goals in order and stop after the first miss for an
/// argument (`types.generic.bound`, `types.generic.default.bound`).
pub(crate) fn goals(
    pool: Types<'_>,
    def: DefId,
    generics: &[Generic],
    args: &[Ty],
) -> Vec<(usize, Ty, TraitRef)> {
    let subst = |t| {
        pool.subst(t, &|p: ParamRef| {
            (p.owner == def)
                .then(|| args.get(usize::from(p.index)).copied())
                .flatten()
        })
    };
    let mut goals = Vec::new();
    for (i, g) in generics.iter().enumerate() {
        let Some(&arg) = args.get(i) else { continue };
        if g.default.is_some_and(|default| arg == subst(default)) {
            continue;
        }
        for &bound in &g.bounds {
            let TyData::TraitValue {
                def: trait_, args, ..
            } = pool.get(subst(bound))
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
            ));
        }
    }
    goals
}
