//! References as addresses (`crates/hd_wasm_linear/DESIGN.md` §23.5, §23.20). A
//! reference is the address of its place: a local's cell, a top-level
//! binding's static slot, a field inside either or inside what another
//! reference refers to, a data object's field, or a list element in its
//! buffer. Reading or writing through it needs only the address; a count
//! through it reaches the object that holds the place through the page
//! table, and a place in static data has none. A reference to a trait
//! value is that trait value, whose data is already one address, as Rust's
//! `&dyn Tr` is a pointer and a vtable.
//!
//! One resolver serves every place (`docs/compiler/tir-v2.md`, "The
//! emitter after each stage"): the places of `Ref`, `Read` and `Store`
//! (`hd_tir::ir::Place`), and those the older `RefPlace`, `RefRead` and
//! `RefWrite` name, which reach this backend without their closure forms
//! (`hd_tir::ir::strip_closure_forms`). Each is read as a [`Spot`].

use hd_base::{DefId, StageResult};
use hd_tir::ir::{NONE, Place, PlaceRoot, Proj, Tag, place_root};
use hd_types::{Ty, TyData};
use wasm_encoder::Instruction as I;

use super::Em;
use super::value::Src;
use crate::layout::{Kind, Leaf, Part, mem as mem_of, payload_mem, struct_mem};
use crate::module::{EMPTY, HEAP, Sym, Val, mem};
use crate::rt::{self, Helper, LIST_ELEMS, LIST_LEN, RefLayout};
use crate::unsupported;

/// Where a place starts: an address in a local, or a value's locals.
enum Base {
    At(u32),
    Vals(Vec<u32>),
}

/// Where a place's root is ([`hd_tir::ir::PlaceRoot`]): a TIR local, a
/// top-level binding with its type, or an instruction's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Start {
    Local(u32),
    Global(DefId, Ty),
    Value(u32),
}

/// What a place projects from its root before its struct fields, as
/// stage 1 lowers it: nothing, a dereference of a reference, a field of a
/// data object (a dereference of the object, then the field), or a list
/// element at an index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Head {
    Direct,
    Deref,
    Data(u32),
    Elem(Index),
}

/// Where an element's index is: a TIR local (`RefPlace`) or a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Index {
    Local(u32),
    Value(u32),
}

/// A place as the resolver reads it: its root, its head, then the struct
/// field indices read from there in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Spot {
    pub start: Start,
    pub head: Head,
    pub path: Vec<u32>,
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

    /// The place of `Ref`, `Read`, `Store`, `RefPlace`, `RefRead` or
    /// `RefWrite` instruction `i`, as the resolver reads it. A `RefRead`
    /// or `RefWrite` reads or writes through its reference value.
    pub(super) fn spot(&self, i: u32) -> StageResult<Spot> {
        let a = self.b.data[i as usize][0];
        match self.b.tags[i as usize] {
            Tag::RefPlace => {
                let Some(p) = self.b.place(i as usize) else {
                    return unsupported("a reference without its place");
                };
                let (start, head) = match p.kind {
                    place_root::LOCAL => (Start::Local(p.x), Head::Direct),
                    place_root::GLOBAL => {
                        (Start::Global(DefId::from_raw(p.x), Ty(p.y)), Head::Direct)
                    }
                    place_root::DEREF => (Start::Local(p.x), Head::Deref),
                    place_root::DATA_FIELD => (Start::Local(p.x), Head::Data(p.y)),
                    place_root::ELEM => (Start::Local(p.x), Head::Elem(Index::Local(p.y))),
                    _ => return unsupported("a place of an unknown kind"),
                };
                Ok(Spot {
                    start,
                    head,
                    path: p.path.to_vec(),
                })
            }
            Tag::RefRead => Ok(Spot {
                start: Start::Value(a),
                head: Head::Deref,
                path: vec![],
            }),
            Tag::RefWrite => match self.b.record(a) {
                [r, _] => Ok(Spot {
                    start: Start::Value(*r),
                    head: Head::Deref,
                    path: vec![],
                }),
                _ => unsupported("a write through a reference without its operands"),
            },
            Tag::Ref | Tag::Read | Tag::Store => match self.b.decode_place(a) {
                Some(p) => self.spot_of(&p),
                None => unsupported("a malformed place"),
            },
            _ => unsupported("a place of an instruction that has none"),
        }
    }

    /// Place `p` as a [`Spot`]. A dereference of a data value followed by
    /// a field is that field of the data object; stage 1 lowers no
    /// dereference, element or variant after a struct field.
    fn spot_of(&self, p: &Place) -> StageResult<Spot> {
        let start = match p.root {
            PlaceRoot::Local(l) => Start::Local(l.raw()),
            PlaceRoot::Global(d, t) => Start::Global(d, t),
            PlaceRoot::Value(r) => Start::Value(r.0),
            PlaceRoot::Frame(_) => return unsupported("a frame slot, which only tier 2 has"),
        };
        let data = matches!(self.kind(self.start_ty(start)), Kind::Data(_));
        let (head, rest) = match p.proj.as_slice() {
            [Proj::Deref, Proj::Field(k), rest @ ..] if data => (Head::Data(*k), rest),
            [Proj::Deref, rest @ ..] => (Head::Deref, rest),
            [Proj::Elem(r), rest @ ..] => (Head::Elem(Index::Value(r.0)), rest),
            rest => (Head::Direct, rest),
        };
        let mut path = Vec::with_capacity(rest.len());
        for step in rest {
            let Proj::Field(k) = step else {
                return unsupported(
                    "a place that dereferences, indexes or matches after a field, which stage 1 does not lower",
                );
            };
            path.push(*k);
        }
        Ok(Spot { start, head, path })
    }

    /// The type of the value at a place's root.
    pub(super) fn start_ty(&self, s: Start) -> Ty {
        match s {
            Start::Local(l) => self.sub(self.b.local_ty[l as usize]),
            Start::Global(_, t) => self.sub(t),
            Start::Value(r) => self.ty_of(r),
        }
    }

    /// Locals holding the value at a place's root, a read with no count
    /// of its own: a local's (loaded from its cell when it has one) or an
    /// instruction's. A top-level binding's is no value of the body.
    fn start_values(&mut self, s: Start) -> StageResult<Vec<u32>> {
        match s {
            Start::Local(l) => self.local_value(l),
            Start::Value(r) => self.local_value_of(r),
            Start::Global(..) => unsupported("a top-level binding as a place's value"),
        }
    }

    /// Pushes the value at a place's root, or only its value `k`.
    pub(super) fn push_start(&mut self, s: Start, k: Option<usize>) -> StageResult<()> {
        if let Start::Value(r) = s {
            return match k {
                Some(k) => self.comp(r, k),
                None => self.push(r),
            };
        }
        let ls = self.start_values(s)?;
        match k {
            Some(k) => match ls.get(k) {
                Some(l) => self.f.get(*l),
                None => return unsupported("a component a value does not have"),
            },
            None => {
                for l in ls {
                    self.f.get(l);
                }
            }
        }
        Ok(())
    }

    /// The type a struct field path reads from type `t`, and where it lies
    /// in `t`'s values and memory ([`Em::path_step`], folded). No path is
    /// all of `t`.
    fn fold_path(&self, t: Ty, path: &[u32]) -> StageResult<(Ty, Part)> {
        let (mut ty, mut first, mut offset, mut count) = (t, 0, 0, None);
        for &f in path {
            let (ft, part) = self.path_step(ty, f)?;
            first += part.first;
            offset += part.offset;
            count = Some(part.count);
            ty = ft;
        }
        let count = match count {
            Some(n) => n,
            None => mem_of(self.pool(), self.env(), t)?.leaves.len(),
        };
        Ok((
            ty,
            Part {
                first,
                count,
                offset,
            },
        ))
    }

    /// `Ref` or `RefPlace` `i`: the place's address, or the trait value
    /// there for a reference to a trait value, with a count of its own.
    pub(super) fn place_ref(&mut self, i: u32) -> StageResult<()> {
        let s = self.spot(i)?;
        let Some((_, fat)) = self.ref_target(self.ty(i)) else {
            return unsupported("a reference whose type is no reference");
        };
        let path = s.path.clone();
        let mut owned_base = None;
        let (base, mut ty) = match (s.start, s.head) {
            (Start::Local(x), Head::Direct) => {
                let t = self.start_ty(s.start);
                if self.is_cell(x as usize)
                    && self.cell_converts(x as usize)
                    && self.ref_layout(t)?.boxed
                {
                    let at = self.cell_of(x as usize)?;
                    let view = self.ref_view(at, self.cell_ty(x as usize))?;
                    owned_base = Some(view);
                    (Base::At(view), t)
                } else if self.is_cell(x as usize) {
                    (Base::At(self.cell_of(x as usize)?), t)
                } else {
                    (Base::Vals(self.local(x)?), t)
                }
            }
            (Start::Global(def, t), Head::Direct) => {
                let h = self.env().path_hash(def);
                let Some(&at) = self.m.addressed.get(&h) else {
                    return unsupported("a top-level binding without its static slot");
                };
                let x = self.f.local(Val::I32);
                self.f.static_addr(at);
                self.f.set(x);
                (Base::At(x), self.sub(t))
            }
            (Start::Value(r), Head::Direct) => (Base::Vals(self.local_value_of(r)?), self.ty_of(r)),
            (st, Head::Deref) => {
                let Some((target, f)) = self.ref_target(self.start_ty(st)) else {
                    return unsupported("a place through a value that is no reference");
                };
                let ls = self.start_values(st)?;
                (if f { Base::Vals(ls) } else { Base::At(ls[0]) }, target)
            }
            (st, Head::Data(k)) => {
                let Kind::Data(tys) = self.kind(self.start_ty(st)) else {
                    return unsupported("a data field of a value that is no data value");
                };
                let (_, parts) = payload_mem(self.pool(), self.env(), &tys)?;
                let (Some(part), Some(&ft)) = (parts.get(k as usize), tys.get(k as usize)) else {
                    return unsupported("a data field the data type does not have");
                };
                let off = part.offset;
                let o = self.start_values(st)?[0];
                (Base::At(self.offset_local(o, off)), ft)
            }
            (st, Head::Elem(index)) => {
                let (et, e) = self.elem(self.start_ty(st))?;
                let h = self.start_values(st)?[0];
                let ix = match index {
                    Index::Local(y) => Some(self.local_value(y)?[0]),
                    Index::Value(_) => None,
                };
                let at = self.f.local(Val::I32);
                self.f.get(h);
                self.f.op(I::I32Load(mem(HEAP, 0, 2)));
                // An element of no bytes lies at no address of its own;
                // the length word keeps the address inside the buffer.
                if e.size == 0 {
                    self.f.u32(u32::try_from(LIST_LEN).expect("offset"));
                } else {
                    match (ix, index) {
                        (Some(l), _) => self.f.get(l),
                        (None, Index::Value(v)) => self.comp(v, 0)?,
                        (None, Index::Local(_)) => {
                            return unsupported("an element index without its local");
                        }
                    }
                    self.f.u32(e.size);
                    self.f.op(I::I32Mul);
                    self.f.op(I::I32Add);
                    self.f.u32(LIST_ELEMS);
                }
                self.f.op(I::I32Add);
                self.f.set(at);
                (Base::At(at), et)
            }
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
            // Physical places project by an offset. Only a representation
            // view needs a forwarding object and its helper call.
            if let Some(flag) = loan {
                self.f.get(flag);
            } else {
                self.f.get(*at);
                self.f.helper(Helper::RefIsView);
            }
            self.f
                .op(I::If(wasm_encoder::BlockType::Result(Val::I32.wasm())));
            self.f.get(*at);
            self.f.helper(Helper::RefProject {
                borrowed: loan.is_some(),
                parent,
                field: field.clone(),
                first: u32::try_from(first).expect("field"),
                offset: off,
            });
            self.f.op(I::Else);
            self.f.get(*at);
            if off != 0 {
                self.f.u32(off);
                self.f.op(I::I32Add);
            }
            if loan.is_none()
                && let Some(runtime) = self.m.runtime
            {
                let physical = self.f.local(Val::I32);
                self.f.set(physical);
                self.f.get(physical);
                self.f.call(Sym::Inst(runtime.retain));
                self.f.get(physical);
            }
            self.f.op(I::End);
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
            return self.store_owned(i);
        }
        // `&x` at the last access of cell local `x` takes the local's count
        // of its cell ([`Em::take_cell`]).
        if let (Start::Local(x), Head::Direct) = (s.start, s.head)
            && !has_path
            && self.take_cell(i, x as usize)
        {
            self.store_owned(i)?;
            let c = self.local(x)?[0];
            self.f.zero(c);
            return Ok(());
        }
        self.store(i)
    }

    /// `Read` or `RefRead` `i`: the value at a place through a reference,
    /// a copy with its own count; a reference to a trait value is that
    /// value. Stage 1 reads a place only through a dereference; a field
    /// path after it reads only that field.
    pub(super) fn place_read(&mut self, i: u32) -> StageResult<()> {
        let s = self.spot(i)?;
        if s.head != Head::Deref {
            return unsupported(
                "a read of a place with no dereference, which stage 1 reads with `LocalGet` and `Field`",
            );
        }
        let Some((target, fat)) = self.ref_target(self.start_ty(s.start)) else {
            return unsupported("a read through a value that is no reference");
        };
        if fat {
            if !s.path.is_empty() {
                return unsupported("a field read through a trait value reference");
            }
            if self.moves.unused_reads.contains(&i) {
                return Ok(());
            }
            self.push_start(s.start, None)?;
            return self.store(i);
        }
        let value = self.ref_layout(target)?;
        let at = self.f.local(Val::I32);
        self.push_start(s.start, Some(0))?;
        self.f.set(at);
        if self.moves.unused_reads.contains(&i) {
            // The source has already been evaluated. A discarded load of
            // a physical place need not copy/count its contents. A view
            // must keep its getter and the release of that owned result,
            // at this original read position, including in loops.
            self.f.get(at);
            self.f.helper(Helper::RefIsView);
            self.f.op(I::If(wasm_encoder::BlockType::Empty));
            let ls = rt::ref_load(&mut self.f, at, &value);
            self.release_locals(&ls, target)?;
            self.f.op(I::End);
            return Ok(());
        }
        if !s.path.is_empty() {
            let (_, part) = self.fold_path(target, &s.path)?;
            return self.read_part(i, at, &value, target, part);
        }
        if let Some((field, part)) = self.projected_field(i)? {
            self.read_part(field, at, &value, target, part)?;
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

    /// Reads only `part` of the `target` value that the reference in
    /// local `at` names, into instruction `dest`'s value: a view's getter
    /// makes the whole value, of which `dest` keeps that part; a physical
    /// place loads only its leaves.
    fn read_part(
        &mut self,
        dest: u32,
        at: u32,
        value: &RefLayout,
        target: Ty,
        part: Part,
    ) -> StageResult<()> {
        // A borrowed field owns a count only on a view's branch.
        let flag = self.begin_view_loan(dest)?;
        self.f.get(at);
        self.f.helper(Helper::RefIsView);
        self.f.op(I::If(wasm_encoder::BlockType::Empty));
        if value.boxed {
            // A view's getter owns a box in this native layout. Keep
            // only the selected field before releasing that box;
            // unboxing the whole parent here grows with every read.
            let boxed = self.f.local(Val::I32);
            self.f.get(at);
            self.f.get(at);
            self.f.op(I::I32Load(mem(HEAP, 4, 2)));
            self.f.call_indirect(vec![Val::I32], vec![Val::I32]);
            self.f.set(boxed);
            for leaf in &value.leaves[part.first..part.first + part.count] {
                self.f.get(boxed);
                self.f.op(leaf.load(0));
            }
            self.store(dest)?;
            self.own_view_field(dest, flag)?;
            self.f.get(boxed);
            self.f.helper(Helper::ReleaseRef);
        } else {
            let parent = rt::ref_load(&mut self.f, at, value);
            for l in &parent[part.first..part.first + part.count] {
                self.f.get(*l);
            }
            self.store(dest)?;
            self.own_view_field(dest, flag)?;
            self.release_locals(&parent, target)?;
        }
        self.f.op(I::Else);
        for leaf in &value.leaves[part.first..part.first + part.count] {
            self.f.get(at);
            self.f.op(leaf.load(0));
        }
        self.store(dest)?;
        self.f.op(I::End);
        Ok(())
    }

    /// A borrowed field read through a view ([`Em::read_part`]) came
    /// from the getter's fresh value, not the caller's place: it takes a
    /// count of its own, and `flag` says its drops release it.
    fn own_view_field(&mut self, field: u32, flag: Option<u32>) -> StageResult<()> {
        let Some(flag) = flag else {
            return Ok(());
        };
        let ls = self.slot(field)?;
        self.retain_locals(&ls, self.ty(field))?;
        self.f.i32(1);
        self.f.set(flag);
        Ok(())
    }

    /// Pushes the value that the reference in local `at` names, with
    /// counts of its own ([`rt::ref_load`]).
    fn load_through(&mut self, at: u32, value: &RefLayout) {
        for l in rt::ref_load(&mut self.f, at, value) {
            self.f.get(l);
        }
    }

    /// `Store` or `RefWrite` `i`: replaces the value at a place, which
    /// takes a count of the new value before the old one's is released.
    /// Stage 1 lowers a store through a dereference, into a data
    /// object's field or a struct field in it, into a local or top-level
    /// binding, whole or a struct field of it, and into a list value's
    /// element or a struct field of it ([`Em::elem_store`]).
    pub(super) fn place_store(&mut self, i: u32) -> StageResult<()> {
        let s = self.spot(i)?;
        let [a, w] = self.b.data[i as usize];
        let v = match self.b.tags[i as usize] {
            Tag::RefWrite => self.b.record(a).get(1).copied().unwrap_or(NONE),
            _ => w,
        };
        match (s.start, s.head, s.path.is_empty()) {
            (_, Head::Deref, true) => self.deref_write(s.start, v),
            (_, Head::Deref, false) => self.deref_field_write(s.start, &s.path, v),
            (_, Head::Data(k), true) => self.field_set(s.start, k, v),
            (_, Head::Data(k), false) => self.data_field_store(s.start, k, &s.path, v),
            (Start::Local(l), Head::Direct, true) => self.local_set(i, l, v),
            (Start::Local(l), Head::Direct, false) => self.local_field_store(l, &s.path, v),
            (Start::Global(def, _), Head::Direct, true) => self.global_set(def, v),
            (Start::Global(def, t), Head::Direct, false) => {
                self.global_field_store(def, t, &s.path, v)
            }
            (Start::Value(list), Head::Elem(Index::Value(ix)), _) => {
                self.elem_store(list, ix, &s.path, v)
            }
            _ => unsupported("a store into a place that is not writable"),
        }
    }

    /// A write of `v` into the struct field `path` reaches in TIR local
    /// `l`: only that field's values change, in the local's Wasm locals or
    /// in its cell.
    fn local_field_store(&mut self, l: u32, path: &[u32], v: u32) -> StageResult<()> {
        if v != NONE && self.diverges(v) {
            self.f.op(I::Unreachable);
            return Ok(());
        }
        let t = self.start_ty(Start::Local(l));
        let (ft, part) = self.fold_path(t, path)?;
        if self.is_cell(l as usize) {
            if self.cell_converts(l as usize) {
                // The cell holds the erased view: its value, copied out,
                // changed and stored back whole.
                let ls = self.cell_read(l as usize)?;
                self.retain_locals(&ls, t)?;
                self.part_store(&ls[part.first..part.first + part.count], ft, v)?;
                let src = self.cell_src(l as usize, Src::Locals(ls))?;
                return self.cell_put(l as usize, &src);
            }
            let m = mem_of(self.pool(), self.env(), t)?;
            let c = self.cell_of(l as usize)?;
            let leaves = m.leaves[part.first..part.first + part.count].to_vec();
            return self.store_leaves(c, &leaves, ft, v);
        }
        let dst = self.local(l)?[part.first..part.first + part.count].to_vec();
        self.part_store(&dst, ft, v)
    }

    /// Stores `v`, of type `ft`, in the Wasm locals `dst` that hold a
    /// value's part: they take a count of the new value (a copy of each
    /// collection a place holds) before the old one's is released.
    fn part_store(&mut self, dst: &[u32], ft: Ty, v: u32) -> StageResult<()> {
        let s = self.slot_src(v, ft, false)?;
        if self.counted(ft).is_some() {
            self.retain_src(&s, ft)?;
            self.release_locals(dst, ft)?;
        }
        for k in 0..dst.len() {
            self.src(&s, k)?;
        }
        for x in dst.iter().rev() {
            self.f.set(*x);
        }
        Ok(())
    }

    /// Outlined `Store` `i` into a struct field of TIR local `l` in an
    /// erased body (`hd_mono::outlined_store_local`): its thunk `k`
    /// returns the local's new value, with a count of its own, which
    /// replaces the old one whole.
    pub(super) fn open_local_store(&mut self, i: u32, k: u32, l: u32) -> StageResult<()> {
        self.open_call(i, k)?;
        let t = self.sub(self.b.local_ty[l as usize]);
        let new: Vec<u32> = self.vals(t)?.into_iter().map(|x| self.f.local(x)).collect();
        for x in new.iter().rev() {
            self.f.set(*x);
        }
        if self.is_cell(l as usize) {
            let src = self.cell_src(l as usize, Src::Locals(new))?;
            return self.cell_put(l as usize, &src);
        }
        let ls = self.local(l)?;
        if self.counted(t).is_some() {
            self.release_locals(&ls, t)?;
        }
        for (x, n) in ls.iter().zip(&new) {
            self.f.get(*n);
            self.f.set(*x);
        }
        Ok(())
    }

    /// A write of `v` into the struct field `path` reaches in top-level
    /// binding `def` of type `t`: in its static slot when some reference
    /// refers to it, else in its Wasm globals.
    fn global_field_store(&mut self, def: DefId, t: Ty, path: &[u32], v: u32) -> StageResult<()> {
        if v != NONE && self.diverges(v) {
            self.f.op(I::Unreachable);
            return Ok(());
        }
        let t = self.sub(t);
        let (ft, part) = self.fold_path(t, path)?;
        let h = self.def_hash(def);
        let m = mem_of(self.pool(), self.env(), t)?;
        let leaves = m.leaves[part.first..part.first + part.count].to_vec();
        if let Some(at) = self.def_slot(def) {
            self.note_stored(h, t, Some((at, m.leaves.clone())))?;
            let a = self.f.local(Val::I32);
            self.f.static_addr(at);
            self.f.set(a);
            return self.store_leaves(a, &leaves, ft, v);
        }
        self.note_stored(h, t, None)?;
        let vs = self.vals(t)?;
        let global =
            |k: usize| crate::module::Global::Storage(h, u32::try_from(k).expect("k"), vs[k]);
        let s = self.slot_src(v, ft, false)?;
        if self.counted(ft).is_some() {
            self.retain_src(&s, ft)?;
            let old: Vec<u32> = (part.first..part.first + part.count)
                .map(|k| {
                    let o = self.f.local(vs[k]);
                    self.f.global_get(global(k));
                    self.f.set(o);
                    o
                })
                .collect();
            self.release_locals(&old, ft)?;
        }
        for k in 0..part.count {
            self.src(&s, k)?;
        }
        for k in (part.first..part.first + part.count).rev() {
            self.f.global_set(global(k));
        }
        Ok(())
    }

    /// A write of `v` into the struct field `path` reaches in field `k` of
    /// the data object at `obj`.
    fn data_field_store(&mut self, obj: Start, k: u32, path: &[u32], v: u32) -> StageResult<()> {
        if v != NONE && self.diverges(v) {
            self.f.op(I::Unreachable);
            return Ok(());
        }
        let Kind::Data(tys) = self.kind(self.start_ty(obj)) else {
            return unsupported("a data field of a value that is no data value");
        };
        let (m, parts) = payload_mem(self.pool(), self.env(), &tys)?;
        let (Some(pk), Some(&fk)) = (parts.get(k as usize).copied(), tys.get(k as usize)) else {
            return unsupported("a data field the data type does not have");
        };
        let (ft, part) = self.fold_path(fk, path)?;
        let first = pk.first + part.first;
        let leaves = m.leaves[first..first + part.count].to_vec();
        let at = self.f.local(Val::I32);
        self.push_start(obj, Some(0))?;
        self.f.set(at);
        self.store_leaves(at, &leaves, ft, v)
    }

    /// Stores `v`, of type `ft`, as the leaves `leaves` of the memory at
    /// the address in local `at`: the place takes a count of the new value
    /// (a copy of each collection a place holds, `types.coll.value`)
    /// before the old one's is released.
    fn store_leaves(&mut self, at: u32, leaves: &[Leaf], ft: Ty, v: u32) -> StageResult<()> {
        let s = self.slot_src(v, ft, false)?;
        if self.counted(ft).is_some() {
            self.retain_src(&s, ft)?;
            let old: Vec<u32> = leaves.iter().map(|l| self.f.local(l.val)).collect();
            for (l, o) in leaves.iter().zip(&old) {
                self.f.get(at);
                self.f.op(l.load(0));
                self.f.set(*o);
            }
            self.release_locals(&old, ft)?;
        }
        for (k, l) in leaves.iter().enumerate() {
            self.store_leaf(
                &|em: &mut Self| {
                    em.f.get(at);
                    Ok(())
                },
                0,
                *l,
                &mut |em: &mut Self| em.src(&s, k),
            )?;
        }
        Ok(())
    }

    /// A write of `v` through the reference at `r`: the place takes a
    /// count of the new value before the old one's is released.
    fn deref_write(&mut self, r: Start, v: u32) -> StageResult<()> {
        if v != NONE && self.diverges(v) {
            self.f.op(I::Unreachable);
            return Ok(());
        }
        let Some((target, false)) = self.ref_target(self.start_ty(r)) else {
            return unsupported("a write of a whole value through a trait value reference");
        };
        let value = self.ref_layout(target)?;
        let at = self.f.local(Val::I32);
        self.push_start(r, Some(0))?;
        self.f.set(at);
        let ls: Vec<u32> = value.leaves.iter().map(|l| self.f.local(l.val)).collect();
        self.push(v)?;
        for l in ls.iter().rev() {
            self.f.set(*l);
        }
        rt::ref_store(&mut self.f, at, &ls, &value);
        Ok(())
    }

    /// A write of `v` into the struct field `path` reaches in what the
    /// reference at `r` refers to. A physical place takes the value at
    /// its offset; a representation view takes it through a field view
    /// (`RefProject`) made for the write and released after it.
    fn deref_field_write(&mut self, r: Start, path: &[u32], v: u32) -> StageResult<()> {
        if v != NONE && self.diverges(v) {
            self.f.op(I::Unreachable);
            return Ok(());
        }
        let Some((target, false)) = self.ref_target(self.start_ty(r)) else {
            return unsupported("a field write through a trait value reference");
        };
        let (ft, part) = self.fold_path(target, path)?;
        let parent = self.ref_layout(target)?;
        let field = self.ref_layout(ft)?;
        let at = self.f.local(Val::I32);
        self.push_start(r, Some(0))?;
        self.f.set(at);
        let ls: Vec<u32> = field.leaves.iter().map(|l| self.f.local(l.val)).collect();
        self.push(v)?;
        for l in ls.iter().rev() {
            self.f.set(*l);
        }
        let view = self.f.local(Val::I32);
        self.f.get(at);
        self.f.helper(Helper::RefIsView);
        self.f.set(view);
        let dst = self.f.local(Val::I32);
        self.f.get(view);
        self.f
            .op(I::If(wasm_encoder::BlockType::Result(Val::I32.wasm())));
        self.f.get(at);
        // Only a view reaches the helper here, and a field view always
        // holds its source; `borrowed` drops only the retain of the
        // helper's physical branch, which this write never takes.
        self.f.helper(Helper::RefProject {
            borrowed: true,
            parent,
            field: field.clone(),
            first: u32::try_from(part.first).expect("field"),
            offset: part.offset,
        });
        self.f.op(I::Else);
        self.f.get(at);
        if part.offset != 0 {
            self.f.u32(part.offset);
            self.f.op(I::I32Add);
        }
        self.f.op(I::End);
        self.f.set(dst);
        rt::ref_store(&mut self.f, dst, &ls, &field);
        if self.input.runtime.is_some() {
            self.f.get(view);
            self.f.op(I::If(wasm_encoder::BlockType::Empty));
            self.release_open(dst);
            self.f.op(I::End);
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
        self.f.helper(Helper::RefAddress);
        self.f.u32(EMPTY);
        self.f.i32(0);
        self.store(i)
    }

    /// The static slot of the top-level binding a `GlobalGet` or
    /// `GlobalSet` names, when some reference refers to it.
    pub(super) fn global_slot(&self, a: u32) -> Option<u32> {
        self.def_slot(DefId::from_raw(*self.b.record(a).first()?))
    }

    /// The static slot of top-level binding `def`, when some reference
    /// refers to it.
    pub(super) fn def_slot(&self, def: DefId) -> Option<u32> {
        self.m.addressed.get(&self.env().path_hash(def)).copied()
    }

    /// `GlobalGet` `i` of a binding in static slot `at`.
    pub(super) fn slot_get(&mut self, i: u32, at: u32) -> StageResult<()> {
        let m = mem_of(self.pool(), self.env(), self.ty(i))?;
        for leaf in &m.leaves {
            self.f.static_addr(at);
            self.f.op(leaf.load(0));
        }
        self.store(i)
    }

    /// A store of `v` into top-level binding `def` in static slot `at`.
    pub(super) fn slot_set(&mut self, def: DefId, at: u32, v: u32) -> StageResult<()> {
        if v != NONE && self.diverges(v) {
            self.f.op(I::Unreachable);
            return Ok(());
        }
        let t = self.ty_of(v);
        let m = mem_of(self.pool(), self.env(), t)?;
        let counted = self.counted(t).is_some();
        if counted {
            let h = self.def_hash(def);
            self.note_stored(h, t, Some((at, m.leaves.clone())))?;
        }
        let a = self.f.local(Val::I32);
        self.f.static_addr(at);
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
