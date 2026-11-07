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
//! (`hd_tir::ir::Place`). Each is walked ([`Em::walk_place`]) from where
//! [`Em::place_start`] starts it, then read, written or addressed.

use crate::op::Op as I;
use hd_base::{DefId, StageResult};
use hd_mono::layout::{CanonMemo, canon};
use hd_tir::ir::{NONE, Place, PlaceRoot, Proj, Ref, Tag};
use hd_types::{Ty, TyData};

use super::Em;
use super::value::{Dst, Src};
use crate::layout::{Kind, Leaf, Part, mem as mem_of, payload_mem, struct_mem};
use crate::module::{EMPTY, HEAP, Sym, Val, mem};
use crate::rt::{self, Helper, LIST_ELEMS, LIST_LEN, RefLayout};
use crate::unsupported;

/// Where a step of a place lies while [`Em::walk_place`] resolves it: in
/// Wasm locals; in a top-level binding's Wasm globals from value `first`
/// on, of the types `vals` (`(h, first, vals)`, the root of a store); in
/// memory at the address in a local plus an offset, laid out as the
/// step's type is; or as single leaves at their offsets from the address
/// in a local (an enum payload's fields, which the enum's layout
/// scatters).
pub(super) enum At {
    Vals(Vec<u32>),
    Globals(hd_base::Hash128, usize, Vec<Val>),
    Mem(u32, u32),
    Leaves(u32, Vec<Leaf>),
}

/// The type a reference refers to, and whether the reference is fat (a
/// trait value's).
type Referent = (Ty, bool);

impl Em<'_, '_, '_> {
    pub(super) fn ref_layout(&self, ty: Ty) -> StageResult<RefLayout> {
        let m = mem_of(self.pool(), self.env(), ty)?;
        let walk = self.counted(ty);
        let boxed = self.vals(ty)? != [Val::I32]
            || !crate::layout::walk(self.pool(), self.env(), ty).is_some_and(|w| w.plain_single());
        let key = canon(
            self.pool(),
            &|d| self.env().path_hash(d),
            &mut CanonMemo::default(),
            ty,
        );
        Ok(RefLayout {
            known: crate::layout::block_start(self.pool(), self.env(), ty),
            size: m.size,
            leaves: m.leaves,
            walk,
            drops: if matches!(self.kind(ty), Kind::Value) {
                self.drop_calls(ty).into()
            } else {
                Box::default()
            },
            colls: self.copied(ty).into(),
            boxed,
            weak: (self.m.open_world || self.m.weak_types.contains(&key)).then_some(key),
        })
    }

    /// One count less on the counted address in local `o`.
    pub(super) fn release_open(&mut self, o: u32) {
        if self.input.runtime.is_some() {
            self.f.get(o);
            self.f.helper(Helper::ReleaseRef);
        }
    }

    /// Locals holding value `r`: its instruction's, or a constant's,
    /// pushed into fresh ones.
    pub(super) fn local_value_of(&mut self, r: u32) -> StageResult<Vec<u32>> {
        if r != NONE && Ref(r).as_inst().is_some() {
            return match self.vals.get(&r) {
                Some(ls) => Ok(ls.clone()),
                None => unsupported("a value its instruction did not compute"),
            };
        }
        let vs = self.vals(self.ty_of(r))?;
        let ls: Vec<u32> = vs.into_iter().map(|v| self.f.local(v)).collect();
        self.push(r)?;
        for l in ls.iter().rev() {
            self.f.set(*l);
        }
        Ok(ls)
    }

    /// The type reference type `t` refers to, and whether the reference
    /// is fat: a trait value.
    pub(super) fn ref_target(&self, t: Ty) -> Option<(Ty, bool)> {
        match self.kind(t) {
            Kind::Ref { target, fat, .. } => Some((target, fat)),
            _ => None,
        }
    }

    /// The place in operand word `word`.
    pub(super) fn place(&self, word: u32) -> StageResult<Place> {
        self.b
            .decode_place(word)
            .map_or_else(|| unsupported("a malformed place"), Ok)
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

    /// The type a struct field path reads from type `t`, and where it lies
    /// in `t`'s values and memory ([`Em::path_step`], folded). No path is
    /// all of `t`.
    pub(super) fn fold_path(&self, t: Ty, path: &[u32]) -> StageResult<(Ty, Part)> {
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
            if !(whole || matches!(self.kind(ty), Kind::Value)) {
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
                    let flat = self.flat(ty)?;
                    let positions = &flat.1;
                    let tys = self.variant_tys(ty).unwrap_or_default();
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
            PlaceRoot::Local(l) if !self.is_cell(l.idx()) => {
                Ok(Some((self.at_root_ty(p.root), self.local(l.raw())?)))
            }
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
        let p = self.place(self.b.data[i as usize][0])?;
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
        self.mark_drop_place(&p, false);
        if self.needs_drop(pt) {
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
        self.release_drop_place(&part, pt, p)?;
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
        let Some(&blk) = self.topo.blk.get(i as usize).filter(|&&blk| blk != NONE) else {
            return Ok(false);
        };
        let pos = self.topo.pos[i as usize];
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
            if !taken.contains(&f) && self.needs_drop(ft) {
                rest.push((ft, slots, f));
            }
        }
        // More than one part left: the whole value's glue releases them in
        // one call, the parts taken being zero.
        if rest.len() > 1 {
            return Ok(false);
        }
        for (ft, slots, field) in rest {
            let part: Vec<u32> = slots.iter().map(|&k| ls[k]).collect();
            let place = Place {
                root: PlaceRoot::Value(hd_tir::ir::Ref(r)),
                proj: step(field),
            };
            self.release_drop_place(&part, ft, &place)?;
            for l in part {
                self.f.zero(l);
            }
        }
        Ok(true)
    }

    /// A whole map behind a thin reference: uniqueness consumes
    /// the view's original value and restores it through the setter.
    /// Moving it out first keeps counts balanced and avoids forcing `CoW`
    /// by copying a view's header before the uniqueness check.
    pub(super) fn unique_ref_header(&mut self, i: u32) -> StageResult<Option<u32>> {
        if self.input.runtime.is_none() {
            return Ok(None);
        }
        let p = self.place(self.b.data[i as usize][0])?;
        let Some((d, Some((target, false)))) = self.last_through(&p)? else {
            return Ok(None);
        };
        if d + 1 != p.proj.len() || !self.is_map(target) {
            return Ok(None);
        }
        let (at, _) = self.walk_place(i, &p, d, None)?;
        let reference = self.at_i32(&at)?;
        let value = self.ref_layout(target)?;
        let helper = Helper::MapUnique {
            entry: self.entry(target)?.2,
        };
        let unique = |me: &mut Self| {
            let start = me.f.body.len();
            me.f.helper(helper.clone());
            if me.f.origins.is_some() {
                let subject = crate::views::count_subject(me.pool(), me.env(), target);
                me.f.note_count(
                    start,
                    u32::try_from(me.b.anchor_len()).expect("TIR instructions"),
                    crate::views::CountAction::Unique,
                    &subject,
                    hd_tir::own::reason(me.b, i as usize),
                );
            }
        };
        let header = self.f.local(Val::I32);
        self.f.get(reference);
        self.f.helper(Helper::RefIsView);
        self.f.op(I::If(wasm_encoder::BlockType::Empty));
        let [taken] = rt::ref_take(&mut self.f, reference, &value)[..] else {
            return unsupported("a collection view with more than one header");
        };
        self.f.get(taken);
        unique(self);
        self.f.set(header);
        rt::ref_store(&mut self.f, reference, &[header], &value, true);
        self.f.op(I::Else);
        self.f.get(reference);
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        unique(self);
        self.f.set(header);
        self.f.get(reference);
        self.f.get(header);
        self.f.op(I::I32Store(mem(HEAP, 0, 2)));
        self.f.op(I::End);
        Ok(Some(header))
    }

    /// The list or map buffer at the place `word` of an `IsUnique` or a
    /// `Pin`, in a local, with the collection's type: the place resolved
    /// from its root's value ([`Em::walk_place`]), each element
    /// bounds-checked. Nothing takes a count: the buffer is the one the
    /// place holds, which a view lends. Also where the
    /// walk found it in memory, where the place keeps it ([`Em::put_at`]).
    pub(super) fn place_header(&mut self, word: u32) -> StageResult<(u32, Ty, Option<At>)> {
        let p = self.place(word)?;
        let (at, ty) = self.walk_place(NONE, &p, p.proj.len(), None)?;
        let h = self.at_i32(&at)?;
        Ok((h, ty, (!matches!(at, At::Vals(_))).then_some(at)))
    }

    /// Stores the buffer in local `b` at `at`, where a walk found the
    /// place's collection ([`Em::place_header`]).
    pub(super) fn put_at(&mut self, at: &At, b: u32) -> StageResult<()> {
        let (a, off) = match at {
            At::Vals(ls) if ls.len() == 1 => {
                if ls[0] != b {
                    self.f.get(b);
                    self.f.set(ls[0]);
                }
                return Ok(());
            }
            At::Mem(a, off) => (*a, *off),
            At::Leaves(a, lv) if lv.len() == 1 => (*a, lv[0].offset),
            _ => return unsupported("a collection of several values"),
        };
        self.f.get(a);
        self.f.get(b);
        self.f.op(I::I32Store(mem(HEAP, u64::from(off), 2)));
        Ok(())
    }

    /// Stores the buffer in local `b`, which a change of its operand `r`
    /// left, where `r` came from: into the place a `borrow` read `r` names
    /// right after its `IsUnique`, else into `r`'s own value.
    pub(super) fn put_back(&mut self, r: u32, b: u32) -> StageResult<()> {
        if self.b.is_borrow(r) {
            return self.put_buffer(self.b.data[r as usize][0], b);
        }
        let ls = self.local_value_of(r)?;
        self.put_at(&At::Vals(ls), b)
    }

    /// Stores the buffer in local `b` into place `word`, raw: the change
    /// that replaced the place's buffer gave up its count of the old one,
    /// and the new one's count is the place's (`docs/compiler/tir-v2.md`,
    /// "List and Map buffers", 3f). The walk goes to the place's last
    /// element or dereference: an element's slot, or a reference's
    /// referent at its address, past the fields that follow. A place with
    /// neither is in its root: a local's Wasm locals or cell, a value's
    /// locals, a top-level binding's static slot or Wasm globals.
    pub(super) fn put_buffer(&mut self, word: u32, b: u32) -> StageResult<()> {
        let p = self.place(word)?;
        let (at, ty, from) = match self.last_through(&p)? {
            Some((d, Some((target, false)))) => {
                let (at, _) = self.walk_place(NONE, &p, d, None)?;
                let r = self.at_i32(&at)?;
                // A view's referent has no address of its own to store at.
                self.f.get(r);
                self.f.helper(Helper::RefIsView);
                self.f.op(I::If(wasm_encoder::BlockType::Empty));
                if d + 1 == p.proj.len() && self.is_map(target) {
                    // The change consumed the old table's count. Take its
                    // stale header out without releasing it before the
                    // setter restores the replacement's count.
                    let value = self.ref_layout(target)?;
                    rt::ref_take(&mut self.f, r, &value);
                    rt::ref_store(&mut self.f, r, &[b], &value, true);
                    self.f.op(I::Else);
                    self.f.get(r);
                    self.f.get(b);
                    self.f.op(I::I32Store(mem(HEAP, 0, 2)));
                    self.f.op(I::End);
                    return Ok(());
                }
                self.f.helper(Helper::Panic(
                    "internal: a collection changed through a reference view".into(),
                ));
                self.f.op(I::End);
                (At::Mem(r, 0), target, d + 1)
            }
            Some(_) => {
                let (at, ty) = self.walk_place(NONE, &p, p.proj.len(), None)?;
                (at, ty, p.proj.len())
            }
            None => match p.root {
                PlaceRoot::Local(l) if self.is_cell(l.idx()) => {
                    let t = self.at_root_ty(p.root);
                    // The change read the place, so the cell exists.
                    (At::Mem(self.local(l.raw())?[0], 0), t, 0)
                }
                root => {
                    let (at, ty) = self.at_root(root)?;
                    (at, ty, 0)
                }
            },
        };
        let globals = match (&at, p.root) {
            (At::Vals(ls), PlaceRoot::Global(def, _)) => Some((self.def_hash(def), ls.clone())),
            _ => None,
        };
        let (at, _) = self.walk_steps(at, ty, &p.proj[from..])?;
        self.put_at(&at, b)?;
        // A binding in Wasm globals takes its changed value back.
        if let (Some((h, ls)), At::Vals(x)) = (globals, &at) {
            let k = ls.iter().position(|l| x.first() == Some(l)).unwrap_or(0);
            self.f.get(b);
            self.f.global_set(crate::module::Global::Storage(
                h,
                u32::try_from(k).expect("k"),
                Val::I32,
            ));
        }
        Ok(())
    }

    /// `Ref` `i`: the place's address, or the trait value
    /// there for a reference to a trait value, with a count of its own.
    pub(super) fn place_ref(&mut self, i: u32) -> StageResult<()> {
        // A promoted argument is its place's value (§23.40), moved out of
        // its local when the reference takes the local's count.
        if self.promoted.contains(&i) {
            if self.b.data[i as usize][1] & hd_tir::ir::ref_word::TAKES != 0 {
                return self.promoted_take(i);
            }
            return self.place_read(i);
        }
        // A reference the move pass marked to take its local's count of
        // the cell (`docs/compiler/tir-v2.md`, "Move" rule 4): the cell's
        // address, owned, and the local left empty. Where the reference
        // is no cell's address at this instance (a trait value's), it
        // takes a count of its own and the local's count
        // is released here, as the `Drop` the pass deleted did. A frame
        // local's count is never taken (`ref_word::FRAME`).
        if self.b.data[i as usize][1] & hd_tir::ir::ref_word::TAKES != 0 {
            let p = self.place(self.b.data[i as usize][0])?;
            let (PlaceRoot::Local(l), true) = (p.root, p.proj.is_empty()) else {
                return unsupported("a reference that takes the count of no whole local");
            };
            let x = l.idx();
            if self.slots.contains_key(&x) {
                return unsupported("a reference that takes a frame slot's local");
            }
            let fat = self.ref_target(self.ty(i)).is_some_and(|(_, f)| f);
            if self.is_cell(x) && !fat {
                let at = self.cell_of(x)?;
                self.f.get(at);
                self.store_owned(i)?;
                self.f.zero(at);
                return Ok(());
            }
            self.counted_ref(i)?;
            return self.release_local(x);
        }
        self.counted_ref(i)
    }

    /// Promoted `Ref` `i` that takes its whole local's count: the local's
    /// value with that count, and the local left empty.
    fn promoted_take(&mut self, i: u32) -> StageResult<()> {
        let p = self.place(self.b.data[i as usize][0])?;
        let (PlaceRoot::Local(l), true) = (p.root, p.proj.is_empty()) else {
            return unsupported("a promoted reference that takes the count of no whole local");
        };
        let x = l.idx();
        if self.is_cell(x) || self.slots.contains_key(&x) {
            return unsupported("a promoted reference that takes a cell's local");
        }
        let ls = self.local(u32::try_from(x).expect("locals"))?;
        for &v in &ls {
            self.f.get(v);
        }
        self.store_owned(i)?;
        if self.counted(self.ty(i)).is_some() {
            for v in ls {
                self.f.zero(v);
            }
        }
        Ok(())
    }

    /// `Ref` `i`: the address of its place, with a count of its own. The
    /// walk ([`Em::walk_place`]) goes from where [`Em::place_start`]
    /// starts it to the head, the place's last element or dereference: an
    /// element's slot, bounds-checked where the walk takes the place
    /// ([`Em::rooted_at`]), else at the buffer's address with no check, or
    /// at a map entry's value; a reference's referent, or the trait value
    /// for a reference to one; a data object's field. A place with
    /// neither is in its root's storage. [`Em::ref_from`] addresses the
    /// struct field the fields after the head name.
    fn counted_ref(&mut self, i: u32) -> StageResult<()> {
        let p = self.place(self.b.data[i as usize][0])?;
        let Some((_, fat)) = self.ref_target(self.ty(i)) else {
            return unsupported("a reference whose type is no reference");
        };
        let last = self.last_through(&p)?;
        let start = self.place_start(i, &p, last)?;
        // A data object's field is the head with its dereference.
        let (upto, head) = match last {
            Some((d, None)) if p.proj[d] == Proj::Deref => ((d + 2).min(p.proj.len()), None),
            Some((d, through)) => (d, Some(through)),
            None => (0, None),
        };
        let (mut at, mut ty) = self.walk_place(i, &p, upto, start)?;
        // A capture's copy lies in the closure's environment too, which
        // outlives the code's call: a local reference points there. (A
        // receiver's counted carrier points at a whole object: a copy.)
        if let (At::Vals(_), PlaceRoot::Local(l), None) = (&at, p.root, head)
            && let Some(&(env, off)) = self.env_caps.get(&l.idx())
            && matches!(self.kind(self.ty(i)), Kind::Ref { whole: false, .. })
        {
            at = At::Mem(env, off);
        }
        let mut from = upto;
        if let Some(through) = head {
            from += 1;
            (at, ty) = match (p.proj[upto], through) {
                (Proj::Elem(ix), _) if self.rooted_at(i) => {
                    let b = self.at_i32(&at)?;
                    let (et, slot) = self.elem_slot(b, ty, ix.0)?;
                    (At::Mem(slot, LIST_ELEMS), et)
                }
                (Proj::Elem(ix), _) if self.is_map(ty) => {
                    let h = self.at_i32(&at)?;
                    let (vt, a) = self.map_value_at(h, ty, ix.0)?;
                    (At::Mem(a, 0), vt)
                }
                (Proj::Elem(ix), _) => {
                    let (et, e) = self.elem(ty)?;
                    let b = self.at_i32(&at)?;
                    let a = self.f.local(Val::I32);
                    self.f.get(b);
                    // An element of no bytes lies at no address of its own;
                    // the length word keeps the address inside the buffer.
                    if e.size == 0 {
                        self.f.u32(u32::try_from(LIST_LEN).expect("offset"));
                    } else {
                        self.comp(ix.0, 0)?;
                        self.f.u32(e.size);
                        self.f.op(I::I32Mul);
                        self.f.op(I::I32Add);
                        self.f.u32(LIST_ELEMS);
                    }
                    self.f.op(I::I32Add);
                    self.f.set(a);
                    (At::Mem(a, 0), et)
                }
                (_, Some((target, true))) => (at, target),
                (_, Some((target, false))) => (At::Mem(self.at_i32(&at)?, 0), target),
                (_, None) => return unsupported("a dereference of a value that is no reference"),
            };
        }
        let Some(path) = fields(&p.proj[from..]) else {
            self.payload_ref(i, &at, ty, &p.proj[from..], fat)?;
            return Ok(());
        };
        self.ref_from(i, at, ty, &path, fat)?;
        Ok(())
    }

    /// The rest of `Ref` `i` when `steps` go through a variant's payload
    /// (`ref.match.borrow`): the field's address in the enum at `base`, of
    /// type `ty`, or, where the enum's shared slots hold its leaves at no
    /// one offset, a view whose getter and setter pick them out.
    fn payload_ref(
        &mut self,
        i: u32,
        base: &At,
        parent_ty: Ty,
        steps: &[Proj],
        fat: bool,
    ) -> StageResult<()> {
        if fat {
            return unsupported("a reference to a payload field that is a trait value");
        }
        let &At::Mem(a, off) = base else {
            return unsupported("a reference to a payload field in no memory");
        };
        let parent = self.ref_layout(parent_ty)?;
        let mut picks: Vec<u32> =
            (0..u32::try_from(parent.leaves.len()).expect("leaves")).collect();
        let mut ty = parent_ty;
        let mut k = 0;
        while k < steps.len() {
            match (steps[k], steps.get(k + 1)) {
                (Proj::Variant(v), Some(&Proj::Field(f))) => {
                    if matches!(self.kind(ty), Kind::Boxed) {
                        return unsupported("a reference to a payload field of a boxed enum");
                    }
                    let layout = self.flat(ty)?;
                    let tys = self.variant_tys(ty)?;
                    let (Some(sel), Some(&ft)) = (
                        layout.1.get(v as usize).and_then(|fs| fs.get(f as usize)),
                        tys.get(v as usize).and_then(|fs| fs.get(f as usize)),
                    ) else {
                        return unsupported("a payload field the enum does not have");
                    };
                    picks = sel.iter().map(|&p| picks[p]).collect();
                    ty = ft;
                    k += 2;
                }
                (Proj::Field(f), _) => {
                    let (ft, part) = self.path_step(ty, f)?;
                    picks = picks[part.first..part.first + part.count].to_vec();
                    ty = ft;
                    k += 1;
                }
                _ => return unsupported("a reference to a payload field through this place"),
            }
        }
        let field = self.ref_layout(ty)?;
        let fits = picks.len() == field.leaves.len()
            && picks
                .iter()
                .zip(&field.leaves)
                .all(|(&p, l)| parent.leaves[p as usize].val == l.val);
        if !fits {
            return unsupported("a payload field whose leaves differ from its slots");
        }
        let offset = picks
            .first()
            .map_or(0, |&p| parent.leaves[p as usize].offset);
        let at = self.offset_local(a, off);
        // A field laid out at one offset of the enum, in leaves that follow
        // each other, is an address there: a narrow one reads from its
        // slot's low bytes, and an unsigned one writes there too.
        let mutable = crate::layout::ref_mutable(self.pool(), self.env(), self.ty(i));
        let first = picks.first().copied().unwrap_or(0);
        let physical = picks
            .iter()
            .zip(&field.leaves)
            .zip(first..)
            .all(|((&p, l), k)| {
                let slot = &parent.leaves[p as usize];
                p == k
                    && slot.offset == offset + l.offset
                    && (slot.bytes == l.bytes || (slot.bytes > l.bytes && !(mutable && l.signed)))
            });
        if physical {
            return self.ref_projection(i, at, parent, &field, first, offset, false);
        }
        self.f.get(at);
        self.f.helper(Helper::RefPayload {
            parent,
            field,
            picks,
            offset,
        });
        self.store_owned(i)?;
        Ok(())
    }

    /// The rest of `Ref` `i` of the part at `off` of the value at address
    /// `at`, the leaves from `first`: a physical place projects by the
    /// offset, and only a representation view needs a forwarding object and
    /// its helper call.
    #[allow(clippy::too_many_arguments)]
    fn ref_projection(
        &mut self,
        i: u32,
        at: u32,
        parent: RefLayout,
        field: &RefLayout,
        first: u32,
        off: u32,
        fat: bool,
    ) -> StageResult<()> {
        let loan = self.begin_view_loan(i)?;
        if let Some(flag) = loan {
            self.f.get(at);
            self.f.helper(Helper::RefIsView);
            self.f.set(flag);
        }
        if let Some(flag) = loan {
            self.f.get(flag);
        } else {
            self.f.get(at);
            self.f.helper(Helper::RefIsView);
        }
        self.f
            .op(I::If(wasm_encoder::BlockType::Result(Val::I32.wasm())));
        self.f.get(at);
        self.f.helper(Helper::RefProject {
            borrowed: loan.is_some(),
            parent,
            field: field.clone(),
            first,
            offset: off,
        });
        self.f.op(I::Else);
        self.f.get(at);
        if off != 0 {
            self.f.u32(off);
            self.f.op(I::I32Add);
        }
        // A plain address counts nothing; a trait value read at it is
        // released below as the view's projection would be.
        if loan.is_none()
            && fat
            && let Some(runtime) = self.m.runtime
        {
            let physical = self.f.local(Val::I32);
            self.f.set(physical);
            self.f.get(physical);
            self.f
                .count_call(Sym::Inst(runtime.retain), crate::views::CountAction::Retain);
            self.f.get(physical);
        }
        self.f.op(I::End);
        if fat {
            let projected = self.f.local(Val::I32);
            self.f.set(projected);
            self.load_through(projected, field);
            self.release_open(projected);
        }
        self.store_owned(i)?;
        Ok(())
    }

    /// The rest of `Ref` `i` from where its place's head lies (`base`, of
    /// type `ty`, in memory or in locals): the address of the struct field
    /// `path` reaches there, with a count of its own.
    fn ref_from(
        &mut self,
        i: u32,
        base: At,
        mut ty: Ty,
        path: &[u32],
        fat: bool,
    ) -> StageResult<()> {
        let base = match base {
            At::Mem(a, off) => At::Mem(self.offset_local(a, off), 0),
            base => base,
        };
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
        if let At::Mem(at, _) = &base
            && has_path
        {
            let parent = self.ref_layout(parent_ty)?;
            let field = self.ref_layout(ty)?;
            let first = u32::try_from(first).expect("field");
            return self.ref_projection(i, *at, parent, &field, first, off, fat);
        }
        match base {
            At::Mem(at, _) if fat => {
                let value = self.ref_layout(ty)?;
                self.load_through(at, &value);
                self.store_owned(i)?;
                return Ok(());
            }
            At::Mem(at, _) => {
                self.f.get(at);
                if off != 0 {
                    self.f.u32(off);
                    self.f.op(I::I32Add);
                }
            }
            At::Vals(ls) if fat => {
                for l in &ls[first..first + m.leaves.len()] {
                    self.f.get(*l);
                }
            }
            At::Vals(_) if crate::layout::ref_mutable(self.pool(), self.env(), self.ty(i)) => {
                return unsupported("a `&mut` to a local that lives in no cell");
            }
            At::Vals(ls) => {
                // A local no cell holds, as a closure's copy of what it
                // captured, which never changes: the reference is to a new
                // object holding a copy (`ref.expr.allocate`).
                let w = self.counted(ty);
                let cyclic = self.cyclic_holding(&[ty]);
                let recursive = self.recursive_object(ty);
                let obj = self.object(&m, w.as_ref(), cyclic, recursive, Some(ty));
                let src = Src::Locals(ls[first..first + m.leaves.len()].to_vec());
                let all = Part {
                    first: 0,
                    count: m.leaves.len(),
                    offset: 0,
                };
                self.store_part(obj, &m, all, &src)?;
                let colls = self.copied(ty);
                self.retain_fields(obj, &m, w.as_ref(), &colls, colls.len());
                self.f.get(obj);
                // A local reference owns the copy; a counted carrier's
                // count of it is the carrier's own.
                if matches!(self.counted(self.ty(i)), Some(crate::layout::Walk::View(_))) {
                    self.owned_refs.insert(i);
                }
                return self.store_owned(i);
            }
            At::Globals(..) | At::Leaves(..) => {
                return unsupported("a reference to a place in no memory");
            }
        }
        self.store(i)
    }

    /// `Read` `i`: only the leaves the place names, a copy with a count of
    /// its own or a borrow with none. The walk ([`Em::walk_place`]) goes
    /// from where [`Em::place_start`] starts it; an element read of a
    /// value (`Read [%xs [%i]]`) starts at its buffer, unchecked where
    /// the loop's range proof covers it. A read through a reference that
    /// only fields follow reads that part of its referent
    /// ([`Em::read_part`]); a reference to a trait value is that value. A
    /// `move` reaches here only for a part at no place of the root's
    /// locals, which is read as a copy: the root's `Drop`, which the move
    /// pass keeps for such a root, releases it.
    pub(super) fn place_read(&mut self, i: u32) -> StageResult<()> {
        let p = self.place(self.b.data[i as usize][0])?;
        let last = self.last_through(&p)?;
        let start = self.place_start(i, &p, last)?;
        if let Some((d, Some((target, fat)))) = last {
            let (at, ref_ty) = self.walk_place(i, &p, d, start)?;
            let steps = &p.proj[d + 1..];
            let fields = fields(steps);
            if fat {
                if fields.as_ref().is_none_or(|fields| !fields.is_empty()) {
                    return unsupported("a field read through a trait value reference");
                }
                self.push_at(&at, ref_ty)?;
                self.store(i)?;
                return Ok(());
            }
            let r = self.at_i32(&at)?;
            let value = self.ref_layout(target)?;
            if fields.as_ref().is_some_and(Vec::is_empty) {
                // A loan is the leaves its place holds, through a view
                // too.
                if let Some(flag) = self.begin_view_loan(i)? {
                    for l in rt::ref_load_borrowed(&mut self.f, r, &value, flag) {
                        self.f.get(l);
                    }
                } else if self.loans.contains(&i) {
                    for l in rt::ref_lend(&mut self.f, r, &value) {
                        self.f.get(l);
                    }
                } else {
                    self.load_through(r, &value);
                }
                self.store_owned(i)?;
            } else if let Some(fields) = fields {
                let declared = self.declared_referent(&p, d)?;
                let (_, part) = self.fold_path(declared, &fields)?;
                self.read_part(i, r, &value, part)?;
            } else {
                let declared = self.declared_referent(&p, d)?;
                self.read_path(i, r, declared, steps)?;
            }
            return Ok(());
        }
        if self.cell_part_move(i, &p)? {
            return Ok(());
        }
        let (at, ty) = self.walk_place(i, &p, p.proj.len(), start)?;
        self.push_at(&at, ty)?;
        self.store(i)?;
        Ok(())
    }

    /// `Read move` `i` of a field path of a local whose value lives in
    /// memory (a cell or a frame slot, `[$l .a]`): the part's count, and
    /// zeros left in its place, which the local's `Drop` then releases as
    /// nothing. False, emitting nothing, for any other read.
    fn cell_part_move(&mut self, i: u32, p: &Place) -> StageResult<bool> {
        let PlaceRoot::Local(l) = p.root else {
            return Ok(false);
        };
        let l = l.idx();
        let Some(path) = fields(&p.proj) else {
            return Ok(false);
        };
        if self.b.data[i as usize][1] != hd_tir::ir::ReadMode::Move as u32
            || path.is_empty()
            || !self.is_cell(l)
        {
            return Ok(false);
        }
        let (m, _) = self.cell_mem(l)?;
        let (pt, part) = self.fold_path(self.sub(self.b.local_ty[l]), &path)?;
        let leaves = m.leaves[part.first..part.first + part.count].to_vec();
        let c = self.cell_of(l)?;
        for leaf in &leaves {
            self.f.get(c);
            self.f.op(leaf.load(0));
        }
        self.store_owned(i)?;
        self.mark_drop_place(p, false);
        if self.needs_drop(pt) {
            for leaf in &leaves {
                self.f.get(c);
                self.zero(leaf.val);
                self.f.op(leaf.store(0));
            }
        }
        Ok(true)
    }

    /// Reads only `part` of the value that the reference in local `at`
    /// names, into instruction `dest`'s value, as a read of a physical
    /// place does: through a view, of the leaves the view lends
    /// ([`rt::ref_lend`]), else of the leaves at the address.
    fn read_part(&mut self, dest: u32, at: u32, value: &RefLayout, part: Part) -> StageResult<()> {
        self.f.get(at);
        self.f.helper(Helper::RefIsView);
        self.f.op(I::If(wasm_encoder::BlockType::Empty));
        let parent = rt::view_lend(&mut self.f, at, value);
        for l in &parent[part.first..part.first + part.count] {
            self.f.get(*l);
        }
        self.store(dest)?;
        self.f.op(I::Else);
        for leaf in &value.leaves[part.first..part.first + part.count] {
            self.f.get(at);
            self.f.op(leaf.load(0));
        }
        self.store(dest)?;
        self.f.op(I::End);
        Ok(())
    }

    /// Reads a payload path from the value reference `at` names. A recursive
    /// enum stores its payload in its boxed cell, while a flat enum selects
    /// shared slots; [`Em::walk_steps`] handles both layouts for a view and a
    /// physical referent.
    fn read_path(&mut self, dest: u32, at: u32, ty: Ty, steps: &[Proj]) -> StageResult<()> {
        if steps
            .iter()
            .any(|step| matches!(step, Proj::Deref | Proj::Elem(_)))
        {
            return unsupported("a payload read through this reference");
        }
        let value = self.ref_layout(ty)?;
        self.f.get(at);
        self.f.helper(Helper::RefIsView);
        self.f.op(I::If(wasm_encoder::BlockType::Empty));
        let parent = rt::view_lend(&mut self.f, at, &value);
        let (view, view_ty) = self.walk_steps(At::Vals(parent), ty, steps)?;
        self.push_at(&view, view_ty)?;
        self.store(dest)?;
        self.f.op(I::Else);
        let (place, place_ty) = self.walk_steps(At::Mem(at, 0), ty, steps)?;
        self.push_at(&place, place_ty)?;
        self.store(dest)?;
        self.f.op(I::End);
        Ok(())
    }

    /// Pushes the value that the reference in local `at` names, with
    /// counts of its own ([`rt::ref_load`]).
    fn load_through(&mut self, at: u32, value: &RefLayout) {
        for l in rt::ref_load(&mut self.f, at, value) {
            self.f.get(l);
        }
    }

    /// `Store` `i`: replaces the value at its place ([`Em::replace`]),
    /// which takes a count of the new value before the old one's is
    /// released; or writes through the reference its last dereference
    /// reads, when only fields follow ([`Em::deref_write_at`],
    /// [`Em::deref_field_write_at`]). The walk ([`Em::walk_place`])
    /// resolves the place from where [`Em::place_start`] starts it. A
    /// whole local in a cell takes its value as [`Em::cell_set`] says; a
    /// whole local that borrows ([`Em::loan_locals`]) takes it with no
    /// count, and `NONE` (an empty initialization) not at all; a whole
    /// top-level binding in its static slot releases its old value after
    /// the write, as a cell does ([`Em::replace_late`]). The passes'
    /// `IsUnique` came before a store into an element.
    pub(super) fn place_store(&mut self, i: u32) -> StageResult<()> {
        let v = self.b.data[i as usize][1];
        let p = self.place(self.b.data[i as usize][0])?;
        let rooted = Self::rooted(&p);
        let whole = p.proj.is_empty();
        // A whole local's Wasm locals are made at its store, whatever it
        // stores.
        let local = match p.root {
            PlaceRoot::Local(l) if whole && !self.is_cell(l.idx()) => {
                Some((l.idx(), self.local(l.raw())?))
            }
            PlaceRoot::Local(l) if whole => Some((l.idx(), Vec::new())),
            _ => None,
        };
        // A local reference that owns its object moves it into the local,
        // whose release then releases the object.
        if let Some((l, _)) = &local
            && !self.is_cell(*l)
            && v != NONE
            && self.owned_refs.contains(&v)
        {
            self.owned_locals.insert(*l);
        }
        match &local {
            Some((l, _)) if self.is_cell(*l) && (v == NONE || !self.diverges(v)) => {
                return self.cell_set(i, *l, v);
            }
            Some((l, ls)) if !self.is_cell(*l) && self.loan_locals.contains_key(l) => {
                // A projected reference can own a fresh runtime view even
                // when the physical reference is a loan. Transfer that
                // ownership with the binding, until the local's last use.
                self.take(v);
                return self.write_new(Dst::Locals(ls), &Src::Ref(v));
            }
            Some(_) if v == NONE => return Ok(()),
            // Stage 1's store into a value's element does not test its
            // value.
            None if !rooted && matches!(p.proj.first(), Some(Proj::Elem(_))) => {}
            _ if v != NONE && self.diverges(v) => {
                self.f.op(I::Unreachable);
                return Ok(());
            }
            _ => {}
        }
        let last = self.last_through(&p)?;
        let start = self.place_start(i, &p, last)?;
        if let Some((d, Some((target, fat)))) = last {
            if fat {
                return unsupported("a write through a trait value reference");
            }
            let (at, _) = self.walk_place(i, &p, d, start)?;
            let r = self.at_i32(&at)?;
            match fields(&p.proj[d + 1..]) {
                Some(fs) if fs.is_empty() => self.deref_write_at(r, target, v)?,
                Some(fs) => self.deref_field_write_at(r, target, &fs, v)?,
                None => return unsupported("a payload write through a reference"),
            }
            return Ok(());
        }
        let (at, ty) = self.walk_place(i, &p, p.proj.len(), start)?;
        let leaves;
        let d = match &at {
            At::Vals(ls) if !rooted => Dst::Locals(ls),
            At::Globals(h, first, vals) => Dst::Globals(*h, *first, vals),
            _ => {
                let Some((a, lv)) = self.at_leaves(&at, ty)? else {
                    return unsupported("a store into a place in no memory");
                };
                leaves = lv;
                Dst::Mem(a, 0, &leaves)
            }
        };
        if whole && matches!(at, At::Mem(..)) {
            // The old value's locals come before the new value's count.
            let old = self.old_locals(d);
            let s = self.consumed(v)?;
            self.replace_late(d, ty, &s, &old)?;
        } else {
            self.replace(d, ty, v)?;
        }
        Ok(())
    }

    /// Where the walk of place `p` of `Read`, `Store` or `Ref` `i`, whose
    /// last element or dereference is `last` ([`Em::last_through`]),
    /// starts: at its root's value ([`Em::at_root`], `None`) for a place
    /// the walk takes ([`Em::rooted_at`]). A place stage 1 lowers starts
    ///
    /// - at an element of a list or a map value, read or written: from
    ///   its buffer ([`Em::buffer`]);
    /// - at a dereference that a store writes through, or of a reference
    ///   a read loads through: from the address, copied into a local;
    /// - with no element or dereference, written or addressed: in its
    ///   root, a local's Wasm locals or cell, or a top-level binding's
    ///   static slot or Wasm globals (a store's only).
    ///
    /// Any other read or reference starts at its root's value; any other
    /// store is into a place that is not writable.
    fn place_start(
        &mut self,
        i: u32,
        p: &Place,
        last: Option<(usize, Option<Referent>)>,
    ) -> StageResult<Option<(At, Ty)>> {
        let tag = self.b.tags[i as usize];
        let t = self.at_root_ty(p.root);
        let at = match (p.root, last) {
            (_, None) if tag == Tag::Store && self.rooted_at(i) => {
                return unsupported("a store into a place in no memory");
            }
            _ if self.rooted_at(i) => return Ok(None),
            (PlaceRoot::Value(r), Some((0, _)))
                if tag != Tag::Ref && matches!(p.proj[0], Proj::Elem(_)) =>
            {
                At::Vals(vec![self.buffer(r.0)?])
            }
            (root, Some((0, through)))
                if tag == Tag::Store || tag == Tag::Read && matches!(through, Some((_, false))) =>
            {
                let x = self.f.local(Val::I32);
                match root {
                    PlaceRoot::Local(l) => {
                        let ls = self.local_value(l.raw())?;
                        self.f.get(ls[0]);
                    }
                    PlaceRoot::Value(r) => self.comp(r.0, 0)?,
                    _ => return unsupported("a dereference of a binding's or a frame's place"),
                }
                self.f.set(x);
                At::Vals(vec![x])
            }
            (_, Some(_)) if tag != Tag::Store => return Ok(None),
            (_, None) if tag == Tag::Read => return Ok(None),
            (PlaceRoot::Local(l), None) if !self.is_cell(l.idx()) => At::Vals(self.local(l.raw())?),
            (PlaceRoot::Local(l), None) => At::Mem(self.cell_of(l.idx())?, 0),
            (PlaceRoot::Global(def, _), None) if tag == Tag::Store => {
                let h = self.def_hash(def);
                let Some(slot) = self.def_slot(def) else {
                    self.note_stored(h, t, None)?;
                    return Ok(Some((At::Globals(h, 0, self.vals(t)?), t)));
                };
                let m = mem_of(self.pool(), self.env(), t)?;
                self.note_stored(h, t, Some((slot, m.leaves)))?;
                return self.at_root(p.root).map(Some);
            }
            (PlaceRoot::Global(def, _), None) if self.def_slot(def).is_none() => {
                return unsupported("a top-level binding without its static slot");
            }
            (root, None) if tag == Tag::Ref => return self.at_root(root).map(Some),
            _ => return unsupported("a store into a place that is not writable"),
        };
        Ok(Some((at, t)))
    }

    /// A write of `v` through the reference in local `at` to a `target`.
    fn deref_write_at(&mut self, at: u32, target: Ty, v: u32) -> StageResult<()> {
        let value = self.ref_layout(target)?;
        let ls: Vec<u32> = value.leaves.iter().map(|l| self.f.local(l.val)).collect();
        // A value the store consumes moves its count into the place.
        let moved = self.take(v);
        self.write_new(Dst::Locals(&ls), &Src::Ref(v))?;
        rt::ref_store(&mut self.f, at, &ls, &value, moved);
        Ok(())
    }

    /// A write of `v` into the struct field `path` reaches in the `target`
    /// the reference in local `at` refers to. A physical place takes the
    /// value at its offset; a representation view takes it through a field
    /// view (`RefProject`) made for the write and released after it.
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
        // A value the store consumes moves its count into the place.
        let moved = self.take(v);
        self.write_new(Dst::Locals(&ls), &Src::Ref(v))?;
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
        rt::ref_store(&mut self.f, dst, &ls, &field, moved);
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
    pub(super) fn def_slot(&self, def: DefId) -> Option<crate::module::Address> {
        self.m.addressed.get(&self.env().path_hash(def)).copied()
    }

    /// `Read [@g]` `i` of a binding in static slot `at`.
    pub(super) fn slot_get(&mut self, i: u32, at: crate::module::Address) -> StageResult<()> {
        let m = mem_of(self.pool(), self.env(), self.ty(i))?;
        for leaf in &m.leaves {
            self.f.address(at);
            self.f.op(leaf.load(0));
        }
        self.store(i)
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
    /// walk from its root's value: rooted at a local, with an element or
    /// a variant among its steps, or a dereference after its first step;
    /// or rooted at a top-level binding, read through any step or reached
    /// through one that is no field. Any other place starts where stage 1
    /// lowered it ([`Em::place_start`]).
    pub(super) fn rooted_at(&self, i: u32) -> bool {
        let read = self.b.tags[i as usize] == Tag::Read;
        self.b
            .decode_place(self.b.data[i as usize][0])
            .is_some_and(|p| {
                Self::rooted(&p)
                    || read && matches!(p.root, PlaceRoot::Global(..)) && !p.proj.is_empty()
            })
    }

    /// Whether place `p` takes the walk ([`Em::rooted_at`]).
    pub(super) fn rooted(p: &Place) -> bool {
        let global = matches!(p.root, PlaceRoot::Global(..));
        // A store folded through memory behind another value
        // (`[*%r [%i]]`, `[%v [%i] [%j]]`).
        let behind = matches!(p.root, PlaceRoot::Value(_))
            && p.proj
                .iter()
                .filter(|s| matches!(s, Proj::Elem(_) | Proj::Deref))
                .count()
                > 1;
        (behind || matches!(p.root, PlaceRoot::Local(_) | PlaceRoot::Global(..)))
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
            PlaceRoot::Local(l) => {
                Ok((At::Vals(self.local_value(l.raw())?), self.at_root_ty(root)))
            }
            PlaceRoot::Global(def, t) => {
                let t = self.sub(t);
                if let Some(slot) = self.def_slot(def) {
                    let a = self.f.local(Val::I32);
                    self.f.address(slot);
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
            PlaceRoot::Value(r) => Ok((At::Vals(self.local_value_of(r.0)?), self.ty_of(r.0))),
            _ => unsupported("a place rooted at a frame slot or a pointer's target"),
        }
    }

    /// A local holding the one 32-bit value at `at`: a list buffer, a
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
            At::Vals(_) | At::Globals(..) => None,
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
        let r = part.first..part.first + part.count;
        let at = match at {
            At::Vals(ls) => At::Vals(ls[r].to_vec()),
            At::Globals(h, k, vals) => At::Globals(h, k + part.first, vals[r].to_vec()),
            At::Mem(a, off) => At::Mem(a, off + part.offset),
            At::Leaves(a, lv) => At::Leaves(a, lv[r].to_vec()),
        };
        Ok((at, ft))
    }

    /// Field `f` of variant `v`'s payload of the enum at `at`: its values
    /// among the enum's, in the boxed enum's cell when it has one.
    fn at_variant(&mut self, at: At, ty: Ty, v: u32, f: u32) -> StageResult<(At, Ty)> {
        let flat = self.flat(ty)?;
        let (m, positions) = (&flat.0, &flat.1);
        let tys = self.variant_tys(ty).unwrap_or_default();
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
                At::Globals(..) => return unsupported("a payload field in Wasm globals"),
            }
        };
        Ok((at, ft))
    }

    /// Walks the steps of `p` up to step `upto` from its root's value
    /// (`docs/compiler/tir-v2.md`, "Places"): a field picks its part, a
    /// variant its payload field, a dereference of a data value the
    /// object's field, an element its slot in the buffer, bounds-checked
    /// (unchecked where the loop's range proof covers read `i`, and from the
    /// loop's cached buffer where the list is a stable list of a local).
    /// A dereference of a reference goes to its referent's address; where
    /// a reference view may be reached, the walk reads on in the value the
    /// view lends to the next step's address. So every dereference before
    /// `upto` is followed, by `upto`, by another element or dereference, or
    /// by fields and variants down to one 32-bit value.
    pub(super) fn walk_place(
        &mut self,
        i: u32,
        p: &Place,
        upto: usize,
        start: Option<(At, Ty)>,
    ) -> StageResult<(At, Ty)> {
        let steps = &p.proj[..upto];
        let local = match p.root {
            PlaceRoot::Local(l) => Some(l.raw()),
            _ => None,
        };
        // A stable list's buffer from the loop's cache: an element of the
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
        // The first element step of a local's list, of the list the
        // reference it holds refers to, or of a value's list: a loop's
        // range proof names these, and its cache a local's.
        let lead = match (local, steps) {
            (_, [Proj::Elem(_), ..]) => Some(0),
            (Some(_), [Proj::Deref, Proj::Elem(_), ..]) => Some(1),
            _ => None,
        };
        let lead_cache = lead.and_then(|e| cached(self, e == 1).map(|c| (c, e)));
        if let Some((cache, elem)) = lead_cache {
            // The cached buffer, where no view is reachable; else the walk.
            let h = self.f.local(Val::I32);
            self.f.helper(Helper::RefViewsAbsent);
            self.f
                .op(I::If(wasm_encoder::BlockType::Result(Val::I32.wasm())));
            self.f.get(cache.buffer);
            self.f.op(I::Else);
            let (a, t) = self.at_root(p.root)?;
            let (a, _) = self.walk_steps(a, t, &steps[..elem])?;
            let x = self.at_i32(&a)?;
            self.f.get(x);
            self.f.op(I::End);
            self.f.set(h);
            self.cached_buffers.push((h, cache.len));
            ty = self.walk_ty(self.at_root_ty(p.root), &steps[..elem])?;
            at = At::Vals(vec![h]);
            k = elem;
        } else {
            (at, ty) = match start {
                Some(s) => s,
                None => self.at_root(p.root)?,
            };
        }
        while k < steps.len() {
            let Proj::Elem(ix) = steps[k] else {
                let next = steps[k + 1..]
                    .iter()
                    .position(|s| matches!(s, Proj::Elem(_)))
                    .map_or(steps.len(), |x| k + 1 + x);
                (at, ty) = self.walk_steps(at, ty, &steps[k..next])?;
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
            let b = self.at_i32(&at)?;
            // The first element of a list, which a loop's range proof may
            // cover.
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
    pub(super) fn at_root_ty(&self, root: PlaceRoot) -> Ty {
        match root {
            PlaceRoot::Local(l) => self.sub(self.b.local_ty[l.idx()]),
            PlaceRoot::Value(r) => self.ty_of(r.0),
            PlaceRoot::Global(_, t) => self.sub(t),
            PlaceRoot::Frame(_) | PlaceRoot::Ptr(_) => Ty::VOID,
        }
    }

    /// The type steps `steps` reach from type `ty`.
    pub(super) fn walk_ty(&self, mut ty: Ty, steps: &[Proj]) -> StageResult<Ty> {
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
                    let tys = self.variant_tys(ty).unwrap_or_default();
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
    fn walk_steps(&mut self, mut at: At, mut ty: Ty, steps: &[Proj]) -> StageResult<(At, Ty)> {
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
                    Kind::Ref {
                        target, fat: false, ..
                    } => {
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
                        // in locals both ways: through a view, from the
                        // leaves it lends ([`rt::ref_lend`]); an open value
                        // there may be a box that goes once the view has it
                        // back. Else loaded from the referent's
                        // address.
                        let t = self.walk_ty(target, &steps[k + 1..next])?;
                        let xs: Vec<u32> =
                            self.vals(t)?.into_iter().map(|v| self.f.local(v)).collect();
                        let value = self.ref_layout(target)?;
                        self.f.get(r);
                        self.f.helper(Helper::RefIsView);
                        self.f.op(I::If(wasm_encoder::BlockType::Empty));
                        let vals = rt::view_lend(&mut self.f, r, &value);
                        let (a, _) =
                            self.walk_steps(At::Vals(vals.clone()), target, &steps[k + 1..next])?;
                        self.push_at(&a, t)?;
                        for x in xs.iter().rev() {
                            self.f.set(*x);
                        }
                        self.f.op(I::Else);
                        let (a, _) = self.walk_steps(At::Mem(r, 0), target, &steps[k + 1..next])?;
                        self.push_at(&a, t)?;
                        for x in xs.iter().rev() {
                            self.f.set(*x);
                        }
                        self.f.op(I::End);
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
        let ty = self.walk_ty(self.at_root_ty(p.root), &p.proj[..d])?;
        Ok(Some((d, self.ref_target(ty))))
    }

    /// The type that the reference at step `d` of `p` refers to, as
    /// declared. [`Em::last_through`]'s target has its newtypes erased; this
    /// one keeps them, so a field path through the reference steps into a
    /// newtype's one field ([`Em::path_step`]), as `string(*name)` does.
    pub(super) fn declared_referent(&self, p: &Place, d: usize) -> StageResult<Ty> {
        let ty = self.walk_ty(self.at_root_ty(p.root), &p.proj[..d])?;
        let (pool, env) = (self.pool(), self.env());
        let declared = match pool.get(crate::layout::strip(pool, env, ty)) {
            TyData::Adt { def, args } if env.ref_cells().holds(def) => {
                pool.list_items(args).first().copied()
            }
            _ => None,
        };
        declared.map_or_else(
            || unsupported("a dereference of a value that is not a reference"),
            |t| Ok(self.strip_mut(t)),
        )
    }
}

/// The struct field indices `steps` name, when they are all fields.
fn fields(steps: &[Proj]) -> Option<Vec<u32>> {
    steps
        .iter()
        .map(|s| match s {
            Proj::Field(f) => Some(*f),
            _ => None,
        })
        .collect()
}
