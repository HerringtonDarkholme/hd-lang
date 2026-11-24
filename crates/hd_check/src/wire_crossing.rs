//! A `Wire` crossing closure as it is passed (`fn.crossing.lift`, stage 6
//! of #2288): the closure literal is lifted to a function of its captures,
//! whose number a worker of the same build finds again, and the caller
//! passes its code (`KnownItems::crossing_code`), a closure of the moved
//! captures that runs the lifted function here for `false` and gives the
//! literal's frame, its number and encoded captures, for `true`. So a call
//! here never encodes, and only a domain that sends pays for the frame.
//! Swift lifts a closure's captures to parameters the same way; Rust's
//! closure is a struct of its captures that `serde` would walk.
//!
//! A `Send` crossing closure takes the captures that may hold a counted
//! buffer unique as it is passed (`fn.crossing.mode.unique`).

use hd_base::{DefId, LocalId, NodeIdx};
use hd_resolve::known::REMOTE_MODULE;
use hd_syntax::NodeRef;
use hd_tir::ir::{
    Callee, NONE, Place, Providers, Ref, SubMark, Tag, TirSink as _, local_flags, place, ref_word,
};
use hd_types::{Prim, Ty, TyData};

use crate::body::Ck;

/// A literal whose captures [`Ck::wire_lift`] lifted: the tuple local that
/// now holds them, and their types.
pub(crate) struct WireLift {
    held: LocalId,
    tys: Vec<Ty>,
}

impl Ck<'_, '_> {
    /// Before the literal `n`'s sub-body `m` closes, when it is written for
    /// a `Wire` crossing parameter: moves its captures into one tuple and
    /// makes that tuple `m`'s leading parameter, so `m` captures nothing.
    /// Returns the lifted type. Cost: the closure's instructions once.
    pub(crate) fn wire_lift(
        &mut self,
        m: SubMark,
        from: usize,
        ft: Ty,
        n: NodeRef<'_>,
    ) -> Option<Ty> {
        if !self.crossing_literal(n, self.cx.names.known.wire) {
            return None;
        }
        let pool = self.pool();
        let syn = n.index();
        let caps = self.b.open_captures(m);
        let tys: Vec<Ty> = caps.iter().map(|&l| self.b.local_ty(l)).collect();
        let tuple_ty = pool.product(pool.list(&tys), None);
        let p = self.wire_local(tuple_ty, "$captures", local_flags::PARAM, syn);
        self.b.lift_captures(m, from, &caps, p);
        let reads: Vec<Ref> = (caps.iter().zip(&tys))
            .map(|(&l, &t)| self.b.get(l, t, syn))
            .collect();
        let rec = self.b.refs_record(&reads);
        let tuple = self.b.emit(Tag::NewTuple, NONE, rec, tuple_ty, syn);
        let held = self.wire_local(tuple_ty, "$held", local_flags::CAPTURED, syn);
        self.b.set(held, tuple, syn);
        self.wire_lifted = Some(WireLift { held, tys });
        pool.fn_led(ft, tuple_ty, None)
    }

    /// Before the literal `n`'s sub-body `m` closes, when it is written for
    /// a `Send` crossing parameter (`fn.crossing.mode.unique`): each
    /// capture that may hold a counted buffer is read into a local, which
    /// `std.rt.unique` makes unique in place, and moved to the one `m`
    /// captures instead. The read moves at the capture's last use, so a
    /// buffer nothing else holds crosses as it is; a copy counts it, so
    /// the call copies it. Both locals keep the source's name, so the move
    /// rules see the binding's. Cost: one call per such capture.
    pub(crate) fn send_unique(&mut self, m: SubMark, from: usize, n: NodeRef<'_>) {
        if !self.crossing_literal(n, self.cx.names.known.send) {
            return;
        }
        let (pool, syn) = (self.pool(), n.index());
        let mut to = Vec::new();
        for l in self.b.open_captures(m) {
            let t = self.b.local_ty(l);
            let z = self.zonk(t);
            if !(matches!(pool.get(z), TyData::Ref { .. } | TyData::Never)
                || matches!(pool.get(z), TyData::Prim(p) if p != Prim::String))
            {
                let name = self.b.body().local_name[l.idx()];
                to.push((l, self.b.local(t, name, local_flags::CAPTURED, syn)));
            }
        }
        // The literal's own places only: the reads below stay the source's.
        self.b.rename_captures(m, from, &to);
        for (l, own) in to {
            let t = self.b.local_ty(l);
            let v = self.b.get(l, t, syn);
            // Not `own`: an address taken of it would make it a cell.
            let held = self.b.local(t, self.b.body().local_name[l.idx()], 0, syn);
            self.b.set(held, v, syn);
            let at = self.b.place(&Place::local(held));
            let r_ty = pool.intern_ty(&TyData::Ref {
                inner: t,
                mutable: true,
                counted: false,
            });
            let r = self.b.emit(Tag::Ref, at, ref_word::MUT, r_ty, syn);
            let unique = self
                .cx
                .names
                .item(hd_resolve::known::RUNTIME_MODULE, "unique");
            let callee = Callee::Item {
                def: unique,
                targs: pool.list(&[t]),
            };
            self.b.call(&callee, &[r], Providers::None, Ty::VOID, syn);
            let v = self.b.get(held, t, syn);
            self.b.set(own, v, syn);
        }
    }

    /// After the lifted literal `lifted` closed, at the call: the code the
    /// callee takes, `fn(send: bool, ..)`. For `false` it calls `lifted` on
    /// the held captures and returns `Ok` of its result, so a call here
    /// runs the literal as written; for `true` it returns `Err` of the
    /// frame (`crossing_frame`). Returns the captures' types for the
    /// marker check, and the code to pass. Cost: one number per call.
    pub(crate) fn wire_pass(
        &mut self,
        lifted: Ref,
        ft: Ty,
        e: NodeRef<'_>,
    ) -> Option<(Vec<Ty>, Ref)> {
        let w = self.wire_lifted.take()?;
        let pool = self.pool();
        let syn = e.index();
        let kfn_ty = self.b.body().ty[lifted.as_inst()?.0 as usize];
        let code = self.wire_local(kfn_ty, "$lifted", local_flags::CAPTURED, syn);
        self.b.set(code, lifted, syn);
        // Collection supplies the first scalar argument from the lifted
        // code's full instance key, after its enclosing arguments are known.
        let u64_ = Ty::prim(Prim::U64);
        let unresolved = self.b.const_value(u64_, 0);
        let f = self.b.get(code, kfn_ty, syn);
        let number = self.remote_call("crossing_number", &[kfn_ty], &[unresolved, f], u64_, syn);
        let num = self.wire_local(u64_, "$number", local_flags::CAPTURED, syn);
        self.b.set(num, number, syn);
        let g = self.cx.names.known.crossing_code(pool, ft);
        let TyData::Fn {
            params, result: r, ..
        } = pool.get(g)
        else {
            return None;
        };
        let (_, parts) = pool.adt_parts(r);
        let value_ty = pool.list_items(parts)[0];
        let args: Vec<LocalId> = (pool.list_items(params).iter())
            .map(|&t| self.wire_local(t, "$arg", local_flags::PARAM, syn))
            .collect();
        let m = self.b.open_sub(&args);
        let blk = self.b.open_block();
        for l in [code, w.held, num] {
            self.b.capture(m, l);
        }
        let tuple = self.b.local_ty(w.held);
        let flag = self.b.get(args[0], Ty::BOOL, syn);
        let sent = self.b.open_block();
        let frame = [self.b.get(num, u64_, syn), self.b.get(w.held, tuple, syn)];
        let bytes = pool.applied(self.cx.names.known.list, Ty::prim(Prim::U8));
        let frame = self.remote_call("crossing_frame", &[tuple], &frame, bytes, syn);
        let rec = self.b.refs_record(&[frame]);
        let err = self.b.emit(Tag::NewVariant, 1, rec, r, syn);
        let sent = self.b.close_block(sent, Some(err), r, syn);
        let here = self.b.open_block();
        let callee = self.b.get(code, kfn_ty, syn);
        let mut inputs = vec![self.b.get(w.held, tuple, syn)];
        for &l in &args[1..] {
            inputs.push(self.b.get(l, self.b.local_ty(l), syn));
        }
        let rec = self.b.call_args_record(&inputs, Providers::None);
        // A literal that suspends is called cold; the callee awaits it.
        let value = self.b.emit(Tag::CallValue, callee.0, rec, value_ty, syn);
        let rec = self.b.refs_record(&[value]);
        let ok = self.b.emit(Tag::NewVariant, 0, rec, r, syn);
        let here = self.b.close_block(here, Some(ok), r, syn);
        let rec = self.b.refs_record(&[sent, here]);
        let chosen = self.b.emit(Tag::If, flag.0, rec, r, syn);
        let root = self.b.close_block(blk, Some(chosen), r, syn);
        let passed = self.b.close_sub(m, root, g, syn);
        if let Some(c) = passed.as_inst() {
            self.crossing_closures.push((c.0, self.cx.names.known.wire));
        }
        Some((w.tys, passed))
    }

    /// A call through `f` of `args`, of type `ty`. When `f` reads a `Wire`
    /// crossing parameter, its code is called with `false` first, and the
    /// call is the `Ok` that returns (`fn.crossing.lift`).
    pub(crate) fn value_call(&mut self, f: Ref, args: &[Ref], ty: Ty, syn: NodeIdx) -> Ref {
        let wire = self.cx.names.known.wire;
        let read = f.as_inst().map_or(NONE, |i| {
            let b = self.b.body();
            let i = i.0 as usize;
            if b.tags[i] == Tag::Read {
                b.data[i][0]
            } else {
                NONE
            }
        });
        let param = (self.crossing_params.iter())
            .find(|&&(l, m)| m == wire && read == l.raw() | place::BARE_LOCAL);
        let Some(&(l, _)) = param else {
            let rec = self.b.call_args_record(args, Providers::None);
            return self.b.emit(Tag::CallValue, f.0, rec, ty, syn);
        };
        let code = self
            .cx
            .names
            .known
            .crossing_code(self.pool(), self.b.local_ty(l));
        let TyData::Fn { result, .. } = self.pool().get(code) else {
            return Ref(NONE);
        };
        let args: Vec<Ref> = std::iter::once(self.b.const_value(Ty::BOOL, 0))
            .chain(args.iter().copied())
            .collect();
        let rec = self.b.call_args_record(&args, Providers::None);
        let ran = self.b.emit(Tag::CallValue, f.0, rec, result, syn);
        self.b.payload(ran, 0, 0, ty, syn)
    }

    /// At the body's end: each `Wire` crossing parameter holds its
    /// literal's code (`fn.crossing.lift`), and so does each read of it.
    pub(crate) fn wire_params_finish(&mut self) {
        let wire = self.cx.names.known.wire;
        for (l, _) in self
            .crossing_params
            .clone()
            .into_iter()
            .filter(|&(_, m)| m == wire)
        {
            let f = self.zonk(self.b.local_ty(l));
            let code = self.cx.names.known.crossing_code(self.pool(), f);
            let b = self.b.body_mut();
            b.local_ty[l.idx()] = code;
            for i in 0..b.len() {
                if b.tags[i] == Tag::Read && b.data[i][0] == l.raw() | place::BARE_LOCAL {
                    b.ty[i] = code;
                }
            }
        }
    }

    /// Whether `n` is a literal written for a crossing parameter of
    /// `marker`, which it then stops being: a literal inside another
    /// closes first.
    fn crossing_literal(&mut self, n: NodeRef<'_>, marker: DefId) -> bool {
        let at = (self.crossing_literals.iter()).rposition(|&l| l == (n.index(), marker));
        at.map(|at| self.crossing_literals.remove(at)).is_some()
    }

    /// A hidden local of the literal's.
    fn wire_local(&mut self, ty: Ty, name: &str, flags: u16, syn: NodeIdx) -> LocalId {
        let name = self.cx.names.syms.intern(name);
        self.b.local(ty, name, flags, syn)
    }

    /// A call of `std.remote`'s `name` at `targs`.
    fn remote_call(&mut self, name: &str, targs: &[Ty], args: &[Ref], ty: Ty, syn: NodeIdx) -> Ref {
        let callee = Callee::Item {
            def: self.cx.names.item(REMOTE_MODULE, name),
            targs: self.pool().list(targs),
        };
        self.b.call(&callee, args, Providers::None, ty, syn)
    }
}
