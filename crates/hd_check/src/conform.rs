//! A trait implementation's method signatures against the trait's
//! (spec 09 "Implementations": an implementing method has the trait
//! method's signature after substituting `Self` and the trait's
//! arguments). Error: `trait-method-signature`.

use hd_base::DefId;
use hd_base::SliceSort as _;
use hd_diag::Code;
use hd_resolve::{FnSig, ImplKind, ItemData, Lookup, Names};
use hd_syntax::NodeRef;
use hd_types::{ParamRef, Ty, TyData, TyList, Types};
use std::fmt::Write as _;

use crate::body::Ck;

/// The trait method's types at the implementation (`inst`), and a
/// projection's binding in the implementation (`bound`).
type SigMaps<'x> = (&'x dyn Fn(Ty) -> Ty, &'x dyn Fn(Ty) -> Option<Ty>);

/// Whether the implementation's type `g` is the trait's type `w`. A
/// projection the implementation binds (`bound`, its `type X = ...` with
/// no projection of its own) is that binding; any other projection is the
/// bodies' to compare and matches any type. Every form around one (a
/// reference, a type's or a trait value's arguments, a tuple, a function
/// type) must be `g`'s form, so `&Self::Out` never matches `i32`. One pass
/// over `w`'s forms down to its projections, with an explicit stack.
fn same_but_projections(
    pool: Types<'_>,
    (w, g): (Ty, Ty),
    bound: &dyn Fn(Ty) -> Option<Ty>,
) -> bool {
    let lists = |a: TyList, b: TyList, stack: &mut Vec<(Ty, Ty)>| {
        let (a, b) = (pool.list_items(a), pool.list_items(b));
        stack.extend(a.iter().copied().zip(b.iter().copied()));
        a.len() == b.len()
    };
    let mut stack = vec![(w, g)];
    let mut seen = hd_base::HashSet::default();
    while let Some((w, g)) = stack.pop() {
        #[cfg(test)]
        comparison_tests::visit();
        if w == g {
            continue;
        }
        if !pool.has_assoc(w) {
            return false;
        }
        if !seen.insert((w, g)) {
            continue;
        }
        let fits = match (pool.get(w), pool.get(g)) {
            (TyData::Assoc { .. }, _) => {
                if let Some(b) = bound(w) {
                    stack.push((b, g));
                }
                true
            }
            (
                TyData::Ref {
                    inner,
                    mutable,
                    counted,
                },
                TyData::Ref {
                    inner: gi,
                    mutable: gm,
                    counted: gc,
                },
            ) => {
                stack.push((inner, gi));
                mutable == gm && counted == gc
            }
            (TyData::Adt { def, args }, TyData::Adt { def: gd, args: ga }) => {
                def == gd && lists(args, ga, &mut stack)
            }
            (
                TyData::TraitValue { def, args, .. },
                TyData::TraitValue {
                    def: gd, args: ga, ..
                },
            ) => def == gd && lists(args, ga, &mut stack),
            (
                TyData::Tuple { elems, rest },
                TyData::Tuple {
                    elems: ge,
                    rest: gr,
                },
            ) => {
                match (rest, gr) {
                    (Some(r), Some(q)) => stack.push((r, q)),
                    (None, None) => {}
                    _ => return false,
                }
                lists(elems, ge, &mut stack)
            }
            (
                TyData::Fn { params, result, .. },
                TyData::Fn {
                    params: gp,
                    result: gr,
                    ..
                },
            ) => {
                stack.push((result, gr));
                lists(params, gp, &mut stack)
            }
            _ => false,
        };
        if !fits {
            return false;
        }
    }
    true
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
    AssocFaults {
        omitted: td
            .assoc
            .iter()
            .map(|(_, d)| *d)
            .filter(|d| !assoc.iter().any(|(b, _)| b == d))
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
        let Some(impl_item) = self.cx.lookup.item(*owner) else {
            return Ok(());
        };
        let ItemData::Impl {
            trait_,
            trait_args,
            trait_lifetimes,
            self_lifetimes,
            trait_slots,
            self_ty,
            kind,
            assoc: assoc_bindings,
            ..
        } = &impl_item.data
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
        // A structure protocol's method may strengthen the generic that
        // stands for the walked member (`annot.walker.strengthen-member-hook`).
        let known = self.cx.names.known;
        let free_bounds =
            crate::derive::structure_member_role(self.cx.names, self.cx.lookup, trait_, tm)
                .is_some();
        // A result left out is `missing-result-type` already.
        let omitted = hd_resolve::Src::type_child(node).is_none();
        // `Self::X` of this implementation is its own `type X = ...`.
        let bound = |t: Ty| match pool.get(t) {
            TyData::Assoc {
                assoc,
                trait_: tr,
                self_ty: st,
                ..
            } if tr == trait_ && st == self_ty => assoc_bindings
                .iter()
                .find(|(d, _)| *d == assoc)
                .map(|&(_, b)| b)
                .filter(|&b| !pool.has_assoc(b)),
            _ => None,
        };
        // Reuse the body's memoized projection normalizer for every nested
        // form, including chained impl bindings and function rows.
        let mut checked = std::borrow::Cow::Borrowed(sig);
        let mut normalized = hd_base::HashMap::default();
        for (index, &(_, ty)) in sig.params.iter().enumerate() {
            let resolved = self.normalize_deep_in(ty, &mut normalized)?;
            if resolved != ty {
                checked.to_mut().params[index].1 = resolved;
            }
        }
        let resolved = self.normalize_deep_in(sig.ret, &mut normalized)?;
        if resolved != sig.ret {
            checked.to_mut().ret = resolved;
        }
        // R2: an implementation of a remote method declares the trait's
        // result without `CrossError` (`module.remote.r2.impl`).
        let narrowed;
        let tsig = match crate::remote::is_remote_trait(self.cx.lookup, pool, known, trait_)
            .then(|| crate::remote::raw_result(pool, known, tsig.ret))
            .flatten()
        {
            Some(raw) => {
                narrowed = FnSig {
                    ret: raw,
                    ..tsig.clone()
                };
                &narrowed
            }
            None => tsig,
        };
        if let Some(why) = Self::sig_differs(pool, (tsig, &checked), free_bounds, (&inst, &bound))
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
        } else if !self.fn_deps_fit(
            self.sig_fn_deps(sig, 0),
            self.trait_fn_deps(
                tsig,
                sig,
                (trait_, *trait_args),
                (trait_lifetimes, self_lifetimes, trait_slots),
                impl_item.generics.iter().filter(|g| g.lifetime).count(),
            ),
        ) {
            // `trait.impl.lifetime.subtype`: an implementation may depend
            // on fewer parameter positions than the trait (whose opaque
            // results open over the implementation's result), never more.
            let msg = format!(
                "`{}` differs from the trait's declaration: its result borrows from other parameters",
                self.cx.names.text(it.name)
            );
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
        let known = &self.cx.names.known;
        crate::body::walk_supertraits(
            self.cx.lookup,
            self.pool(),
            (trait_, hd_types::TyList::EMPTY),
            crate::body::Fill::No,
            |def, _, st| {
                (def != trait_
                    && known.is_sealed(def)
                    && st.methods.iter().any(|(n, _)| *n == name))
                .then_some(())
            },
        )
        .is_some()
    }

    fn sig_differs(
        pool: Types<'_>,
        (want, got): (&FnSig, &FnSig),
        free_bounds: bool,
        (inst, bound): SigMaps<'_>,
    ) -> Option<&'static str> {
        // A type that failed to resolve was reported where it is
        // written; it matches anything here.
        let same = |w: Ty, g: Ty| {
            let w = inst(w);
            pool.has_poison(w) || pool.has_poison(g) || same_but_projections(pool, (w, g), bound)
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
        if !pool.has_opaque(want.ret) && !same(want.ret, got.ret) {
            return Some("the result type");
        }
        if want.suspends != got.suspends {
            return Some("suspension (`!`)");
        }
        let want_row = pool.subst_row(want.row, &|p| {
            let p = pool.intern_ty(&TyData::Param(p));
            let t = inst(p);
            (t != p).then_some(t)
        });
        if want_row != got.row {
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
            let poisoned = wb.iter().chain(&gb).any(|b| pool.has_poison(*b));
            if wb != gb && !free_bounds && !poisoned {
                return Some("a type parameter's bounds");
            }
            if w.default.map(inst) != g.default {
                return Some("a type parameter's default");
            }
        }
        // `trait.where.impl.repeat`: the trait's `where` bounds whose
        // subject is still a type parameter, in order; one whose subject
        // became another type is not written (`trait.where.impl.concrete`).
        let wheres = want
            .wheres
            .iter()
            .map(|&(p, b)| (inst(p), inst(b)))
            .filter(|&(p, _)| matches!(pool.get(p), TyData::Param(_)));
        let mut written = got.wheres.iter();
        for w in wheres {
            match written.next() {
                Some(g) if w == *g || pool.has_poison(g.1) || pool.has_poison(w.1) => {}
                _ => return Some("the `where` bounds"),
            }
        }
        if written.next().is_some() {
            return Some("the `where` bounds");
        }
        // The trait declaration gives the parameter defaults
        // (`trait.impl.param-default.no-restate`).
        if got.defaults.iter().any(|&d| d) {
            return Some("a parameter default, which only the trait declares");
        }
        None
    }
}

#[cfg(test)]
mod comparison_tests {
    use super::*;
    use std::cell::Cell;

    thread_local! { static VISITS: Cell<usize> = const { Cell::new(0) }; }

    pub(super) fn visit() {
        VISITS.set(VISITS.get() + 1);
    }

    #[test]
    fn nested_impl_projection_comparison_visits_shared_pairs_once() {
        let pool = hd_types::InternPool::new();
        let projection = pool.intern_ty(&TyData::Assoc {
            assoc: DefId::from_raw(1),
            trait_: DefId::from_raw(2),
            self_ty: Ty::I32,
            args: TyList::EMPTY,
        });
        let pair = |values: &[Ty]| {
            pool.intern_ty(&TyData::Tuple {
                elems: pool.list(values),
                rest: None,
            })
        };
        let bound = |ty| (ty == projection).then_some(Ty::I32);
        let mut work = Vec::new();
        for width in [100, 1_000] {
            let (mut want, mut got) = (Ty::I32, projection);
            for _ in 0..20 {
                want = pair(&[want, want]);
                got = pair(&[got, got]);
            }
            want = pair(&vec![want; width]);
            got = pair(&vec![got; width]);
            VISITS.set(0);
            assert!(same_but_projections(pool.types(), (got, want), &bound));
            work.push(VISITS.get());
        }
        assert!(work[0] > 100);
        assert!(work[1] <= work[0] * 10);
        assert!(!same_but_projections(
            pool.types(),
            (Ty::BOOL, projection),
            &bound
        ));
    }
}
