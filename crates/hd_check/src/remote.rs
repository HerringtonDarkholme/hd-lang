//! Remote traits (`module.remote.*`): R1 on each method of a trait that
//! extends `Remote` ([`ItemCheck::remote_method`]), R2's narrower result
//! for an implementation ([`raw_result`]), and the public result its
//! callers see ([`public_result`], [`Ck::public_exit`]).

use hd_base::{DefId, StageResult};
use hd_diag::Code;
use hd_resolve::{FnSig, ItemData, KnownItems, Lookup};
use hd_syntax::NodeRef;
use hd_tir::TirSink as _;
use hd_tir::ir::{Ref, Tag};
use hd_types::solver::TraitRef;
use hd_types::{Ty, TyData, TyList, Types};

use crate::body::{BodyCx, Ck, Fill, walk_supertraits};
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

/// The public `Result[V, E | CrossError]` of `raw`, the written
/// `Result[V, E]` of `def`, when `def` implements a remote trait's method
/// (`module.remote.r2.public`). `None` for any other method or result;
/// R2 reports a wrong one. Cost: the trait's supertraits once.
pub(crate) fn public_result(cx: &BodyCx<'_>, pool: Types<'_>, def: DefId, raw: Ty) -> Option<Ty> {
    let (lookup, known) = (cx.lookup, cx.names.known);
    let ItemData::Method { owner, .. } = lookup.item(def)?.data else {
        return None;
    };
    let ItemData::Impl { trait_, .. } = lookup.item(owner)?.data else {
        return None;
    };
    let TyData::Adt { def: result, args } = pool.get(raw) else {
        return None;
    };
    let [v, e] = *pool.list_items(args) else {
        return None;
    };
    if result != known.result
        || raw_result(pool, known, raw).is_some()
        || !is_remote_trait(lookup, pool, known, trait_)
    {
        return None;
    }
    let cross = pool.intern_ty(&TyData::Adt {
        def: known.cross_error,
        args: TyList::EMPTY,
    });
    let union = pool.intern_ty(&TyData::Union(pool.list(&[e, cross])));
    Some(pool.intern_ty(&TyData::Adt {
        def: result,
        args: pool.list(&[v, union]),
    }))
}

/// Each remote implementation method of the module with its public
/// result, which the module's items carry for every reader but the
/// method's own body check. Cost: one pass over the module's items.
pub fn public_results(cx: &BodyCx<'_>) -> Vec<crate::results::Inferred> {
    let pool = cx.names.pool.types();
    (cx.lookup.own.iter())
        .filter_map(|it| {
            let ret = public_result(cx, pool, it.def, it.sig()?.ret)?;
            Some(crate::results::Inferred {
                def: it.def,
                ret: Some(ret),
                row: None,
            })
        })
        .collect()
}

impl Ck<'_, '_> {
    /// The written argument of `remote::[T]`, which names a trait as
    /// `$.use(K)` names a key (`module.remote.builtin.remote.trait`): its
    /// trait value, or poison after a trait that is not remote
    /// (`unsatisfied-trait-bound`). `None` for another callee.
    pub(crate) fn remote_targ(
        &mut self,
        callee: NodeRef<'_>,
        tl: NodeRef<'_>,
    ) -> StageResult<Option<Ty>> {
        let remote_fn = self.cx.names.known.remote_fn;
        let args: Vec<_> = tl.children().filter(|c| c.kind().is_type()).collect();
        let [arg] = args[..] else {
            return Ok(None);
        };
        if remote_fn == DefId::NONE
            || self.resolve_path(&self.segments_expr(callee)) != Some(remote_fn)
        {
            return Ok(None);
        }
        let t = self.ty_node_as(arg, crate::ty::NameRole::Trait)?;
        let (pool, known) = (self.pool(), self.cx.names.known);
        match pool.get(t) {
            TyData::TraitValue { def, .. } if is_remote_trait(self.cx.lookup, pool, known, def) => {
                Ok(Some(t))
            }
            TyData::TraitValue { def, .. } => {
                let msg = format!(
                    "`{}` is not a remote trait: `remote::[T]` takes a trait that extends `Remote`",
                    self.cx.names.display_name(def)
                );
                self.err(Code::UnsatisfiedTraitBound, arg, &msg);
                Ok(Some(Ty::POISON))
            }
            _ => Ok(Some(Ty::POISON)),
        }
    }

    /// `module.remote.handle-call.root`: the root type `S` of a
    /// `handle_call!` call implements a remote trait, or the call is
    /// `unsatisfied-trait-bound`. A root that is a parameter is not
    /// asked: its generated body serves what its instance implements.
    /// Cost: one goal per remote trait with a row for `S`'s head.
    pub(crate) fn served_root(&mut self, s: Ty, n: NodeRef<'_>) -> StageResult<()> {
        let (pool, known) = (self.pool(), self.cx.names.known);
        let s = self.infer.resolve(pool, s);
        let Some((_, codes, tables)) = self.cx.head_tables(pool, s) else {
            return Ok(());
        };
        let mut traits = Vec::new();
        let impls = hd_types::solver::Impls::Owned(self.cx.impls);
        for &at in tables.iter() {
            impls.traits_at(at, codes, &mut traits);
        }
        traits.retain(|&t| is_remote_trait(self.cx.lookup, pool, known, t));
        for trait_ in traits {
            let arity = self.cx.lookup.item(trait_).map_or(0, |t| t.generics.len());
            let args: Vec<Ty> = (0..arity)
                .map(|_| self.infer.fresh(pool, hd_types::VarKind::General))
                .collect();
            let tref = TraitRef {
                trait_,
                self_ty: s,
                args: pool.list(&args),
            };
            let at = self.checkpoint();
            let fails = matches!(self.solve(tref)?, hd_types::solver::Answer::Fails(_));
            self.rollback(at);
            if !fails {
                return Ok(());
            }
        }
        let msg = format!(
            "`handle_call!` serves a root that implements a remote trait, and `{}` implements none",
            self.show(s)
        );
        self.err(Code::UnsatisfiedTraitBound, n, &msg);
        Ok(())
    }

    /// `r`, which the function frame returns as `target`, widened to the
    /// public result (`module.remote.r2.wrap`): `.Ok(v)` is rebuilt and
    /// `.Err(e)` is `.Err(._0(e))`. A closure's exit is `r`. Cost: one
    /// `SwitchTag` per exit of a remote implementation method.
    pub(crate) fn public_exit(&mut self, r: Ref, target: Ty, n: NodeRef<'_>) -> Ref {
        let Some(public) = self.public_ret else {
            return r;
        };
        if self.rets.depth() != 0 || target != *self.rets.base() {
            return r;
        }
        let pool = self.pool();
        let (TyData::Adt { args: raw, .. }, TyData::Adt { args: wide, .. }) =
            (pool.get(target), pool.get(public))
        else {
            return r;
        };
        let (&[v, e], &[_, union]) = (pool.list_items(raw), pool.list_items(wide)) else {
            return r;
        };
        let at = n.index();
        let arm = |ck: &mut Self, case: u32, payload: Ty| {
            let block = ck.b.open_block();
            let mut x = ck.b.payload(r, case, 0, payload, at);
            if case == 1 {
                let rec = ck.b.refs_record(&[x]);
                x = ck.b.emit(Tag::NewVariant, 0, rec, union, at);
            }
            let rec = ck.b.refs_record(&[x]);
            let x = ck.b.emit(Tag::NewVariant, case, rec, public, at);
            ck.b.close_block(block, Some(x), public, at)
        };
        let ok = arm(self, 0, v);
        let err = arm(self, 1, e);
        let rec = self.b.refs_record(&[Ref(0), ok, err]);
        self.b.emit(Tag::SwitchTag, r.0, rec, public, at)
    }
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
