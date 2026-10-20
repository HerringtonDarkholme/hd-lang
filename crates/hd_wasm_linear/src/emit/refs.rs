//! References as addresses (`crates/hd_wasm/DESIGN.md` §23.5, §23.20). A
//! reference is the address of its place: a local's cell, a top-level
//! binding's static slot, a field inside either or inside what another
//! reference refers to, a data object's field, or a list element in its
//! buffer. Reading or writing through it needs only the address; a count
//! through it reaches the object that holds the place through the page
//! table, and a place in static data has none. A reference to a trait
//! value is that trait value, whose data is already one address, as Rust's
//! `&dyn Tr` is a pointer and a vtable. The checker's `RefPlace`,
//! `RefRead`, `RefWrite`, `RefDyn` and `RefIdentity` reach this backend
//! without their closure forms (`hd_tir::ir::strip_closure_forms`).

use hd_base::{DefId, StageResult};
use hd_tir::ir::{NONE, place_root};
use hd_types::{Ty, TyData};
use wasm_encoder::Instruction as I;

use super::Em;
use super::value::Src;
use crate::layout::{Kind, Part, mem as mem_of, payload_mem, struct_mem};
use crate::module::{EMPTY, HEAP, Val, mem};
use crate::rt::{LIST_ELEMS, LIST_LEN};
use crate::unsupported;

/// Where a place starts: an address in a local, or a value's locals.
enum Base {
    At(u32),
    Vals(Vec<u32>),
}

impl Em<'_, '_, '_> {
    /// The type reference type `t` refers to, and whether the reference
    /// is fat: a trait value.
    pub(super) fn ref_target(&self, t: Ty) -> Option<(Ty, bool)> {
        match self.kind(t) {
            Kind::Ref { target, fat } => Some((target, fat)),
            _ => None,
        }
    }

    /// Fresh locals holding TIR local `l`'s value, loaded from its cell
    /// when it lives in one; a read with no count of its own.
    pub(super) fn local_value(&mut self, l: u32) -> StageResult<Vec<u32>> {
        if !self.is_cell(l as usize) {
            return self.local(l);
        }
        self.cell_read(l as usize)
    }

    /// A local holding the address in local `at` plus `off`.
    fn offset_local(&mut self, at: u32, off: u32) -> u32 {
        if off == 0 {
            return at;
        }
        let x = self.f.local(Val::I32);
        self.f.get(at);
        self.f.u32(off);
        self.f.op(I::I32Add);
        self.f.set(x);
        x
    }

    /// The type of field `f` of struct type `t`, and where it lies: its
    /// part of `t`'s values and memory. A newtype's one field is the
    /// value itself.
    fn path_step(&self, t: Ty, f: u32) -> StageResult<(Ty, Part)> {
        if self.newtype(t) {
            let TyData::Adt { def, args } = self.pool().get(self.strip_mut(t)) else {
                return unsupported("a newtype that is no nominal type");
            };
            let Some(&base) = self.env().data_fields(def).unwrap_or_default().first() else {
                return unsupported("a newtype without its field");
            };
            let ft = hd_mono::subst(self.pool(), self.env(), def, args, base);
            let n = self.vals(ft)?.len();
            return Ok((
                ft,
                Part {
                    first: 0,
                    count: n,
                    offset: 0,
                },
            ));
        }
        let (_, parts) = struct_mem(self.pool(), self.env(), t)?;
        let (Some(part), Some(&ft)) = (
            parts.get(f as usize).copied(),
            self.struct_fields(t).get(f as usize),
        ) else {
            return unsupported("a reference to a field the struct does not have");
        };
        Ok((ft, part))
    }

    /// `RefPlace` `i`: the place's address, or the trait value there for
    /// a reference to a trait value, with a count of its own.
    pub(super) fn ref_place(&mut self, i: u32) -> StageResult<()> {
        let Some(p) = self.b.place(i as usize) else {
            return unsupported("a reference without its place");
        };
        let Some((_, fat)) = self.ref_target(self.ty(i)) else {
            return unsupported("a reference whose type is no reference");
        };
        let path = p.path.to_vec();
        let local_ty = |em: &Self, l: u32| em.sub(em.b.local_ty[l as usize]);
        let (base, mut ty) = match p.kind {
            place_root::LOCAL => {
                let t = local_ty(self, p.x);
                // A cell that boxes its value for an erased body holds no
                // value of the reference's type (`erased.rs`).
                if self.is_cell(p.x as usize) && self.cell_converts(p.x as usize) {
                    return unsupported(
                        "a reference to a local that closures share in a generic method",
                    );
                }
                if self.is_cell(p.x as usize) {
                    (Base::At(self.cell_of(p.x as usize)?), t)
                } else {
                    (Base::Vals(self.local(p.x)?), t)
                }
            }
            place_root::GLOBAL => {
                let h = self.env().path_hash(DefId::from_raw(p.x));
                let Some(&at) = self.m.addressed.get(&h) else {
                    return unsupported("a top-level binding without its static slot");
                };
                let x = self.f.local(Val::I32);
                self.f.u32(at);
                self.f.set(x);
                (Base::At(x), self.sub(Ty(p.y)))
            }
            place_root::DEREF => {
                let Some((target, f)) = self.ref_target(local_ty(self, p.x)) else {
                    return unsupported("a place through a value that is no reference");
                };
                let ls = self.local_value(p.x)?;
                (if f { Base::Vals(ls) } else { Base::At(ls[0]) }, target)
            }
            place_root::DATA_FIELD => {
                let Kind::Data(tys) = self.kind(local_ty(self, p.x)) else {
                    return unsupported("a data field of a value that is no data value");
                };
                let (_, parts) = payload_mem(self.pool(), self.env(), &tys)?;
                let (Some(part), Some(&ft)) = (parts.get(p.y as usize), tys.get(p.y as usize))
                else {
                    return unsupported("a data field the data type does not have");
                };
                let off = part.offset;
                let o = self.local_value(p.x)?[0];
                (Base::At(self.offset_local(o, off)), ft)
            }
            place_root::ELEM => {
                let (et, e) = self.elem(local_ty(self, p.x))?;
                let h = self.local_value(p.x)?[0];
                let ix = self.local_value(p.y)?[0];
                let at = self.f.local(Val::I32);
                self.f.get(h);
                self.f.op(I::I32Load(mem(HEAP, 0, 2)));
                // An element of no bytes lies at no address of its own;
                // the length word keeps the address inside the buffer.
                if e.size == 0 {
                    self.f.u32(u32::try_from(LIST_LEN).expect("offset"));
                } else {
                    self.f.get(ix);
                    self.f.u32(e.size);
                    self.f.op(I::I32Mul);
                    self.f.op(I::I32Add);
                    self.f.u32(LIST_ELEMS);
                }
                self.f.op(I::I32Add);
                self.f.set(at);
                (Base::At(at), et)
            }
            _ => return unsupported("a place of an unknown kind"),
        };
        let (mut off, mut first) = (0, 0);
        for f in path {
            let (ft, part) = self.path_step(ty, f)?;
            off += part.offset;
            first += part.first;
            ty = ft;
        }
        let m = mem_of(self.pool(), self.env(), ty)?;
        match base {
            Base::At(at) if fat => {
                for leaf in &m.leaves {
                    self.f.get(at);
                    self.f.op(leaf.load(off));
                }
            }
            Base::At(at) => {
                self.f.get(at);
                if off != 0 {
                    self.f.u32(off);
                    self.f.op(I::I32Add);
                }
            }
            Base::Vals(ls) if fat => {
                for l in &ls[first..first + m.leaves.len()] {
                    self.f.get(*l);
                }
            }
            Base::Vals(_) if crate::layout::ref_mutable(self.pool(), self.env(), self.ty(i)) => {
                return unsupported("a `&mut` to a local that lives in no cell");
            }
            Base::Vals(ls) => {
                // A local no cell holds, as a closure's copy of what it
                // captured, which never changes: the reference is to a new
                // object holding a copy (`ref.expr.allocate`).
                let w = self.counted(ty);
                let obj = self.object(&m, w.as_ref());
                let src = Src::Locals(ls[first..first + m.leaves.len()].to_vec());
                let all = Part {
                    first: 0,
                    count: m.leaves.len(),
                    offset: 0,
                };
                self.store_part(obj, &m, all, &src)?;
                self.retain_fields(obj, &m, w.as_ref());
                self.f.get(obj);
                return self.store_owned(i);
            }
        }
        self.store(i)
    }

    /// `RefRead` `i` of reference `r`: the value at its address, a copy
    /// with its own count; a reference to a trait value is that value.
    pub(super) fn deref_read(&mut self, i: u32, r: u32) -> StageResult<()> {
        let Some((target, fat)) = self.ref_target(self.ty_of(r)) else {
            return unsupported("a read through a value that is no reference");
        };
        if fat {
            self.push(r)?;
            return self.store(i);
        }
        let m = mem_of(self.pool(), self.env(), target)?;
        for leaf in &m.leaves {
            self.comp(r, 0)?;
            self.f.op(leaf.load(0));
        }
        self.store(i)
    }

    /// `RefWrite` of `v` through reference `r`: the place takes a count
    /// of the new value before the old one's is released.
    pub(super) fn deref_write(&mut self, r: u32, v: u32) -> StageResult<()> {
        if v != NONE && self.diverges(v) {
            self.f.op(I::Unreachable);
            return Ok(());
        }
        let Some((target, false)) = self.ref_target(self.ty_of(r)) else {
            return unsupported("a write of a whole value through a trait value reference");
        };
        let m = mem_of(self.pool(), self.env(), target)?;
        let counted = self.counted(target).is_some();
        let at = self.f.local(Val::I32);
        self.comp(r, 0)?;
        self.f.set(at);
        let old: Vec<u32> = m.leaves.iter().map(|l| self.f.local(l.val)).collect();
        if counted && !self.take(v) {
            self.retain_ref(v)?;
        }
        if counted {
            for (l, o) in m.leaves.iter().zip(&old) {
                self.f.get(at);
                self.f.op(l.load(0));
                self.f.set(*o);
            }
        }
        let all = Part {
            first: 0,
            count: m.leaves.len(),
            offset: 0,
        };
        self.store_part(at, &m, all, &Src::Ref(v))?;
        if counted {
            self.release_locals(&old, target)?;
        }
        Ok(())
    }

    /// `RefIdentity` `i` of reference `r` (std's `RefIdentity`): no base
    /// object, the address as the serial and no key, so two references
    /// share an identity exactly when they share an address.
    pub(super) fn ref_identity(&mut self, i: u32, r: u32) -> StageResult<()> {
        if self.vals(self.ty(i))? != [Val::I32, Val::I32, Val::I64, Val::I32, Val::I32] {
            return unsupported("a reference identity of an unexpected layout");
        }
        self.f.i32(0);
        self.f.i32(0);
        self.comp(r, 0)?;
        self.f.op(I::I64ExtendI32U);
        self.f.u32(EMPTY);
        self.f.i32(0);
        self.store(i)
    }

    /// The static slot of the top-level binding a `GlobalGet` or
    /// `GlobalSet` names, when some reference refers to it.
    pub(super) fn global_slot(&self, a: u32) -> Option<u32> {
        let def = DefId::from_raw(*self.b.record(a).first()?);
        self.m.addressed.get(&self.env().path_hash(def)).copied()
    }

    /// `GlobalGet` `i` of a binding in static slot `at`.
    pub(super) fn slot_get(&mut self, i: u32, at: u32) -> StageResult<()> {
        let m = mem_of(self.pool(), self.env(), self.ty(i))?;
        for leaf in &m.leaves {
            self.f.u32(at);
            self.f.op(leaf.load(0));
        }
        self.store(i)
    }

    /// `GlobalSet` `g` of `v` into a binding in static slot `at`.
    pub(super) fn slot_set(&mut self, g: u32, at: u32, v: u32) -> StageResult<()> {
        if v != NONE && self.diverges(v) {
            self.f.op(I::Unreachable);
            return Ok(());
        }
        let t = self.ty_of(v);
        let m = mem_of(self.pool(), self.env(), t)?;
        let counted = self.counted(t).is_some();
        if counted {
            let h = self.global_hash(g);
            self.note_stored(h, t, Some((at, m.leaves.clone())))?;
        }
        let a = self.f.local(Val::I32);
        self.f.u32(at);
        self.f.set(a);
        let old: Vec<u32> = m.leaves.iter().map(|l| self.f.local(l.val)).collect();
        if counted && !self.take(v) {
            self.retain_ref(v)?;
        }
        if counted {
            for (l, o) in m.leaves.iter().zip(&old) {
                self.f.get(a);
                self.f.op(l.load(0));
                self.f.set(*o);
            }
        }
        let all = Part {
            first: 0,
            count: m.leaves.len(),
            offset: 0,
        };
        self.store_part(a, &m, all, &Src::Ref(v))?;
        if counted {
            self.release_locals(&old, t)?;
        }
        Ok(())
    }
}
