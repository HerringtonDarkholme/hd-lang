//! Remote traits (`module.remote.*`): R1 on each method of a trait that
//! extends `Remote` ([`ItemCheck::remote_method`]), and R2's narrower
//! result for an implementation ([`raw_result`]).

use hd_base::{DefId, StageResult};
use hd_diag::Code;
use hd_resolve::{FnSig, KnownItems, Lookup};
use hd_types::solver::TraitRef;
use hd_types::{Ty, TyData, TyList, Types};

use crate::body::{Fill, walk_supertraits};
use crate::header::ItemCheck;

/// Whether `def` is a remote trait: it extends `Remote`, directly or
/// through another trait (`module.remote.trait`). Cost: its supertraits
/// once.
pub(crate) fn is_remote_trait(
    lookup: &Lookup<'_>,
    pool: Types<'_>,
    known: &KnownItems,
    def: DefId,
) -> bool {
    known.remote != DefId::NONE
        && def != known.remote
        && walk_supertraits(lookup, pool, (def, TyList::EMPTY), Fill::No, |_, _, t| {
            t.supers
                .iter()
                .any(|&s| matches!(pool.get(s), TyData::TraitValue { def, .. } if def == known.remote))
                .then_some(())
        })
        .is_some()
}

/// `Result[V, E]` of a remote method's declared `Result[V, E | CrossError]`
/// (`module.remote.r2.result`): `None` for any other result.
pub(crate) fn raw_result(pool: Types<'_>, known: &KnownItems, ret: Ty) -> Option<Ty> {
    let TyData::Adt { def, args } = pool.get(ret) else {
        return None;
    };
    let [v, u] = *pool.list_items(args) else {
        return None;
    };
    let TyData::Union(members) = pool.get(u) else {
        return None;
    };
    let [e, cross] = *pool.list_items(members) else {
        return None;
    };
    let is_cross = matches!(pool.get(cross), TyData::Adt { def, .. } if def == known.cross_error);
    (def == known.result && is_cross).then(|| {
        pool.intern_ty(&TyData::Adt {
            def,
            args: pool.list(&[v, e]),
        })
    })
}

impl ItemCheck<'_, '_> {
    /// R1 on method `sig` of a remote trait (`module.remote.r1.*`,
    /// `module.remote.r2.result`): a bang method, with no generic
    /// parameter and no row, whose result is `Result[V, E | CrossError]`
    /// and whose parameters, `V` and `E` are `Wire`. One report per
    /// method, the first rule it breaks.
    pub(crate) fn remote_method(&mut self, sig: &FnSig) -> StageResult<()> {
        let known = self.cx.names.known;
        let pool = self.pool();
        let why = if !sig.suspends {
            Some("is not a bang method".to_owned())
        } else if !sig.generics.is_empty() {
            Some("declares a generic parameter".to_owned())
        } else if !pool.row_data(sig.row).keys.is_empty()
            || !pool.row_data(sig.row).params.is_empty()
        {
            Some("has a requirement row".to_owned())
        } else if let Some(raw) = raw_result(pool, known, sig.ret) {
            let TyData::Adt { args, .. } = pool.get(raw) else {
                return Ok(());
            };
            let parts = (sig.params.iter())
                .filter(|(n, _)| self.cx.names.text(*n) != "self")
                .map(|&(_, t)| t)
                .chain(pool.list_items(args).iter().copied());
            let mut not_wire = None;
            for t in parts.collect::<Vec<_>>() {
                let tref = TraitRef {
                    trait_: known.wire,
                    self_ty: t,
                    args: TyList::EMPTY,
                };
                if known.wire != DefId::NONE && self.fails_ref(tref, 0)? {
                    not_wire = Some(format!(
                        "takes or returns `{}`, which is not Wire",
                        self.show(t)
                    ));
                    break;
                }
            }
            not_wire
        } else {
            Some("does not return `Result[V, E | CrossError]`".to_owned())
        };
        if let Some(why) = why {
            let msg = format!("a remote trait's method {why} (R1, R2)");
            self.report(0, Code::InvalidRemoteMethod, msg);
        }
        Ok(())
    }
}
