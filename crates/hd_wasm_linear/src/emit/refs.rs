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
//! (`hd_tir::ir::Place`). Each is read as a [`Spot`].

use crate::op::Op as I;
use hd_base::{DefId, StageResult};
use hd_tir::ir::{NONE, Place, PlaceRoot, Proj, Tag};
use hd_types::{Ty, TyData};

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
/// element at an index value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Head {
    Direct,
    Deref,
    Data(u32),
    Elem(u32),
}

/// A place as the resolver reads it: its root, its head, then the struct
/// field indices read from there in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Spot {
    pub start: Start,
    pub head: Head,
    pub path: Vec<u32>,
}

/// Where a step of a place lies while [`Em::walk_place`] resolves it: in
/// Wasm locals; in memory at the address in a local plus an offset, laid
/// out as the step's type is; or as single leaves at their offsets from
/// the address in a local (an enum payload's fields, which the enum's
/// layout scatters).
pub(super) enum At {
    Vals(Vec<u32>),
    Mem(u32, u32),
    Leaves(u32, Vec<Leaf>),
}

/// The type a reference refers to, and whether the reference is fat (a
/// trait value's).
type Referent = (Ty, bool);

/// A view's fresh value a walk read through ([`Em::walk_place`]): the
/// getter's value, with counts of its own, in `vals` when `flag` is set.
/// What the walk found lives in memory the view's source holds too, so
/// the instruction releases this ([`Em::release_fresh`]) once it is done.
pub(super) struct Fresh {
    flag: u32,
    vals: Vec<u32>,
    ty: Ty,
}

impl Em<'_, '_, '_> {
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
        let fields = if self.tuple(t) {
            crate::layout::tuple_members(
                self.pool(),
                crate::layout::strip(self.pool(), self.env(), t),
            )
        } else {
            self.struct_fields(t)
        };
        let (Some(part), Some(&ft)) = (parts.get(f as usize).copied(), fields.get(f as usize))
        else {
            return unsupported("a reference to a field the struct does not have");
        };
        Ok((ft, part))
    }

    /// The place of `Ref`, `Read` or `Store` instruction `i`, as the
    /// resolver reads it.
    pub(super) fn spot(&self, i: u32) -> StageResult<Spot> {
        if !matches!(self.b.tags[i as usize], Tag::Ref | Tag::Read | Tag::Store) {
            return unsupported("a place of an instruction that has none");
        }
        match self.b.decode_place(self.b.data[i as usize][0]) {
            Some(p) => self.spot_of(&p),
            None => unsupported("a malformed place"),
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
            PlaceRoot::Frame(_) | PlaceRoot::Ptr(_) => {
                return unsupported("a frame slot or a pointer's target as a general place");
            }
        };
        let data = matches!(self.kind(self.start_ty(start)), Kind::Data(_));
        let (head, rest) = match p.proj.as_slice() {
            [Proj::Deref, Proj::Field(k), rest @ ..] if data => (Head::Data(*k), rest),
            [Proj::Deref, rest @ ..] => (Head::Deref, rest),
            [Proj::Elem(r), rest @ ..] => (Head::Elem(r.0), rest),
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

    /// The part at projection `proj` of a value of type `t` laid out in
    /// its locals: the part's type and which of the value's locals hold
    /// it, through struct and tuple fields, a newtype's one field and an
    /// enum's payload fields (`as .v .f`). `None` when a step leaves the
    /// locals: a boxed enum, a data value, an open value, a dereference,
    /// an element.
    pub(super) fn part_slots(&self, t: Ty, proj: &[Proj]) -> StageResult<Option<(Ty, Vec<usize>)>> {
        let mut slots: Vec<usize> = (0..self.vals(t)?.len()).collect();
        let mut ty = t;
        let mut k = 0;
        while k < proj.len() {
            // A newtype's one field is all of it, whatever it holds.
            let whole = self.newtype(ty) && proj[k] == Proj::Field(0);
            if hd_mono::is_open(self.pool(), ty) || !(whole || matches!(self.kind(ty), Kind::Value))
            {
                return Ok(None);
            }
            match proj[k] {
                Proj::Field(f) => {
                    let (ft, part) = self.path_step(ty, f)?;
                    slots = slots[part.first..part.first + part.count].to_vec();
                    ty = ft;
                    k += 1;
                }
                Proj::Variant(v) => {
                    let Some(&Proj::Field(f)) = proj.get(k + 1) else {
                        return Ok(None);
                    };
                    let (_, positions) = self.flat(ty)?;
                    let tys =
                        crate::layout::variants(self.pool(), self.env(), ty).unwrap_or_default();
                    let (Some(at), Some(&ft)) = (
                        positions.get(v as usize).and_then(|fs| fs.get(f as usize)),
                        tys.get(v as usize).and_then(|fs| fs.get(f as usize)),
                    ) else {
                        return unsupported("a payload field the enum does not have");
                    };
                    slots = at.iter().map(|&p| slots[p]).collect();
                    ty = ft;
                    k += 2;
                }
                Proj::Deref | Proj::Elem(_) => return Ok(None),
            }
        }
        Ok(Some((ty, slots)))
    }

    /// The locals of place `p`'s root, when the root's value lives in
    /// them (a local in no cell, or an instruction's value), and its type.
    fn root_locals(&mut self, p: &Place) -> StageResult<Option<(Ty, Vec<u32>)>> {
        match p.root {
            PlaceRoot::Local(l) if !self.is_cell(l.idx()) => Ok(Some((
                self.start_ty(Start::Local(l.raw())),
                self.local(l.raw())?,
            ))),
            PlaceRoot::Value(r) if r.as_inst().is_some() => match self.vals.get(&r.0) {
                Some(ls) => Ok(Some((self.ty_of(r.0), ls.clone()))),
                None => unsupported("a value its instruction did not compute"),
            },
            _ => Ok(None),
        }
    }

    /// `Read move` `i` of an owned body whose place is a part of a local
    /// or a value laid out in the root's locals (`[$l .a]`,
    /// `[%v as .1 .0]`): the part's locals with their counts, and zeros
    /// left in their place. False, emitting nothing, when the part is not
    /// in the root's locals at this instance (a boxed enum's payload, an
    /// open value's part), where the read is emitted as a copy and the
    /// root's `Drop`, which the move pass keeps for such a root, releases
    /// the whole.
    pub(super) fn own_move_part(&mut self, i: u32) -> StageResult<bool> {
        let Some(p) = self.b.decode_place(self.b.data[i as usize][0]) else {
            return unsupported("a malformed place");
        };
        let Some((t, ls)) = self.root_locals(&p)? else {
            return Ok(false);
        };
        let Some((pt, slots)) = self.part_slots(t, &p.proj)? else {
            return Ok(false);
        };
        for &k in &slots {
            self.f.get(ls[k]);
        }
        self.store_owned(i)?;
        if self.counted(pt).is_some() {
            for &k in &slots {
                self.f.zero(ls[k]);
            }
        }
        Ok(true)
    }

    /// `Drop` `i` of a part of a local or a value (`[$l .b]`,
    /// `[%v as .1 .1]`), which the move pass writes for the rest of a base
    /// it moved a part out of: the part's release, and zeros in its place.
    pub(super) fn own_drop_part(&mut self, p: &Place) -> StageResult<()> {
        let Some((t, ls)) = self.root_locals(p)? else {
            return unsupported("a `Drop` of a part of a place whose root is in no locals");
        };
        let Some((pt, slots)) = self.part_slots(t, &p.proj)? else {
            return unsupported("a `Drop` of a part that is not laid out in its root's locals");
        };
        let part: Vec<u32> = slots.iter().map(|&k| ls[k]).collect();
        self.release_locals(&part, pt)?;
        for l in part {
            self.f.zero(l);
        }
        Ok(())
    }

    /// `Drop [%r]` `i` of a whole value after moves or drops of its parts
    /// in the same block (the move pass keeps a whole drop of a base whose
    /// type names a type parameter, which it cannot split): where this
    /// instance lays the value out in its locals, it releases only the
    /// field those took nothing of, of the variant the parts name, or
    /// nothing when none is left, since the parts taken are zero and the
    /// tag is that variant's. True when it emitted that; false, emitting
    /// nothing, where the whole value's release runs: an open value's parts
    /// (read as copies), or more than one field left, which the whole
    /// value's glue releases in one call.
    pub(super) fn own_drop_rest(&mut self, i: u32, r: u32) -> StageResult<bool> {
        let Some(&(blk, pos)) = self.own_at.get(i as usize) else {
            return Ok(false);
        };
        if blk == NONE {
            return Ok(false);
        }
        // The fields taken: one level below the value, of one variant.
        let mut variant = None;
        let mut taken = Vec::new();
        let members = self.rec(self.b.data[blk as usize][0]);
        for &m in &members[..pos as usize] {
            let mi = m as usize;
            let mode = self.b.data[mi][1];
            let part = match self.b.tags[mi] {
                Tag::Read if mode == hd_tir::ir::ReadMode::Move as u32 => true,
                Tag::Drop => true,
                _ => false,
            };
            if !part {
                continue;
            }
            let Some(p) = self.b.decode_place(self.b.data[mi][0]) else {
                continue;
            };
            if p.root != PlaceRoot::Value(hd_tir::ir::Ref(r)) || p.proj.is_empty() {
                continue;
            }
            let (v, f) = match p.proj.as_slice() {
                [Proj::Variant(v), Proj::Field(f)] => (Some(*v), *f),
                [Proj::Field(f)] => (None, *f),
                _ => return Ok(false),
            };
            if taken.is_empty() {
                variant = v;
            } else if variant != v {
                return Ok(false);
            }
            taken.push(f);
        }
        if taken.is_empty() {
            return Ok(false);
        }
        let t = self.ty_of(r);
        let Some(ls) = self.vals.get(&r).cloned() else {
            return Ok(false);
        };
        let mut fields = Vec::new();
        if !crate::in_place_parts(self.pool(), self.env(), t, variant, &mut fields) {
            return Ok(false);
        }
        let step = |f: u32| match variant {
            Some(v) => vec![Proj::Variant(v), Proj::Field(f)],
            None => vec![Proj::Field(f)],
        };
        // Every part taken must be in the locals at this instance: one that
        // is not was read as a copy, and the whole release frees it.
        let mut rest = Vec::new();
        for (f, _) in fields.iter().enumerate() {
            let f = u32::try_from(f).expect("fields");
            let Some((ft, slots)) = self.part_slots(t, &step(f))? else {
                return Ok(false);
            };
            if !taken.contains(&f) && self.counted(ft).is_some() {
                rest.push((ft, slots));
            }
        }
        // More than one part left: the whole value's glue releases them in
        // one call, the parts taken being zero.
        if rest.len() > 1 {
            return Ok(false);
        }
        for (ft, slots) in rest {
            let part: Vec<u32> = slots.iter().map(|&k| ls[k]).collect();
            self.release_locals(&part, ft)?;
            for l in part {
                self.f.zero(l);
            }
        }
        Ok(true)
    }

    /// The list or map header at the place `word` of an `IsUnique` or a
    /// `Pin`, in a local, with the collection's type: the place resolved
    /// from its root's value ([`Em::walk_place`]), each element
    /// bounds-checked. Nothing takes a count: the header is the one the
    /// place holds, which a view's fresh value only shares.
    pub(super) fn place_header(&mut self, word: u32) -> StageResult<(u32, Ty)> {
        let Some(p) = self.b.decode_place(word) else {
            return unsupported("a malformed place");
        };
        let mut fresh = Vec::new();
        let (at, ty) = self.walk_place(NONE, &p, p.proj.len(), &mut fresh)?;
        let h = self.at_i32(&at)?;
        self.release_fresh(&fresh)?;
        Ok((h, ty))
    }

    /// `Ref` `i`: the place's address, or the trait value
    /// there for a reference to a trait value, with a count of its own.
    pub(super) fn place_ref(&mut self, i: u32) -> StageResult<()> {
        // A reference the move pass marked to take its local's count of
        // the cell (`docs/compiler/tir-v2.md`, "Move" rule 4): the cell's
        // address, owned, and the local left empty. Where the reference
        // is no cell's address at this instance (a trait value's, an open
        // cell's view), it takes a count of its own and the local's count
        // is released here, as the `Drop` the pass deleted did.
        if self.owned && self.b.data[i as usize][1] & hd_tir::ir::ref_word::TAKES != 0 {
            let s = self.spot(i)?;
            let (Start::Local(x), Head::Direct, true) = (s.start, s.head, s.path.is_empty()) else {
                return unsupported("a reference that takes the count of no whole local");
            };
            let fat = self.ref_target(self.ty(i)).is_some_and(|(_, f)| f);
            let cell = self.is_cell(x as usize);
            let view = cell
                && self.cell_converts(x as usize)
                && self.ref_layout(self.start_ty(s.start))?.boxed;
            if cell && !fat && !view {
                let at = self.cell_of(x as usize)?;
                self.f.get(at);
                self.store_owned(i)?;
                self.f.zero(at);
                return Ok(());
            }
            self.counted_ref(i)?;
            return self.release_local(x as usize);
        }
        self.counted_ref(i)
    }

    /// `Ref` `i`: the address of its place, with a count of its own.
    fn counted_ref(&mut self, i: u32) -> StageResult<()> {
        if self.rooted_at(i) {
            return self.rooted_ref(i);
        }
        let s = self.spot(i)?;
        let Some((_, fat)) = self.ref_target(self.ty(i)) else {
            return unsupported("a reference whose type is no reference");
        };
        let path = s.path.clone();
        let mut owned_base = None;
        let (base, ty) = match (s.start, s.head) {
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
            // A map's value at entry `ix` (`ref.elem.map`).
            (st, Head::Elem(ix)) if self.is_map(self.start_ty(st)) => {
                let h = self.start_values(st)?[0];
                let (vt, at) = self.map_value_at(h, self.start_ty(st), ix)?;
                (Base::At(at), vt)
            }
            (st, Head::Elem(ix)) => {
                let (et, e) = self.elem(self.start_ty(st))?;
                let h = self.start_values(st)?[0];
                let at = self.f.local(Val::I32);
                self.f.get(h);
                self.f.op(I::I32Load(mem(HEAP, 0, 2)));
                // An element of no bytes lies at no address of its own;
                // the length word keeps the address inside the buffer.
                if e.size == 0 {
                    self.f.u32(u32::try_from(LIST_LEN).expect("offset"));
                } else {
                    self.comp(ix, 0)?;
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
        self.ref_from(i, base, ty, &path, owned_base, fat)
    }

    /// The rest of `Ref` `i` from where its place's head lies (`base`, of
    /// type `ty`): the address of the struct field `path` reaches there,
    /// with a count of its own. `owned_base` is a view the head made,
    /// released after.
    fn ref_from(
        &mut self,
        i: u32,
        base: Base,
        mut ty: Ty,
        path: &[u32],
        owned_base: Option<u32>,
        fat: bool,
    ) -> StageResult<()> {
        let parent_ty = ty;
        let has_path = !path.is_empty();
        let (mut off, mut first) = (0, 0);
        for &f in path {
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
        self.store(i)
    }

    /// `Read` `i`: the value at a place through a reference,
    /// a copy with its own count; a reference to a trait value is that
    /// value. Stage 1 reads a place only through a dereference; a field
    /// path after it reads only that field.
    pub(super) fn place_read(&mut self, i: u32) -> StageResult<()> {
        if self.rooted_at(i) {
            return self.rooted_read(i);
        }
        let s = self.spot(i)?;
        // A field of a local (`[$l .x .y]`), which the ownership passes
        // root at the local: that part of its values, a copy with a count
        // of its own.
        if let (Start::Local(l), Head::Direct, false) = (s.start, s.head, s.path.is_empty()) {
            let t = self.start_ty(s.start);
            let (_, part) = self.fold_path(t, &s.path)?;
            let ls = self.local_value(l)?;
            for x in &ls[part.first..part.first + part.count] {
                self.f.get(*x);
            }
            return self.store(i);
        }
        if s.head != Head::Deref {
            return unsupported(
                "a read of a place with no dereference that is no plain or projection read",
            );
        }
        let Some((target, fat)) = self.ref_target(self.start_ty(s.start)) else {
            return unsupported("a read through a value that is no reference");
        };
        if fat {
            if !s.path.is_empty() {
                return unsupported("a field read through a trait value reference");
            }
            self.push_start(s.start, None)?;
            return self.store(i);
        }
        let value = self.ref_layout(target)?;
        let at = self.f.local(Val::I32);
        self.push_start(s.start, Some(0))?;
        self.f.set(at);
        if !s.path.is_empty() {
            let (_, part) = self.fold_path(target, &s.path)?;
            return self.read_part(i, at, &value, target, part);
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

    /// `Store` `i`: replaces the value at a place, which
    /// takes a count of the new value before the old one's is released.
    /// Stage 1 lowers a store through a dereference, into a data
    /// object's field or a struct field in it, into a local or top-level
    /// binding, whole or a struct field of it, and into a list value's
    /// element or a struct field of it ([`Em::elem_store`]).
    pub(super) fn place_store(&mut self, i: u32) -> StageResult<()> {
        if self.rooted_at(i) {
            return self.rooted_store(i);
        }
        let s = self.spot(i)?;
        let v = self.b.data[i as usize][1];
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
            (Start::Value(list), Head::Elem(ix), _) => self.elem_store(list, ix, &s.path, v),
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
        let at = self.f.local(Val::I32);
        self.push_start(r, Some(0))?;
        self.f.set(at);
        self.deref_write_at(at, target, v)
    }

    /// A write of `v` through the reference in local `at` to a `target`.
    fn deref_write_at(&mut self, at: u32, target: Ty, v: u32) -> StageResult<()> {
        let value = self.ref_layout(target)?;
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
        let at = self.f.local(Val::I32);
        self.push_start(r, Some(0))?;
        self.f.set(at);
        self.deref_field_write_at(at, target, path, v)
    }

    /// A write of `v` into the struct field `path` reaches in the `target`
    /// the reference in local `at` refers to ([`Em::deref_field_write`]).
    fn deref_field_write_at(
        &mut self,
        at: u32,
        target: Ty,
        path: &[u32],
        v: u32,
    ) -> StageResult<()> {
        let (ft, part) = self.fold_path(target, path)?;
        let parent = self.ref_layout(target)?;
        let field = self.ref_layout(ft)?;
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

    /// The static slot of top-level binding `def`, when some reference
    /// refers to it.
    pub(super) fn def_slot(&self, def: DefId) -> Option<u32> {
        self.m.addressed.get(&self.env().path_hash(def)).copied()
    }

    /// `Read [@g]` `i` of a binding in static slot `at`.
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

/// Places rooted at a local (`docs/compiler/tir-v2.md`, stage 2 commit
/// 12a): the checker names a place from its local through every step,
/// `[*$0 [%5] .x]`, not each base read into a value first. One walk
/// resolves such a place step by
/// step from the root's value, and each instruction reads, stores or
/// addresses what it finds: only the leaves the place names are loaded.
impl Em<'_, '_, '_> {
    /// Whether the place of instruction `i` of an owned body takes the
    /// walk: rooted at a local, with an element or a variant among its
    /// steps, or a dereference after its first step; or rooted at a
    /// top-level binding, read through any step or reached through one
    /// that is no field. The old lowering keeps a local's fields and one
    /// leading dereference with fields after it, and a store into or a
    /// reference to a top-level binding's fields (`Em::spot_of`).
    pub(super) fn rooted_at(&self, i: u32) -> bool {
        let read = self.b.tags[i as usize] == Tag::Read;
        self.owned
            && self
                .b
                .decode_place(self.b.data[i as usize][0])
                .is_some_and(|p| {
                    Self::rooted(&p)
                        || read && matches!(p.root, PlaceRoot::Global(..)) && !p.proj.is_empty()
                })
    }

    /// Whether place `p` takes the walk ([`Em::rooted_at`]).
    pub(super) fn rooted(p: &Place) -> bool {
        let global = matches!(p.root, PlaceRoot::Global(..));
        matches!(p.root, PlaceRoot::Local(_) | PlaceRoot::Global(..))
            && p.proj.iter().enumerate().any(|(k, s)| match s {
                Proj::Elem(_) | Proj::Variant(_) => true,
                Proj::Deref => k > 0 || global,
                Proj::Field(_) => false,
            })
    }

    /// The value at a place's root, where the walk starts: a local's (read
    /// from its cell when it has one), a top-level binding's (its static
    /// slot, or its Wasm globals read into locals) or an instruction's.
    fn at_root(&mut self, root: PlaceRoot) -> StageResult<(At, Ty)> {
        match root {
            PlaceRoot::Local(l) => Ok((
                At::Vals(self.local_value(l.raw())?),
                self.start_ty(Start::Local(l.raw())),
            )),
            PlaceRoot::Global(def, t) => {
                let t = self.sub(t);
                if let Some(slot) = self.def_slot(def) {
                    let a = self.f.local(Val::I32);
                    self.f.static_addr(slot);
                    self.f.set(a);
                    return Ok((At::Mem(a, 0), t));
                }
                let h = self.def_hash(def);
                let mut ls = Vec::new();
                for (k, v) in self.vals(t)?.into_iter().enumerate() {
                    let k = u32::try_from(k).expect("k");
                    ls.push(self.f.local(v));
                    self.f.global_get(crate::module::Global::Storage(h, k, v));
                    self.f.set(ls[ls.len() - 1]);
                }
                Ok((At::Vals(ls), t))
            }
            PlaceRoot::Value(r) if r.as_inst().is_some() => {
                Ok((At::Vals(self.local_value_of(r.0)?), self.ty_of(r.0)))
            }
            _ => unsupported("a place rooted at a constant"),
        }
    }

    /// A local holding the one 32-bit value at `at`: a list header, a
    /// reference, a data object's or a boxed enum's address.
    pub(super) fn at_i32(&mut self, at: &At) -> StageResult<u32> {
        let (base, leaf) = match at {
            At::Vals(ls) if ls.len() == 1 => return Ok(ls[0]),
            At::Mem(a, off) => (
                *a,
                Leaf {
                    offset: *off,
                    val: Val::I32,
                    bytes: 4,
                    signed: false,
                },
            ),
            At::Leaves(a, lv) if lv.len() == 1 && lv[0].val == Val::I32 => (*a, lv[0]),
            _ => return unsupported("an address step from a value of several parts"),
        };
        let x = self.f.local(Val::I32);
        self.f.get(base);
        self.f.op(leaf.load(0));
        self.f.set(x);
        Ok(x)
    }

    /// The leaves of a value of type `ty` at `at` in memory, at their
    /// offsets from its address local; `None` for a value in locals.
    fn at_leaves(&self, at: &At, ty: Ty) -> StageResult<Option<(u32, Vec<Leaf>)>> {
        Ok(match at {
            At::Vals(_) => None,
            At::Mem(a, off) => {
                let mut lv = mem_of(self.pool(), self.env(), ty)?.leaves;
                for l in &mut lv {
                    l.offset += off;
                }
                Some((*a, lv))
            }
            At::Leaves(a, lv) => Some((*a, lv.clone())),
        })
    }

    /// Pushes the value of type `ty` at `at`, with no count.
    fn push_at(&mut self, at: &At, ty: Ty) -> StageResult<()> {
        match self.at_leaves(at, ty)? {
            None => {
                let At::Vals(ls) = at else {
                    return unsupported("a value in no locals");
                };
                for l in ls {
                    self.f.get(*l);
                }
            }
            Some((a, lv)) => {
                for l in lv {
                    self.f.get(a);
                    self.f.op(l.load(0));
                }
            }
        }
        Ok(())
    }

    /// Field `f` of the struct, tuple or newtype at `at`.
    fn at_field(&mut self, at: At, ty: Ty, f: u32) -> StageResult<(At, Ty)> {
        let (ft, part) = self.path_step(ty, f)?;
        let at = match at {
            At::Vals(ls) => At::Vals(ls[part.first..part.first + part.count].to_vec()),
            At::Mem(a, off) => At::Mem(a, off + part.offset),
            At::Leaves(a, lv) => At::Leaves(a, lv[part.first..part.first + part.count].to_vec()),
        };
        Ok((at, ft))
    }

    /// Field `f` of variant `v`'s payload of the enum at `at`: its values
    /// among the enum's, in the boxed enum's cell when it has one.
    fn at_variant(&mut self, at: At, ty: Ty, v: u32, f: u32) -> StageResult<(At, Ty)> {
        let (m, positions) = self.flat(ty)?;
        let tys = crate::layout::variants(self.pool(), self.env(), ty).unwrap_or_default();
        let (Some(sel), Some(&ft)) = (
            positions.get(v as usize).and_then(|fs| fs.get(f as usize)),
            tys.get(v as usize).and_then(|fs| fs.get(f as usize)),
        ) else {
            return unsupported("a payload field the enum does not have");
        };
        let at = if matches!(self.kind(ty), Kind::Boxed) {
            let cell = self.at_i32(&at)?;
            At::Leaves(cell, sel.iter().map(|&p| m.leaves[p]).collect())
        } else {
            match at {
                At::Vals(ls) => At::Vals(sel.iter().map(|&p| ls[p]).collect()),
                At::Mem(a, off) => At::Leaves(
                    a,
                    sel.iter()
                        .map(|&p| Leaf {
                            offset: m.leaves[p].offset + off,
                            ..m.leaves[p]
                        })
                        .collect(),
                ),
                At::Leaves(a, lv) => At::Leaves(a, sel.iter().map(|&p| lv[p]).collect()),
            }
        };
        Ok((at, ft))
    }

    /// Walks the steps of `p` up to step `upto` from its root's value
    /// (`docs/compiler/tir-v2.md`, "Places"): a field picks its part, a
    /// variant its payload field, a dereference of a data value the
    /// object's field, an element its slot in the buffer, bounds-checked
    /// (unchecked where the loop's range proof covers read `i`, and from the
    /// loop's cached header where the list is a stable list of a local).
    /// A dereference of a reference goes to its referent's address; where
    /// a reference view may be reached, a view's getter makes the referent
    /// fresh instead, the walk reads on in that value to the next step's
    /// address (which the view's source holds too), and the fresh value
    /// joins `fresh`. So every dereference before `upto` is followed, by
    /// `upto`, by another element or dereference, or by fields and
    /// variants down to one 32-bit value.
    pub(super) fn walk_place(
        &mut self,
        i: u32,
        p: &Place,
        upto: usize,
        fresh: &mut Vec<Fresh>,
    ) -> StageResult<(At, Ty)> {
        let steps = &p.proj[..upto];
        let local = match p.root {
            PlaceRoot::Local(l) => Some(l.raw()),
            _ => None,
        };
        // A stable list's header from the loop's cache: an element of the
        // local's list, or of the list the reference it holds refers to.
        let cached = |em: &Self, indirect: bool| {
            let root = super::list::ListRoot::new(local?, indirect);
            em.list_caches
                .iter()
                .find(|(key, _)| *key == root)
                .map(|(_, cache)| *cache)
        };
        let mut k = 0;
        let mut at;
        let mut ty;
        // The first element step of a local's list, or of the list the
        // reference it holds refers to: a loop's range proof and its cache
        // name these.
        let lead = match (local, steps) {
            (Some(_), [Proj::Elem(_), ..]) => Some(0),
            (Some(_), [Proj::Deref, Proj::Elem(_), ..]) => Some(1),
            _ => None,
        };
        let lead_cache = lead.and_then(|e| cached(self, e == 1).map(|c| (c, e)));
        if let Some((cache, elem)) = lead_cache {
            // The cached header, where no view is reachable; else the walk.
            let h = self.f.local(Val::I32);
            self.f.helper(Helper::RefViewsAbsent);
            self.f
                .op(I::If(wasm_encoder::BlockType::Result(Val::I32.wasm())));
            self.f.get(cache.header);
            self.f.op(I::Else);
            let (a, t) = self.at_root(p.root)?;
            let (a, _) = self.walk_steps(a, t, &steps[..elem], fresh)?;
            let x = self.at_i32(&a)?;
            self.f.get(x);
            self.f.op(I::End);
            self.f.set(h);
            self.cached_headers.push((h, cache));
            let (_, root_ty) = self.at_root_ty(p.root);
            ty = self.walk_ty(root_ty, &steps[..elem])?;
            at = At::Vals(vec![h]);
            k = elem;
        } else {
            (at, ty) = self.at_root(p.root)?;
        }
        while k < steps.len() {
            let Proj::Elem(ix) = steps[k] else {
                let next = steps[k + 1..]
                    .iter()
                    .position(|s| matches!(s, Proj::Elem(_)))
                    .map_or(steps.len(), |x| k + 1 + x);
                (at, ty) = self.walk_steps(at, ty, &steps[k..next], fresh)?;
                k = next;
                continue;
            };
            // A map's value at entry `ix` (`ref.elem.map`).
            if self.is_map(ty) {
                let h = self.at_i32(&at)?;
                let (vt, a) = self.map_value_at(h, ty, ix.0)?;
                at = At::Mem(a, 0);
                ty = vt;
                k += 1;
                continue;
            }
            let (et, e) = self.elem(ty)?;
            let h = self.at_i32(&at)?;
            let b = self.buffer(h);
            // The first element of a local's list, which a loop's range
            // proof may cover.
            let proven = lead == Some(k) && self.proven_list_indices.contains(&i);
            let ix = if proven {
                let x = self.f.local(Val::I32);
                self.f.helper(Helper::RefViewsAbsent);
                self.f
                    .op(I::If(wasm_encoder::BlockType::Result(Val::I32.wasm())));
                self.comp(ix.0, 0)?;
                self.f.op(I::Else);
                let checked = self.index(b, ix.0)?;
                self.f.get(checked);
                self.f.op(I::End);
                self.f.set(x);
                x
            } else {
                self.index(b, ix.0)?
            };
            let slot = self.slot_at(b, ix, e.size);
            at = At::Mem(slot, LIST_ELEMS);
            ty = et;
            k += 1;
        }
        Ok((at, ty))
    }

    /// The type of a place's root.
    fn at_root_ty(&self, root: PlaceRoot) -> (bool, Ty) {
        match root {
            PlaceRoot::Local(l) => (true, self.start_ty(Start::Local(l.raw()))),
            PlaceRoot::Value(r) => (false, self.ty_of(r.0)),
            PlaceRoot::Global(_, t) => (false, self.sub(t)),
            PlaceRoot::Frame(_) | PlaceRoot::Ptr(_) => (false, Ty::VOID),
        }
    }

    /// The type steps `steps` reach from type `ty`.
    fn walk_ty(&self, mut ty: Ty, steps: &[Proj]) -> StageResult<Ty> {
        let mut k = 0;
        while k < steps.len() {
            match steps[k] {
                Proj::Field(f) => ty = self.path_step(ty, f)?.0,
                Proj::Elem(_) if self.is_map(ty) => ty = self.entry(ty)?.1,
                Proj::Elem(_) => ty = self.elem(ty)?.0,
                Proj::Deref => match self.kind(ty) {
                    Kind::Ref { target, .. } => ty = target,
                    Kind::Data(tys) => {
                        let Some(&Proj::Field(f)) = steps.get(k + 1) else {
                            return unsupported("a data value's dereference without its field");
                        };
                        ty = tys.get(f as usize).copied().unwrap_or(Ty::VOID);
                        k += 1;
                    }
                    _ => return unsupported("a dereference of a value that is no reference"),
                },
                Proj::Variant(v) => {
                    let Some(&Proj::Field(f)) = steps.get(k + 1) else {
                        return unsupported("a variant without its payload field");
                    };
                    let tys =
                        crate::layout::variants(self.pool(), self.env(), ty).unwrap_or_default();
                    ty = tys
                        .get(v as usize)
                        .and_then(|fs| fs.get(f as usize))
                        .copied()
                        .unwrap_or(Ty::VOID);
                    k += 1;
                }
            }
            k += 1;
        }
        Ok(ty)
    }

    /// Walks `steps`, which hold no element, from `at` of type `ty`
    /// ([`Em::walk_place`]).
    fn walk_steps(
        &mut self,
        mut at: At,
        mut ty: Ty,
        steps: &[Proj],
        fresh: &mut Vec<Fresh>,
    ) -> StageResult<(At, Ty)> {
        let mut k = 0;
        while k < steps.len() {
            match steps[k] {
                Proj::Field(f) => {
                    (at, ty) = self.at_field(at, ty, f)?;
                    k += 1;
                }
                Proj::Variant(v) => {
                    let Some(&Proj::Field(f)) = steps.get(k + 1) else {
                        return unsupported("a variant without its payload field");
                    };
                    (at, ty) = self.at_variant(at, ty, v, f)?;
                    k += 2;
                }
                Proj::Elem(_) => return unsupported("an element among a walk's plain steps"),
                Proj::Deref => match self.kind(ty) {
                    Kind::Data(tys) => {
                        let Some(&Proj::Field(f)) = steps.get(k + 1) else {
                            return unsupported("a data value's dereference without its field");
                        };
                        let (_, parts) = payload_mem(self.pool(), self.env(), &tys)?;
                        let (Some(part), Some(&ft)) = (parts.get(f as usize), tys.get(f as usize))
                        else {
                            return unsupported("a data field the data type does not have");
                        };
                        let obj = self.at_i32(&at)?;
                        at = At::Mem(obj, part.offset);
                        ty = ft;
                        k += 2;
                    }
                    Kind::Ref { target, fat: false } => {
                        let r = self.at_i32(&at)?;
                        // The steps to the next dereference, or to the end.
                        let next = steps[k + 1..]
                            .iter()
                            .position(|s| *s == Proj::Deref)
                            .map_or(steps.len(), |x| k + 1 + x);
                        if self.deref_physical {
                            at = At::Mem(r, 0);
                            ty = target;
                            k += 1;
                            continue;
                        }
                        // The value the steps to the next dereference reach,
                        // in locals both ways: from a view's fresh value, or
                        // loaded from the referent's address.
                        let t = self.walk_ty(target, &steps[k + 1..next])?;
                        let xs: Vec<u32> =
                            self.vals(t)?.into_iter().map(|v| self.f.local(v)).collect();
                        let value = self.ref_layout(target)?;
                        let flag = self.f.local(Val::I32);
                        self.f.zero(flag);
                        self.f.get(r);
                        self.f.helper(Helper::RefIsView);
                        self.f.op(I::If(wasm_encoder::BlockType::Empty));
                        let vals = rt::ref_load(&mut self.f, r, &value);
                        self.f.i32(1);
                        self.f.set(flag);
                        let (a, _) = self.walk_steps(
                            At::Vals(vals.clone()),
                            target,
                            &steps[k + 1..next],
                            fresh,
                        )?;
                        self.push_at(&a, t)?;
                        for x in xs.iter().rev() {
                            self.f.set(*x);
                        }
                        self.f.op(I::Else);
                        let (a, _) =
                            self.walk_steps(At::Mem(r, 0), target, &steps[k + 1..next], fresh)?;
                        self.push_at(&a, t)?;
                        for x in xs.iter().rev() {
                            self.f.set(*x);
                        }
                        self.f.op(I::End);
                        fresh.push(Fresh {
                            flag,
                            vals,
                            ty: target,
                        });
                        at = At::Vals(xs);
                        ty = t;
                        k = next;
                    }
                    _ => {
                        return unsupported(
                            "a step through a trait value reference or a value that is no reference",
                        );
                    }
                },
            }
        }
        Ok((at, ty))
    }

    /// Releases the views' fresh values a walk read through, each where it
    /// was made, and clears their flags.
    pub(super) fn release_fresh(&mut self, fresh: &[Fresh]) -> StageResult<()> {
        for x in fresh {
            self.f.get(x.flag);
            self.f.op(I::If(wasm_encoder::BlockType::Empty));
            self.release_locals(&x.vals, x.ty)?;
            self.f.zero(x.flag);
            self.f.op(I::End);
        }
        Ok(())
    }

    /// The index of the last step of `p` that goes into memory (an element
    /// or a dereference), with whether it is a dereference of a reference
    /// that only fields and variants follow: its referent is then read,
    /// written or addressed through the reference, which may be a view.
    pub(super) fn last_through(&self, p: &Place) -> StageResult<Option<(usize, Option<Referent>)>> {
        let Some(d) = p
            .proj
            .iter()
            .rposition(|s| matches!(s, Proj::Elem(_) | Proj::Deref))
        else {
            return Ok(None);
        };
        if p.proj[d] != Proj::Deref {
            return Ok(Some((d, None)));
        }
        let (_, root_ty) = self.at_root_ty(p.root);
        let ty = self.walk_ty(root_ty, &p.proj[..d])?;
        Ok(Some((d, self.ref_target(ty))))
    }

    /// `Read` `i` of a place the walk takes ([`Em::rooted`]): only the
    /// leaves the place names, a copy with a count of its own or a borrow
    /// with none. A read through a reference that only fields follow reads
    /// that part of its referent as `Read [*%r .x]` does
    /// ([`Em::read_part`]). A `move` reaches here only for a part at no
    /// place of the root's locals, which is read as a copy: the root's
    /// `Drop`, which the move pass keeps for such a root, releases it.
    pub(super) fn rooted_read(&mut self, i: u32) -> StageResult<()> {
        let Some(p) = self.b.decode_place(self.b.data[i as usize][0]) else {
            return unsupported("a malformed place");
        };
        let mut fresh = Vec::new();
        if let Some((d, Some((target, fat)))) = self.last_through(&p)? {
            let (at, ref_ty) = self.walk_place(i, &p, d, &mut fresh)?;
            let tail = &p.proj[d + 1..];
            let fields: Option<Vec<u32>> = tail
                .iter()
                .map(|s| match s {
                    Proj::Field(f) => Some(*f),
                    _ => None,
                })
                .collect();
            let Some(fields) = fields else {
                return unsupported("a payload read through a reference");
            };
            // A reference to a trait value is that trait value.
            if fat {
                if !fields.is_empty() {
                    return unsupported("a field read through a trait value reference");
                }
                self.push_at(&at, ref_ty)?;
                self.store(i)?;
                return self.release_fresh(&fresh);
            }
            let r = self.at_i32(&at)?;
            let value = self.ref_layout(target)?;
            if fields.is_empty() {
                if let Some(flag) = self.begin_view_loan(i)? {
                    for l in rt::ref_load_borrowed(&mut self.f, r, &value, flag) {
                        self.f.get(l);
                    }
                } else {
                    self.load_through(r, &value);
                }
                self.store_owned(i)?;
            } else {
                let (_, part) = self.fold_path(target, &fields)?;
                self.read_part(i, r, &value, target, part)?;
            }
            return self.release_fresh(&fresh);
        }
        let (at, ty) = self.walk_place(i, &p, p.proj.len(), &mut fresh)?;
        self.push_at(&at, ty)?;
        self.store(i)?;
        self.release_fresh(&fresh)
    }

    /// `Store` `i` into a place the walk takes ([`Em::rooted`]): into the
    /// leaves in memory the place names, which take a count of the new
    /// value before the old one's is released; or through a reference that
    /// only fields follow, as `Store [*%r .x]` writes ([`Em::deref_field_write`]).
    /// The passes' `IsUnique` came before it.
    pub(super) fn rooted_store(&mut self, i: u32) -> StageResult<()> {
        let v = self.b.data[i as usize][1];
        if v != NONE && self.diverges(v) {
            self.f.op(I::Unreachable);
            return Ok(());
        }
        let Some(p) = self.b.decode_place(self.b.data[i as usize][0]) else {
            return unsupported("a malformed place");
        };
        let mut fresh = Vec::new();
        match self.last_through(&p)? {
            None => return unsupported("a store into a place in no memory"),
            Some((d, Some((target, fat)))) => {
                if fat {
                    return unsupported("a write through a trait value reference");
                }
                let (at, _) = self.walk_place(i, &p, d, &mut fresh)?;
                let r = self.at_i32(&at)?;
                let fields: Option<Vec<u32>> = p.proj[d + 1..]
                    .iter()
                    .map(|s| match s {
                        Proj::Field(f) => Some(*f),
                        _ => None,
                    })
                    .collect();
                match fields {
                    Some(fs) if fs.is_empty() => self.deref_write_at(r, target, v)?,
                    Some(fs) => self.deref_field_write_at(r, target, &fs, v)?,
                    None => return unsupported("a payload write through a reference"),
                }
            }
            Some(_) => {
                let (at, ty) = self.walk_place(i, &p, p.proj.len(), &mut fresh)?;
                let Some((a, lv)) = self.at_leaves(&at, ty)? else {
                    return unsupported("a store into a place in no memory");
                };
                self.store_leaves(a, &lv, ty, v)?;
            }
        }
        self.release_fresh(&fresh)
    }

    /// `Ref` `i` of a place the walk takes ([`Em::rooted`]): the address
    /// of the place the last element or dereference reaches, then of the
    /// struct field the fields after it name, as a `Ref` of that head
    /// lowers ([`Em::ref_from`]); an element is bounds-checked.
    pub(super) fn rooted_ref(&mut self, i: u32) -> StageResult<()> {
        let Some(p) = self.b.decode_place(self.b.data[i as usize][0]) else {
            return unsupported("a malformed place");
        };
        let Some((_, fat)) = self.ref_target(self.ty(i)) else {
            return unsupported("a reference whose type is no reference");
        };
        let Some((d, through)) = self.last_through(&p)? else {
            return unsupported("a reference to a place in no memory");
        };
        let mut fresh = Vec::new();
        let (at, ty) = self.walk_place(i, &p, d, &mut fresh)?;
        let mut rest = &p.proj[d + 1..];
        let (base, head_ty) = match (p.proj[d], through) {
            (Proj::Elem(ix), _) => {
                let h = self.at_i32(&at)?;
                let (et, slot) = self.elem_slot(h, ty, ix.0)?;
                (Base::At(self.offset_local(slot, LIST_ELEMS)), et)
            }
            (Proj::Deref, Some((target, f))) => {
                let r = self.at_i32(&at)?;
                if f {
                    let At::Vals(ls) = &at else {
                        return unsupported("a trait value reference in memory");
                    };
                    (Base::Vals(ls.clone()), target)
                } else {
                    (Base::At(r), target)
                }
            }
            (Proj::Deref, None) => {
                let Kind::Data(tys) = self.kind(ty) else {
                    return unsupported("a dereference of a value that is no reference");
                };
                let Some((&Proj::Field(k), more)) = rest.split_first() else {
                    return unsupported("a data value's dereference without its field");
                };
                rest = more;
                let (_, parts) = payload_mem(self.pool(), self.env(), &tys)?;
                let (Some(part), Some(&ft)) = (parts.get(k as usize), tys.get(k as usize)) else {
                    return unsupported("a data field the data type does not have");
                };
                let o = self.at_i32(&at)?;
                (Base::At(self.offset_local(o, part.offset)), ft)
            }
            _ => return unsupported("a reference to a place in no memory"),
        };
        let path: Option<Vec<u32>> = rest
            .iter()
            .map(|s| match s {
                Proj::Field(f) => Some(*f),
                _ => None,
            })
            .collect();
        let Some(path) = path else {
            return unsupported("a reference to a payload field");
        };
        self.ref_from(i, base, head_ty, &path, None, fat)?;
        self.release_fresh(&fresh)
    }
}
