//! Trials and the choices made by them (type-checking.md §2.4 steps 7 to
//! 9, §2.5 "The instantiation choice", §3.5): a checkpoint of every piece
//! of body state a check can change, exact rollback, the instantiation
//! choice of a method call among the candidates of an `Instantiations`
//! goal, and call inference through a bound.

use hd_base::{DefId, StageResult};
use hd_diag::{Code, DiagMark};
use hd_resolve::FnSig;
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::ir::{ChoiceKind, NONE, Ref, TirCheckpoint, TirSink};
use hd_types::solver::{Answer, Candidate, Goal, TraitRef, plan_goals};
use hd_types::unify::TrialMark;
use hd_types::{ParamRef, Ty, TyData, VarKind};

use crate::body::{Ck, RowFrame, unsupported};
use crate::call::{Args, TraitTarget, subst_owner};

/// Everything a trial may change, as lengths and the few values that are
/// rewritten in place (rule TC-5): append-only columns are truncated, the
/// inference table is unwound and loses the trial's variables, and the
/// stacks must be as deep as they were.
pub(crate) struct Checkpoint {
    tir: TirCheckpoint,
    infer: TrialMark,
    diags: DiagMark,
    pending: usize,
    lit_nodes: usize,
    user_locals: usize,
    pre_args: usize,
    open_params: usize,
    ref_params: usize,
    joins: usize,
    row_waits: usize,
    /// The flags of the locals older than the trial (a read marks them).
    flags: Vec<u8>,
    facts: crate::init::InitFacts,
    /// A closure frame's used keys grow when an argument uses a key.
    rows: Vec<RowFrame>,
    depths: [usize; 5],
}

impl Ck<'_, '_> {
    fn depths(&self) -> [usize; 5] {
        [
            self.scopes.len(),
            self.loops.len(),
            self.subs.len(),
            self.suspends.len(),
            self.rets.len(),
        ]
    }

    pub(crate) fn checkpoint(&mut self) -> Checkpoint {
        let tir = self.b.checkpoint();
        Checkpoint {
            tir,
            infer: self.infer.trial_mark(),
            diags: self.diags.mark(),
            pending: self.pending.len(),
            lit_nodes: self.lit_nodes.len(),
            user_locals: self.user_locals.len(),
            pre_args: self.pre_args.len(),
            open_params: self.open_params.len(),
            ref_params: self.ref_params.len(),
            joins: self.joins.len(),
            row_waits: self.row_waits.len(),
            flags: self.b.body_mut().local_flags[..tir.locals as usize].to_vec(),
            facts: self.facts.clone(),
            rows: self.rows.clone(),
            depths: self.depths(),
        }
    }

    pub(crate) fn rollback(&mut self, c: Checkpoint) {
        self.b.rollback(c.tir);
        self.b.body_mut().local_flags[..c.flags.len()].copy_from_slice(&c.flags);
        self.infer.rollback_trial(c.infer);
        self.diags.truncate(c.diags);
        self.pending.truncate(c.pending);
        self.lit_nodes.truncate(c.lit_nodes);
        self.user_locals.truncate(c.user_locals);
        self.pre_args.truncate(c.pre_args);
        self.open_params.truncate(c.open_params);
        self.ref_params.truncate(c.ref_params);
        self.joins.truncate(c.joins);
        self.row_waits.truncate(c.row_waits);
        self.facts = c.facts;
        self.rows = c.rows;
        // A check that stopped with "not implemented" may leave frames
        // open; the body is abandoned then, so only shrink.
        self.scopes.truncate(c.depths[0]);
        self.loops.truncate(c.depths[1]);
        self.subs.truncate(c.depths[2]);
        self.suspends.truncate(c.depths[3]);
        self.rets.truncate(c.depths[4]);
    }

    /// Runs `f` and rolls everything it did back. `Ok(true)` when it
    /// reported an error: the trial failed.
    pub(crate) fn trial(
        &mut self,
        f: impl FnOnce(&mut Self) -> StageResult<()>,
    ) -> StageResult<bool> {
        let c = self.checkpoint();
        let depths = c.depths;
        let r = f(self);
        let failed = self.diags.errors_since(c.diags) > 0;
        debug_assert!(
            r.is_err() || self.depths() == depths,
            "a trial checks whole expressions, so its stacks balance"
        );
        self.rollback(c);
        r.map(|()| failed)
    }

    // ------------------------------------------------------------ methods

    /// The instantiation choice (§2.5): the candidates of one trait for a
    /// receiver, in content order. One candidate is called directly.
    /// Otherwise the arguments that check alike for every candidate are
    /// inferred once, each candidate is tried, and the one that fits is
    /// checked for real; several fits that differ only by an open literal
    /// default the literal and choose again
    /// (`trait.resolve.literal-arg`); several otherwise are
    /// `ambiguous-method`, none is `type-mismatch` listing them.
    pub(crate) fn choose_method(
        &mut self,
        (trait_, method, self_ty): (DefId, DefId, Ty),
        cands: &[Candidate],
        recv: Option<Ref>,
        args: &Args<'_>,
        (n, bang): (NodeRef<'_>, bool),
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let target = (trait_, method, self_ty);
        if let [only] = cands {
            return self.call_candidate(target, only, recv, args, (n, bang), None);
        }
        let targs = std::mem::take(&mut self.method_targs);
        let pre = self.pre_args.len();
        let first_var = self.infer.var_count();
        self.precheck_args(args)?;
        let last_var = self.infer.var_count();
        let mut fits: Vec<usize> = (0..cands.len()).collect();
        fits = self.try_candidates(target, cands, &fits, (recv, args, n, bang, want), &targs)?;
        if fits.len() > 1 {
            let pool = self.pool();
            let lits: Vec<(Ty, VarKind)> = self
                .infer
                .open_literals_since(pool, first_var)
                .into_iter()
                .filter(|(t, _)| {
                    matches!(pool.get(*t), TyData::Infer(v) if (v.raw() as usize) < last_var)
                })
                .collect();
            if !lits.is_empty() {
                for (t, k) in lits {
                    let d = match k {
                        VarKind::SignedIntLit => Ty::I32,
                        VarKind::IntLit => Ty::prim(hd_types::Prim::Usize),
                        _ => Ty::prim(hd_types::Prim::F64),
                    };
                    let _ = self.infer.unify(pool, t, d);
                }
                fits =
                    self.try_candidates(target, cands, &fits, (recv, args, n, bang, want), &targs)?;
            }
        }
        let r = match fits.as_slice() {
            [one] => {
                self.method_targs = targs;
                self.call_candidate(target, &cands[*one], recv, args, (n, bang), None)
            }
            [] => {
                let msg = format!(
                    "no instantiation of {} fits this call; it has {}",
                    self.cx.names.display_name(trait_),
                    self.instantiations_text(
                        trait_,
                        self_ty,
                        cands,
                        &(0..cands.len()).collect::<Vec<_>>()
                    )
                );
                self.err(Code::TypeMismatch, n, &msg);
                Ok((Ref(NONE), Ty::NEVER))
            }
            several => {
                let msg = format!(
                    "{} fit this call: write `{}::[..]::{}(..)` to choose one",
                    self.instantiations_text(trait_, self_ty, cands, several),
                    self.cx.names.display_name(trait_),
                    self.cx.names.text(
                        self.cx
                            .lookup
                            .item(method)
                            .map_or(hd_base::Symbol::from_raw(0), |i| i.name)
                    )
                );
                self.err(Code::AmbiguousMethod, n, &msg);
                Ok((Ref(NONE), Ty::NEVER))
            }
        };
        self.pre_args.truncate(pre);
        r
    }

    /// The candidates among `which` whose trial has no error.
    fn try_candidates(
        &mut self,
        target: (DefId, DefId, Ty),
        cands: &[Candidate],
        which: &[usize],
        (recv, args, n, bang, want): (Option<Ref>, &Args<'_>, NodeRef<'_>, bool, Option<Ty>),
        targs: &[Ty],
    ) -> StageResult<Vec<usize>> {
        let mut fits = Vec::new();
        for &i in which {
            self.method_targs = targs.to_vec();
            let failed = self.trial(|ck| {
                ck.call_candidate(target, &cands[i], recv, args, (n, bang), want)
                    .map(|_| ())
            })?;
            if !failed {
                fits.push(i);
            }
        }
        Ok(fits)
    }

    /// `Tr[A], Tr[B]` for the listed candidates, as the call sees them.
    fn instantiations_text(
        &self,
        trait_: DefId,
        self_ty: Ty,
        cands: &[Candidate],
        which: &[usize],
    ) -> String {
        let pool = self.pool();
        let shown: Vec<String> = which
            .iter()
            .map(|&i| {
                let t = pool.intern_ty(&TyData::TraitValue {
                    def: trait_,
                    args: cands[i].args,
                    bindings: vec![],
                });
                self.show(t)
            })
            .collect();
        format!("{}: {}", self.show(self_ty), shown.join(", "))
    }

    /// Calls one candidate (§6.5's scheme): a fresh variable for each
    /// impl parameter the receiver left open, the arguments checked
    /// against its method, then, in a trial, the result against the
    /// expected type, then its residual bounds as obligations: one that
    /// fails is an error, one that still waits stays pending.
    fn call_candidate(
        &mut self,
        (trait_, method, self_ty): (DefId, DefId, Ty),
        c: &Candidate,
        recv: Option<Ref>,
        args: &Args<'_>,
        (n, bang): (NodeRef<'_>, bool),
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let Some(table) = self.cx.impl_table(c.row.module) else {
            return unsupported("a candidate outside the impl tables");
        };
        let row = c.row.row as usize;
        let impl_def = table.def[row];
        let n_params = pool.list_items(c.impl_args).len();
        let vars: Vec<Ty> = (0..n_params)
            .map(|_| self.infer.fresh(pool, VarKind::General))
            .collect();
        let inst = |x: Ty| subst_owner(pool, impl_def, &vars, x);
        let impl_args: Vec<Ty> = pool
            .list_items(c.impl_args)
            .iter()
            .map(|x| inst(*x))
            .collect();
        let targs: Vec<Ty> = pool.list_items(c.args).iter().map(|x| inst(*x)).collect();
        let residual: Vec<TraitRef> = plan_goals(pool, table, row, &impl_args)
            .into_iter()
            .filter(|(i, _)| c.residual.contains(i))
            .map(|(_, g)| g)
            .collect();
        let (r, t) = self.trait_method_call(
            TraitTarget {
                trait_,
                method,
                self_ty,
                args: targs,
            },
            recv.map(|r| (r, (ChoiceKind::Impl, impl_def.raw()))),
            args,
            n,
            bang,
        )?;
        if let Some(w) = want.filter(|w| *w != Ty::VOID) {
            self.coerce(r, t, w, n, "result");
        }
        for g in residual {
            self.require_ref(g, n)?;
        }
        Ok((r, t))
    }

    /// Infers once, before the trials, every argument whose check does
    /// not need its expected type.
    fn precheck_args(&mut self, args: &Args<'_>) -> StageResult<()> {
        let all = args
            .positional
            .iter()
            .copied()
            .chain(args.spread)
            .chain(args.named.iter().map(|(_, e)| *e));
        for e in all {
            if needs_expected(e) {
                continue;
            }
            let (r, t) = self.expr(e, None)?;
            self.pre_args.push((e.index(), r, t));
        }
        Ok(())
    }

    /// An argument's value: the one inferred before the trials, or a
    /// check against `want` now.
    pub(crate) fn arg_value(&mut self, e: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        if let Some(&(_, r, t)) = self.pre_args.iter().rev().find(|p| p.0 == e.index()) {
            return Ok((r, t));
        }
        self.expr(e, want)
    }

    // ------------------------------------------------------------ bounds

    /// Steps 7 and 8 of §2.4 for a call of `def`: each parameter named
    /// only in bounds is solved from the bounded parameter's
    /// instantiations of the bound's trait when every bound naming it
    /// allows exactly one and they agree (`types.generic.infer.bound`),
    /// repeated to a fixed point; then each parameter still unsolved
    /// takes its declared default (`types.generic.default.fill`). A
    /// parameter named only in bounds that stays unsolved waits for the
    /// end of the statement. Returns those parameters' indices: their
    /// bounds wait too.
    pub(crate) fn infer_through_bounds(
        &mut self,
        def: DefId,
        sig: &FnSig,
        vars: &[Ty],
        n: NodeRef<'_>,
    ) -> StageResult<Vec<usize>> {
        let pool = self.pool();
        let names = |t: Ty, i: usize| names_param(pool, t, def, i);
        let only: Vec<usize> = (0..sig.generics.len())
            .filter(|&i| {
                !sig.params.iter().any(|p| names(p.1, i))
                    && sig
                        .generics
                        .iter()
                        .enumerate()
                        .any(|(j, g)| j != i && g.bounds.iter().any(|b| names(*b, i)))
            })
            .collect();
        let mut several = vec![false; only.len()];
        let mut none = vec![false; only.len()];
        for _ in 0..=only.len() {
            let mut changed = false;
            for (k, &i) in only.iter().enumerate() {
                if !self.unsolved(vars[i]) {
                    continue;
                }
                match self.bound_instantiations(def, sig, vars, i)? {
                    BoundPick::Wait => {}
                    BoundPick::None => none[k] = true,
                    BoundPick::Several => several[k] = true,
                    BoundPick::One(pairs) => {
                        let snap = self.infer.snapshot();
                        let agree = pairs
                            .iter()
                            .all(|(a, b)| self.infer.unify(pool, *a, *b).is_ok());
                        if agree {
                            changed = true;
                        } else {
                            self.infer.rollback(snap);
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
        for (i, g) in sig.generics.iter().enumerate() {
            if let Some(d) = g.default
                && self.unsolved(vars[i])
            {
                let d = subst_owner(pool, def, vars, d);
                let _ = self.infer.unify(pool, vars[i], d);
            }
        }
        // A bound with no instantiation is `unsatisfied-trait-bound`
        // (`types.generic.infer.bound.none`), checked with the others now.
        let mut open = Vec::new();
        for (k, &i) in only.iter().enumerate() {
            if self.unsolved(vars[i]) && !none[k] {
                self.open_params.push((vars[i], n.index(), several[k]));
                open.push(i);
            }
        }
        Ok(open)
    }

    /// `types.generic.default.fill` for a method call: a type parameter
    /// the arguments left unsolved takes its declared default, after the
    /// expected type had its say (`types.generic.default.argument-wins`).
    /// `inst` substitutes the impl's and the method's parameters.
    pub(crate) fn fill_method_defaults(
        &mut self,
        sig: &FnSig,
        vars: &[Ty],
        inst: &dyn Fn(Ty) -> Ty,
        want: Option<Ty>,
    ) -> StageResult<()> {
        let pool = self.pool();
        let open = |this: &Self| {
            sig.generics
                .iter()
                .enumerate()
                .any(|(i, g)| g.default.is_some() && this.unsolved(vars[i]))
        };
        if !open(self) {
            return Ok(());
        }
        if let Some(w) = want
            && w != Ty::VOID
            && !sig.suspends
        {
            let ret = self.normalize_deep(inst(sig.ret))?;
            let snap = self.infer.snapshot();
            if self.infer.unify(pool, ret, w).is_err() {
                self.infer.rollback(snap);
            }
        }
        for (i, g) in sig.generics.iter().enumerate() {
            if let Some(d) = g.default
                && self.unsolved(vars[i])
            {
                let _ = self.infer.unify(pool, vars[i], inst(d));
            }
        }
        Ok(())
    }

    /// What the bounds naming parameter `i` allow: each bound's one
    /// instantiation, paired with the bound's own arguments to unify.
    fn bound_instantiations(
        &mut self,
        def: DefId,
        sig: &FnSig,
        vars: &[Ty],
        i: usize,
    ) -> StageResult<BoundPick> {
        let pool = self.pool();
        let mut pairs = Vec::new();
        let mut several = false;
        let mut none = false;
        for (j, g) in sig.generics.iter().enumerate() {
            for b in &g.bounds {
                if j == i || !names_param(pool, *b, def, i) {
                    continue;
                }
                let b = subst_owner(pool, def, vars, *b);
                let TyData::TraitValue { def: tr, args, .. } = pool.get(b) else {
                    continue;
                };
                let s = self.strip_mut(self.infer.shallow(pool, vars[j]));
                if matches!(pool.get(s), TyData::Infer(_)) {
                    return Ok(BoundPick::Wait);
                }
                let s = self.infer.resolve(pool, s);
                let Some(found) = self.instantiation_args(s, tr)? else {
                    return Ok(BoundPick::Wait);
                };
                let want = pool.list_items(args).to_vec();
                let fit: Vec<Vec<Ty>> = found
                    .into_iter()
                    .filter(|got| self.can_unify_all(got, &want))
                    .collect();
                match fit.as_slice() {
                    [] => none = true,
                    [one] => pairs.extend(one.iter().copied().zip(want.iter().copied())),
                    _ => several = true,
                }
            }
        }
        Ok(if none {
            BoundPick::None
        } else if several {
            BoundPick::Several
        } else {
            BoundPick::One(pairs)
        })
    }

    /// The instantiations of `tr` that `s` implements, as trait argument
    /// lists over fresh variables: a parameter's clauses, a trait value's
    /// own arguments, or the `Instantiations` goal's candidates. `None`
    /// when the answer waits for inference.
    fn instantiation_args(&mut self, s: Ty, tr: DefId) -> StageResult<Option<Vec<Vec<Ty>>>> {
        let pool = self.pool();
        match pool.get(s) {
            TyData::Param(_) => {
                return Ok(Some(
                    (0..self.env.clause_self.len())
                        .filter(|&c| self.env.clause_self[c] == s && self.env.clause_trait[c] == tr)
                        .map(|c| pool.list_items(self.env.clause_args[c]).to_vec())
                        .collect(),
                ));
            }
            TyData::TraitValue { def, args, .. } => {
                return Ok(Some(if def == tr {
                    vec![pool.list_items(args).to_vec()]
                } else {
                    vec![]
                }));
            }
            TyData::Poison | TyData::Never => return Ok(None),
            _ => {}
        }
        let goal = Goal::Instantiations {
            self_ty: s,
            trait_: tr,
            mut_: false,
        };
        match self.solve_goal(&goal)? {
            Answer::Candidates(cands) => {
                let mut out = Vec::with_capacity(cands.len());
                for c in cands {
                    let Some(table) = self.cx.impl_table(c.row.module) else {
                        continue;
                    };
                    let impl_def = table.def[c.row.row as usize];
                    let vars: Vec<Ty> = (0..pool.list_items(c.impl_args).len())
                        .map(|_| self.infer.fresh(pool, VarKind::General))
                        .collect();
                    out.push(
                        pool.list_items(c.args)
                            .iter()
                            .map(|x| subst_owner(pool, impl_def, &vars, *x))
                            .collect(),
                    );
                }
                Ok(Some(out))
            }
            Answer::Fails(_) => Ok(Some(vec![])),
            Answer::OutOfFuel => unsupported("the solver's fuel ran out (limit diagnostic)"),
            _ => Ok(None),
        }
    }

    fn can_unify_all(&mut self, a: &[Ty], b: &[Ty]) -> bool {
        let pool = self.pool();
        let snap = self.infer.snapshot();
        let ok = a.len() == b.len()
            && a.iter()
                .zip(b)
                .all(|(x, y)| self.infer.unify(pool, *x, *y).is_ok());
        self.infer.rollback(snap);
        ok
    }

    /// An unbound variable that no literal joined.
    pub(crate) fn unsolved(&self, t: Ty) -> bool {
        let pool = self.pool();
        matches!(pool.get(self.infer.shallow(pool, t)), TyData::Infer(_))
            && self.infer.kind_of(pool, t) == Some(VarKind::General)
    }

    /// Step 9 of §2.4 for the parameters recorded since `start`: one
    /// still unsolved is `ambiguous-type` when a bound allowed several
    /// instantiations (`types.generic.infer.bound.no-default.ambiguous`),
    /// else `cannot-infer-type`. It then stands for poison, so its
    /// waiting bounds add nothing.
    pub(crate) fn close_open_params(&mut self, start: usize) {
        if self.open_params.len() <= start {
            return;
        }
        let pool = self.pool();
        for (v, at, several) in self.open_params.split_off(start) {
            if !self.unsolved(v) {
                continue;
            }
            let node = self.cx.src.parse.tree.node(at);
            if several {
                self.err(
                    Code::AmbiguousType,
                    node,
                    "a type argument has several instantiations through its bound; write it explicitly",
                );
            } else {
                self.err(
                    Code::CannotInferType,
                    node,
                    "a type argument named only in bounds has no solution; write it explicitly",
                );
            }
            self.infer
                .rebind(pool, self.infer.shallow(pool, v), Ty::POISON);
        }
    }
}

/// What the bounds naming one parameter allow.
enum BoundPick {
    /// A bounded parameter is not known yet.
    Wait,
    /// Some bound allows no instantiation: the bound check reports it.
    None,
    /// Some bound allows several.
    Several,
    /// Each bound allows one: pairs to unify.
    One(Vec<(Ty, Ty)>),
}

/// Whether `t` names parameter `i` of `owner`.
pub(crate) fn names_param(pool: hd_types::Types<'_>, t: Ty, owner: DefId, i: usize) -> bool {
    let hit = std::cell::Cell::new(false);
    let _ = pool.subst(t, &|p: ParamRef| {
        if p.owner == owner && p.index as usize == i {
            hit.set(true);
        }
        None
    });
    hit.get()
}

/// Whether an argument's check depends on its expected type, so a trial
/// checks it once per candidate (§2.4 step 4): a closure, a contextual
/// variant, an empty collection, a branch, or a composite holding one.
fn needs_expected(e: NodeRef<'_>) -> bool {
    match e.kind() {
        SyntaxKind::ClosureExpr
        | SyntaxKind::VariantExpr
        | SyntaxKind::IfExpr
        | SyntaxKind::MatchExpr
        | SyntaxKind::Block => true,
        SyntaxKind::CallExpr => e
            .children()
            .next()
            .is_some_and(|c| c.kind() == SyntaxKind::VariantExpr),
        SyntaxKind::ListExpr | SyntaxKind::MapExpr => {
            e.children().next().is_none() || e.children().any(needs_expected)
        }
        SyntaxKind::TupleExpr | SyntaxKind::ParenExpr => e.children().any(needs_expected),
        _ => false,
    }
}
