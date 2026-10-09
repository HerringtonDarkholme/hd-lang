//! The unifier (type-checking.md §3): union-find over inference variables
//! with a trail for rollback (§3.5), an occurs check (§3.3) and literal
//! kinds (§3.6). Poison unifies with everything (data-structures.md §3.6).

use hd_base::InferVar;

use crate::pool::{Ty, TyData, Types};

/// A variable's kind: general, integer literal or float literal (§3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VarKind {
    General,
    IntLit,
    /// An integer literal class with a signed member (`-1`, `+5`).
    SignedIntLit,
    FloatLit,
}

impl VarKind {
    /// A literal class's default type (`types.literal.local.default`);
    /// `None` for a general variable.
    #[must_use]
    pub const fn literal_default(self) -> Option<Ty> {
        match self {
            Self::General => None,
            Self::IntLit => Some(Ty::prim(crate::Prim::Usize)),
            Self::SignedIntLit => Some(Ty::I32),
            Self::FloatLit => Some(Ty::prim(crate::Prim::F64)),
        }
    }
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
    Rank(u32, u8),
    Kind(u32, VarKind),
    Rebind(u32, Option<Ty>),
}

/// The body's inference table: union-find with union by rank and a trail
/// (data-structures.md §3.19's `parent`, `rank`, `kind` and `value`
/// columns). Union by rank keeps every path at most log2(n) links long
/// without writing during a lookup, so `root` stays a read and a rollback
/// undoes each link and rank change exactly.
#[derive(Default, Debug, Clone)]
pub struct InferTable {
    parent: Vec<u32>,
    rank: Vec<u8>,
    binding: Vec<Option<Ty>>,
    kind: Vec<VarKind>,
    trail: Vec<Undo>,
}

/// A rollback point (the one rollback contract, §3.5).
#[derive(Clone, Copy, Debug)]
pub struct Snapshot(usize);

/// A trial's rollback point: the trail and the variable count. Rolling
/// back to it also drops the variables the trial made, so a failed trial
/// leaves no variable behind (type-checking.md §3.5, §2.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrialMark {
    trail: usize,
    vars: usize,
}

impl InferTable {
    pub fn fresh(&mut self, pool: Types<'_>, kind: VarKind) -> Ty {
        let v = u32::try_from(self.parent.len()).expect("vars");
        self.parent.push(v);
        self.rank.push(0);
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
                Undo::Rank(v, old) => self.rank[v as usize] = old,
                Undo::Kind(v, k) => self.kind[v as usize] = k,
                Undo::Rebind(v, old) => self.binding[v as usize] = old,
            }
        }
    }

    #[must_use]
    pub fn trial_mark(&self) -> TrialMark {
        TrialMark {
            trail: self.trail.len(),
            vars: self.parent.len(),
        }
    }

    /// Undoes everything since `m` and drops the variables made since.
    /// After the trail is undone no older variable links to or binds a
    /// newer one, so the newer ones are unreachable.
    pub fn rollback_trial(&mut self, m: TrialMark) {
        self.rollback(Snapshot(m.trail));
        self.parent.truncate(m.vars);
        self.rank.truncate(m.vars);
        self.binding.truncate(m.vars);
        self.kind.truncate(m.vars);
    }

    /// Replaces a bound variable's solution with `t`, undoably: the
    /// permission join of several arguments lowers `mut X` to `X`
    /// (type-checking.md §3.4, "Permission join").
    pub fn rebind(&mut self, pool: Types<'_>, var: Ty, t: Ty) {
        let TyData::Infer(v) = pool.get(var) else {
            return;
        };
        let r = self.root(v.raw());
        self.trail.push(Undo::Rebind(r, self.binding[r as usize]));
        self.binding[r as usize] = Some(t);
    }

    /// Replaces bound variables at the top of `t`.
    #[must_use]
    pub fn shallow(&self, pool: Types<'_>, t: Ty) -> Ty {
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
    pub fn kind_of(&self, pool: Types<'_>, t: Ty) -> Option<VarKind> {
        match pool.get(self.shallow(pool, t)) {
            TyData::Infer(v) => Some(self.kind[self.root(v.raw()) as usize]),
            _ => None,
        }
    }

    /// The number of variables made so far.
    #[must_use]
    pub fn var_count(&self) -> usize {
        self.parent.len()
    }

    /// Variables from `start` on whose class is an unbound literal, with
    /// the class kind (the literal classes a statement leaves open).
    #[must_use]
    pub fn open_literals_since(&self, pool: Types<'_>, start: usize) -> Vec<(Ty, VarKind)> {
        (start..self.parent.len())
            .filter_map(|v| {
                let t = pool.intern_ty(&TyData::Infer(InferVar::from_raw(
                    u32::try_from(v).expect("vars"),
                )));
                match self.kind_of(pool, t) {
                    Some(k) if k != VarKind::General => Some((t, k)),
                    _ => None,
                }
            })
            .collect()
    }

    /// Marks an unbound integer literal class as holding a signed literal.
    pub fn mark_signed(&mut self, pool: Types<'_>, t: Ty) {
        if let TyData::Infer(v) = pool.get(self.shallow(pool, t)) {
            let r = self.root(v.raw());
            if self.kind[r as usize] == VarKind::IntLit {
                self.trail.push(Undo::Kind(r, VarKind::IntLit));
                self.kind[r as usize] = VarKind::SignedIntLit;
            }
        }
    }

    /// Resolves every variable it can, recursively.
    #[must_use]
    pub fn resolve(&self, pool: Types<'_>, t: Ty) -> Ty {
        let t = self.shallow(pool, t);
        if !pool.has_infer(t) {
            return t;
        }
        let r = |x: Ty| self.resolve(pool, x);
        let rl = |l| {
            pool.list(
                &pool
                    .list_items(l)
                    .iter()
                    .copied()
                    .map(r)
                    .collect::<Vec<_>>(),
            )
        };
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
                vararg,
            } => TyData::Fn {
                params: rl(params),
                result: r(result),
                row: self.resolve_row(pool, row),
                suspends,
                vararg,
            },
            TyData::Row(row) => TyData::Row(self.resolve_row(pool, row)),
            TyData::Context(row) => TyData::Context(self.resolve_row(pool, row)),
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

    /// Resolves a row's keys; a bound row variable's row joins the keys.
    #[must_use]
    pub fn resolve_row(&self, pool: Types<'_>, row: crate::RowId) -> crate::RowId {
        let mut d = pool.row_data(row);
        if !d.keys.iter().any(|k| pool.has_infer(*k)) {
            return row;
        }
        d.keys = d.keys.iter().map(|k| self.resolve(pool, *k)).collect();
        pool.row(&d)
    }

    fn occurs(&self, pool: Types<'_>, v: u32, t: Ty) -> bool {
        let t = self.shallow(pool, t);
        match pool.get(t) {
            TyData::Infer(w) => self.root(w.raw()) == v,
            TyData::Adt { args: l, .. } => pool
                .list_items(l)
                .iter()
                .copied()
                .any(|x| self.occurs(pool, v, x)),
            TyData::Tuple { elems, rest } => pool
                .list_items(elems)
                .iter()
                .copied()
                .chain(rest)
                .any(|x| self.occurs(pool, v, x)),
            TyData::Option(i) | TyData::Mut(i) => self.occurs(pool, v, i),
            TyData::Fn { params, result, .. } => {
                self.occurs(pool, v, result)
                    || pool
                        .list_items(params)
                        .iter()
                        .copied()
                        .any(|x| self.occurs(pool, v, x))
            }
            _ => false,
        }
    }

    fn bind(&mut self, pool: Types<'_>, v: u32, t: Ty) -> Result<(), UnifyError> {
        let var = InferVar::from_raw(v);
        if self.occurs(pool, v, t) {
            return Err(UnifyError::Occurs { var, ty: t });
        }
        let ok = match (self.kind[v as usize], pool.get(t)) {
            (VarKind::General, _) | (_, TyData::Poison | TyData::Never) => true,
            // `types.literal.int-not-float`: an integer literal never
            // takes a floating-point type.
            (VarKind::IntLit | VarKind::SignedIntLit, TyData::Prim(p)) => p.is_integer(),
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
    pub fn unify(&mut self, pool: Types<'_>, a: Ty, b: Ty) -> Result<(), UnifyError> {
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
                    (VarKind::FloatLit, VarKind::FloatLit) => VarKind::FloatLit,
                    // An integer class and a float class never join
                    // (`types.literal.int-not-float`).
                    (VarKind::FloatLit, _) | (_, VarKind::FloatLit) => {
                        return Err(UnifyError::Mismatch {
                            expected: a,
                            found: b,
                        });
                    }
                    (VarKind::SignedIntLit, _) | (_, VarKind::SignedIntLit) => {
                        VarKind::SignedIntLit
                    }
                    _ => VarKind::IntLit,
                };
                if x == y {
                    return Ok(());
                }
                // Union by rank: the lower-ranked root joins the other; on
                // a tie `x` joins `y` and `y`'s rank grows.
                let (rx, ry) = (self.rank[x as usize], self.rank[y as usize]);
                let (child, root) = if rx > ry { (y, x) } else { (x, y) };
                self.trail
                    .push(Undo::Parent(child, self.parent[child as usize]));
                self.parent[child as usize] = root;
                if rx == ry {
                    self.trail.push(Undo::Rank(root, ry));
                    self.rank[root as usize] = ry + 1;
                }
                let kr = self.kind[root as usize];
                if merged != kr {
                    self.trail.push(Undo::Kind(root, kr));
                    self.kind[root as usize] = merged;
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
            // `types.tuple.rest.same`: with rest elements, the fixed
            // elements and the rest elements are each pairwise equal; a
            // tuple with one never meets a tuple without.
            (
                TyData::Tuple {
                    elems: e1,
                    rest: Some(r1),
                },
                TyData::Tuple {
                    elems: e2,
                    rest: Some(r2),
                },
            ) => {
                self.unify_lists(pool, e1, e2, a, b)?;
                self.unify(pool, r1, r2)
            }
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
                    vararg: v1,
                    ..
                },
                TyData::Fn {
                    params: p2,
                    result: r2,
                    suspends: s2,
                    vararg: v2,
                    ..
                },
            ) if s1 == s2 && v1 == v2 => {
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
        pool: Types<'_>,
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
        for (&s, &t) in x.iter().zip(y) {
            self.unify(pool, s, t)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{InferTable, UnifyError, VarKind};
    use crate::pool::{InternPool, LocalPool, Ty, TyData, Types};

    #[test]
    fn unify_resolve_rollback_and_occurs() {
        let (gp, lp) = (InternPool::new(), LocalPool::new());
        let p = Types::with_local(&gp, &lp);
        let mut t = InferTable::default();
        let a = t.fresh(p, VarKind::General);
        let opt_a = p.intern_ty(&TyData::Option(a));
        let opt_i32 = p.intern_ty(&TyData::Option(Ty::I32));
        let snap = t.snapshot();
        t.unify(p, opt_a, opt_i32).expect("unify");
        assert_eq!(t.resolve(p, opt_a), opt_i32);
        t.rollback(snap);
        assert_eq!(t.resolve(p, opt_a), opt_a);
        assert!(matches!(
            t.unify(p, a, opt_a),
            Err(UnifyError::Occurs { .. })
        ));
        let lit = t.fresh(p, VarKind::IntLit);
        assert!(matches!(
            t.unify(p, lit, Ty::STRING),
            Err(UnifyError::Kind { .. })
        ));
        assert!(t.unify(p, lit, Ty::POISON).is_ok());
    }

    /// Union by rank: unions keep classes shallow, kinds merge at the
    /// root, and a rollback restores every link, rank and kind exactly.
    #[test]
    fn union_by_rank_rolls_back_exactly() {
        let (gp, lp) = (InternPool::new(), LocalPool::new());
        let p = Types::with_local(&gp, &lp);
        let mut t = InferTable::default();
        let vars: Vec<Ty> = (0..16).map(|_| t.fresh(p, VarKind::General)).collect();
        let lit = t.fresh(p, VarKind::IntLit);
        let before: Vec<Ty> = vars.iter().map(|v| t.shallow(p, *v)).collect();
        let snap = t.snapshot();
        for w in vars.windows(2) {
            t.unify(p, w[0], w[1]).expect("unify vars");
        }
        t.unify(p, vars[7], lit).expect("unify literal");
        let root = t.shallow(p, vars[0]);
        assert!(vars.iter().all(|v| t.shallow(p, *v) == root));
        assert_eq!(t.kind_of(p, vars[3]), Some(VarKind::IntLit));
        assert!(t.rank.iter().all(|r| *r <= 5), "log2(17) bounds the rank");
        t.unify(p, vars[0], Ty::I32).expect("bind");
        assert_eq!(t.resolve(p, vars[15]), Ty::I32);
        t.rollback(snap);
        let after: Vec<Ty> = vars.iter().map(|v| t.shallow(p, *v)).collect();
        assert_eq!(before, after);
        assert!(t.rank.iter().all(|r| *r == 0));
        assert_eq!(t.kind_of(p, vars[3]), Some(VarKind::General));
        assert_eq!(t.kind_of(p, lit), Some(VarKind::IntLit));
    }

    /// A failed trial leaves no inference trace: older variables that the
    /// trial linked to, bound through, or re-kinded with its own new
    /// variables come back exactly, and the new variables are gone.
    #[test]
    fn a_trial_rollback_drops_its_variables_and_undoes_every_link() {
        let (gp, lp) = (InternPool::new(), LocalPool::new());
        let p = Types::with_local(&gp, &lp);
        let mut t = InferTable::default();
        let old: Vec<Ty> = (0..4).map(|_| t.fresh(p, VarKind::General)).collect();
        t.unify(p, old[0], old[1]).expect("before the trial");
        let before = format!("{t:?}");
        let mark = t.trial_mark();
        let new = t.fresh(p, VarKind::General);
        let lit = t.fresh(p, VarKind::IntLit);
        t.unify(p, old[2], new)
            .expect("an old variable joins a new one");
        t.unify(p, new, lit).expect("the class becomes a literal");
        t.unify(p, old[0], p.intern_ty(&TyData::Option(new)))
            .expect("an old class binds through a new variable");
        t.mark_signed(p, lit);
        assert_eq!(t.kind_of(p, old[2]), Some(VarKind::SignedIntLit));
        t.rollback_trial(mark);
        assert_eq!(format!("{t:?}"), before, "every column as before");
        assert_eq!(t.var_count(), 4);
        assert_eq!(t.kind_of(p, old[2]), Some(VarKind::General));
        assert_eq!(t.resolve(p, old[0]), t.shallow(p, old[1]));
    }

    /// `types.tuple.rest.same`: tuples with rest elements unify their
    /// fixed elements and their rest elements pairwise, and a tuple with a
    /// rest element never meets one without.
    #[test]
    fn rest_tuples_unify_elementwise_and_never_with_plain_tuples() {
        let (gp, lp) = (InternPool::new(), LocalPool::new());
        let p = Types::with_local(&gp, &lp);
        let mut t = InferTable::default();
        let list_of = |x: Ty| {
            p.intern_ty(&TyData::Adt {
                def: hd_base::DefId::from_raw(7),
                args: p.list(&[x]),
            })
        };
        let rest_tuple = |fixed: Ty, item: Ty| {
            p.intern_ty(&TyData::Tuple {
                elems: p.list(&[fixed]),
                rest: Some(list_of(item)),
            })
        };
        let (a, b) = (t.fresh(p, VarKind::General), t.fresh(p, VarKind::General));
        t.unify(p, rest_tuple(a, b), rest_tuple(Ty::I32, Ty::BOOL))
            .expect("fixed and rest elements unify");
        assert_eq!(t.resolve(p, a), Ty::I32);
        assert_eq!(t.resolve(p, b), Ty::BOOL);
        let plain = p.intern_ty(&TyData::Tuple {
            elems: p.list(&[Ty::I32]),
            rest: None,
        });
        assert!(matches!(
            t.unify(p, rest_tuple(Ty::I32, Ty::BOOL), plain),
            Err(UnifyError::Mismatch { .. })
        ));
        assert!(matches!(
            t.unify(
                p,
                rest_tuple(Ty::I32, Ty::BOOL),
                rest_tuple(Ty::I32, Ty::I32)
            ),
            Err(UnifyError::Mismatch { .. })
        ));
        let c = t.fresh(p, VarKind::General);
        assert!(matches!(
            t.unify(p, c, rest_tuple(c, Ty::I32)),
            Err(UnifyError::Occurs { .. })
        ));
    }
}
