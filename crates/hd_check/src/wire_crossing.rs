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

/// A literal whose captures [`Ck::wire_lift`] lifted: the locals that now
/// hold them, in order, and the one holding their encoding.
pub(crate) struct WireLift {
    held: Vec<LocalId>,
    frame: LocalId,
}

impl Ck<'_, '_> {
    /// Before the literal `n`'s sub-body `m` closes, when it is written for
    /// a `Wire` crossing parameter: moves its captures into one tuple,
    /// encodes it (`crossing_frame`), and makes them `m`'s leading
    /// parameters, so `m` captures nothing. Returns the lifted type.
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
        let tuple_ty = pool.intern_ty(&TyData::Tuple {
            elems: pool.list(&tys),
            rest: None,
        });
        let params: Vec<LocalId> = (caps.iter().zip(&tys))
            .map(|(&l, &t)| {
                let name = self.b.body().local_name[l.idx()];
                self.b.local(t, name, local_flags::PARAM, syn)
            })
            .collect();
        self.b.lift_captures(m, from, &caps, &params);
        let reads: Vec<Ref> = (caps.iter().zip(&tys))
            .map(|(&l, &t)| self.b.get(l, t, syn))
            .collect();
        let rec = self.b.refs_record(&reads);
        let tuple = self.b.emit(Tag::NewTuple, NONE, rec, tuple_ty, syn);
        let bytes = self.bytes_ty();
        let pair_ty = pool.intern_ty(&TyData::Tuple {
            elems: pool.list(&[tuple_ty, bytes]),
            rest: None,
        });
        let pair = self.remote_call("crossing_frame", &[tuple_ty], &[tuple], pair_ty, syn);
        let pair_local = self.wire_local(pair_ty, "$crossing", syn);
        self.b.set(pair_local, pair, syn);
        let mut held = Vec::with_capacity(caps.len());
        for (k, (&l, &t)) in (0u32..).zip(caps.iter().zip(&tys)) {
            let name = self.b.body().local_name[l.idx()];
            let h = self.b.local(t, name, 0, syn);
            let v = self.held_field(pair_local, &[0, k], t, syn);
            self.b.set(h, v, syn);
            self.add_flags(h, local_flags::CAPTURED);
            held.push(h);
        }
        let frame = self.wire_local(bytes, "$frame", syn);
        let v = self.held_field(pair_local, &[1], bytes, syn);
        self.b.set(frame, v, syn);
        self.wire_lifted = Some(WireLift { held, frame });
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
        let all: Vec<Ty> = tys.iter().chain(pool.list_items(own)).copied().collect();
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
    ) -> Option<(Vec<LocalId>, Ref)> {
        let w = self.wire_lifted.take()?;
        let pool = self.pool();
        let syn = e.index();
        let i = lifted.as_inst()?;
        let kfn_ty = self.b.body().ty[i.0 as usize];
        let code = self.wire_local(kfn_ty, "$lifted", syn);
        self.b.set(code, lifted, syn);
        self.add_flags(code, local_flags::CAPTURED);
        // The literal: its item and node (`fn.crossing.lift`); a worker of
        // the same build reads the same constants.
        let item = self.b.body().item.raw();
        let literal = (u64::from(item) << 32) | u64::from(syn.raw());
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
        let mut args = Vec::with_capacity(w.held.len());
        for &h in &w.held {
            self.b.capture(m, h);
            args.push(self.b.get(h, self.b.local_ty(h), syn));
        }
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
        Some((w.held, value))
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
        pool.intern_ty(&TyData::Adt {
            def: self.cx.names.item(REMOTE_MODULE, "Crossing"),
            args: pool.list(&[f]),
        })
    }

    /// `List[u8]`.
    fn bytes_ty(&self) -> Ty {
        let pool = self.pool();
        pool.intern_ty(&TyData::Adt {
            def: self.cx.names.known.list,
            args: pool.list(&[Ty::prim(hd_types::Prim::U8)]),
        })
    }

    /// `Suspend[T]`.
    fn suspend_ty(&self, t: Ty) -> Ty {
        let pool = self.pool();
        pool.intern_ty(&TyData::Adt {
            def: self.cx.names.known.suspend,
            args: pool.list(&[t]),
        })
    }

    /// A hidden local of the literal's.
    fn wire_local(&mut self, ty: Ty, name: &str, syn: NodeIdx) -> LocalId {
        let name = self.cx.names.syms.intern(name);
        self.b.local(ty, name, 0, syn)
    }

    /// `Read [$l .path...]`, moving what it reads.
    fn held_field(&mut self, l: LocalId, path: &[u32], ty: Ty, syn: NodeIdx) -> Ref {
        let word = self.b.place(&Place {
            root: PlaceRoot::Local(l),
            proj: path.iter().map(|&k| Proj::Field(k)).collect(),
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
