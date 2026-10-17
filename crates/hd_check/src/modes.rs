//! Modes in bodies (spec/lang/13-ownership.md, Modes In Bodies): the
//! conversions between plain, owned and borrowed values, freshness, and
//! receivers. Moves, drops and loans are later sections; what they need
//! stays `unsupported` through the body's ownership gap.

use hd_base::StageResult;
use hd_diag::Code;
use hd_syntax::NodeRef;
use hd_tir::ir::{Callee, Coercion, NONE, Ref, Tag, TirSink};
use hd_types::solver::{Answer, Impls, TraitRef};
use hd_types::{Ty, TyData, TyList};

use crate::body::Ck;

impl Ck<'_, '_> {
    /// The type through which a moded value's fields and methods are
    /// found: an owner and a `^mut` borrow have mutable access, a shared
    /// borrow has readonly access (`own.body.field-read`).
    pub(crate) fn mode_view(&self, t: Ty) -> Ty {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        let (inner, mutable) = match pool.get(t) {
            TyData::Own(i) => (i, true),
            TyData::Borrow { ty, mutable, .. } => (ty, mutable),
            _ => return t,
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
    /// `^mut` borrow and the field is `mut` or owned.
    pub(crate) fn borrowed_field(&self, ft: Ty, mutable: bool, lifetimes: TyList) -> Ty {
        let pool = self.pool();
        let ft = self.infer.resolve(pool, ft);
        if self.exempt_part(ft) && !matches!(pool.get(ft), TyData::Own(_)) {
            return ft;
        }
        let (ty, edge) = match pool.get(ft) {
            TyData::Own(i) | TyData::Mut(i) => (i, true),
            _ => (ft, false),
        };
        pool.intern_ty(&TyData::Borrow {
            ty,
            mutable: mutable && edge,
            lifetimes,
        })
    }

    /// A value that is not checked for freshness: a primitive, an owned
    /// or borrowed value, `never`, and optionals and tuples of these
    /// (`own.body.fresh`).
    fn exempt_part(&self, t: Ty) -> bool {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        match pool.get(t) {
            TyData::Prim(_)
            | TyData::Never
            | TyData::Poison
            | TyData::Own(_)
            | TyData::Borrow { .. } => true,
            TyData::Option(i) | TyData::Mut(i) => self.exempt_part(i),
            TyData::Tuple { elems, rest } => pool
                .list_items(elems)
                .iter()
                .chain(rest.iter())
                .all(|&e| self.exempt_part(e)),
            _ => false,
        }
    }

    /// Whether `r` is a fresh value (`own.body.fresh`): a construction
    /// whose every part that is not exempt is fresh too.
    pub(crate) fn fresh(&mut self, r: Ref) -> bool {
        let Some(i) = r.as_inst() else {
            return false;
        };
        let i = i.0 as usize;
        let body = self.b.body_mut();
        let tag = body.tags[i];
        let operands = match tag {
            Tag::NewData | Tag::NewVariant | Tag::NewMap | Tag::NewTuple | Tag::NewList => {
                body.value_operands(i)
            }
            Tag::Closure => Some(Vec::new()),
            // A conversion that keeps the value.
            Tag::Coerce
                if body.record(body.data[i][1]).first().is_some_and(|&k| {
                    [Coercion::WrapSome, Coercion::Weaken, Coercion::Variance]
                        .iter()
                        .any(|c| *c as u32 == k)
                }) =>
            {
                body.value_operands(i)
            }
            _ => None,
        };
        let Some(ops) = operands else {
            return false;
        };
        ops.into_iter().filter(|&o| o != NONE).all(|o| {
            let t = self.b.ty_of(Ref(o));
            self.exempt_part(t) || self.fresh(Ref(o))
        })
    }

    /// The conversions between modes (`own.body.*`); `None` when neither
    /// type has a mode at its top, or `want` is open.
    pub(crate) fn coerce_mode(&mut self, r: Ref, got: Ty, want: Ty, n: NodeRef<'_>) -> Option<Ref> {
        let pool = self.pool();
        let g = self.infer.resolve(pool, got);
        let w = self.infer.resolve(pool, want);
        let moded = |t: Ty| matches!(pool.get(t), TyData::Own(_) | TyData::Borrow { .. });
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
                    TyData::Own(i) => (i, true),
                    TyData::Borrow { ty, mutable: m, .. } => (ty, m || !mutable),
                    _ => (self.strip_mut(g), !mutable || self.has_mut_access(g)),
                };
                if !ok {
                    let msg = format!(
                        "a `^mut` borrow needs an owner, a `mut` view, or a `^mut` borrow, but this is {}",
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
            (TyData::Own(gt), TyData::Own(wt)) => {
                self.expect(gt, wt, n, what, crate::body::ExpectPosition::General);
                Some(r)
            }
            (TyData::Borrow { .. }, TyData::Own(_)) => {
                let msg = format!(
                    "{} is borrowed, so it cannot be given up as an owned value",
                    self.show(g)
                );
                self.err(Code::OwnUpgrade, n, &msg);
                Some(r)
            }
            (_, TyData::Own(wt)) => {
                let gt = self.strip_mut(g);
                if self.expect(gt, wt, n, what, crate::body::ExpectPosition::General) {
                    return Some(r);
                }
                if self.fresh(r) {
                    // A fresh value is owned where it is made.
                    if let Some(i) = r.as_inst() {
                        self.b.body_mut().ty[i.0 as usize] = w;
                    }
                } else {
                    let made = r.as_inst().is_some_and(|i| {
                        matches!(
                            self.b.body_mut().tags[i.0 as usize],
                            Tag::NewData
                                | Tag::NewVariant
                                | Tag::NewList
                                | Tag::NewMap
                                | Tag::NewTuple
                        )
                    });
                    let msg = if made {
                        format!(
                            "this {} holds a plain part that may have other paths, so it cannot become owned; build each plain part fresh",
                            self.show(gt)
                        )
                    } else {
                        format!(
                            "a plain {} may have other paths, so it cannot become owned; pass a fresh value or an owned one",
                            self.show(gt)
                        )
                    };
                    self.err(Code::OwnUpgrade, n, &msg);
                }
                Some(r)
            }
            // An optional wraps its payload, which converts on its own.
            (_, TyData::Option(_)) => None,
            (TyData::Own(_), _) => {
                let msg = format!(
                    "{} cannot be used as a plain {}: a plain copy would be a second path",
                    self.show(g),
                    self.show(w)
                );
                self.err(Code::OwnToPlain, n, &msg);
                Some(r)
            }
            // A borrow in a plain position needs the lifetime checks of a
            // later section.
            _ => {
                self.mode_gap.get_or_insert(n.index());
                Some(r)
            }
        }
    }

    /// `own T` in a body (`own.header.own-on-value`,
    /// `own.header.own-on-type-parameter`).
    pub(crate) fn own_ty(&mut self, t: Ty, n: NodeRef<'_>) -> Ty {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        match pool.get(t) {
            TyData::Prim(_) => {
                self.err(
                    Code::OwnOnValue,
                    n,
                    "a primitive value is immutable and has no owner: drop `own`",
                );
                t
            }
            TyData::Param(_) if Some(t) != self.self_ty => {
                self.err(
                    Code::OwnOnTypeParameter,
                    n,
                    "a generic writes no `own T`; bound the parameter with `T < own Any`",
                );
                t
            }
            TyData::Own(_) | TyData::Poison => t,
            _ => pool.intern_ty(&TyData::Own(t)),
        }
    }

    /// A method receiver against the method's `self` type `st`, which
    /// follows the rules of a parameter (`own.body.receiver`).
    pub(crate) fn receiver_mode(&mut self, recv: Ref, rt: Ty, st: Ty, n: NodeRef<'_>) -> Ref {
        let pool = self.pool();
        let rt = self.infer.resolve(pool, rt);
        let st = self.infer.resolve(pool, st);
        let moded = |t: Ty| matches!(pool.get(t), TyData::Own(_) | TyData::Borrow { .. });
        if !moded(rt) && !moded(st) {
            return recv;
        }
        let self_ty = self.strip_mut(self.mode_view(rt));
        let want = match pool.get(st) {
            TyData::Own(_) => pool.intern_ty(&TyData::Own(self_ty)),
            TyData::Borrow { mutable, .. } => pool.intern_ty(&TyData::Borrow {
                ty: self_ty,
                mutable,
                lifetimes: TyList::EMPTY,
            }),
            _ => st,
        };
        self.coerce_mode(recv, rt, want, n).unwrap_or(recv)
    }

    /// `own.body.store-fresh`: a store into a plain field of an owned
    /// value takes a fresh value or an owned one. The base is owned when
    /// its type is `own T`, or a `^mut` borrow of a type that owns.
    pub(crate) fn store_fresh(
        &mut self,
        base: Ty,
        ft: Ty,
        v: Ref,
        n: NodeRef<'_>,
    ) -> StageResult<()> {
        let pool = self.pool();
        let base = self.infer.resolve(pool, base);
        let owned = match pool.get(base) {
            TyData::Own(_) => true,
            TyData::Borrow {
                ty, mutable: true, ..
            } => self.owns(ty)?,
            _ => false,
        };
        if !owned || self.exempt_part(ft) || self.exempt_part(self.b.ty_of(v)) || self.fresh(v) {
            return Ok(());
        }
        let msg = format!(
            "a store into a plain field of an owned value takes a fresh value or an owned one, but this {} may have other paths",
            self.show(self.b.ty_of(v))
        );
        self.err(Code::OwnUpgrade, n, &msg);
        Ok(())
    }

    /// Whether a value of type `t` owns (`own.structure.owns`).
    pub(crate) fn owns(&mut self, t: Ty) -> StageResult<bool> {
        let pool = self.pool();
        let cx = self.cx;
        let own_only = cx.names.known.own_only;
        let mut memo = std::mem::take(&mut self.structure);
        let mut written = |t: Ty| -> StageResult<bool> {
            let tref = TraitRef {
                trait_: own_only,
                self_ty: t,
                args: TyList::EMPTY,
            };
            Ok(matches!(self.solve(tref)?, Answer::Holds { .. }))
        };
        let view = cx.impls.hiding(&[]);
        let mut c = crate::own::Classify {
            pool,
            lookup: cx.lookup,
            impls: Impls::Owned(&view),
            known: cx.names.known,
            memo: &mut memo,
            written: &mut written,
        };
        let out = c.owns(t);
        self.structure = memo;
        out
    }
    /// Whether `t` holds an owned value outside any borrow: an owned type,
    /// or an optional, tuple, or type argument that holds one.
    pub(crate) fn owned_part(&self, t: Ty) -> bool {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        if !pool.has_mode(t) {
            return false;
        }
        match pool.get(t) {
            TyData::Own(_) => true,
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

    /// The forms the ownership prototype rejects in the first `n`
    /// instructions of a finished body (`own.limit.cold-call`,
    /// `own.limit.child-result`): an owned argument to a cold call, and an
    /// owned result of a child of `all!` or `race!`. Returns whether any
    /// was reported.
    pub(crate) fn prototype_limits(&mut self, n: usize) -> bool {
        let pool = self.pool();
        let suspend = self.cx.names.known.suspend;
        let race = self.cx.names.known.race;
        let mut found: Vec<(usize, &'static str)> = Vec::new();
        for i in 0..n {
            let body = self.b.body_mut();
            let tag = body.tags[i];
            let t = body.ty[i];
            match tag {
                Tag::Call => {
                    let st = self.strip_mut(self.infer.resolve(pool, t));
                    let cold = matches!(pool.get(st), TyData::Adt { def, .. } if def == suspend);
                    let args = self.b.body_mut().value_operands(i).unwrap_or_default();
                    if cold
                        && args
                            .iter()
                            .any(|&a| a != NONE && self.owned_part(self.b.ty_of(Ref(a))))
                    {
                        found.push((
                            i,
                            "an owned argument to a cold call is not in the ownership prototype; call it with `!`, or pass a borrow or a plain value",
                        ));
                    }
                }
                Tag::AwaitAll => {
                    if self.owned_part(t) {
                        found.push((
                            i,
                            "a child of `all!` that returns an owned value is not in the ownership prototype",
                        ));
                    }
                }
                Tag::Await => {
                    let words = body.record(body.data[i][0]).to_vec();
                    let racing = matches!(
                        Callee::from_words(&words),
                        Some(Callee::Item { def, .. }) if def == race
                    );
                    if racing && self.owned_part(t) {
                        found.push((
                            i,
                            "a child of `race!` that returns an owned value is not in the ownership prototype",
                        ));
                    }
                }
                _ => {}
            }
        }
        for &(i, msg) in &found {
            let at = self.b.body_mut().syn[i];
            let node = self.cx.src.parse.tree.node(at);
            self.err(Code::NotInOwnershipPrototype, node, msg);
        }
        self.prototype_limit |= !found.is_empty();
        self.prototype_limit
    }
}
