//! The unifier (type-checking.md §3): union-find over inference variables
//! with a trail for rollback (§3.5), an occurs check (§3.3) and literal
//! kinds (§3.6). Poison unifies with everything (data-structures.md §3.6).

use hd_base::InferVar;

use crate::pool::{InternPool, Ty, TyData};

/// A variable's kind: general, integer literal or float literal (§3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VarKind {
    General,
    IntLit,
    FloatLit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnifyError {
    Mismatch { expected: Ty, found: Ty },
    Occurs { var: InferVar, ty: Ty },
    Kind { var: InferVar, ty: Ty },
}

#[derive(Clone, Copy, Debug)]
enum Undo {
    Bind(u32),
    Parent(u32, u32),
    Kind(u32, VarKind),
    Rebind(u32, Option<Ty>),
}

/// The body's inference table: union-find with path halving and a trail.
#[derive(Default, Debug, Clone)]
pub struct InferTable {
    parent: Vec<u32>,
    binding: Vec<Option<Ty>>,
    kind: Vec<VarKind>,
    trail: Vec<Undo>,
}

/// A rollback point (the one rollback contract, §3.5).
#[derive(Clone, Copy, Debug)]
pub struct Snapshot(usize);

impl InferTable {
    pub fn fresh(&mut self, pool: &InternPool, kind: VarKind) -> Ty {
        let v = u32::try_from(self.parent.len()).expect("vars");
        self.parent.push(v);
        self.binding.push(None);
        self.kind.push(kind);
        pool.intern_ty(&TyData::Infer(InferVar::from_raw(v)))
    }

    fn root(&self, mut v: u32) -> u32 {
        while self.parent[v as usize] != v {
            v = self.parent[v as usize];
        }
        v
    }

    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        Snapshot(self.trail.len())
    }

    pub fn rollback(&mut self, s: Snapshot) {
        while self.trail.len() > s.0 {
            match self.trail.pop().expect("trail") {
                Undo::Bind(v) => self.binding[v as usize] = None,
                Undo::Parent(v, old) => self.parent[v as usize] = old,
                Undo::Kind(v, k) => self.kind[v as usize] = k,
                Undo::Rebind(v, old) => self.binding[v as usize] = old,
            }
        }
    }

    /// Replaces a bound variable's solution with `t`, undoably: the
    /// permission join of several arguments lowers `mut X` to `X`
    /// (type-checking.md §3.4, "Permission join").
    pub fn rebind(&mut self, pool: &InternPool, var: Ty, t: Ty) {
        let TyData::Infer(v) = pool.get(var) else {
            return;
        };
        let r = self.root(v.raw());
        self.trail.push(Undo::Rebind(r, self.binding[r as usize]));
        self.binding[r as usize] = Some(t);
    }

    /// Replaces bound variables at the top of `t`.
    #[must_use]
    pub fn shallow(&self, pool: &InternPool, t: Ty) -> Ty {
        let mut t = t;
        while let TyData::Infer(v) = pool.get(t) {
            match self.binding[self.root(v.raw()) as usize] {
                Some(b) => t = b,
                None => {
                    return pool.intern_ty(&TyData::Infer(InferVar::from_raw(self.root(v.raw()))));
                }
            }
        }
        t
    }

    /// The kind of an unbound variable, if `t` is one.
    #[must_use]
    pub fn kind_of(&self, pool: &InternPool, t: Ty) -> Option<VarKind> {
        match pool.get(self.shallow(pool, t)) {
            TyData::Infer(v) => Some(self.kind[self.root(v.raw()) as usize]),
            _ => None,
        }
    }

    /// Resolves every variable it can, recursively.
    #[must_use]
    pub fn resolve(&self, pool: &InternPool, t: Ty) -> Ty {
        let t = self.shallow(pool, t);
        if !pool.has_infer(t) {
            return t;
        }
        let r = |x: Ty| self.resolve(pool, x);
        let rl = |l| pool.list(&pool.list_items(l).into_iter().map(r).collect::<Vec<_>>());
        let d = match pool.get(t) {
            TyData::Adt { def, args } => TyData::Adt {
                def,
                args: rl(args),
            },
            TyData::Tuple { elems, rest } => TyData::Tuple {
                elems: rl(elems),
                rest: rest.map(r),
            },
            TyData::Option(i) => TyData::Option(r(i)),
            TyData::Mut(i) => TyData::Mut(r(i)),
            TyData::Fn {
                params,
                result,
                row,
                suspends,
            } => TyData::Fn {
                params: rl(params),
                result: r(result),
                row,
                suspends,
            },
            TyData::TraitValue {
                def,
                args,
                bindings,
            } => TyData::TraitValue {
                def,
                args: rl(args),
                bindings: bindings.into_iter().map(|(d, x)| (d, r(x))).collect(),
            },
            TyData::Assoc {
                assoc,
                trait_,
                self_ty,
                args,
            } => TyData::Assoc {
                assoc,
                trait_,
                self_ty: r(self_ty),
                args: rl(args),
            },
            other => other,
        };
        pool.intern_ty(&d)
    }

    fn occurs(&self, pool: &InternPool, v: u32, t: Ty) -> bool {
        let t = self.shallow(pool, t);
        match pool.get(t) {
            TyData::Infer(w) => self.root(w.raw()) == v,
            TyData::Adt { args: l, .. }
            | TyData::Tuple {
                elems: l,
                rest: None,
            } => pool
                .list_items(l)
                .into_iter()
                .any(|x| self.occurs(pool, v, x)),
            TyData::Option(i) | TyData::Mut(i) => self.occurs(pool, v, i),
            TyData::Fn { params, result, .. } => {
                self.occurs(pool, v, result)
                    || pool
                        .list_items(params)
                        .into_iter()
                        .any(|x| self.occurs(pool, v, x))
            }
            _ => false,
        }
    }

    fn bind(&mut self, pool: &InternPool, v: u32, t: Ty) -> Result<(), UnifyError> {
        let var = InferVar::from_raw(v);
        if self.occurs(pool, v, t) {
            return Err(UnifyError::Occurs { var, ty: t });
        }
        let ok = match (self.kind[v as usize], pool.get(t)) {
            (VarKind::General, _) | (_, TyData::Poison | TyData::Never) => true,
            (VarKind::IntLit, TyData::Prim(p)) => p.is_integer() || p.is_float(),
            (VarKind::FloatLit, TyData::Prim(p)) => p.is_float(),
            _ => false,
        };
        if !ok {
            return Err(UnifyError::Kind { var, ty: t });
        }
        self.binding[v as usize] = Some(t);
        self.trail.push(Undo::Bind(v));
        Ok(())
    }

    /// Unifies two types; on error the caller rolls back to its snapshot.
    pub fn unify(&mut self, pool: &InternPool, a: Ty, b: Ty) -> Result<(), UnifyError> {
        let (a, b) = (self.shallow(pool, a), self.shallow(pool, b));
        if a == b {
            return Ok(());
        }
        match (pool.get(a), pool.get(b)) {
            (TyData::Poison, _) | (_, TyData::Poison) => Ok(()),
            // A variable binds to the whole type, its view included, so
            // `T = mut User` is inferred (types.generic.mut-argument).
            (TyData::Infer(x), TyData::Mut(_))
                if self.kind[self.root(x.raw()) as usize] == VarKind::General =>
            {
                self.bind(pool, self.root(x.raw()), b)
            }
            (TyData::Mut(_), TyData::Infer(y))
                if self.kind[self.root(y.raw()) as usize] == VarKind::General =>
            {
                self.bind(pool, self.root(y.raw()), a)
            }
            // A view marker does not change the value's type: `mut T`
            // checks against `T` (mutability is checked separately).
            (TyData::Mut(x), TyData::Mut(y)) => self.unify(pool, x, y),
            (TyData::Mut(x), _) => self.unify(pool, x, b),
            (_, TyData::Mut(y)) => self.unify(pool, a, y),
            (TyData::Infer(x), TyData::Infer(y)) => {
                let (x, y) = (self.root(x.raw()), self.root(y.raw()));
                let (kx, ky) = (self.kind[x as usize], self.kind[y as usize]);
                let merged = match (kx, ky) {
                    (VarKind::General, k) | (k, VarKind::General) => k,
                    (VarKind::FloatLit, _) | (_, VarKind::FloatLit) => VarKind::FloatLit,
                    _ => VarKind::IntLit,
                };
                self.trail.push(Undo::Parent(x, self.parent[x as usize]));
                self.parent[x as usize] = y;
                if merged != ky {
                    self.trail.push(Undo::Kind(y, ky));
                    self.kind[y as usize] = merged;
                }
                Ok(())
            }
            (TyData::Infer(x), _) => self.bind(pool, self.root(x.raw()), b),
            (_, TyData::Infer(y)) => self.bind(pool, self.root(y.raw()), a),
            (TyData::Adt { def: d1, args: a1 }, TyData::Adt { def: d2, args: a2 }) if d1 == d2 => {
                self.unify_lists(pool, a1, a2, a, b)
            }
            (
                TyData::Tuple {
                    elems: e1,
                    rest: None,
                },
                TyData::Tuple {
                    elems: e2,
                    rest: None,
                },
            ) => self.unify_lists(pool, e1, e2, a, b),
            (TyData::Option(x), TyData::Option(y)) => self.unify(pool, x, y),
            (
                TyData::TraitValue {
                    def: d1, args: a1, ..
                },
                TyData::TraitValue {
                    def: d2, args: a2, ..
                },
            ) if d1 == d2 => self.unify_lists(pool, a1, a2, a, b),
            (
                TyData::Assoc {
                    assoc: x1,
                    self_ty: s1,
                    args: a1,
                    ..
                },
                TyData::Assoc {
                    assoc: x2,
                    self_ty: s2,
                    args: a2,
                    ..
                },
            ) if x1 == x2 => {
                self.unify(pool, s1, s2)?;
                self.unify_lists(pool, a1, a2, a, b)
            }
            (
                TyData::Fn {
                    params: p1,
                    result: r1,
                    suspends: s1,
                    ..
                },
                TyData::Fn {
                    params: p2,
                    result: r2,
                    suspends: s2,
                    ..
                },
            ) if s1 == s2 => {
                self.unify_lists(pool, p1, p2, a, b)?;
                self.unify(pool, r1, r2)
            }
            _ => Err(UnifyError::Mismatch {
                expected: a,
                found: b,
            }),
        }
    }

    fn unify_lists(
        &mut self,
        pool: &InternPool,
        l1: crate::TyList,
        l2: crate::TyList,
        a: Ty,
        b: Ty,
    ) -> Result<(), UnifyError> {
        let (x, y) = (pool.list_items(l1), pool.list_items(l2));
        if x.len() != y.len() {
            return Err(UnifyError::Mismatch {
                expected: a,
                found: b,
            });
        }
        for (s, t) in x.into_iter().zip(y) {
            self.unify(pool, s, t)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{InferTable, UnifyError, VarKind};
    use crate::pool::{InternPool, Ty, TyData};

    #[test]
    fn unify_resolve_rollback_and_occurs() {
        let p = InternPool::new();
        let mut t = InferTable::default();
        let a = t.fresh(&p, VarKind::General);
        let opt_a = p.intern_ty(&TyData::Option(a));
        let opt_i32 = p.intern_ty(&TyData::Option(Ty::I32));
        let snap = t.snapshot();
        t.unify(&p, opt_a, opt_i32).expect("unify");
        assert_eq!(t.resolve(&p, opt_a), opt_i32);
        t.rollback(snap);
        assert_eq!(t.resolve(&p, opt_a), opt_a);
        assert!(matches!(
            t.unify(&p, a, opt_a),
            Err(UnifyError::Occurs { .. })
        ));
        let lit = t.fresh(&p, VarKind::IntLit);
        assert!(matches!(
            t.unify(&p, lit, Ty::STRING),
            Err(UnifyError::Kind { .. })
        ));
        assert!(t.unify(&p, lit, Ty::POISON).is_ok());
    }
}
