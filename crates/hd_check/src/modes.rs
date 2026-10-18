//! Modes in bodies (spec/lang/13-ownership.md, Modes In Bodies): the
//! conversions between plain and borrowed values, and receivers. Loans
//! are a later section; what they need stays `unsupported` through the
//! body's ownership gap.

use hd_diag::Code;
use hd_syntax::NodeRef;
use hd_tir::ir::{Coercion, NONE, Ref, Tag, TirSink};
use hd_types::{Ty, TyData, TyList};

use crate::body::Ck;

impl Ck<'_, '_> {
    /// The type through which a borrowed value's fields and methods are
    /// found: a `^mut` borrow has mutable access, a shared borrow has
    /// readonly access (`own.body.field-read`).
    pub(crate) fn mode_view(&self, t: Ty) -> Ty {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        let TyData::Borrow {
            ty: inner, mutable, ..
        } = pool.get(t)
        else {
            return t;
        };
        let inner = self.infer.resolve(pool, inner);
        if mutable
            && matches!(
                pool.get(inner),
                TyData::Adt { .. } | TyData::TraitValue { .. }
            )
        {
            pool.intern_ty(&TyData::Mut(inner))
        } else {
            inner
        }
    }

    /// The type of field `ft` read through a borrow (`own.body.field-read`):
    /// a primitive is copied out, a borrow stays as it is, and any other
    /// part is borrowed for the same lifetimes, mutably when the base is a
    /// `^mut` borrow and the field is `mut`.
    pub(crate) fn borrowed_field(&self, ft: Ty, mutable: bool, lifetimes: TyList) -> Ty {
        let pool = self.pool();
        let ft = self.infer.resolve(pool, ft);
        if self.exempt_part(ft) {
            return ft;
        }
        let (ty, edge) = match pool.get(ft) {
            TyData::Mut(i) => (i, true),
            _ => (ft, false),
        };
        pool.intern_ty(&TyData::Borrow {
            ty,
            mutable: mutable && edge,
            lifetimes,
        })
    }

    /// A field read through a borrow as it is: a primitive, a borrowed
    /// value, `never`, and optionals and tuples of these
    /// (`own.body.field-read`).
    fn exempt_part(&self, t: Ty) -> bool {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        match pool.get(t) {
            TyData::Prim(_) | TyData::Never | TyData::Poison | TyData::Borrow { .. } => true,
            TyData::Option(i) | TyData::Mut(i) => self.exempt_part(i),
            TyData::Tuple { elems, rest } => pool
                .list_items(elems)
                .iter()
                .chain(rest.iter())
                .all(|&e| self.exempt_part(e)),
            _ => false,
        }
    }

    /// The conversions between modes (`own.body.*`); `None` when neither
    /// type has a mode at its top, or `want` is open.
    pub(crate) fn coerce_mode(&mut self, r: Ref, got: Ty, want: Ty, n: NodeRef<'_>) -> Option<Ref> {
        let pool = self.pool();
        let g = self.infer.resolve(pool, got);
        let w = self.infer.resolve(pool, want);
        let moded = |t: Ty| matches!(pool.get(t), TyData::Borrow { .. });
        if !moded(g) && !moded(w)
            || matches!(pool.get(w), TyData::Infer(_))
            || matches!(pool.get(g), TyData::Infer(_) | TyData::Poison)
            || g == w
        {
            return None;
        }
        let what = "this position";
        // Same modes: the lifetimes a later section checks are inferred
        // as types are.
        let snap = self.infer.snapshot();
        if self.infer.unify(pool, g, w).is_ok() {
            return Some(r);
        }
        self.infer.rollback(snap);
        match (pool.get(g), pool.get(w)) {
            (
                _,
                TyData::Borrow {
                    ty: wt, mutable, ..
                },
            ) => {
                let (gt, ok) = match pool.get(g) {
                    TyData::Borrow { ty, mutable: m, .. } => (ty, m || !mutable),
                    _ => (self.strip_mut(g), !mutable || self.has_mut_access(g)),
                };
                if !ok {
                    let msg = format!(
                        "a `^mut` borrow needs a `mut` view or a `^mut` borrow, but this is {}",
                        self.show(g)
                    );
                    self.err(Code::MutableUpgrade, n, &msg);
                    return Some(r);
                }
                if self.expect(gt, wt, n, what, crate::body::ExpectPosition::General) {
                    return Some(r);
                }
                if self.global_place(r) {
                    self.err(
                        Code::NotInOwnershipPrototype,
                        n,
                        "a borrow of a top-level binding is not in the ownership prototype; pass the value to a plain parameter",
                    );
                    self.prototype_limit = true;
                    return Some(r);
                }
                let kind = if mutable {
                    Coercion::LendMut
                } else {
                    Coercion::Lend
                };
                Some(self.b.coerce(kind, NONE, r, w, n.index()))
            }
            // An optional wraps its payload, which converts on its own.
            (_, TyData::Option(_)) => None,
            // A borrow in a plain position needs the lifetime checks of a
            // later section.
            _ => {
                self.mode_gap.get_or_insert(n.index());
                Some(r)
            }
        }
    }

    /// A method receiver against the method's `self` type `st`, which
    /// follows the rules of a parameter (`own.body.receiver`).
    pub(crate) fn receiver_mode(&mut self, recv: Ref, rt: Ty, st: Ty, n: NodeRef<'_>) -> Ref {
        let pool = self.pool();
        let rt = self.infer.resolve(pool, rt);
        let st = self.infer.resolve(pool, st);
        let moded = |t: Ty| matches!(pool.get(t), TyData::Borrow { .. });
        if !moded(rt) && !moded(st) {
            return recv;
        }
        let self_ty = self.strip_mut(self.mode_view(rt));
        let want = match pool.get(st) {
            TyData::Borrow { mutable, .. } => pool.intern_ty(&TyData::Borrow {
                ty: self_ty,
                mutable,
                lifetimes: TyList::EMPTY,
            }),
            _ => st,
        };
        self.coerce_mode(recv, rt, want, n).unwrap_or(recv)
    }

    /// Whether `r` reads a top-level binding or a part of one.
    pub(crate) fn global_place(&mut self, r: Ref) -> bool {
        let mut r = r;
        loop {
            let Some(i) = r.as_inst() else {
                return false;
            };
            let body = self.b.body_mut();
            let i = i.0 as usize;
            match body.tags[i] {
                Tag::GlobalGet => return true,
                Tag::Field | Tag::TupleGet | Tag::Coerce => r = Ref(body.data[i][0]),
                _ => return false,
            }
        }
    }
}
