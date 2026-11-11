//! Trials and the choices made by them (`crates/hd_check/DESIGN.md` (type checking) §2.4 steps 7 to
//! 9, §2.5 "The instantiation choice", §3.5): a checkpoint of every piece
//! of body state a check can change, exact rollback, the instantiation
//! choice of a method call among the candidates of an `Instantiations`
//! goal, and call inference through a bound.

use hd_base::{DefId, HashSet, LocalId, StageResult};
use hd_diag::{Code, DiagMark};
use hd_resolve::FnSig;
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::ir::{ChoiceKind, NONE, Ref, TirCheckpoint, TirSink};
use hd_types::solver::{
    Answer, Candidate, Goal, ImplOrigin, Impls, TraitRef, plan_goals, tuple_goals, union_goals,
};
use hd_types::unify::TrialMark;
use hd_types::{ParamRef, Ty, TyData, VarKind};

use crate::body::{Ck, ExpectPosition, RowFrame};
use crate::call::{Args, TraitTarget, subst_owner};

/// One change to body state that is written in place, logged while a
/// checkpoint is open so that rollback undoes it (rule TC-5): the old
/// value, or what was added.
pub(crate) enum BodyUndo {
    /// A local's flags before bits were set in them.
    Flags(LocalId, u8),
    /// A global read added to the init facts.
    Read(DefId),
    /// A function call added to the init facts.
    Call(DefId),
    /// A binding added to the possibly-uninitialized set.
    UninitAdded(LocalId),
    /// A binding removed from the possibly-uninitialized set.
    UninitRemoved(LocalId),
    /// The possibly-uninitialized set before it was replaced.
    Uninit(HashSet<LocalId>),
    /// A key pushed to the used keys of the row frame at this index.
    RowKey(usize),
    /// A row parameter pushed to the used row of the frame at this index.
    RowParam(usize),
    /// An edge pushed to the inferred row frame at this index.
    RowEdge(usize),
    /// A captured-reference diagnostic first recorded by this trial.
    RefCaptured(LocalId),
    /// A bound receiver's escape first reported by this trial.
    BoundEscape(u32),
    /// A projection's cycle or depth error first reported by this trial.
    NormReported(Ty),
}

/// Everything a trial may change, as lengths and a mark in the undo log
/// (rule TC-5): append-only columns are truncated, the inference table is
/// unwound and loses the trial's variables, the in-place changes are
/// undone from the log, and the stacks must be as deep as they were. No
/// body state is copied, so a checkpoint costs the same in any body.
pub(crate) struct Checkpoint {
    tir: TirCheckpoint,
    infer: TrialMark,
    diags: DiagMark,
    pending: usize,
    tool_facts: crate::comparison::FactsMark,
    mut_bindings: usize,
    ref_taken: usize,
    lit_nodes: usize,
    overflow_lits: usize,
    neg_nodes: usize,
    user_locals: usize,
    mut_locals: usize,
    mut_params: usize,
    changed_locals: usize,
    local_ref_call_targs: usize,
    assoc_self: usize,
    row_banks: usize,
    row_sources: usize,
    local_rows: Option<crate::local_rows::Mark>,
    next_gen: u16,
    pre_args: usize,
    open_params: usize,
    ref_params: usize,
    call_params: usize,
    joins: usize,
    row_waits: usize,
    undo: usize,
    bound: usize,
    opaque_rets: usize,
    strict_rets: usize,
    recursive_returns: usize,
    clauses: usize,
    excl: [usize; 3],
    depths: [usize; 6],
}

impl Ck<'_, '_> {
    fn depths(&self) -> [usize; 6] {
        [
            self.scopes.len(),
            self.loops.len(),
            self.subs.len(),
            self.suspends.len(),
            self.rets.len(),
            self.rows.len(),
        ]
    }

    /// Logs an in-place change for the open checkpoints; nothing when
    /// none is open.
    pub(crate) fn log_undo(&mut self, u: BodyUndo) {
        if self.open_trials > 0 {
            self.undo.push(u);
        }
    }

    /// Sets `bits` in a local's flags.
    pub(crate) fn add_flags(&mut self, l: LocalId, bits: u8) {
        let old = self.b.body_mut().local_flags[l.idx()];
        if old | bits != old {
            self.b.body_mut().local_flags[l.idx()] = old | bits;
            self.log_undo(BodyUndo::Flags(l, old));
        }
    }

    /// Records that the body reads the global `d` (module initialization).
    pub(crate) fn fact_read(&mut self, d: DefId) {
        if self.facts.reads.insert(d) {
            self.log_undo(BodyUndo::Read(d));
        }
    }

    /// Records that the body calls or references the function `d`.
    pub(crate) fn fact_call(&mut self, d: DefId) {
        if self.facts.calls.insert(d) {
            self.log_undo(BodyUndo::Call(d));
        }
    }

    /// Replaces the possibly-uninitialized set.
    pub(crate) fn set_uninit(&mut self, s: HashSet<LocalId>) {
        let old = std::mem::replace(&mut self.uninit, s);
        self.log_undo(BodyUndo::Uninit(old));
    }

    /// Replaces the possibly-uninitialized set and returns the old one.
    /// Inside a trial the log keeps a copy of it, as large as the set.
    pub(crate) fn replace_uninit(&mut self, s: HashSet<LocalId>) -> HashSet<LocalId> {
        let old = std::mem::replace(&mut self.uninit, s);
        if self.open_trials > 0 {
            self.undo.push(BodyUndo::Uninit(old.clone()));
        }
        old
    }

    /// Adds a binding to the possibly-uninitialized set.
    pub(crate) fn uninit_insert(&mut self, l: LocalId) {
        if self.uninit.insert(l) {
            self.log_undo(BodyUndo::UninitAdded(l));
        }
    }

    /// Removes a binding from the possibly-uninitialized set: whether it
    /// was there.
    pub(crate) fn uninit_remove(&mut self, l: LocalId) -> bool {
        let was = self.uninit.remove(&l);
        if was {
            self.log_undo(BodyUndo::UninitRemoved(l));
        }
        was
    }

    /// Undoes the logged changes after `mark`, newest first. A change to
    /// a local or a row frame made after the checkpoint is gone with it.
    fn undo_to(&mut self, mark: usize) {
        while self.undo.len() > mark {
            let Some(u) = self.undo.pop() else { break };
            match u {
                BodyUndo::Flags(l, f) => {
                    if let Some(x) = self.b.body_mut().local_flags.get_mut(l.idx()) {
                        *x = f;
                    }
                }
                BodyUndo::Read(d) => {
                    self.facts.reads.remove(&d);
                }
                BodyUndo::RefCaptured(l) => {
                    self.ref_captured.remove(&l);
                }
                BodyUndo::BoundEscape(lo) => {
                    self.bound_escapes.remove(&lo);
                }
                BodyUndo::NormReported(t) => {
                    self.norm_reported.remove(&t);
                }
                BodyUndo::Call(d) => {
                    self.facts.calls.remove(&d);
                }
                BodyUndo::UninitAdded(l) => {
                    self.uninit.remove(&l);
                }
                BodyUndo::UninitRemoved(l) => {
                    self.uninit.insert(l);
                }
                BodyUndo::Uninit(s) => self.uninit = s,
                BodyUndo::RowKey(i) | BodyUndo::RowParam(i) | BodyUndo::RowEdge(i) => {
                    let Some(frame) = self.rows.get_mut(i) else {
                        continue;
                    };
                    match (u, frame) {
                        (
                            BodyUndo::RowKey(_),
                            RowFrame::Closure { used, .. }
                            | RowFrame::Inferred { used, .. }
                            | RowFrame::Profile { used, .. },
                        ) => {
                            used.keys.pop();
                        }
                        (
                            BodyUndo::RowParam(_),
                            RowFrame::Closure { used, .. } | RowFrame::Inferred { used, .. },
                        ) => {
                            used.params.pop();
                        }
                        (BodyUndo::RowEdge(_), RowFrame::Inferred { edges, .. }) => {
                            edges.pop();
                        }
                        _ => debug_assert!(false, "a row undo for a frame of another kind"),
                    }
                }
            }
        }
    }

    /// Closes a checkpoint; with none left open, nothing can roll back
    /// and the log is emptied.
    fn close_checkpoint(&mut self, _: Checkpoint) {
        self.open_trials = self.open_trials.saturating_sub(1);
        if self.open_trials == 0 {
            self.undo.clear();
        }
    }

    pub(crate) fn checkpoint(&mut self) -> Checkpoint {
        let tir = self.b.checkpoint();
        self.open_trials += 1;
        Checkpoint {
            tir,
            infer: self.infer.trial_mark(),
            diags: self.diags.mark(),
            pending: self.pending.len(),
            tool_facts: self.tool_facts.mark(),
            mut_bindings: self.mut_bindings.len(),
            ref_taken: self.ref_taken.len(),
            lit_nodes: self.lit_nodes.len(),
            overflow_lits: self.overflow_lits.len(),
            neg_nodes: self.neg_nodes.len(),
            user_locals: self.user_locals.len(),
            mut_locals: self.mut_locals.len(),
            mut_params: self.mut_params.len(),
            changed_locals: self.changed_locals.len(),
            local_ref_call_targs: self.local_ref_call_targs.len(),
            assoc_self: self.assoc_self.len(),
            row_banks: self.row_banks.len(),
            row_sources: self.row_sources.len(),
            local_rows: self
                .local_rows
                .as_ref()
                .map(crate::local_rows::LocalRows::mark),
            next_gen: self.next_gen,
            pre_args: self.pre_args.len(),
            open_params: self.open_params.len(),
            ref_params: self.ref_params.len(),
            call_params: self.call_params.len(),
            joins: self.joins.mark(),
            row_waits: self.row_waits.len(),
            undo: self.undo.len(),
            bound: self.bound.len(),
            opaque_rets: self.opaque_rets.len(),
            strict_rets: self.strict_rets.len(),
            recursive_returns: self.recursive_returns.len(),
            clauses: self.env.len(),
            excl: [
                self.excl.len(),
                self.excl_frames.len(),
                self.conflicts.len(),
            ],
            depths: self.depths(),
        }
    }

    /// Keeps everything done since the checkpoint. An enclosing
    /// checkpoint can still roll it back.
    pub(crate) fn commit(&mut self, c: Checkpoint) {
        self.close_checkpoint(c);
    }

    pub(crate) fn rollback(&mut self, c: Checkpoint) {
        let locals = c.tir.locals as usize;
        for i in (c.tir.locals..).take(self.b.body().local_ty.len() - locals) {
            let l = LocalId::from_raw(i);
            self.local_generics.remove(&l);
            self.local_forms.remove(&l);
            self.local_inits.remove(&l);
            self.ref_origins.remove(&l);
            self.ref_bound.remove(&l);
            self.ref_captured.remove(&l);
        }
        self.b.rollback(c.tir);
        self.tool_facts.truncate(c.tool_facts);
        self.mut_bindings.truncate(c.mut_bindings);
        self.ref_taken.truncate(c.ref_taken);
        self.mut_locals.truncate(c.mut_locals);
        self.mut_params.truncate(c.mut_params);
        self.changed_locals.truncate(c.changed_locals);
        self.local_ref_call_targs.truncate(c.local_ref_call_targs);
        self.assoc_self.truncate(c.assoc_self);
        self.row_banks.truncate(c.row_banks);
        self.row_sources.truncate(c.row_sources);
        self.next_gen = c.next_gen;
        if let Some(mark) = c.local_rows
            && let Some(rows) = &mut self.local_rows
        {
            rows.rollback(mark);
        }
        // A check that stopped with "not implemented" may leave frames
        // open; the body is abandoned then, so only shrink.
        self.rows.truncate(c.depths[5]);
        self.undo_to(c.undo);
        self.infer.rollback_trial(c.infer);
        self.diags.truncate(c.diags);
        self.pending.truncate(c.pending);
        self.lit_nodes.truncate(c.lit_nodes);
        self.overflow_lits.truncate(c.overflow_lits);
        self.neg_nodes.truncate(c.neg_nodes);
        self.user_locals.truncate(c.user_locals);
        self.pre_args.truncate(c.pre_args);
        self.open_params.truncate(c.open_params);
        self.ref_params.truncate(c.ref_params);
        self.call_params.truncate(c.call_params);
        self.joins.rollback(c.joins);
        self.row_waits.truncate(c.row_waits);
        self.bound.truncate(c.bound);
        self.opaque_rets.truncate(c.opaque_rets);
        self.strict_rets.truncate(c.strict_rets);
        self.recursive_returns.truncate(c.recursive_returns);
        let clauses_added = self.env.len() > c.clauses;
        self.env.truncate(c.clauses);
        if clauses_added {
            self.env.key = Some(self.cx.global.env_key(&self.env));
        }
        self.excl.truncate(c.excl[0]);
        self.excl_frames.truncate(c.excl[1]);
        self.conflicts.truncate(c.excl[2]);
        self.cond_flow = None;
        // A check that stopped with "not implemented" may leave frames
        // open; the body is abandoned then, so only shrink.
        self.scopes.truncate(c.depths[0]);
        self.loops.truncate(c.depths[1]);
        self.subs.truncate(c.depths[2]);
        self.suspends.truncate(c.depths[3]);
        self.rets.truncate(c.depths[4]);
        self.close_checkpoint(c);
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
            return self.call_candidate(target, only, recv, args, (n, bang), want);
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
                    self.note_fallback(t, k);
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
                self.call_candidate(target, &cands[*one], recv, args, (n, bang), want)
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
                Ok((Ref(NONE), Ty::POISON))
            }
            several => {
                let msg = format!(
                    "{} fit this call: {}",
                    self.instantiations_text(trait_, self_ty, cands, several),
                    self.candidate_advice(
                        trait_,
                        method,
                        several.iter().map(|i| cands[*i].args),
                        n
                    )
                );
                self.err(Code::AmbiguousMethod, n, &msg);
                Ok((Ref(NONE), Ty::POISON))
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
        let table = self
            .cx
            .impl_table(c.row.module)
            .expect("solver candidate table");
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
        // A tuple template's member steps number on after its bound
        // plan's, at the receiver's tuple type (`crates/hd_check/DESIGN.md` (trait solver) §3.9).
        let mut goals = plan_goals(pool, table, row, &impl_args);
        if matches!(
            table.origin[row],
            ImplOrigin::TupleTemplate | ImplOrigin::UnionTemplate
        ) && let Some(steps) = Impls::Owned(self.cx.impls).tuple_plan(impl_def)
        {
            let first = u16::try_from(table.plan[row].len()).unwrap_or(u16::MAX);
            let target = self.resolved(self_ty);
            goals.extend(match table.origin[row] {
                ImplOrigin::TupleTemplate => tuple_goals(pool, impl_def, &steps, target, first),
                ImplOrigin::UnionTemplate => union_goals(pool, impl_def, &steps, target, first),
                _ => Vec::new(),
            });
        }
        let residual: Vec<TraitRef> = goals
            .into_iter()
            .filter(|(i, _)| c.residual.contains(i))
            .map(|(_, g)| g)
            .collect();
        // A closed residual bound may already determine a trait argument
        // used by a method's own bounds, e.g. I: Iterator[T] for ByRef[I].
        // Learn those facts before checking collect's target bound; bounds
        // still waiting on call arguments are checked below as before.
        for &goal in &residual {
            if let hd_types::solver::Answer::Holds { learned, .. } = self.solve(goal)? {
                for (v, t) in learned {
                    let var = pool.intern_ty(&TyData::Infer(v));
                    let _ = self.infer.unify(pool, var, t);
                }
            }
        }
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
            (bang, want),
        )?;
        // Trials need the expected result to reject candidates. Once a
        // winner is committed, the enclosing expression performs its own
        // coercion; reporting here too duplicates a failed conversion.
        if self.open_trials > 0
            && let Some(w) = want.filter(|w| *w != Ty::VOID)
        {
            self.coerce(r, t, w, n, "result", ExpectPosition::General);
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
            if needs_expected(e)
                || self.bound_local_ref(e)
                || self.pre_args.iter().any(|p| p.0 == e.index())
            {
                continue;
            }
            let (r, t) = self.expr(e, None)?;
            self.pre_args.push((e.index(), r, t));
        }
        Ok(())
    }

    /// Whether a closure argument checked against `want` would meet a
    /// parameter type still unknown: `want` is a variable, or a function
    /// type with an input that is one (§2.4 step 4).
    pub(crate) fn open_inputs(&self, want: Ty) -> bool {
        let pool = self.pool();
        let open = |t: Ty| matches!(pool.get(self.infer.shallow(pool, t)), TyData::Infer(_));
        match pool.get(self.resolved(self.infer.shallow(pool, want))) {
            TyData::Infer(_) => true,
            TyData::Fn { params, .. } => pool.list_items(params).iter().any(|&p| open(p)),
            _ => false,
        }
    }

    /// The type `want` takes once the arguments after a postponed closure
    /// are checked (§2.4 step 4): a trial checks each of `later` against
    /// its parameter type and rolls back, so the arguments are still
    /// checked and emitted in source order. For a function type, each
    /// input the trial solved with no variable left replaces its own, and
    /// the result stays, since the closure may solve it; for a variable,
    /// what the trial solved with no variable left. `None` when the trial
    /// reports an error, stops, or solves nothing. A later closure is not
    /// checked: it may need this one's types.
    pub(crate) fn solved_by_later(&mut self, want: Ty, later: &[(NodeRef<'_>, Ty)]) -> Option<Ty> {
        if later.is_empty() {
            return None;
        }
        let pool = self.pool();
        let before = pool.get(self.resolved(self.infer.shallow(pool, want)));
        let c = self.checkpoint();
        let mut checked = Ok(());
        for &(e, p) in later {
            if e.kind() == SyntaxKind::ClosureExpr {
                continue;
            }
            checked = self.normalize_deep(p).and_then(|w| {
                let (r, t) = self.arg_value(e, Some(w), false)?;
                self.coerce(r, t, w, e, "argument", ExpectPosition::General);
                Ok(())
            });
            if checked.is_err() {
                break;
            }
        }
        let solved = |t: Ty| !pool.has_infer(t) && !pool.has_poison(t);
        let got = match before {
            TyData::Fn {
                params,
                result,
                row,
                suspends,
                inputs,
                deps,
            } => {
                let old = pool.list_items(params).to_vec();
                let items: Vec<Ty> = old
                    .iter()
                    .map(|&p| {
                        Some(self.infer.resolve(pool, p))
                            .filter(|&t| solved(t))
                            .unwrap_or(p)
                    })
                    .collect();
                (items != old).then(|| {
                    pool.intern_ty(&TyData::Fn {
                        params: pool.list(&items),
                        result,
                        row,
                        suspends,
                        inputs,
                        deps,
                    })
                })
            }
            _ => Some(self.infer.resolve(pool, want)).filter(|&t| solved(t)),
        };
        let got = got.filter(|_| checked.is_ok() && self.diags.errors_since(c.diags) == 0);
        self.rollback(c);
        got
    }

    /// An argument's value: the one inferred before the trials, or a
    /// check against `want` now.
    pub(crate) fn arg_value(
        &mut self,
        e: NodeRef<'_>,
        want: Option<Ty>,
        scoped_capture: bool,
    ) -> StageResult<(Ref, Ty)> {
        if let Some(&(_, r, t)) = self.pre_args.iter().rev().find(|p| p.0 == e.index()) {
            return Ok((r, t));
        }
        let direct_fn = want.is_some_and(|t| {
            let pool = self.pool();
            matches!(pool.get(self.resolved(t)), TyData::Fn { .. })
        });
        let saved = self.local_capture_argument;
        // A closure, or a bound method reference, passed directly to a
        // `fn(...)` parameter may capture local references
        // (`fn.capture.local-parameter`).
        if scoped_capture && direct_fn {
            self.local_capture_argument = Some(e.index());
        }
        let result = self.expr(e, want);
        self.local_capture_argument = saved;
        result
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
        // A bounded parameter the expected type can leave partly open,
        // as `List[_]` leaves `collect`'s target.
        let holes = |this: &Self| {
            sig.generics.iter().enumerate().any(|(i, g)| {
                !g.bounds.is_empty() && pool.has_infer(this.infer.resolve(pool, vars[i]))
            })
        };
        if !open(self) && !holes(self) {
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
        self.fill_holes_from_bounds(sig, vars, inst)?;
        for (i, g) in sig.generics.iter().enumerate() {
            if let Some(d) = g.default
                && self.unsolved(vars[i])
            {
                let _ = self.infer.unify(pool, vars[i], inst(d));
            }
        }
        Ok(())
    }

    /// `types.generic.placeholder.local`: a type argument that is known
    /// but still holds inference holes, as `List[?]` from `List[_]`, takes
    /// them from each of its bounds whose trait it implements in exactly
    /// one way that fits, as a parameter named only in bounds does
    /// (`types.generic.infer.bound`).
    fn fill_holes_from_bounds(
        &mut self,
        sig: &FnSig,
        vars: &[Ty],
        inst: &dyn Fn(Ty) -> Ty,
    ) -> StageResult<()> {
        let pool = self.pool();
        for (j, g) in sig.generics.iter().enumerate() {
            let s = self.resolved(self.infer.resolve(pool, vars[j]));
            if matches!(pool.get(s), TyData::Infer(_)) || !pool.has_infer(s) {
                continue;
            }
            for b in &g.bounds {
                let b = self.infer.resolve(pool, inst(*b));
                let TyData::TraitValue { def: tr, args, .. } = pool.get(b) else {
                    continue;
                };
                let Some(found) = self.instantiation_args(s, tr)? else {
                    continue;
                };
                let want = pool.list_items(args).to_vec();
                let fit: Vec<Vec<Ty>> = found
                    .into_iter()
                    .filter(|got| self.can_unify_all(got, &want))
                    .collect();
                if let [one] = fit.as_slice() {
                    let snap = self.infer.snapshot();
                    if !one
                        .iter()
                        .zip(&want)
                        .all(|(a, b)| self.infer.unify(pool, *a, *b).is_ok())
                    {
                        self.infer.rollback(snap);
                    }
                }
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
                let s = self.resolved(self.infer.shallow(pool, vars[j]));
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
        if (tr == self.cx.names.known.fn_trait || tr == self.cx.names.known.fn_mut_trait)
            && let Some(args) = self.fn_trait_args_of(s)
        {
            return Ok(Some(vec![args.to_vec()]));
        }
        match pool.get(s) {
            TyData::Param(_) => {
                return Ok(Some(
                    self.env
                        .clauses_of(s)
                        .iter()
                        .map(|&c| c as usize)
                        .filter(|&c| self.env.clause_trait()[c] == tr)
                        .map(|c| pool.list_items(self.env.clause_args()[c]).to_vec())
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
            // The solver answers the rest below.
            TyData::Prim(_)
            | TyData::Adt { .. }
            | TyData::Tuple { .. }
            | TyData::Union(_)
            | TyData::Ref { .. }
            | TyData::Fn { .. }
            | TyData::Opaque { .. }
            | TyData::Assoc { .. }
            | TyData::Const(_)
            | TyData::Infer(_)
            | TyData::Canon(_)
            | TyData::Row(_)
            | TyData::Context(_) => {}
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
