//! A trait implementation's method signatures against the trait's
//! (spec 09 "Implementations": an implementing method has the trait
//! method's signature after substituting `Self` and the trait's
//! arguments). Error: `trait-method-signature`.

use hd_base::DefId;
use hd_base::SliceSort as _;
use hd_diag::Code;
use hd_resolve::{FnSig, ImplKind, ItemData, Lookup, Names};
use hd_syntax::NodeRef;
use hd_types::{ParamRef, Ty, TyData, Types};
use std::fmt::Write as _;

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
        TyData::Mut(i) | TyData::Option(i) | TyData::Ref { inner: i, .. } => has_assoc(pool, i, d),
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
pub fn assoc_faults(lookup: &Lookup<'_>, imp: DefId) -> AssocFaults {
    let Some(ItemData::Impl {
        trait_,
        assoc,
        kind: ImplKind::Written,
        ..
    }) = lookup.item(imp).map(|i| &i.data)
    else {
        return AssocFaults::default();
    };
    if *trait_ == DefId::NONE {
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
    pub(crate) fn check_impl_method(
        &mut self,
        def: DefId,
        node: NodeRef<'_>,
    ) -> hd_base::StageResult<()> {
        let pool = self.pool();
        let Some(it) = self.cx.lookup.item(def) else {
            return Ok(());
        };
        let ItemData::Method { owner, sig, .. } = &it.data else {
            return Ok(());
        };
        let Some(ItemData::Impl {
            trait_,
            trait_args,
            self_ty,
            kind,
            ..
        }) = self.cx.lookup.item(*owner).map(|o| &o.data)
        else {
            return Ok(());
        };
        if *trait_ == DefId::NONE || !matches!(kind, ImplKind::Written | ImplKind::Delegated) {
            return Ok(());
        }
        let Some(ItemData::Trait(td)) = self.cx.lookup.item(*trait_).map(|t| &t.data) else {
            return Ok(());
        };
        let Some(&(_, tm)) = td.methods.iter().find(|(n, _)| *n == it.name) else {
            // A method the trait does not declare (`trait.impl.extra-methods`),
            // unless a sealed supertrait declares it: that is reported as
            // `sealed-trait-implementation` already.
            if self.sealed_super_declares(*trait_, it.name) {
                return Ok(());
            }
            let msg = format!(
                "`{}` is not a member of `{}`; write it in an inherent `impl`",
                self.cx.names.text(it.name),
                self.cx.names.display_name(*trait_)
            );
            let src = self.cx.src;
            let name = src
                .name_after(node, hd_syntax::TokenKind::KwFn)
                .map_or_else(
                    || src.span(node),
                    |t| {
                        let (lo, hi) = src.parse.tokens.span(t);
                        hd_base::Span {
                            file: src.file,
                            lo,
                            hi,
                        }
                    },
                );
            self.diags.error(Code::ExtraTraitMember, name, &msg);
            return Ok(());
        };
        let Some(tsig) = self.cx.lookup.item(tm).and_then(|i| i.sig()) else {
            return Ok(());
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
                    let index = usize::from(p.index);
                    if tsig.generics.get(index).is_some_and(|g| g.row) {
                        // `subst_row` presents a row parameter through
                        // `ParamRef` too: keep its kind when moving it to
                        // the implementation method (`trait.impl.generics`).
                        let row = pool.row(&hd_types::RowData {
                            keys: vec![],
                            params: vec![hd_types::RowParamRef {
                                owner: def,
                                index: p.index,
                            }],
                        });
                        Some(pool.intern_ty(&TyData::Row(row)))
                    } else {
                        Some(pool.intern_ty(&TyData::Param(ParamRef {
                            owner: def,
                            index: p.index,
                        })))
                    }
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
        // A result left out is `missing-result-type` already.
        let omitted = hd_resolve::Src::type_child(node).is_none();
        let own_copy_receiver = self.is_struct_ty(self_ty)
            && tsig
                .params
                .first()
                .is_some_and(|p| self.cx.names.text(p.0) == "self")
            && sig
                .params
                .first()
                .is_some_and(|p| self.cx.names.text(p.0) == "self");
        if let Some(why) =
            Self::sig_differs(pool, (tsig, sig), free_bounds, own_copy_receiver, &inst)
            && !(omitted && why == "the result type")
        {
            let mut msg = format!(
                "`{}` differs from the trait's declaration: {why}",
                self.cx.names.text(it.name)
            );
            if trait_ == known.index_set && why == "a parameter type" {
                let params = self.cx.lookup.item(trait_).map_or_else(Vec::new, |t| {
                    t.generics
                        .iter()
                        .map(|g| self.cx.names.text(g.name))
                        .collect()
                });
                let _ = write!(
                    msg,
                    "; the trait takes {} type arguments, `{}[{}]`, for the key and value types",
                    params.len(),
                    self.cx.names.display_name(trait_),
                    params.join(", ")
                );
            }
            self.err(Code::TraitMethodSignature, node, &msg);
        } else if pool.has_opaque(tsig.ret) && !omitted {
            let declared = inst(tsig.ret);
            let before = self.opaque_rets.len();
            let opened = self.open_opaque_return(declared);
            let got = self.norm_ty(sig.ret);
            let opened = self.norm_ty(opened);
            let snapshot = self.infer.snapshot();
            if self.infer.unify(pool, got, opened).is_err() {
                self.infer.rollback(snapshot);
                self.err(
                    Code::TraitMethodSignature,
                    node,
                    "the result type differs from the trait's opaque result shape",
                );
            } else {
                self.add_opaque_bounds(got);
                self.check_opaque_occurrences(before..self.opaque_rets.len(), node)?;
            }
        }
        Ok(())
    }

    /// Whether a sealed transitive supertrait of `trait_` declares a method
    /// named `name`.
    fn sealed_super_declares(&self, trait_: DefId, name: hd_base::Symbol) -> bool {
        let pool = self.pool();
        let mut visited = vec![trait_];
        let mut todo = match self.cx.lookup.item(trait_).map(|t| &t.data) {
            Some(ItemData::Trait(td)) => td.supers.clone(),
            _ => Vec::new(),
        };
        while let Some(s) = todo.pop() {
            let TyData::TraitValue { def, .. } = pool.get(s) else {
                continue;
            };
            if visited.contains(&def) {
                continue;
            }
            visited.push(def);
            if let Some(ItemData::Trait(st)) = self.cx.lookup.item(def).map(|t| &t.data) {
                if self.cx.names.known.is_sealed(def) && st.methods.iter().any(|(n, _)| *n == name)
                {
                    return true;
                }
                todo.extend(st.supers.iter().copied());
            }
        }
        false
    }

    fn sig_differs(
        pool: Types<'_>,
        (want, got): (&FnSig, &FnSig),
        free_bounds: bool,
        own_copy_receiver: bool,
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
            // The trait head wraps abstract `mut self` in Mut(Self), while
            // lowering a struct impl gives its own changeable copy plain Self.
            // Normalize only that receiver; list/map and reference signatures
            // still compare exactly, as do every non-receiver parameter.
            let own_copy_matches = i == 0
                && own_copy_receiver
                && matches!(pool.get(inst(w.1)), TyData::Mut(inner) if inner == g.1);
            if !same(w.1, g.1) && !own_copy_matches {
                return Some(if i == 0 {
                    "a parameter or the receiver"
                } else {
                    "a parameter type"
                });
            }
        }
        if !pool.has_opaque(want.ret) && !same(want.ret, got.ret) {
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
            wb.sort_stable_by_key(|t| t.0);
            gb.sort_stable_by_key(|t| t.0);
            if wb != gb && !free_bounds {
                return Some("a type parameter's bounds");
            }
            if w.default.map(inst) != g.default {
                return Some("a type parameter's default");
            }
        }
        // The trait declaration gives the parameter defaults
        // (`trait.impl.param-default.no-restate`).
        if got.defaults.iter().any(|&d| d) {
            return Some("a parameter default, which only the trait declares");
        }
        None
    }
}
