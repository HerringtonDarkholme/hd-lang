//! Access permission (`crates/hd_check/DESIGN.md` (type checking) §8;
//! spec 04 "Mutable Paths"): which values are records of places, which
//! traits change their receiver, and the permission part of a
//! conversion. Place writability (`types.value.writable-places`) decides
//! every write.

use hd_diag::Code;
use hd_syntax::NodeRef;
use hd_tir::ir::{Ref, Step};
use hd_types::{Ty, TyData, VarKind};

use crate::body::Ck;

/// A projection read ([`Ck::projection`]) and the value it reads from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Projection {
    /// A field of a struct, newtype or data value, or an element of a
    /// tuple.
    Field(Ref),
    /// A field of an enum value's payload.
    Payload(Ref),
}

impl Ck<'_, '_> {
    /// Whether `def` is a `struct` declaration (`data.struct.decl`).
    pub(crate) fn is_struct_def(&self, def: hd_base::DefId) -> bool {
        self.cx
            .lookup
            .item(def)
            .is_some_and(|i| self.cx.names.known.is_struct(i))
    }

    /// Whether `t` is a struct type (`types.value.struct`).
    pub(crate) fn is_struct_ty(&self, t: Ty) -> bool {
        let pool = self.pool();
        match pool.get(self.resolved(t)) {
            TyData::Adt { def, .. } => self.is_struct_def(def),
            _ => false,
        }
    }

    /// Whether `t` is a struct or a tuple type: a value whose
    /// fields or elements are parts of its place (`types.value.struct`,
    /// `expr.place.tuple-field`).
    pub(crate) fn is_value_record_ty(&self, t: Ty) -> bool {
        let pool = self.pool();
        let t = self.resolved(self.infer.resolve(pool, t));
        self.is_struct_ty(t) || matches!(pool.get(t), TyData::Tuple { .. })
    }

    /// What the projection read `r` reads, and from which value: a field
    /// of a struct or newtype value (`[%v .k]`), an
    /// element of a tuple (`[%v .k]`), or a payload field
    /// (`[%v as .V .k]`). `None` for any other value, a read through a
    /// reference among them.
    pub(crate) fn projection(&mut self, r: Ref) -> Option<Projection> {
        let i = r.as_inst()?.0 as usize;
        let (base, step) = self.b.body_mut().read_step(i)?;
        let base = Ref(base);
        Some(match step {
            Step::Field(_) => Projection::Field(base),
            Step::DerefField(_) => return None,
            Step::Variant(..) => Projection::Payload(base),
        })
    }

    /// The list and the index an element read `r` reads,
    /// `Read [%xs [%i]]` ([`hd_tir::ir::Body::element_read`]).
    pub(crate) fn element_of(&mut self, r: Ref) -> Option<(Ref, Ref)> {
        let i = r.as_inst()?.0 as usize;
        let (list, index) = self.b.body_mut().element_read(i)?;
        Some((Ref(list), Ref(index)))
    }

    /// An element store `e[i] = v` changes a collection value through a
    /// writable place (`types.coll.writable`).
    pub(crate) fn check_store_target(&mut self, (br, bt): (Ref, Ty), at: NodeRef<'_>) {
        if self.is_coll_ty(bt) {
            self.coll_change(br, bt, "this store", at);
        }
    }

    /// Whether a trait, or one of its supertraits, has a `mut self` or
    /// `&mut self` method: a mutable requirement trait
    /// (`req.mut.trait-changing`).
    pub(crate) fn trait_is_mutable(&self, tr: hd_base::DefId) -> bool {
        crate::body::walk_supertraits(
            self.cx.lookup,
            self.pool(),
            (tr, hd_types::TyList::EMPTY),
            crate::body::Fill::No,
            |_, _, t| {
                t.methods
                    .iter()
                    .any(|(_, m)| {
                        self.cx
                            .lookup
                            .item(*m)
                            .and_then(|i| i.sig())
                            .is_some_and(|sig| self.changes_receiver(sig))
                    })
                    .then_some(())
            },
        )
        .is_some()
    }

    /// Whether a method's receiver is `mut self` or `&mut self`
    /// (`req.mut.trait-changing`, `trait.dyn.ref.member.mut`).
    pub(crate) fn changes_receiver(&self, sig: &hd_resolve::FnSig) -> bool {
        sig.mut_self
            || sig.params.first().is_some_and(|(s, st)| {
                self.cx.names.text(*s) == "self"
                    && matches!(self.pool().get(*st), TyData::Ref { mutable: true, .. })
            })
    }

    /// Whether `got` converts to `want` by permission alone, once the two
    /// unify: weakening `&mut U` to `&U` where the declared variance
    /// allows it, exact permissions elsewhere (types.variance.readonly-only,
    /// types.variance.mut-invariant, types.option.invariant). `weaken` is
    /// false for an invariant place.
    pub(crate) fn perm_fits(&self, got: Ty, want: Ty, weaken: bool, depth: u32) -> bool {
        let pool = self.pool();
        let g = self.infer.resolve(pool, got);
        let w = self.infer.resolve(pool, want);
        if depth > 32 || g == w {
            return true;
        }
        let d = depth + 1;
        match (pool.get(g), pool.get(w)) {
            (TyData::Adt { def, args: ga }, TyData::Adt { def: d2, args: wa }) if def == d2 => {
                let vs = self.declared_variances(def);
                pool.list_items(ga)
                    .iter()
                    .copied()
                    .zip(pool.list_items(wa).iter().copied())
                    .enumerate()
                    .all(|(j, (a, b))| match vs.get(j).copied().unwrap_or(0) {
                        1 => self.perm_fits(a, b, true, d),
                        -1 => self.perm_fits(b, a, true, d),
                        _ => self.perm_fits(a, b, false, d),
                    })
            }
            // `ref.type.distinct`: a reference type is invariant in its
            // operand.
            (
                TyData::Ref {
                    inner: a,
                    mutable: am,
                    ..
                },
                TyData::Ref {
                    inner: b,
                    mutable: bm,
                    ..
                },
            ) => {
                (am == bm || (weaken && am && !bm))
                    && self.perm_fits(self.resolved(a), self.resolved(b), false, d)
            }
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
                    row: aw,
                    ..
                },
                TyData::Fn {
                    params: b,
                    result: br,
                    row: bw,
                    ..
                },
            ) => {
                // `types.variance.function`: the row is invariant, so only
                // a function value at the top widens it
                // (`req.row.subsume`); a function type inside another type
                // keeps its row exactly.
                (depth == 0 || self.rows_equal(aw, bw))
                    && pool
                        .list_items(a)
                        .iter()
                        .copied()
                        .zip(pool.list_items(b).iter().copied())
                        .all(|(x, y)| self.perm_fits(y, x, true, d))
                    && self.perm_fits(ar, br, true, d)
            }
            _ => true,
        }
    }

    /// A plain `let mut name` binding of type `t`, local or top-level. A
    /// local of a primitive type is valid, since `&mut` of it needs
    /// `let mut`. This returns the
    /// checked category for optional binding facts; lint judges whether
    /// mutable addressing makes the keyword useful. A
    /// top-level primitive one stays an error
    /// (types.bind.let-mut-primitive-top).
    pub(crate) fn let_mut_binding(
        &mut self,
        t: Ty,
        local: bool,
        at: NodeRef<'_>,
    ) -> Option<crate::body::LetMutKind> {
        let pool = self.pool();
        let lit = matches!(
            self.infer.kind_of(pool, t),
            Some(VarKind::IntLit | VarKind::SignedIntLit | VarKind::FloatLit)
        );
        match pool.get(self.resolved(t)) {
            _ if lit && local => return Some(crate::body::LetMutKind::Primitive),
            TyData::Prim(_) if local => return Some(crate::body::LetMutKind::Primitive),
            _ if lit => self.err(
                Code::MutOnPrimitive,
                at,
                "`let mut` on a primitive top-level binding; a plain `let` is already reassignable",
            ),
            TyData::Prim(_) => {
                self.err(
                    Code::MutOnPrimitive,
                    at,
                    "`let mut` on a primitive top-level binding; a plain `let` is already reassignable",
                );
            }
            _ => {}
        }
        None
    }
}
