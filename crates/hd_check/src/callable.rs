//! Callable values (spec 05 "Callable Values"): `v()` on a value of a type
//! that implements `Apply` is the call `Apply::apply(v)`, and `v() = x`,
//! `v() op= x` store through `Update::[V]::update(&mut v, x)`.

use hd_base::StageResult;
use hd_diag::Code;
use hd_resolve::Src;
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{NONE, Ref};
use hd_types::{Ty, TyData};

use crate::body::Ck;
use crate::call::Args;

impl Ck<'_, '_> {
    /// Whether `t`, a callee that is not a function, implements `Apply`
    /// (`expr.call.apply.read`).
    pub(crate) fn is_applicable(&mut self, t: Ty) -> StageResult<bool> {
        let pool = self.pool();
        let t = self.strip_mut(self.infer.resolve(pool, t));
        if matches!(pool.get(t), TyData::Poison | TyData::Never) {
            return Ok(false);
        }
        let apply = self.cx.names.known.apply;
        self.op_fits(apply, t, None)
    }

    /// `v()` for a callable value `v` of type `vt`: the call
    /// `Apply::apply(v)`, whose type is the implementation's `Out`
    /// (`expr.call.apply.read-type`). A callable value takes no argument
    /// (`expr.call.apply.arguments`).
    pub(crate) fn apply_call(
        &mut self,
        (v, vt): (Ref, Ty),
        args: &Args<'_>,
        n: NodeRef<'_>,
    ) -> StageResult<(Ref, Ty)> {
        if has_arguments(args) {
            self.err(
                Code::ArgumentCount,
                n,
                "a callable value takes no arguments",
            );
            return Ok((Ref(NONE), Ty::NEVER));
        }
        let apply = self.cx.names.known.apply;
        self.trait_call_args(apply, "apply", v, vt, &[], n)
    }

    /// `v() = x` and `v() op= x` (`expr.assign.order.call`,
    /// `expr.assign.compound.call-once`): the callee is evaluated once,
    /// then the right-hand expression; a compound one reads with
    /// `Apply::apply(v)` and every one stores with `Update::update(v, x)`.
    pub(crate) fn call_place(
        &mut self,
        lhs: NodeRef<'_>,
        rhs: NodeRef<'_>,
        compound: Option<TokenKind>,
        s: NodeRef<'_>,
    ) -> StageResult<()> {
        let Some(callee) = lhs.children().next() else {
            return self.gap(lhs, "a call target without a callee");
        };
        // A method call, a variant, a path or a type-argument call names
        // a function, never a callable value (`expr.member.stored-fn`).
        if matches!(
            callee.kind(),
            SyntaxKind::FieldExpr
                | SyntaxKind::PathExpr
                | SyntaxKind::VariantExpr
                | SyntaxKind::TypeArgsExpr
                | SyntaxKind::SuspendExpr
        ) {
            self.not_a_place(lhs);
            return Ok(());
        }
        let args = self.args_of(Src::child(lhs, SyntaxKind::ArgumentList));
        let pool = self.pool();
        let update = self.cx.names.known.update;
        // A place stores through `Update`'s `&mut self` method, which takes
        // the place's address; any other callee must be a `&mut` reference
        // (`expr.call.apply.mut-ref`).
        let peeked = self
            .peek_place_ty(callee)
            .filter(|t| self.ref_of(*t).is_none());
        let evaluated = match peeked {
            Some(_) => None,
            None => Some(self.expr(callee, None)?),
        };
        let t = peeked.or(evaluated.map(|(_, t)| t)).unwrap_or(Ty::POISON);
        let t = match self.ref_of(t) {
            Some((inner, m)) => self.deref_ty(inner, m),
            None => t,
        };
        let rt = self.strip_mut(self.infer.resolve(pool, t));
        if matches!(pool.get(rt), TyData::Poison | TyData::Never) {
            return Ok(());
        }
        if matches!(pool.get(rt), TyData::Fn { .. }) || !self.op_fits(update, rt, None)? {
            if peeked.is_some() {
                self.expr(callee, None)?;
            }
            self.not_a_place(lhs);
            return Ok(());
        }
        if has_arguments(&args) {
            self.err(
                Code::ArgumentCount,
                lhs,
                "a callable value takes no arguments",
            );
            return Ok(());
        }
        let recv = match evaluated {
            None => self.store_receiver(callee, "update")?,
            Some((cr, ct)) => self.stored_through(cr, ct, "update", callee),
        };
        let Some((cr, ct)) = recv else {
            return Ok(());
        };
        let (vr, vt) = match compound {
            None => self.expr(rhs, None)?,
            Some(k) => {
                if !self.is_applicable(ct)? {
                    let msg = format!("{} does not implement `Apply`", self.show(ct));
                    self.err(Code::TypeMismatch, lhs, &msg);
                    return Ok(());
                }
                let apply = self.cx.names.known.apply;
                // `apply` reads the value the reference refers to.
                let (cv, cvt) = self.emit_deref(cr, self.b.ty_of(cr), lhs.index());
                let (cur, curt) = self.trait_call_args(apply, "apply", cv, cvt, &[], lhs)?;
                let v = self.compound(k, cur, curt, rhs, s)?;
                (v, self.b.ty_of(v))
            }
        };
        self.trait_call_args(update, "update", cr, ct, &[(vr, vt, rhs)], s)?;
        Ok(())
    }

    /// `expr.assign.target`, `expr.call.apply.place`: a call that is no
    /// callable value with `Update` is not a place.
    fn not_a_place(&mut self, lhs: NodeRef<'_>) {
        self.err(
            Code::InvalidAssignmentTarget,
            lhs,
            "this call is not a place: its callee's type does not implement `Update`",
        );
    }
}

fn has_arguments(args: &Args<'_>) -> bool {
    !args.positional.is_empty()
        || !args.after_spread.is_empty()
        || args.spread.is_some()
        || !args.named.is_empty()
        || args.trailing.is_some()
}
