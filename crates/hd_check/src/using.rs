//! `using[!] name [: T] = e` (spec/lang/06-control-flow.md, Using Bindings):
//! a binding whose value a `defer` suite disposes. The binding is checked
//! as a `let`, with no reassignment. `using` registers a synchronous
//! `std.ops.Disposable.dispose(&mut self)` call; `using!` registers a
//! suspending `std.ops.AsyncDisposable.dispose!(&mut self)` call. The
//! scope's exit ladder runs either in one last-in, first-out order with
//! its other suites. The call takes the binding's own place, or passes the
//! reference the binding holds. The selected implementation supplies the
//! disposal method's requirement row, checked at the binding.

use hd_base::{LocalId, StageResult};
use hd_diag::Code;
use hd_intern::PathKind;
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{ChoiceKind, TirSink, local_flags};
use hd_types::solver::{Answer, Evidence, TraitRef};
use hd_types::{Ty, TyData, VarKind};

use crate::body::{Ck, supertrait_args, unsupported};
use crate::call::{Args, PENDING_CHOICE, TraitTarget};
use crate::refs::{Place, Root};

impl Ck<'_, '_> {
    /// A `using` statement (`flow.using.*`).
    pub(crate) fn using_stmt(&mut self, s: NodeRef<'_>, kids: &[NodeRef<'_>]) -> StageResult<()> {
        let asynchronous = s
            .direct_token(&self.cx.src.parse.tokens, TokenKind::Bang)
            .is_some();
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
        let Some((l, depth)) = self.find_local(name) else {
            return Ok(());
        };
        // `flow.using.binding.v2`: not reassignable; its dispose reads it.
        self.add_flags(l, local_flags::SHORT | local_flags::READ);
        self.using_locals.push(l);
        let t = self.b.local_ty(l);
        self.dispose_suite((l, depth), t, asynchronous, s)
    }

    /// Registers the suite that disposes local `l` of type `t`
    /// (`flow.using.dispose.v2`, `flow.using.async.driver`). The selected
    /// disposal method takes
    /// `&mut self`: a binding that holds the value passes its own place
    /// (`flow.using.place.v2`), and one that holds a reference passes the
    /// reference, a trait value reference its trait value
    /// (`flow.using.reference.v2`).
    pub(crate) fn dispose_suite(
        &mut self,
        (l, depth): (LocalId, usize),
        t: Ty,
        asynchronous: bool,
        s: NodeRef<'_>,
    ) -> StageResult<()> {
        let pool = self.pool();
        let known = self.cx.names.known;
        let trait_ = if asynchronous {
            known.async_disposable
        } else {
            known.disposable
        };
        let referred = self.ref_of(t);
        let self_ty = match referred {
            Some((inner, _)) => self.strip_mut(self.infer.resolve(pool, inner)),
            None => self.strip_mut(t),
        };
        if pool.has_poison(self_ty) {
            return Ok(());
        }
        // The selected protocol is explicit even when the type implements
        // both. Resolve the trait goal rather than the method name, which
        // would be ambiguous in that case.
        let dyn_trait = match pool.get(self_ty) {
            TyData::TraitValue { def, .. } => Some(def),
            _ => None,
        };
        let (implements, trait_args) =
            if let TyData::TraitValue { def, args, .. } = pool.get(self_ty) {
                match supertrait_args(self.cx.lookup, pool, def, args, trait_, 0) {
                    Some(args) => (true, pool.list_items(args).to_vec()),
                    None => (false, Vec::new()),
                }
            } else {
                // A disposal implementation fixes its cleanup row. A bare goal
                // variable learns the implementation head's whole row, as other
                // omitted trait arguments do.
                let row = self.fresh_disposal_arg();
                let goal = TraitRef {
                    trait_,
                    self_ty,
                    args: pool.list(&[row]),
                };
                let answer = self.solve(goal)?;
                if let Answer::Holds { learned, .. } = &answer {
                    for (v, t) in learned {
                        let var = pool.intern_ty(&TyData::Infer(*v));
                        let _ = self.infer.unify(pool, var, *t);
                    }
                }
                (
                    !matches!(answer, Answer::Fails(_)),
                    vec![self.infer.resolve(pool, row)],
                )
            };
        if !implements {
            let msg = format!(
                "{} does not implement {}, so `{}` cannot dispose it",
                self.show(self_ty),
                if asynchronous {
                    "AsyncDisposable"
                } else {
                    "Disposable"
                },
                if asynchronous { "using!" } else { "using" }
            );
            self.err(Code::UnsatisfiedTraitBound, s, &msg);
            return Ok(());
        }
        let at = s
            .children()
            .find(|k| k.kind() == SyntaxKind::BindingPattern)
            .unwrap_or(s);
        if referred.is_some_and(|(_, mutable)| !mutable) {
            let msg = format!(
                "`dispose` takes `&mut self`, but the receiver is a read-only reference {}; take it with `&mut` to change what it refers to",
                self.show(t)
            );
            self.err(Code::NotWritable, at, &msg);
            return Ok(());
        }
        let m = self.b.open_block();
        let recv = if let Some((inner, _)) = referred {
            let v = self.b.get(l, t, s.index());
            let vt = self.deref_ty(inner, true);
            if matches!(pool.get(self.strip_mut(vt)), TyData::TraitValue { .. }) {
                // A trait value passes its payload to the selected trait's
                // reference receiver (`trait.dyn.ref.member`).
                self.emit_deref(v, t, s.index()).0
            } else {
                let counted = self.ref_counted(t).unwrap_or(true);
                let rty = self.ref_ty(self.strip_mut(vt), true, counted);
                self.retype(v, rty, at.index())
            }
        } else {
            // The binding's own place, which the call changes in place.
            let place = Place {
                root: Root::Local(l, depth),
                path: vec![],
                ty: t,
            };
            let inner = self.strip_mut(t);
            let rty = pool.intern_ty(&TyData::Ref {
                inner,
                mutable: true,
                counted: true,
            });
            self.make_ref(&place, true, rty, s)
        };
        let none = Args {
            positional: Vec::new(),
            spread: None,
            after_spread: Vec::new(),
            named: Vec::new(),
            trailing: None,
        };
        // Only `using!` writes a bang call. Existing driver-context rules
        // decide where it may stand (`req.bang.driver-forms`,
        // `flow.using.async.driver`); `using` stays synchronous.
        let method = self.cx.names.member(trait_, PathKind::Member, "dispose");
        self.trait_method_call(
            TraitTarget {
                trait_,
                method,
                self_ty,
                args: trait_args,
            },
            Some((
                recv,
                dyn_trait.map_or(PENDING_CHOICE, |def| (ChoiceKind::TraitValue, def.raw())),
            )),
            &none,
            s,
            (asynchronous, Some(Ty::VOID)),
        )?;
        let suite = self.b.close_block(m, None, Ty::VOID, s.index());
        self.b.defer(suite, s.index());
        Ok(())
    }

    /// Whether `t` implements the compiler-known `AutoClosing` marker.
    pub(crate) fn implements_auto_closing(&mut self, t: Ty) -> StageResult<bool> {
        let self_ty = self.strip_mut(t);
        if self.pool().has_poison(self_ty) {
            return Ok(false);
        }
        let goal = TraitRef {
            trait_: self.cx.names.known.auto_closing,
            self_ty,
            args: hd_types::TyList::EMPTY,
        };
        Ok(matches!(
            self.solve(goal)?,
            Answer::Holds { evidence, .. } if !matches!(evidence, Evidence::Poison)
        ))
    }

    /// Checks an expression whose value is about to be stored in an
    /// existing place or inside a newly constructed value.
    pub(crate) fn stored_expr(
        &mut self,
        expression: NodeRef<'_>,
        want: Option<Ty>,
        place: &str,
    ) -> StageResult<(hd_tir::ir::Ref, Ty)> {
        let before = self.diags.mark();
        let (value, ty) = self.expr(expression, want)?;
        if self.diags.errors_since(before) == 0 && self.implements_auto_closing(ty)? {
            let message = format!(
                "an AutoClosing value cannot be stored {place}; call its `@auto_closing` constructor as a statement"
            );
            self.err(Code::UnregisteredAutoClosing, expression, &message);
        }
        Ok((value, ty))
    }

    /// Which disposal protocol an auto-closing value uses. A synchronous
    /// implementation is selected when present; otherwise the suspending
    /// protocol is tried. The marker implementation is checked separately.
    fn auto_closing_protocol(&mut self, t: Ty) -> StageResult<Option<bool>> {
        let self_ty = self.strip_mut(t);
        if self.pool().has_poison(self_ty) {
            return Ok(None);
        }
        for (asynchronous, trait_) in [
            (false, self.cx.names.known.disposable),
            (true, self.cx.names.known.async_disposable),
        ] {
            let row = self.fresh_disposal_arg();
            let goal = TraitRef {
                trait_,
                self_ty,
                args: self.pool().list(&[row]),
            };
            if matches!(
                self.solve(goal)?,
                Answer::Holds { evidence, .. } if !matches!(evidence, Evidence::Poison)
            ) {
                return Ok(Some(asynchronous));
            }
        }
        Ok(None)
    }

    /// Keeps an `@auto_closing` call's result in an unnameable local and
    /// registers its selected disposal suite at the call statement.
    pub(crate) fn register_auto_closing(
        &mut self,
        value: hd_tir::ir::Ref,
        ty: Ty,
        at: NodeRef<'_>,
    ) -> StageResult<()> {
        if self.cleanup_blocks.is_empty() {
            self.err(
                Code::DeferOutsideCleanupScope,
                at,
                "a module's top level is not a cleanup scope",
            );
            return Ok(());
        }
        let Some(asynchronous) = self.auto_closing_protocol(ty)? else {
            return Ok(());
        };
        let (local, depth) = self.hold(value, ty, at);
        self.add_flags(local, local_flags::SHORT | local_flags::READ);
        self.dispose_suite((local, depth), ty, asynchronous, at)?;
        if let Some(cleanup) = self.cleanup_blocks.last_mut() {
            *cleanup = true;
        }
        Ok(())
    }

    /// Whether a value type implements either protocol which requires an
    /// explicit cleanup binding. Analysis records only this neutral answer;
    /// `hd_lint` owns the `unbound-disposable` policy.
    pub(crate) fn implements_disposal_protocol(&mut self, t: Ty) -> StageResult<bool> {
        let self_ty = self.strip_mut(t);
        if self.pool().has_poison(self_ty) {
            return Ok(false);
        }
        for trait_ in [
            self.cx.names.known.disposable,
            self.cx.names.known.async_disposable,
        ] {
            let row = self.fresh_disposal_arg();
            let goal = TraitRef {
                trait_,
                self_ty,
                args: self.pool().list(&[row]),
            };
            if !matches!(self.solve(goal)?, Answer::Fails(_)) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// An omitted disposal trait argument whose implementation fixes the
    /// whole requirement row.
    fn fresh_disposal_arg(&mut self) -> Ty {
        let pool = self.pool();
        self.infer.fresh(pool, VarKind::General)
    }
}
