//! A `Wire` crossing closure as it is passed (`fn.crossing.lift`, stage 6
//! of #2288): the closure literal is lifted to a function of its captures,
//! whose number a worker of the same build finds again, and the caller
//! passes `std.remote.Crossing { code, number, captures }`: a closure that
//! calls the lifted function on the captures here, and the captures encoded
//! for another instance. Inside the callee the parameter is that struct, so
//! a call reads its `code` and a domain reads the rest. Swift lifts a
//! closure's captures to parameters the same way; Rust's closure is a
//! struct of its captures that `serde` would walk.

use hd_base::{LocalId, NodeIdx};
use hd_resolve::known::REMOTE_MODULE;
use hd_syntax::NodeRef;
use hd_tir::ir::{
    Callee, NONE, Place, PlaceRoot, Proj, Providers, ReadMode, Ref, SubMark, Tag, TirSink as _,
    local_flags, place,
};
use hd_types::{Ty, TyData};

use crate::body::Ck;

/// A literal whose captures [`Ck::wire_lift`] lifted: the tuple local that
/// now holds them, their types, and the local holding their encoding.
pub(crate) struct WireLift {
    held: LocalId,
    tys: Vec<Ty>,
    frame: LocalId,
}

impl Ck<'_, '_> {
    /// Before the literal `n`'s sub-body `m` closes, when it is written for
    /// a `Wire` crossing parameter: moves its captures into one tuple,
    /// encodes it (`crossing_frame`), and makes that tuple `m`'s leading
    /// parameter, so `m` captures nothing. Returns the lifted type.
    /// Cost: the closure's instructions once, and one encode per call.
    pub(crate) fn wire_lift(
        &mut self,
        m: SubMark,
        from: usize,
        ft: Ty,
        n: NodeRef<'_>,
    ) -> Option<Ty> {
        if self.wire_literal != Some(n.index()) {
            return None;
        }
        self.wire_literal = None;
        let pool = self.pool();
        let syn = n.index();
        let caps = self.b.open_captures(m);
        let tys: Vec<Ty> = caps.iter().map(|&l| self.b.local_ty(l)).collect();
        let tuple_ty = pool.product(pool.list(&tys), None);
        let p = self.wire_local(tuple_ty, "$captures", syn);
        self.b.body_mut().local_flags[p.idx()] |= local_flags::PARAM;
        self.b.lift_captures(m, from, &caps, p);
        let reads: Vec<Ref> = (caps.iter().zip(&tys))
            .map(|(&l, &t)| self.b.get(l, t, syn))
            .collect();
        let rec = self.b.refs_record(&reads);
        let tuple = self.b.emit(Tag::NewTuple, NONE, rec, tuple_ty, syn);
        let bytes = self.bytes_ty();
        let pair_ty = pool.product(pool.list(&[tuple_ty, bytes]), None);
        let pair = self.remote_call("crossing_frame", &[tuple_ty], &[tuple], pair_ty, syn);
        let pair_local = self.wire_local(pair_ty, "$crossing", syn);
        self.b.set(pair_local, pair, syn);
        let held = self.wire_local(tuple_ty, "$held", syn);
        let v = self.held_field(pair_local, 0, tuple_ty, syn);
        self.b.set(held, v, syn);
        self.add_flags(held, local_flags::CAPTURED);
        let frame = self.wire_local(bytes, "$frame", syn);
        let v = self.held_field(pair_local, 1, bytes, syn);
        self.b.set(frame, v, syn);
        self.wire_lifted = Some(WireLift { held, tys, frame });
        let TyData::Fn {
            params: own,
            result,
            row,
            suspends,
            inputs,
            deps,
        } = pool.get(ft)
        else {
            return None;
        };
        let all: Vec<Ty> = std::iter::once(tuple_ty)
            .chain(pool.list_items(own).iter().copied())
            .collect();
        Some(pool.intern_ty(&TyData::Fn {
            params: pool.list(&all),
            result,
            row,
            suspends,
            inputs,
            deps,
        }))
    }

    /// After the lifted literal `lifted` closed, at the call: the
    /// `Crossing` the callee takes. Its `code` calls `lifted` on the held
    /// captures; its number is `crossing_number` of the literal and its
    /// function. Returns the held captures for the marker check, and the
    /// value to pass.
    pub(crate) fn wire_pass(
        &mut self,
        lifted: Ref,
        ft: Ty,
        e: NodeRef<'_>,
    ) -> Option<(Vec<Ty>, Ref)> {
        let w = self.wire_lifted.take()?;
        let pool = self.pool();
        let syn = e.index();
        let i = lifted.as_inst()?;
        let kfn_ty = self.b.body().ty[i.0 as usize];
        let code = self.wire_local(kfn_ty, "$lifted", syn);
        self.b.set(code, lifted, syn);
        self.add_flags(code, local_flags::CAPTURED);
        // The literal: its item's path digest and its node
        // (`fn.crossing.lift`), which every build of the same source keeps.
        let item = self.cx.names.path_hash(self.b.body().item).0;
        let item = u64::try_from(item & u128::from(u64::MAX)).unwrap_or(0);
        let literal = item ^ u64::from(syn.raw());
        let literal = self
            .b
            .const_value(hd_types::Ty::prim(hd_types::Prim::U64), literal);
        let f = self.b.get(code, kfn_ty, syn);
        let u64_ = hd_types::Ty::prim(hd_types::Prim::U64);
        let number = self.remote_call("crossing_number", &[kfn_ty], &[literal, f], u64_, syn);
        // `code`: a closure of the held captures that calls the lifted
        // function on them, so a call here runs the literal as written.
        let TyData::Fn {
            suspends, result, ..
        } = pool.get(ft)
        else {
            return None;
        };
        let m = self.b.open_sub(&[]);
        let blk = self.b.open_block();
        self.b.capture(m, code);
        let callee = self.b.get(code, kfn_ty, syn);
        self.b.capture(m, w.held);
        let tuple = self.b.local_ty(w.held);
        let args = [self.b.get(w.held, tuple, syn)];
        let rec = self.b.call_args_record(&args, Providers::None);
        let value = if suspends {
            let st = self.suspend_ty(result);
            let cold = self.b.emit(Tag::CallValue, callee.0, rec, st, syn);
            let empty = self.b.call_args_record(&[], Providers::None);
            self.b.emit(Tag::AwaitValue, cold.0, empty, result, syn)
        } else {
            self.b.emit(Tag::CallValue, callee.0, rec, result, syn)
        };
        let root = self.b.close_block(blk, Some(value), result, syn);
        let call = self.b.close_sub(m, root, ft, syn);
        let frame = self.b.get(w.frame, self.bytes_ty(), syn);
        let rec = self.b.refs_record(&[call, number, frame]);
        let crossing = self.crossing_ty(ft);
        let value = self.b.emit(Tag::NewStruct, NONE, rec, crossing, syn);
        if let Some(c) = call.as_inst() {
            self.crossing_closures.push((c.0, self.cx.names.known.wire));
        }
        Some((w.tys, value))
    }

    /// At the body's end: each `Wire` crossing parameter is a `Crossing`
    /// (`fn.crossing.lift`). A call of it reads its `code`; passing it on
    /// passes the struct.
    pub(crate) fn wire_params_finish(&mut self) {
        let wire = self.cx.names.known.wire;
        let params: Vec<LocalId> = (self.crossing_params.iter())
            .filter(|&&(_, m)| m == wire)
            .map(|&(l, _)| l)
            .collect();
        if params.is_empty() {
            return;
        }
        let callees: hd_base::HashSet<u32> = {
            let b = self.b.body();
            (0..b.len())
                .filter(|&i| b.tags[i] == Tag::CallValue)
                .map(|i| b.data[i][0])
                .collect()
        };
        for l in params {
            let f = self.zonk(self.b.local_ty(l));
            let crossing = self.crossing_ty(f);
            self.b.body_mut().local_ty[l.idx()] = crossing;
            let code = self.b.place(&Place {
                root: PlaceRoot::Local(l),
                proj: vec![Proj::Field(0)],
            });
            let b = self.b.body_mut();
            for i in 0..b.len() {
                if b.tags[i] != Tag::Read || b.data[i][0] != l.raw() | place::BARE_LOCAL {
                    continue;
                }
                if callees.contains(&hd_tir::ir::u32_of(i)) {
                    b.data[i][0] = code;
                } else {
                    b.ty[i] = crossing;
                }
            }
        }
    }

    /// `std.remote.Crossing[F]`.
    fn crossing_ty(&self, f: Ty) -> Ty {
        let pool = self.pool();
        pool.applied(self.cx.names.item(REMOTE_MODULE, "Crossing"), f)
    }

    /// `List[u8]`.
    fn bytes_ty(&self) -> Ty {
        let pool = self.pool();
        pool.applied(self.cx.names.known.list, Ty::prim(hd_types::Prim::U8))
    }

    /// `Suspend[T]`.
    fn suspend_ty(&self, t: Ty) -> Ty {
        let pool = self.pool();
        pool.applied(self.cx.names.known.suspend, t)
    }

    /// A hidden local of the literal's.
    fn wire_local(&mut self, ty: Ty, name: &str, syn: NodeIdx) -> LocalId {
        let name = self.cx.names.syms.intern(name);
        self.b.local(ty, name, 0, syn)
    }

    /// `Read [$l .k]`, moving what it reads.
    fn held_field(&mut self, l: LocalId, k: u32, ty: Ty, syn: NodeIdx) -> Ref {
        let word = self.b.place(&Place {
            root: PlaceRoot::Local(l),
            proj: vec![Proj::Field(k)],
        });
        self.b.emit(Tag::Read, word, ReadMode::Use as u32, ty, syn)
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
