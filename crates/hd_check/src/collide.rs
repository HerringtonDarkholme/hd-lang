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
    !hd_base::stack::all(t, |t, stack| {
        if !pool.has_param(t) {
            return true;
        }
        let t = resolve_param(pool, subst, t);
        match pool.get(t) {
            TyData::Param(q) => return q != p,
            TyData::Adt { args, .. } | TyData::TraitValue { args, .. } | TyData::Union(args) => {
                stack.extend(pool.list_items(args).iter().rev().copied());
            }
            TyData::Tuple { elems, rest } => {
                stack.extend(rest);
                stack.extend(pool.list_items(elems).iter().rev().copied());
            }
            TyData::Ref { inner, .. } => stack.push(inner),
            _ => return true,
        }
        true
    })
}

enum Coincide {
    Pair(Ty, Ty),
    Bindings(Vec<(hd_base::DefId, Ty)>, Vec<(hd_base::DefId, Ty)>),
}

fn all_coincide(pool: Types<'_>, stack: &mut Vec<Coincide>, x: TyList, y: TyList) -> bool {
    let (x, y) = (pool.list_items(x), pool.list_items(y));
    if x.len() != y.len() {
        return false;
    }
    stack.extend(x.iter().zip(y).rev().map(|(&a, &b)| Coincide::Pair(a, b)));
    true
}

/// Whether some substitution of the type parameters makes `a` and `b` the
/// same type, binding each parameter at most once.
fn can_coincide(pool: Types<'_>, subst: &mut Vec<(ParamRef, Ty)>, a: Ty, b: Ty) -> bool {
    hd_base::stack::all(Coincide::Pair(a, b), |work, stack| {
        let (a, b) = match work {
            Coincide::Pair(a, b) => (a, b),
            Coincide::Bindings(left, right) => {
                stack.extend(left.iter().rev().flat_map(|(item, a)| {
                    right
                        .iter()
                        .rev()
                        .filter(move |(other, _)| other == item)
                        .map(move |(_, b)| Coincide::Pair(*a, *b))
                }));
                return true;
            }
        };
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
                d1 == d2 && all_coincide(pool, stack, x, y)
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
                if d1 != d2 {
                    return false;
                }
                stack.push(Coincide::Bindings(b1, b2));
                all_coincide(pool, stack, x, y)
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
            )
            | (TyData::Union(x), TyData::Union(y)) => all_coincide(pool, stack, x, y),
            _ => false,
        }
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collision_conversions_keep_bindings_and_occurs_on_a_small_stack() {
        std::thread::Builder::new()
            .stack_size(96 << 10)
            // cost: one 96 KiB thread for the generated chains.
            .spawn(|| {
                let global = hd_types::InternPool::new();
                let pool = global.types();
                let def = hd_base::DefId::from_raw(1);
                let param = ParamRef {
                    owner: def,
                    index: 0,
                };
                let ty = pool.intern_ty(&TyData::Param(param));
                let (mut left, mut right) = (ty, Ty::I32);
                for _ in 0..1_000 {
                    left = pool.applied(def, left);
                    right = pool.applied(def, right);
                }
                let mut subst = Vec::new();
                assert!(can_coincide(pool, &mut subst, left, right));
                assert_eq!(subst, [(param, Ty::I32)]);
                assert!(mentions_param(pool, &[], left, param));
                assert!(!can_coincide(pool, &mut Vec::new(), ty, left));
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
