//! References as addresses (`crates/hd_wasm_linear/DESIGN.md` §23.5, §23.20). A
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
use crate::rt::{self, Helper, LIST_ELEMS, LIST_LEN, RefLayout};
use crate::unsupported;

/// Where a place starts: an address in a local, or a value's locals.
enum Base {
    At(u32),
    Vals(Vec<u32>),
}

impl Em<'_, '_, '_> {
    /// Read just a sole field consumer at the original read's position.
    /// In particular, do not defer a view read until after its source drops.
    pub(super) fn projected_field(&self, read: u32) -> StageResult<Option<(u32, Part)>> {
        let Some(&(field, _)) = self.moves.sole.get(&read) else {
            return Ok(None);
        };
        let parent = self.ty(read);
        if self.b.tags[field as usize] != hd_tir::ir::Tag::Field
            || self.b.data[field as usize][0] != read
            || self.calls.contains_key(&field)
            || hd_mono::is_open(self.pool(), parent)
            || hd_mono::is_open(self.pool(), self.ty(field))
            || !matches!(self.kind(parent), Kind::Value)
            || self.newtype(parent)
        {
            return Ok(None);
        }
        let (_, parts) = struct_mem(self.pool(), self.env(), parent)?;
        Ok(parts
            .get(self.b.data[field as usize][1] as usize)
            .copied()
            .map(|part| (field, part)))
    }

    pub(super) fn ref_layout(&self, ty: Ty) -> StageResult<RefLayout> {
        let m = mem_of(self.pool(), self.env(), ty)?;
        let walk = self.counted(ty);
        let boxed = self.vals(ty)? != [Val::I32]
            || crate::layout::walk(self.pool(), self.env(), ty)
                != Some(crate::layout::Walk::Ref(0));
        Ok(RefLayout {
            known: crate::layout::block_start(self.pool(), self.env(), ty),
            size: m.size,
            leaves: m.leaves,
            walk,
            boxed,
        })
    }

    /// An owned representation view over a borrowed source address.
    pub(super) fn ref_view(&mut self, at: u32, source: Ty) -> StageResult<u32> {
        let layout = self.ref_layout(source)?;
        let view = self.f.local(Val::I32);
        self.f.get(at);
        self.f.helper(Helper::RefView { source: layout });
        self.f.set(view);
        Ok(view)
    }

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
        let mut owned_base = None;
        let (base, mut ty) = match p.kind {
            place_root::LOCAL => {
                let t = local_ty(self, p.x);
                if self.is_cell(p.x as usize)
                    && self.cell_converts(p.x as usize)
                    && self.ref_layout(t)?.boxed
                {
                    let at = self.cell_of(p.x as usize)?;
                    let view = self.ref_view(at, self.cell_ty(p.x as usize))?;
                    owned_base = Some(view);
                    (Base::At(view), t)
                } else if self.is_cell(p.x as usize) {
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
        let parent_ty = ty;
        let has_path = !path.is_empty();
        let (mut off, mut first) = (0, 0);
        for f in path {
            let (ft, part) = self.path_step(ty, f)?;
            off += part.offset;
            first += part.first;
            ty = ft;
        }
        let m = mem_of(self.pool(), self.env(), ty)?;
        if let Base::At(at) = &base
            && has_path
        {
            let parent = self.ref_layout(parent_ty)?;
            let field = self.ref_layout(ty)?;
            let loan = self.begin_view_loan(i)?;
            if let Some(flag) = loan {
                self.f.get(*at);
                self.f.helper(Helper::RefIsView);
                self.f.set(flag);
            }
            self.f.get(*at);
            self.f.helper(Helper::RefProject {
                borrowed: loan.is_some(),
                parent,
                field: field.clone(),
                first: u32::try_from(first).expect("field"),
                offset: off,
            });
            if fat {
                let projected = self.f.local(Val::I32);
                self.f.set(projected);
                self.load_through(projected, &field);
                self.release_open(projected);
            }
            self.store_owned(i)?;
            if let Some(view) = owned_base {
                self.release_open(view);
            }
            return Ok(());
        }
        match base {
            Base::At(at) if fat => {
                let value = self.ref_layout(ty)?;
                self.load_through(at, &value);
                self.store_owned(i)?;
                if let Some(view) = owned_base {
                    self.release_open(view);
                }
                return Ok(());
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
                let cyclic = self.cyclic_holding(&[ty]);
                let obj = self.object(&m, w.as_ref(), cyclic);
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
        if owned_base.is_some() {
            self.store_owned(i)
        } else {
            self.store(i)
        }
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
        let value = self.ref_layout(target)?;
        let at = self.f.local(Val::I32);
        self.comp(r, 0)?;
        self.f.set(at);
        if let Some((field, part)) = self.projected_field(i)? {
            self.f.get(at);
            self.f.helper(Helper::RefIsView);
            self.f.op(I::If(wasm_encoder::BlockType::Empty));
            let parent = rt::ref_load(&mut self.f, at, &value);
            for l in &parent[part.first..part.first + part.count] {
                self.f.get(*l);
            }
            self.store(field)?;
            self.release_locals(&parent, target)?;
            self.f.op(I::Else);
            for leaf in &value.leaves[part.first..part.first + part.count] {
                self.f.get(at);
                self.f.op(leaf.load(0));
            }
            self.store(field)?;
            self.f.op(I::End);
            self.precomputed_fields.insert(field);
            return Ok(());
        }
        if let Some(flag) = self.begin_view_loan(i)? {
            for l in rt::ref_load_borrowed(&mut self.f, at, &value, flag) {
                self.f.get(l);
            }
        } else {
            self.load_through(at, &value);
        }
        self.store_owned(i)
    }

    /// Pushes the value that the reference in local `at` names, with
    /// counts of its own ([`rt::ref_load`]).
    fn load_through(&mut self, at: u32, value: &RefLayout) {
        for l in rt::ref_load(&mut self.f, at, value) {
            self.f.get(l);
        }
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
        let value = self.ref_layout(target)?;
        let at = self.f.local(Val::I32);
        self.comp(r, 0)?;
        self.f.set(at);
        let ls: Vec<u32> = value.leaves.iter().map(|l| self.f.local(l.val)).collect();
        self.push(v)?;
        for l in ls.iter().rev() {
            self.f.set(*l);
        }
        rt::ref_store(&mut self.f, at, &ls, &value);
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
        self.f.helper(Helper::RefAddress);
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
