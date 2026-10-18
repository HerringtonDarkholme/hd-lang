//! Modes in bodies (spec/lang/13-ownership.md, Modes In Bodies): the
//! conversions between plain and borrowed values, and receivers. Loans
//! are a later section; what they need stays `unsupported` through the
//! body's ownership gap.

use hd_diag::Code;
use hd_syntax::NodeRef;
use hd_tir::ir::{Callee, Coercion, NONE, Ref, Tag, TirSink};
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

    /// Whether `t` holds an owned value outside any borrow, through an
    /// optional, tuple, or type argument. With no owned mode nothing does,
    /// so the drops this guards have no input.
    pub(crate) fn owned_part(&self, t: Ty) -> bool {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        if !pool.has_mode(t) {
            return false;
        }
        match pool.get(t) {
            TyData::Option(i) | TyData::Mut(i) => self.owned_part(i),
            TyData::Tuple { elems, rest } => pool
                .list_items(elems)
                .iter()
                .chain(rest.iter())
                .any(|&e| self.owned_part(e)),
            TyData::Adt { args, .. } => pool.list_items(args).iter().any(|&a| self.owned_part(a)),
            _ => false,
        }
    }

    /// Registers local `l`'s drop where it is bound, when its type `t`
    /// holds an owned value (the TIR's `DropLocal`).
    pub(crate) fn register_drop(&mut self, l: hd_base::LocalId, t: Ty, at: hd_base::NodeIdx) {
        if self.owned_part(t) {
            self.b.drop_local(l, at);
        }
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

impl Ck<'_, '_> {
    /// Whether `def` is `DropOwn.drop` or a method that implements it.
    fn is_drop_method(&self, def: hd_base::DefId) -> bool {
        let drop_own = self.cx.names.known.drop_own;
        let Some(hd_resolve::ItemData::Method { owner, .. }) =
            self.cx.lookup.item(def).map(|i| &i.data)
        else {
            return false;
        };
        if *owner == drop_own {
            return true;
        }
        matches!(
            self.cx.lookup.item(*owner).map(|i| &i.data),
            Some(hd_resolve::ItemData::Impl { trait_, .. }) if *trait_ == drop_own
        )
    }

    /// `own.drop.direct`: a type's `drop` runs only when a value is dropped,
    /// so a call or reference of it in a body is an error.
    pub(crate) fn check_drop_calls(&mut self, n: usize) {
        let mut found = Vec::new();
        for i in 0..n {
            let body = self.b.body_mut();
            let def = match body.tags[i] {
                Tag::Call => match Callee::from_words(body.record(body.data[i][0])) {
                    Some(Callee::Item { def, .. }) => def,
                    Some(Callee::TraitMethod { method, .. }) => method,
                    None => continue,
                },
                Tag::ItemRef => match body.record(body.data[i][0]).first() {
                    Some(&d) => hd_base::DefId::from_raw(d),
                    None => continue,
                },
                _ => continue,
            };
            let at = body.syn[i];
            if at != hd_base::NodeIdx::NONE && self.is_drop_method(def) {
                found.push(at);
            }
        }
        for at in found {
            let node = self.cx.src.parse.tree.node(at);
            self.err(
                Code::DirectDropCall,
                node,
                "a type's `drop` runs only when one of its values is dropped; end the value with `std.ownership.drop(value)` instead",
            );
        }
    }
}
