//! Opens each declared opaque return occurrence independently while checking
//! its defining body. Callers retain the opaque descriptors in the header.

use crate::body::Ck;
use hd_base::StageResult;
use hd_resolve::view::Src;
use hd_syntax::{NodeRef, SyntaxKind};
use hd_types::{Ty, TyData, VarKind};

#[derive(Clone, Copy)]
enum Mode {
    Open,
    Recursive,
}

impl Ck<'_, '_> {
    pub(crate) fn open_opaque_return(&mut self, ty: Ty) -> Ty {
        self.map_opaque_return(ty, Mode::Open)
    }

    pub(crate) fn recursive_opaque_return(&mut self, ty: Ty) -> Ty {
        self.map_opaque_return(ty, Mode::Recursive)
    }

    fn opaque_return_row(&mut self, row: hd_types::RowId, mode: Mode) -> hd_types::RowId {
        let pool = self.pool();
        let mut row = pool.row_data(row);
        row.keys = row
            .keys
            .into_iter()
            .map(|ty| self.map_opaque_return(ty, mode))
            .collect();
        pool.row(&row)
    }

    fn map_opaque_return(&mut self, ty: Ty, mode: Mode) -> Ty {
        let pool = self.pool();
        if !pool.has_opaque(ty) {
            return ty;
        }
        match pool.get(ty) {
            TyData::Opaque { bound, .. } => {
                if let Some(&(_, _, result)) = self.opaque_rets.iter().find(|&&(t, _, result)| {
                    t == ty && (matches!(mode, Mode::Open) || self.strict_rets.contains(&result))
                }) {
                    if matches!(mode, Mode::Recursive) {
                        self.recursive_returns.push(result);
                    }
                    return result;
                }
                if !matches!(mode, Mode::Open) {
                    return ty;
                }
                let result = self.infer.fresh(pool, VarKind::General);
                self.strict_rets.push(result);
                self.opaque_rets.push((ty, bound, result));
                result
            }
            _ => pool.map_children(ty, &mut OpaqueMap { ck: self, mode }),
        }
    }
    pub(crate) fn strict_opaque_slot(&self, ty: Ty) -> bool {
        if self.strict_rets.is_empty() {
            return false;
        }
        let pool = self.pool();
        let Some(class) = self.infer.class_of(pool, ty) else {
            return false;
        };
        self.strict_rets
            .iter()
            .any(|&slot| self.infer.class_of(pool, slot) == Some(class))
    }

    pub(crate) fn strict_opaque_return(&self, ty: Ty) -> bool {
        let pool = self.pool();
        if self.strict_rets.is_empty() || !pool.has_infer(ty) {
            return false;
        }
        if matches!(pool.get(ty), TyData::Infer(_)) {
            return self.strict_opaque_slot(ty);
        }
        let mut slots = self.strict_rets.iter().filter_map(|&slot| {
            hd_types::work::type_node();
            self.infer.class_of(pool, slot)
        });
        let mut classes = hd_base::HashSet::default();
        let mut seen = hd_base::HashSet::default();
        !hd_base::stack::all(ty, |ty, stack| {
            if !pool.has_infer(ty) || !seen.insert(ty) {
                return true;
            }
            hd_types::work::type_node();
            if let Some(class) = self.infer.class_of(pool, ty) {
                if classes.contains(&class) {
                    return false;
                }
                // Index lazily: an early match still reads only the slots
                // before it, and a false query reads each slot at most once.
                for candidate in slots.by_ref() {
                    if candidate == class {
                        return false;
                    }
                    classes.insert(candidate);
                }
            }
            let start = stack.len();
            match pool.get(ty) {
                TyData::Opaque { .. } | TyData::Union(_) => return true,
                _ => pool.children(ty, stack),
            }
            stack[start..].reverse();
            true
        })
    }

    /// Keep the chosen hidden types invariant inside ordinary outer
    /// conversions (for example an optional injection around the result).
    pub(crate) fn constrain_opaque_return(&mut self, got: Ty, want: Ty, node: NodeRef<'_>) -> bool {
        hd_base::stack::all((got, want), |(got, want), stack| {
            if !self.strict_opaque_return(want) {
                return true;
            }
            let pool = self.pool();
            let got = self.infer.shallow(pool, got);
            if got == Ty::NEVER {
                return true;
            }
            if self.strict_opaque_slot(want) {
                let got = self.norm_ty(got);
                let wanted = self.norm_ty(want);
                let snapshot = self.infer.snapshot();
                if self.infer.unify(pool, got, wanted).is_ok() {
                    return true;
                }
                self.infer.rollback(snapshot);
                let (expected, found) = self.show_pair(wanted, got);
                self.err(hd_diag::Code::TypeMismatch, node,
                &format!("all returns of `impl Trait` need one concrete type: expected {expected}, found {found}"));
                return false;
            }
            let wanted = self.expected_shape(want);
            let incoming = self.expected_shape(got);
            let list = |stack: &mut Vec<(Ty, Ty)>, got, want| {
                stack.extend(
                    pool.list_items(got)
                        .iter()
                        .zip(pool.list_items(want))
                        .rev()
                        .map(|(&g, &w)| (g, w)),
                );
            };
            if let Some(inner) = self.option_arg(wanted)
                && self.injects_into(incoming, wanted)
            {
                stack.push((got, inner));
                return true;
            }
            match (pool.get(incoming), pool.get(wanted)) {
                (TyData::Ref { inner: g, .. }, TyData::Ref { inner: w, .. }) => {
                    stack.push((g, w));
                }
                (TyData::Adt { def: a, args: g }, TyData::Adt { def: b, args: w }) if a == b => {
                    list(stack, g, w);
                }
                (TyData::Tuple { elems: g, rest: a }, TyData::Tuple { elems: w, rest: b }) => {
                    if let (Some(g), Some(w)) = (a, b) {
                        stack.push((g, w));
                    }
                    list(stack, g, w);
                }
                (
                    TyData::TraitValue {
                        def: a,
                        args: g,
                        bindings: a_bindings,
                    },
                    TyData::TraitValue {
                        def: b,
                        args: w,
                        bindings: b_bindings,
                    },
                ) if a == b => {
                    stack.extend(
                        a_bindings
                            .into_iter()
                            .zip(b_bindings)
                            .rev()
                            .filter_map(|((a, g), (b, w))| (a == b).then_some((g, w))),
                    );
                    list(stack, g, w);
                }
                _ => return true,
            }
            true
        })
    }

    pub(crate) fn local_return_annotation(
        &mut self,
        annotation: NodeRef<'_>,
        function: NodeRef<'_>,
        borrows: bool,
    ) -> StageResult<Ty> {
        let before = self
            .opaque_annotation
            .replace((function.index().raw() + 1, borrows));
        let result = self.ty_node(annotation);
        self.opaque_annotation = before;
        result
    }

    pub(crate) fn declared_opaque_annotation(&mut self, n: NodeRef<'_>) -> StageResult<Ty> {
        let Some((site, borrows)) = self.opaque_annotation else {
            self.err(
                hd_diag::Code::TypeMismatch,
                n,
                "`impl Trait` needs a named function's return annotation",
            );
            return Ok(Ty::POISON);
        };
        // An opaque type in the bound's arguments borrows nothing.
        self.opaque_annotation = Some((site + 1, false));
        let bound = match Src::child(n, SyntaxKind::NamedType) {
            Some(t) => self.ty_node_as(t, crate::ty::NameRole::Trait),
            None => Ok(Ty::POISON),
        };
        if let Some((s, _)) = self.opaque_annotation {
            self.opaque_annotation = Some((s, borrows));
        }
        let bound = bound?;
        let pool = self.pool();
        let args: Vec<Ty> = self
            .gens
            .iter()
            .map(|(name, ty)| {
                self.row_gens
                    .iter()
                    .find(|(s, _)| s == name)
                    .map_or(*ty, |(_, row)| {
                        pool.intern_ty(&TyData::Row(pool.row(&hd_types::RowData {
                            keys: vec![],
                            params: vec![*row],
                        })))
                    })
            })
            .collect();
        let owner = self.b.body_mut().item;
        let args = if let Some(hd_resolve::ItemData::Method { owner: parent, .. }) =
            self.cx.lookup.item(owner).map(|item| &item.data)
            && matches!(
                self.cx.lookup.item(*parent).map(|item| &item.data),
                Some(hd_resolve::ItemData::Trait(_))
            ) {
            self.self_ty.into_iter().chain(args).collect()
        } else {
            args
        };
        Ok(pool.intern_ty(&TyData::Opaque {
            owner,
            site,
            borrows,
            args: pool.list(&args),
            bound,
        }))
    }

    pub(crate) fn check_opaque_occurrences(
        &mut self,
        range: std::ops::Range<usize>,
        node: NodeRef<'_>,
    ) -> StageResult<()> {
        for index in range {
            let (_, bound, result) = self.opaque_rets[index];
            self.check_opaque_result(result, bound, node)?;
        }
        Ok(())
    }
}

/// [`Ck::map_opaque_return`] applied to each child of a type, and to each
/// row's keys.
struct OpaqueMap<'k, 'a, 'c> {
    ck: &'k mut Ck<'a, 'c>,
    mode: Mode,
}

impl hd_types::TyMap for OpaqueMap<'_, '_, '_> {
    fn ty(&mut self, t: Ty) -> Ty {
        self.ck.map_opaque_return(t, self.mode)
    }
    fn row(&mut self, r: hd_types::RowId) -> hd_types::RowId {
        self.ck.opaque_return_row(r, self.mode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::with_checker;

    #[test]
    fn strict_returns_at_depth_1000_fit_a_small_stack() {
        std::thread::Builder::new()
            // cost: one 96 KiB thread for this test.
            .stack_size(96 << 10)
            .spawn(|| {
                with_checker(|ck| {
                    let pool = ck.pool();
                    let strict = ck.infer.fresh(pool, VarKind::General);
                    let other = ck.infer.fresh(pool, VarKind::General);
                    ck.strict_rets.push(strict);
                    for depth in [100, 1_000] {
                        let mut chain = [strict, other];
                        for _ in 0..depth {
                            for t in &mut chain {
                                *t = pool.intern_ty(&TyData::Tuple {
                                    elems: pool.list(&[*t, *t]),
                                    rest: None,
                                });
                            }
                        }
                        for (t, expected) in [(chain[0], true), (chain[1], false)] {
                            let (answer, work) =
                                hd_types::work::count_type_nodes(|| ck.strict_opaque_return(t));
                            assert_eq!(answer, expected);
                            assert_eq!(work, depth + 2, "one graph walk and one strict-slot read");
                        }
                    }
                    let mut deep = strict;
                    for _ in 0..1_000 {
                        deep = pool.intern_ty(&TyData::Ref {
                            inner: deep,
                            mutable: false,
                            counted: true,
                        });
                    }
                    assert!(ck.constrain_opaque_return(deep, deep, ck.cx.src.parse.tree.root()));
                    let owner = ck.cx.names.item("app.main", "Shape");
                    let row = pool.row(&hd_types::RowData {
                        keys: vec![strict],
                        params: vec![],
                    });
                    for t in [
                        pool.intern_ty(&TyData::Adt {
                            def: owner,
                            args: pool.list(&[strict]),
                        }),
                        pool.intern_ty(&TyData::Tuple {
                            elems: hd_types::TyList::EMPTY,
                            rest: Some(strict),
                        }),
                        pool.intern_ty(&TyData::Ref {
                            inner: strict,
                            mutable: false,
                            counted: false,
                        }),
                        pool.intern_ty(&TyData::Fn {
                            params: hd_types::TyList::EMPTY,
                            result: Ty::I32,
                            row,
                            suspends: false,
                            inputs: hd_types::Inputs::Fixed,
                            deps: hd_types::FnDeps::NONE,
                        }),
                        pool.intern_ty(&TyData::TraitValue {
                            def: owner,
                            args: hd_types::TyList::EMPTY,
                            bindings: vec![(owner, strict)],
                        }),
                        pool.intern_ty(&TyData::Assoc {
                            assoc: owner,
                            trait_: owner,
                            self_ty: strict,
                            args: hd_types::TyList::EMPTY,
                        }),
                        pool.intern_ty(&TyData::Row(row)),
                        pool.intern_ty(&TyData::Context(row)),
                    ] {
                        assert!(
                            ck.strict_opaque_return(t),
                            "the original holder forms are traversed"
                        );
                    }
                    let opaque = pool.intern_ty(&TyData::Opaque {
                        owner,
                        site: 0,
                        args: pool.list(&[strict]),
                        bound: strict,
                        borrows: false,
                    });
                    let union = pool.intern_ty(&TyData::Union(pool.list(&[strict, Ty::BOOL])));
                    for t in [opaque, union] {
                        assert!(
                            !ck.strict_opaque_return(t),
                            "the original opaque/union boundaries stay leaves"
                        );
                    }
                    let root = pool.intern_ty(&TyData::Tuple {
                        elems: pool.list(&[other]),
                        rest: None,
                    });
                    assert!(!ck.strict_opaque_return(root));
                    let mark = ck.infer.snapshot();
                    ck.infer.unify(pool, strict, other).unwrap();
                    assert!(ck.strict_opaque_return(root));
                    ck.infer.rollback(mark);
                    assert!(!ck.strict_opaque_return(root));
                    ck.infer.unify(pool, strict, other).unwrap();
                    assert!(
                        ck.strict_opaque_return(root),
                        "a new query sees merged classes"
                    );
                    ck.infer.unify(pool, other, Ty::I32).unwrap();
                    assert!(
                        ck.strict_opaque_return(root),
                        "a solved occurrence stays strict"
                    );
                });
            })
            .expect("thread")
            .join()
            .expect("no stack overflow");
    }

    #[test]
    fn strict_return_slot_index_is_lazy_and_linear() {
        for width in [100, 1_000] {
            with_checker(|ck| {
                let pool = ck.pool();
                for _ in 0..width {
                    let slot = ck.infer.fresh(pool, VarKind::General);
                    ck.strict_rets.push(slot);
                }
                let others: Vec<_> = (0..width)
                    .map(|_| ck.infer.fresh(pool, VarKind::General))
                    .collect();
                let root = pool.intern_ty(&TyData::Tuple {
                    elems: pool.list(&others),
                    rest: None,
                });
                let (answer, work) =
                    hd_types::work::count_type_nodes(|| ck.strict_opaque_return(root));
                assert!(!answer);
                assert_eq!(work, 2 * width + 1);
                let first = pool.intern_ty(&TyData::Tuple {
                    elems: pool.list(&[ck.strict_rets[0]]),
                    rest: None,
                });
                let (answer, work) =
                    hd_types::work::count_type_nodes(|| ck.strict_opaque_return(first));
                assert!(answer);
                assert_eq!(work, 3, "the first match does not index unrelated slots");
            });
        }
    }
}
