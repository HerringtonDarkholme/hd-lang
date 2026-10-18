//! `using name [: T] = e` (spec/lang/06-control-flow.md, Using Bindings):
//! a binding whose value a `defer` suite disposes. The binding is checked
//! as a `let`, with mutable access and no reassignment; then a suite that
//! calls `std.ops.Disposable.dispose!` on it registers at this point, so the scope's exit ladder runs it in one
//! last-in, first-out order with the scope's other suites.

use hd_base::{LocalId, StageResult};
use hd_diag::Code;
use hd_intern::PathKind;
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::ir::{Callee, TirSink, local_flags};
use hd_types::solver::{Answer, TraitRef};
use hd_types::{Ty, TyList};

use crate::body::{Ck, unsupported};

impl Ck<'_, '_> {
    /// A `using` statement (`flow.using.*`).
    pub(crate) fn using_stmt(&mut self, s: NodeRef<'_>, kids: &[NodeRef<'_>]) -> StageResult<()> {
        let Some(pat) = kids.first().copied() else {
            return unsupported("a `using` without a name");
        };
        let ty_node = kids.iter().copied().find(|k| k.kind().is_type());
        let Some(rhs) = kids.iter().copied().skip(1).find(|k| !k.kind().is_type()) else {
            return unsupported("a `using` without a value");
        };
        self.using_bind = true;
        self.let_pattern(pat, ty_node, rhs, None, s)?;
        let name = self.sym_of(pat);
        let Some((l, _)) = self.find_local(name) else {
            return Ok(());
        };
        // `flow.using.binding`: not reassignable; its dispose reads it.
        self.add_flags(l, local_flags::SHORT | local_flags::READ);
        self.using_locals.push(l);
        let t = self.b.local_ty(l);
        self.dispose_suite(l, t, ty_node.is_some(), s)
    }

    /// The binding's access (`flow.using.access`): mutable, as the
    /// receiver of `dispose(mut self)` needs.
    fn using_access(&mut self, t: Ty, annotated: bool, at: NodeRef<'_>) {
        if !self.is_composite(self.strip_mut(t)) || self.let_mut_ok(t) {
            return;
        }
        if annotated {
            let msg = format!(
                "a `using` binding is disposed through `mut self`; write `mut {}`",
                self.show(t)
            );
            self.err(Code::LetMutReadonlyType, at, &msg);
        } else {
            let msg = format!(
                "a `using` binding needs mutable access, but the value is a readonly {}",
                self.show(t)
            );
            self.err(Code::MutableUpgrade, at, &msg);
        }
    }

    /// Registers the suite that disposes local `l` of type `t`
    /// (`flow.using.dispose`, `flow.using.bang`).
    fn dispose_suite(
        &mut self,
        l: LocalId,
        t: Ty,
        annotated: bool,
        s: NodeRef<'_>,
    ) -> StageResult<()> {
        let pool = self.pool();
        let known = self.cx.names.known;
        let self_ty = self.strip_mut(t);
        if pool.has_poison(self_ty) {
            return Ok(());
        }
        let trait_ = known.disposable;
        let goal = TraitRef {
            trait_,
            self_ty,
            args: TyList::EMPTY,
        };
        if let Answer::Fails(_) = self.solve(goal)? {
            let msg = format!(
                "{} does not implement Disposable, so `using` cannot dispose it",
                self.show(self_ty)
            );
            self.err(Code::UnsatisfiedTraitBound, s, &msg);
            return Ok(());
        }
        let at = s
            .children()
            .find(|k| k.kind() == SyntaxKind::BindingPattern)
            .unwrap_or(s);
        self.using_access(t, annotated, at);
        let method = self.cx.names.member(trait_, PathKind::Member, "dispose");
        let sig = self.sig_of(method)?;
        let ev = self.require_ref(goal, s)?;
        let choice = self.choice_of(ev.as_ref());
        let m = self.b.open_block();
        let recv = self.b.get(l, t, s.index());
        let c = Callee::TraitMethod {
            trait_,
            method,
            self_ty,
            targs: TyList::EMPTY,
            choice,
        };
        // `dispose!` is a bang call, so the existing driver-context rules
        // decide where a `using` may stand (`req.bang.driver-forms`,
        // `flow.defer.suspend`).
        self.emit_call(&c, &[recv], Ty::VOID, sig.suspends, true, s);
        let suite = self.b.close_block(m, None, Ty::VOID, s.index());
        self.b.defer(suite, s.index());
        Ok(())
    }
}
