//! `using name [: T] = e` (spec/lang/06-control-flow.md, Using Bindings):
//! a binding whose value a `defer` suite disposes. The binding is checked
//! as a `let`, with no reassignment; then a suite that calls
//! `std.ops.Disposable.dispose!(&mut self)` on it registers at this point,
//! so the scope's exit ladder runs it in one last-in, first-out order with
//! the scope's other suites. The call takes the binding's own place, or
//! passes the reference the binding holds.

use hd_base::{LocalId, StageResult};
use hd_diag::Code;
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::ir::{TirSink, local_flags};
use hd_types::solver::{Answer, TraitRef};
use hd_types::{Ty, TyData, TyList};

use crate::body::{Ck, unsupported};
use crate::call::{Args, Hit};
use crate::refs::{Place, Root};

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
        let Some((l, depth)) = self.find_local(name) else {
            return Ok(());
        };
        // `flow.using.binding`: not reassignable; its dispose reads it.
        self.add_flags(l, local_flags::SHORT | local_flags::READ);
        self.using_locals.push(l);
        let t = self.b.local_ty(l);
        self.dispose_suite((l, depth), t, s)
    }

    /// Registers the suite that disposes local `l` of type `t`
    /// (`flow.using.dispose`, `flow.using.bang`). `dispose!` takes
    /// `&mut self`: a binding that holds the value passes its own place
    /// (`flow.using.place`), and one that holds a reference passes the
    /// reference, a trait value reference its trait value
    /// (`flow.using.reference`).
    fn dispose_suite(
        &mut self,
        (l, depth): (LocalId, usize),
        t: Ty,
        s: NodeRef<'_>,
    ) -> StageResult<()> {
        let pool = self.pool();
        let known = self.cx.names.known;
        let trait_ = known.disposable;
        let referred = self.ref_of(t);
        let self_ty = match referred {
            Some((inner, _)) => self.strip_mut(self.infer.resolve(pool, inner)),
            None => self.strip_mut(t),
        };
        if pool.has_poison(self_ty) {
            return Ok(());
        }
        let via_dyn = referred.is_some() && matches!(pool.get(self_ty), TyData::TraitValue { .. });
        // A trait value reference reaches its trait's methods and its
        // supertraits' (`trait.dyn.ref.member`); any other type proves
        // the bound.
        let implements = if via_dyn {
            matches!(
                self.resolve_method(self_ty, "dispose")?,
                Some(Hit::Trait { trait_: d, .. }) if d == trait_
            )
        } else {
            let goal = TraitRef {
                trait_,
                self_ty,
                args: TyList::EMPTY,
            };
            !matches!(self.solve(goal)?, Answer::Fails(_))
        };
        if !implements {
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
        let m = self.b.open_block();
        let (recv, rt) = if referred.is_none() {
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
            });
            (self.make_ref(&place, inner, true, rty, s), t)
        } else {
            let v = self.b.get(l, t, s.index());
            if let Some(passed) = self.pass_reference_receiver(v, t, "dispose", at)? {
                passed
            } else {
                // A trait value reference passes its trait value, whose
                // payload is the reference receiver (`trait.dyn.ref.member`).
                let mutable = referred.is_some_and(|(_, m)| m);
                let (v, vt) = self.emit_deref(v, t, s.index());
                self.dyn_recv = Some((mutable, at.index()));
                (v, vt)
            }
        };
        let none = Args {
            positional: Vec::new(),
            spread: None,
            after_spread: Vec::new(),
            named: Vec::new(),
            trailing: None,
        };
        // `dispose!` is a bang call, so the existing driver-context rules
        // decide where a `using` may stand (`req.bang.driver-forms`,
        // `flow.defer.suspend`).
        self.method_on(recv, rt, "dispose", &none, (s, true), Some(Ty::VOID))?;
        let suite = self.b.close_block(m, None, Ty::VOID, s.index());
        self.b.defer(suite, s.index());
        Ok(())
    }
}
