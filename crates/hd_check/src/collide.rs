//! Generic requirement key collisions (spec/lang/
//! 11-requirements-and-suspension.md, "Generic Key Collisions"): two
//! keys that can become identical under some substitution of their type
//! parameters may not be visible together.

use hd_diag::Code;
use hd_syntax::NodeRef;
use hd_types::{ParamRef, Ty, TyData, TyList, Types};

use crate::body::{Ck, RowFrame};

/// Follows the type parameters `subst` has bound to the type they stand for.
fn resolve_param(pool: Types<'_>, subst: &[(ParamRef, Ty)], mut t: Ty) -> Ty {
    while let TyData::Param(p) = pool.get(t) {
        match subst.iter().find(|(q, _)| *q == p) {
            Some((_, to)) => t = *to,
            None => break,
        }
    }
    t
}

/// Whether the parameter `p` occurs in `t` under `subst`.
fn mentions_param(pool: Types<'_>, subst: &[(ParamRef, Ty)], t: Ty, p: ParamRef) -> bool {
    if !pool.has_param(t) {
        return false;
    }
    let t = resolve_param(pool, subst, t);
    let any = |l: TyList| {
        pool.list_items(l)
            .iter()
            .any(|x| mentions_param(pool, subst, *x, p))
    };
    match pool.get(t) {
        TyData::Param(q) => q == p,
        TyData::Adt { args, .. } | TyData::TraitValue { args, .. } => any(args),
        TyData::Tuple { elems, rest } => {
            any(elems) || rest.is_some_and(|r| mentions_param(pool, subst, r, p))
        }
        TyData::Option(x) | TyData::Mut(x) => mentions_param(pool, subst, x, p),
        _ => false,
    }
}

fn all_coincide(pool: Types<'_>, subst: &mut Vec<(ParamRef, Ty)>, x: TyList, y: TyList) -> bool {
    let (x, y) = (pool.list_items(x), pool.list_items(y));
    x.len() == y.len()
        && x.iter()
            .zip(y)
            .all(|(p, q)| can_coincide(pool, subst, *p, *q))
}

/// Whether some substitution of the type parameters makes `a` and `b` the
/// same type, binding each parameter at most once.
fn can_coincide(pool: Types<'_>, subst: &mut Vec<(ParamRef, Ty)>, a: Ty, b: Ty) -> bool {
    let a = resolve_param(pool, subst, a);
    let b = resolve_param(pool, subst, b);
    if a == b {
        return true;
    }
    match (pool.get(a), pool.get(b)) {
        (TyData::Param(p), _) if !mentions_param(pool, subst, b, p) => {
            subst.push((p, b));
            true
        }
        (_, TyData::Param(q)) if !mentions_param(pool, subst, a, q) => {
            subst.push((q, a));
            true
        }
        (TyData::Adt { def: d1, args: x }, TyData::Adt { def: d2, args: y }) => {
            d1 == d2 && all_coincide(pool, subst, x, y)
        }
        (
            TyData::TraitValue {
                def: d1,
                args: x,
                bindings: b1,
            },
            TyData::TraitValue {
                def: d2,
                args: y,
                bindings: b2,
            },
        ) => {
            d1 == d2
                && all_coincide(pool, subst, x, y)
                && b1.iter().all(|(item, t1)| {
                    b2.iter()
                        .filter(|(i2, _)| i2 == item)
                        .all(|(_, t2)| can_coincide(pool, subst, *t1, *t2))
                })
        }
        (
            TyData::Tuple {
                elems: x,
                rest: None,
            },
            TyData::Tuple {
                elems: y,
                rest: None,
            },
        ) => all_coincide(pool, subst, x, y),
        (TyData::Option(x), TyData::Option(y)) | (TyData::Mut(x), TyData::Mut(y)) => {
            can_coincide(pool, subst, x, y)
        }
        _ => false,
    }
}

impl Ck<'_, '_> {
    /// The keys a lookup at this point can select from the enclosing
    /// scopes: the `$.with` blocks up to and including the enclosing
    /// function's or closure's declared row (`req.with.collision.compared`,
    /// `req.with.collision.closure`).
    pub(crate) fn visible_keys(&self) -> Vec<Ty> {
        let pool = self.pool();
        let mut keys = Vec::new();
        for frame in self.rows.iter().rev() {
            match frame {
                RowFrame::With(r) => keys.extend(pool.row_data(*r).keys),
                RowFrame::Declared(r)
                | RowFrame::Closure {
                    written: Some(r), ..
                } => {
                    keys.extend(pool.row_data(*r).keys);
                    break;
                }
                RowFrame::Any
                | RowFrame::Closure { written: None, .. }
                | RowFrame::Inferred { .. }
                | RowFrame::Profile { .. } => break,
            }
        }
        keys
    }

    /// One `$.with` or `$.context` entry binds the keys `new`: none may be
    /// able to become the same key as one of the `earlier` keys of the
    /// same expression or of the `visible` keys of the enclosing scopes
    /// (`req.with.collision`). Reported once per entry, on the entry.
    pub(crate) fn check_key_collision(
        &mut self,
        entry: NodeRef<'_>,
        new: &[Ty],
        earlier: &[Ty],
        visible: &[Ty],
    ) {
        let pool = self.pool();
        for &n in new {
            for &other in earlier.iter().chain(visible) {
                if n != other && can_coincide(pool, &mut Vec::new(), n, other) {
                    let msg = format!(
                        "the keys `{}` and `{}` can become the same key for some type arguments",
                        hd_resolve::show_ty_in(&self.cx.names, pool, n),
                        hd_resolve::show_ty_in(&self.cx.names, pool, other)
                    );
                    self.err(Code::GenericRequirementKeyCollision, entry, &msg);
                    return;
                }
            }
        }
    }
}
