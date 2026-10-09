//! Access permission (type-checking.md §8; spec 04 "Composite Values And
//! Access Permission", "Mutable Paths"). Every expression's type carries
//! its access, `mut T` or `T`; unification ignores the marker, so the
//! permission rules are checked here, at bindings, coercions, field
//! reads and mutations.

use hd_diag::Code;
use hd_resolve::ItemData;
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::ir::{Ref, Tag};
use hd_types::{ParamRef, Ty, TyData};

use crate::body::Ck;

/// How a field's declared type relates to its container's access
/// (types.path.field).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldKind {
    /// `field: mut U`.
    MutableEdge,
    /// `field: U` with a composite `U`.
    ReadonlyEdge,
    /// An embedded field.
    Embedded,
    /// `field: P` for a generic parameter `P`, or a non-composite field.
    Other,
}

impl Ck<'_, '_> {
    /// Whether a type is a composite reference type that has a readonly
    /// and a `mut` view: data, non-optional enums, collections and trait
    /// values.
    pub(crate) fn is_composite(&self, t: Ty) -> bool {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        match pool.get(t) {
            TyData::Mut(i) => self.is_composite(i),
            TyData::Adt { .. } | TyData::TraitValue { .. } => true,
            _ => false,
        }
    }

    /// Whether an expression of type `t` has mutable access
    /// (types.path.access): a `mut U` type, or a parameter bounded by a
    /// `mut` trait. A type with no readonly view, or one not known yet,
    /// counts as mutable so that no error is reported on it.
    pub(crate) fn has_mut_access(&self, t: Ty) -> bool {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        match pool.get(t) {
            TyData::Mut(_) => true,
            TyData::Param(_) => (0..self.env.clause_self.len())
                .any(|i| self.env.clause_self[i] == t && self.env.clause_mut[i]),
            TyData::Adt { .. } | TyData::TraitValue { .. } => false,
            _ => true,
        }
    }

    /// Whether `let mut` may bind a value of type `t`: mutable access, or
    /// an optional of a `mut` payload (types.bind.let-mut-optional).
    pub(crate) fn let_mut_ok(&self, t: Ty) -> bool {
        let pool = self.pool();
        let r = self.infer.resolve(pool, t);
        match pool.get(r) {
            TyData::Option(i) => {
                let i = self.infer.resolve(pool, i);
                matches!(pool.get(i), TyData::Mut(_)) || !self.is_composite(i)
            }
            _ => self.has_mut_access(r),
        }
    }

    /// The readonly view of `t`: its outer `mut` removed
    /// (types.bind.short, types.bind.let-readonly).
    pub(crate) fn readonly_view(&self, t: Ty) -> Ty {
        let pool = self.pool();
        let r = self.infer.resolve(pool, t);
        match pool.get(r) {
            TyData::Mut(i) => i,
            _ => t,
        }
    }

    /// The declared kind of a data field of `t`'s type.
    pub(crate) fn field_kind(&self, t: Ty, name: &str) -> Option<FieldKind> {
        let pool = self.pool();
        let t = self.strip_mut(t);
        let TyData::Adt { def, .. } = pool.get(t) else {
            return None;
        };
        let ItemData::Data(fields) = &self.cx.lookup.item(def)?.data else {
            return None;
        };
        let sym = self.cx.names.syms.intern(name);
        let f = fields.iter().find(|f| f.name == sym)?;
        Some(if f.embedded {
            FieldKind::Embedded
        } else {
            match pool.get(f.ty) {
                TyData::Mut(_) => FieldKind::MutableEdge,
                TyData::Param(ParamRef { .. }) => FieldKind::Other,
                TyData::Adt { .. } | TyData::TraitValue { .. } => FieldKind::ReadonlyEdge,
                _ => FieldKind::Other,
            }
        })
    }

    /// The access type of the field read `e.name` whose substituted type
    /// is `ft` (types.path.field): a mutable edge loses its `mut` through
    /// a readonly `e`; an embedded field takes `e`'s access.
    pub(crate) fn field_access(&self, base: Ty, name: &str, ft: Ty) -> Ty {
        let pool = self.pool();
        let mutable = self.has_mut_access(base);
        match self.field_kind(base, name) {
            Some(FieldKind::MutableEdge) if !mutable => self.readonly_view(ft),
            Some(FieldKind::Embedded) if mutable => {
                if matches!(pool.get(self.infer.resolve(pool, ft)), TyData::Mut(_)) {
                    ft
                } else {
                    pool.intern_ty(&TyData::Mut(ft))
                }
            }
            Some(FieldKind::Embedded) => self.readonly_view(ft),
            _ => ft,
        }
    }

    /// A store `e.f = v` or `e[i] = v` needs mutable access to `e`
    /// (types.path.store.readonly-edge, types.path.store.readonly-root).
    pub(crate) fn check_store_target(
        &mut self,
        base: NodeRef<'_>,
        (br, bt): (Ref, Ty),
        at: NodeRef<'_>,
    ) {
        if self.has_mut_access(bt) {
            return;
        }
        let mut edge = false;
        if base.kind() == SyntaxKind::FieldExpr
            && let Some(i) = br.as_inst()
            && self.b.body_mut().tags.get(i.0 as usize) == Some(&Tag::Field)
        {
            let inner = Ref(self.b.body_mut().data[i.0 as usize][0]);
            let it = self.b.ty_of(inner);
            let name = self.cx.src.text(self.cx.src.last(base)).to_owned();
            edge = self.field_kind(it, &name) == Some(FieldKind::ReadonlyEdge);
        }
        if edge {
            self.err(
                Code::ReadonlyEdge,
                at,
                "the field is declared without `mut`, so its value is readonly",
            );
        } else {
            let msg = format!(
                "{} is a readonly view; mutation needs `mut` access",
                self.show(bt)
            );
            self.err(Code::ReadonlyRoot, at, &msg);
        }
    }

    /// A `mut self` call needs mutable access to its receiver
    /// (types.path.mutation.receiver). `self_param` is the method's
    /// declared receiver type.
    pub(crate) fn check_receiver(&mut self, self_param: Ty, rt: Ty, at: NodeRef<'_>) {
        let pool = self.pool();
        let TyData::Mut(inner) = pool.get(self_param) else {
            return;
        };
        // A primitive `Self` has no `mut` form (types.prim.no-mut.self-call).
        if matches!(pool.get(self.strip_mut(inner)), TyData::Prim(_)) {
            return;
        }
        if self.has_mut_access(rt) {
            return;
        }
        let msg = format!(
            "this method takes `mut self`, but the receiver is a readonly {}",
            self.show(rt)
        );
        self.err(Code::MutableReceiverRequired, at, &msg);
    }

    /// A readonly value where `mut` is wanted (types.mut.no-upgrade,
    /// types.mut.argument). Returns whether it reported.
    pub(crate) fn check_upgrade(&mut self, got: Ty, want: Ty, n: NodeRef<'_>, what: &str) -> bool {
        let pool = self.pool();
        let w = self.infer.shallow(pool, want);
        let TyData::Mut(wi) = pool.get(w) else {
            return false;
        };
        if !self.is_composite(wi) || !self.is_composite(got) || self.has_mut_access(got) {
            return false;
        }
        // A declared `mut` parameter, not one generic inference made `mut`.
        // Building a `mut` trait value from a readonly value is an upgrade
        // by the conversion (trait.erase.mut.source).
        let erases = matches!(
            pool.get(self.infer.resolve(pool, wi)),
            TyData::TraitValue { .. }
        ) && !matches!(pool.get(self.strip_mut(got)), TyData::TraitValue { .. });
        let declared = matches!(pool.get(want), TyData::Mut(_)) && !erases;
        if what == "argument" && declared {
            let msg = format!(
                "the parameter takes {}, the argument is a readonly {}",
                self.show(want),
                self.show(got)
            );
            self.err(Code::ReadonlyArgumentToMutableParameter, n, &msg);
        } else {
            let msg = format!(
                "in {what}: a readonly {} cannot become {}",
                self.show(got),
                self.show(want)
            );
            self.err(Code::MutableUpgrade, n, &msg);
        }
        true
    }

    /// A `mut` bound needs a type argument with mutable access
    /// (trait.bound.mut, trait.bound.unsatisfied.cases).
    pub(crate) fn check_mut_bound(&mut self, t: Ty, at: NodeRef<'_>) {
        let pool = self.pool();
        let r = self.infer.resolve(pool, t);
        let readonly = match pool.get(r) {
            TyData::Param(_) => !self.has_mut_access(r),
            _ => self.is_composite(r) && !self.has_mut_access(r),
        };
        if readonly {
            let msg = format!(
                "a `mut` bound needs mutable access, but {} is readonly",
                self.show(t)
            );
            self.err(Code::UnsatisfiedTraitBound, at, &msg);
        }
    }

    /// Whether a trait, or one of its supertraits, has a `mut self`
    /// method: a mutable requirement trait (req.use, req.with).
    pub(crate) fn trait_is_mutable(&self, tr: hd_base::DefId, depth: u32) -> bool {
        let pool = self.pool();
        let Some(ItemData::Trait(t)) = self.cx.lookup.item(tr).map(|i| &i.data) else {
            return false;
        };
        if depth > 16 {
            return false;
        }
        t.methods.iter().any(|(_, m)| {
            self.cx
                .lookup
                .item(*m)
                .and_then(|i| i.sig())
                .and_then(|s| s.params.first())
                .is_some_and(|(s, st)| {
                    self.cx.names.text(*s) == "self" && matches!(pool.get(*st), TyData::Mut(_))
                })
        }) || t.supers.iter().any(|s| match pool.get(*s) {
            TyData::TraitValue { def, .. } => self.trait_is_mutable(def, depth + 1),
            _ => false,
        })
    }

    /// Whether `got` converts to `want` by permission alone, once the two
    /// unify: weakening `mut U` to `U` where a readonly outer view's
    /// declared variance allows it, exact permissions elsewhere
    /// (types.variance.readonly-only, types.variance.mut-invariant,
    /// types.option.invariant). `weaken` is false for an invariant place.
    pub(crate) fn perm_fits(&self, got: Ty, want: Ty, weaken: bool, depth: u32) -> bool {
        let pool = self.pool();
        let g = self.infer.resolve(pool, got);
        let w = self.infer.resolve(pool, want);
        if depth > 32 || g == w {
            return true;
        }
        let (gm, gi) = match pool.get(g) {
            TyData::Mut(i) => (true, i),
            _ => (false, g),
        };
        let (wm, wi) = match pool.get(w) {
            TyData::Mut(i) => (true, i),
            _ => (false, w),
        };
        if gm != wm && self.is_composite(gi) && self.is_composite(wi) && (wm || !weaken) {
            return false;
        }
        let d = depth + 1;
        match (pool.get(gi), pool.get(wi)) {
            (TyData::Adt { def, args: ga }, TyData::Adt { def: d2, args: wa }) if def == d2 => {
                let known = self.cx.names.known;
                let vs: Vec<i8> = match def {
                    d if d == known.list => vec![1],
                    d if d == known.map => vec![0, 1],
                    _ => self
                        .cx
                        .lookup
                        .item(def)
                        .map(|i| i.generics.iter().map(|g| g.variance).collect())
                        .unwrap_or_default(),
                };
                pool.list_items(ga)
                    .iter()
                    .copied()
                    .zip(pool.list_items(wa).iter().copied())
                    .enumerate()
                    .all(|(j, (a, b))| {
                        // A `mut` outer view is invariant in every argument.
                        let v = if wm {
                            0
                        } else {
                            vs.get(j).copied().unwrap_or(0)
                        };
                        match v {
                            1 => self.perm_fits(a, b, true, d),
                            -1 => self.perm_fits(b, a, true, d),
                            _ => self.perm_fits(a, b, false, d),
                        }
                    })
            }
            (TyData::Option(a), TyData::Option(b)) => self.perm_fits(a, b, false, d),
            (TyData::Tuple { elems: a, .. }, TyData::Tuple { elems: b, .. }) => pool
                .list_items(a)
                .iter()
                .copied()
                .zip(pool.list_items(b).iter().copied())
                .all(|(x, y)| self.perm_fits(x, y, weaken, d)),
            (TyData::TraitValue { args: a, .. }, TyData::TraitValue { args: b, .. }) => pool
                .list_items(a)
                .iter()
                .copied()
                .zip(pool.list_items(b).iter().copied())
                .all(|(x, y)| self.perm_fits(x, y, false, d)),
            // `fn.type.declared-variance`: contravariant in each input,
            // covariant in the output.
            (
                TyData::Fn {
                    params: a,
                    result: ar,
                    ..
                },
                TyData::Fn {
                    params: b,
                    result: br,
                    ..
                },
            ) => {
                pool.list_items(a)
                    .iter()
                    .copied()
                    .zip(pool.list_items(b).iter().copied())
                    .all(|(x, y)| self.perm_fits(y, x, true, d))
                    && self.perm_fits(ar, br, true, d)
            }
            _ => true,
        }
    }

    /// The permission join: a variable an earlier argument solved as
    /// `mut X` is lowered to `X` when this argument is a readonly `X`
    /// (types.generic.infer.join.outer-permission).
    pub(crate) fn join_down(&mut self, var: Ty, got: Ty) -> bool {
        let pool = self.pool();
        let TyData::Mut(x) = pool.get(self.infer.shallow(pool, var)) else {
            return false;
        };
        if self.is_composite(got) && !self.has_mut_access(got) && self.can_unify(got, x) {
            self.infer.rebind(pool, var, x);
            return true;
        }
        false
    }

    /// Whether `t` is a composite type, or an optional of one.
    fn composite_or_optional(&self, t: Ty) -> bool {
        let pool = self.pool();
        let r = self.infer.resolve(pool, t);
        match pool.get(r) {
            TyData::Option(i) => self.is_composite(i),
            _ => self.is_composite(r),
        }
    }

    /// A name written `let mut` (or `mut` in a `let` pattern) bound to a
    /// value of type `t` (types.bind.let-mut-upgrade,
    /// types.bind.let-mut-annotated, types.bind.let-mut-annotation).
    pub(crate) fn let_mut_name(&mut self, t: Ty, annotated: bool, at: NodeRef<'_>) {
        if !self.composite_or_optional(t) {
            return;
        }
        if !annotated {
            if !self.let_mut_ok(t) {
                let msg = format!(
                    "`let mut` needs mutable access, but the value is a readonly {}",
                    self.show(t)
                );
                self.err(Code::MutableUpgrade, at, &msg);
            }
            return;
        }
        if self.let_mut_ok(t) {
            let span = self.cx.src.span(at);
            self.diags.push(
                Code::RedundantLetMut,
                hd_diag::Severity::Warning,
                span,
                "the annotation already grants `mut`; remove the `mut` before the name",
                None,
            );
        } else {
            let msg = format!(
                "`let mut` with the readonly annotation {}; write `mut` in the type or drop it after `let`",
                self.show(t)
            );
            self.err(Code::LetMutReadonlyType, at, &msg);
        }
    }
}
