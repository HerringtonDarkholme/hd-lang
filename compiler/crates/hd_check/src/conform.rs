//! A trait implementation's method signatures against the trait's
//! (spec 09 "Implementations": an implementing method has the trait
//! method's signature after substituting `Self` and the trait's
//! arguments). Error: `trait-method-signature`.

use hd_base::DefId;
use hd_diag::Code;
use hd_resolve::{FnSig, ImplKind, ItemData, Lookup, Names};
use hd_syntax::NodeRef;
use hd_types::{ParamRef, Ty, TyData, Types};

use crate::body::Ck;

/// Whether a type mentions an associated type projection, which this
/// comparison leaves to the bodies.
fn has_assoc(pool: Types<'_>, t: Ty, depth: u32) -> bool {
    if depth > 32 {
        return true;
    }
    let d = depth + 1;
    match pool.get(t) {
        TyData::Assoc { .. } => true,
        TyData::Mut(i) | TyData::Option(i) => has_assoc(pool, i, d),
        TyData::Adt { args, .. } | TyData::TraitValue { args, .. } => pool
            .list_items(args)
            .iter()
            .copied()
            .any(|a| has_assoc(pool, a, d)),
        TyData::Tuple { elems, rest } => {
            pool.list_items(elems)
                .iter()
                .copied()
                .any(|a| has_assoc(pool, a, d))
                || rest.is_some_and(|r| has_assoc(pool, r, d))
        }
        TyData::Fn {
            params,
            result,
            row,
            ..
        } => {
            pool.list_items(params)
                .iter()
                .copied()
                .any(|a| has_assoc(pool, a, d))
                || has_assoc(pool, result, d)
                || pool
                    .row_data(row)
                    .keys
                    .into_iter()
                    .any(|k| has_assoc(pool, k, d))
        }
        _ => false,
    }
}

/// The names of the trait methods that the written implementation `imp`
/// omits: a trait method with no default body that the impl does not write
/// (spec 09 `trait.impl.required`). A delegating implementation forwards
/// every method with a receiver, so it can omit only an associated
/// function (trait.by.assoc-fn). Derived and template implementations
/// supply their methods another way and yield none.
#[must_use]
pub fn omitted_trait_methods(names: &Names<'_>, lookup: &Lookup<'_>, imp: DefId) -> Vec<String> {
    let Some(ItemData::Impl {
        trait_,
        methods,
        kind,
        ..
    }) = lookup.item(imp).map(|i| &i.data)
    else {
        return Vec::new();
    };
    if *trait_ == DefId::NONE || !matches!(kind, ImplKind::Written | ImplKind::Delegated) {
        return Vec::new();
    }
    // A written implementation of a sealed trait is already
    // `sealed-trait-implementation`; its methods are the compiler's.
    if names.known.is_sealed(*trait_) {
        return Vec::new();
    }
    let Some(ItemData::Trait(td)) = lookup.item(*trait_).map(|t| &t.data) else {
        return Vec::new();
    };
    let delegated = *kind == ImplKind::Delegated;
    td.methods
        .iter()
        .filter(|(name, def)| {
            !methods.iter().any(|(w, _)| w == name)
                && matches!(
                    lookup.item(*def).map(|m| &m.data),
                    Some(ItemData::Method {
                        has_body: false,
                        sig,
                        ..
                    }) if !delegated || sig.params.first().is_none_or(|p| names.text(p.0) != "self")
                )
        })
        .map(|(name, _)| names.text(*name).to_owned())
        .collect()
}

/// How the associated types of a written implementation differ from its
/// trait's.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AssocFaults {
    /// The trait's associated types the implementation binds no type for
    /// and the trait gives no default (`trait.impl.required`).
    pub omitted: Vec<DefId>,
    /// The implementation's bindings of associated types the trait does
    /// not declare (`trait.impl.extra-methods`).
    pub extra: Vec<DefId>,
}

/// The associated types of the written implementation `imp` against the
/// trait's. Derived, template and delegating implementations take their
/// associated types another way and yield none.
#[must_use]
pub fn assoc_faults(names: &Names<'_>, lookup: &Lookup<'_>, imp: DefId) -> AssocFaults {
    let Some(ItemData::Impl {
        trait_,
        assoc,
        kind: ImplKind::Written,
        ..
    }) = lookup.item(imp).map(|i| &i.data)
    else {
        return AssocFaults::default();
    };
    if *trait_ == DefId::NONE || names.known.is_sealed(*trait_) {
        return AssocFaults::default();
    }
    let Some(ItemData::Trait(td)) = lookup.item(*trait_).map(|t| &t.data) else {
        return AssocFaults::default();
    };
    let has_default = |d: DefId| {
        matches!(
            lookup.item(d).map(|i| &i.data),
            Some(ItemData::AssocType {
                default: Some(_),
                ..
            })
        )
    };
    AssocFaults {
        omitted: td
            .assoc
            .iter()
            .map(|(_, d)| *d)
            .filter(|d| !assoc.iter().any(|(b, _)| b == d) && !has_default(*d))
            .collect(),
        extra: assoc
            .iter()
            .map(|(b, _)| *b)
            .filter(|b| !td.assoc.iter().any(|(_, d)| d == b))
            .collect(),
    }
}

impl Ck<'_, '_> {
    /// Compares the method `def` of a written trait implementation with
    /// the trait's method of its name; reports at `node`.
    pub(crate) fn check_impl_method(&mut self, def: DefId, node: NodeRef<'_>) {
        let pool = self.pool();
        let Some(it) = self.cx.lookup.item(def) else {
            return;
        };
        let ItemData::Method { owner, sig, .. } = &it.data else {
            return;
        };
        let Some(ItemData::Impl {
            trait_,
            trait_args,
            self_ty,
            kind,
            ..
        }) = self.cx.lookup.item(*owner).map(|o| &o.data)
        else {
            return;
        };
        if *trait_ == DefId::NONE || *kind != ImplKind::Written {
            return;
        }
        let Some(ItemData::Trait(td)) = self.cx.lookup.item(*trait_).map(|t| &t.data) else {
            return;
        };
        let Some(&(_, tm)) = td.methods.iter().find(|(n, _)| *n == it.name) else {
            return;
        };
        let Some(tsig) = self.cx.lookup.item(tm).and_then(|i| i.sig()) else {
            return;
        };
        let (trait_, self_ty) = (*trait_, *self_ty);
        let targs = pool.list_items(*trait_args);
        let inst = |t: Ty| {
            pool.subst(t, &|p: ParamRef| {
                if p.owner == trait_ {
                    if p.index == 0 {
                        Some(self_ty)
                    } else {
                        targs.get(usize::from(p.index) - 1).copied()
                    }
                } else if p.owner == tm {
                    Some(pool.intern_ty(&TyData::Param(ParamRef {
                        owner: def,
                        index: p.index,
                    })))
                } else {
                    None
                }
            })
        };
        // A walker, describer or source may strengthen `member`'s bound,
        // and a walker `rest`'s (annot.walker.strengthen-member).
        let known = self.cx.names.known;
        let mname = self.cx.names.text(it.name);
        let free_bounds =
            (trait_ == known.walker || trait_ == known.describer || trait_ == known.source)
                && (mname == "member" || (mname == "rest" && trait_ == known.walker));
        if let Some(why) = Self::sig_differs(pool, (tsig, sig), free_bounds, &inst) {
            let msg = format!(
                "`{}` differs from the trait's declaration: {why}",
                self.cx.names.text(it.name)
            );
            self.err(Code::TraitMethodSignature, node, &msg);
        }
    }

    fn sig_differs(
        pool: Types<'_>,
        (want, got): (&FnSig, &FnSig),
        free_bounds: bool,
        inst: &dyn Fn(Ty) -> Ty,
    ) -> Option<&'static str> {
        let same = |w: Ty, g: Ty| {
            let w = inst(w);
            has_assoc(pool, w, 0) || w == g
        };
        if want.params.len() != got.params.len() {
            return Some("the parameter count");
        }
        for (i, (w, g)) in want.params.iter().zip(&got.params).enumerate() {
            if !same(w.1, g.1) {
                return Some(if i == 0 {
                    "a parameter or the receiver"
                } else {
                    "a parameter type"
                });
            }
        }
        if !same(want.ret, got.ret) {
            return Some("the result type");
        }
        if want.suspends != got.suspends {
            return Some("suspension (`!`)");
        }
        let wk: Vec<Ty> = pool.row_data(want.row).keys.into_iter().map(inst).collect();
        let gk = pool.row_data(got.row).keys;
        if wk.len() != gk.len() || wk.iter().any(|k| !gk.contains(k)) {
            return Some("the requirement row");
        }
        if want.generics.len() != got.generics.len() {
            return Some("the number of type parameters");
        }
        for (w, g) in want.generics.iter().zip(&got.generics) {
            let mut wb: Vec<Ty> = w.bounds.iter().map(|b| inst(*b)).collect();
            let mut gb = g.bounds.clone();
            wb.sort_by_key(|t| t.0);
            gb.sort_by_key(|t| t.0);
            if wb != gb && !free_bounds {
                return Some("a type parameter's bounds");
            }
            if w.default.map(inst) != g.default {
                return Some("a type parameter's default");
            }
        }
        None
    }
}
