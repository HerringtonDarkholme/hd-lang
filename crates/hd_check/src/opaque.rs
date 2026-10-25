//! Opens each declared opaque return occurrence independently while checking
//! its defining body. Callers retain the opaque descriptors in the header.

use crate::body::Ck;
use hd_base::StageResult;
use hd_resolve::view::Src;
use hd_syntax::{NodeRef, SyntaxKind};
use hd_types::{Ty, TyData, TyList, VarKind};

#[derive(Clone, Copy)]
enum Mode {
    Open,
    Recursive,
}

impl Ck<'_, '_> {
    fn opaque_return_args(&mut self, args: TyList, mode: Mode) -> TyList {
        let pool = self.pool();
        let args: Vec<_> = pool
            .list_items(args)
            .iter()
            .map(|&ty| self.map_opaque_return(ty, mode))
            .collect();
        pool.list(&args)
    }

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
        let data = match pool.get(ty) {
            TyData::Opaque { .. } => {
                if let Some(&(_, result)) = self.opaque_rets.iter().find(|&&(t, result)| {
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
                self.opaque_rets.push((ty, result));
                return result;
            }
            TyData::Adt { def, args } => TyData::Adt {
                def,
                args: self.opaque_return_args(args, mode),
            },
            TyData::Tuple { elems, rest } => TyData::Tuple {
                elems: self.opaque_return_args(elems, mode),
                rest: rest.map(|ty| self.map_opaque_return(ty, mode)),
            },
            TyData::Option(inner) => TyData::Option(self.map_opaque_return(inner, mode)),
            TyData::Mut(inner) => TyData::Mut(self.map_opaque_return(inner, mode)),
            TyData::Ref {
                inner,
                mutable,
                counted,
            } => TyData::Ref {
                inner: self.map_opaque_return(inner, mode),
                mutable,
                counted,
            },
            TyData::Fn {
                params,
                result,
                row,
                suspends,
                inputs,
            } => TyData::Fn {
                params: self.opaque_return_args(params, mode),
                result: self.map_opaque_return(result, mode),
                row: self.opaque_return_row(row, mode),
                suspends,
                inputs,
            },
            TyData::TraitValue {
                def,
                args,
                bindings,
            } => TyData::TraitValue {
                def,
                args: self.opaque_return_args(args, mode),
                bindings: bindings
                    .into_iter()
                    .map(|(def, ty)| (def, self.map_opaque_return(ty, mode)))
                    .collect(),
            },
            TyData::Assoc {
                assoc,
                trait_,
                self_ty,
                args,
            } => TyData::Assoc {
                assoc,
                trait_,
                self_ty: self.map_opaque_return(self_ty, mode),
                args: self.opaque_return_args(args, mode),
            },
            TyData::Row(row) => TyData::Row(self.opaque_return_row(row, mode)),
            TyData::Context(row) => TyData::Context(self.opaque_return_row(row, mode)),
            data => data,
        };
        pool.intern_ty(&data)
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
        if self.strict_rets.is_empty() {
            return false;
        }
        if self.strict_opaque_slot(ty) {
            return true;
        }
        let pool = self.pool();
        if !pool.has_infer(ty) {
            return false;
        }
        let list = |args| {
            pool.list_items(args)
                .iter()
                .any(|&t| self.strict_opaque_return(t))
        };
        match pool.get(ty) {
            TyData::Adt { args, .. } => list(args),
            TyData::Tuple { elems, rest } => {
                list(elems) || rest.is_some_and(|t| self.strict_opaque_return(t))
            }
            TyData::Option(t) | TyData::Mut(t) | TyData::Ref { inner: t, .. } => {
                self.strict_opaque_return(t)
            }
            TyData::Fn {
                params,
                result,
                row,
                ..
            } => {
                list(params)
                    || self.strict_opaque_return(result)
                    || pool
                        .row_data(row)
                        .keys
                        .into_iter()
                        .any(|t| self.strict_opaque_return(t))
            }
            TyData::TraitValue { args, bindings, .. } => {
                list(args)
                    || bindings
                        .into_iter()
                        .any(|(_, t)| self.strict_opaque_return(t))
            }
            TyData::Assoc { self_ty, args, .. } => self.strict_opaque_return(self_ty) || list(args),
            TyData::Row(row) | TyData::Context(row) => pool
                .row_data(row)
                .keys
                .into_iter()
                .any(|t| self.strict_opaque_return(t)),
            _ => false,
        }
    }

    /// Keep the chosen hidden types invariant inside ordinary outer
    /// conversions (for example an optional injection around the result).
    pub(crate) fn constrain_opaque_return(&mut self, got: Ty, want: Ty, node: NodeRef<'_>) -> bool {
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
        let list = |me: &mut Self, got, want| {
            pool.list_items(got)
                .iter()
                .zip(pool.list_items(want))
                .all(|(&g, &w)| me.constrain_opaque_return(g, w, node))
        };
        match (pool.get(incoming), pool.get(wanted)) {
            (_, TyData::Option(inner)) if self.injects_into(incoming, wanted) => {
                self.constrain_opaque_return(got, inner, node)
            }
            (TyData::Option(g), TyData::Option(w))
            | (TyData::Ref { inner: g, .. }, TyData::Ref { inner: w, .. }) => {
                self.constrain_opaque_return(g, w, node)
            }
            (TyData::Adt { def: a, args: g }, TyData::Adt { def: b, args: w }) if a == b => {
                list(self, g, w)
            }
            (TyData::Tuple { elems: g, rest: a }, TyData::Tuple { elems: w, rest: b }) => {
                list(self, g, w)
                    && match (a, b) {
                        (Some(g), Some(w)) => self.constrain_opaque_return(g, w, node),
                        _ => true,
                    }
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
                list(self, g, w)
                    && a_bindings
                        .into_iter()
                        .zip(b_bindings)
                        .all(|((a, g), (b, w))| a != b || self.constrain_opaque_return(g, w, node))
            }
            _ => true,
        }
    }

    pub(crate) fn local_return_annotation(
        &mut self,
        annotation: NodeRef<'_>,
        function: NodeRef<'_>,
    ) -> StageResult<Ty> {
        let before = self.opaque_annotation.replace(function.index().raw() + 1);
        let result = self.ty_node(annotation);
        self.opaque_annotation = before;
        result
    }

    pub(crate) fn declared_opaque_annotation(&mut self, n: NodeRef<'_>) -> StageResult<Ty> {
        let Some(site) = self.opaque_annotation else {
            self.err(
                hd_diag::Code::TypeMismatch,
                n,
                "`impl Trait` needs a named function's return annotation",
            );
            return Ok(Ty::POISON);
        };
        self.opaque_annotation = Some(site + 1);
        let bound = match Src::child(n, SyntaxKind::NamedType) {
            Some(t) => self.ty_node_as(t, crate::ty::NameRole::Trait)?,
            None => Ty::POISON,
        };
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
        Ok(pool.intern_ty(&TyData::Opaque {
            owner: self.b.body_mut().item,
            site,
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
            let (opaque, result) = self.opaque_rets[index];
            let TyData::Opaque { bound, .. } = self.pool().get(opaque) else {
                unreachable!()
            };
            self.check_opaque_result(result, bound, node)?;
        }
        Ok(())
    }
}
