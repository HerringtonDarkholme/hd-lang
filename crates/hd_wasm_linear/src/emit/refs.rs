//! References as addresses (`crates/hd_wasm_linear/DESIGN.md` §23.5, §23.20). A
//! reference is the address of its place: a local's cell, a top-level
//! binding's static slot, a field inside either or inside what another
//! reference refers to, or a list element in its buffer. Reading or writing through it needs only the address; a count
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
use hd_tir::ir::{NONE, Place, PlaceRoot, Proj, Ref, Tag};
use hd_types::{Ty, TyData};

use super::Em;
use super::value::{Dst, Src};
use crate::layout::{Kind, Leaf, Part, Walk, mem as mem_of, struct_mem, walk};
use crate::module::{EMPTY, HEAP, Val, mem};
use crate::rt::{self, Helper, LIST_ELEMS, LIST_LEN, RefLayout};
use crate::unsupported;

/// A value-backed root after globals and raw-pointer instructions split off.
#[derive(Clone, Copy)]
enum ValueRoot {
    Local(hd_base::LocalId),
    Value(Ref),
}

impl ValueRoot {
    fn checked(root: PlaceRoot) -> Self {
        let value = match root {
            PlaceRoot::Local(l) => Some(Self::Local(l)),
            PlaceRoot::Value(r) => Some(Self::Value(r)),
            PlaceRoot::Global(..) | PlaceRoot::Frame(_) | PlaceRoot::Ptr(_) => None,
        };
        // ruled out: callers split off globals; inst_tag handles Ptr and
        // invariant 20 excludes Frame from these value place walks.
        assert!(value.is_some(), "the place must have a value-backed root");
        value.as_slice()[0]
    }
}

/// Where a step of a place lies while [`Em::walk_place`] resolves it: in
/// Wasm locals; in a top-level binding's Wasm globals from value `first`
/// on, of the types `vals` (`(h, first, vals)`, the root of a store); in
/// memory at the address in a local plus an offset, laid out as the
/// step's type is.
pub(super) enum At {
    Vals(Vec<u32>),
    Globals(hd_base::Hash128, usize, Vec<Val>),
    Mem(u32, u32),
}

/// A materialized value's location; store-only Wasm globals are excluded.
#[derive(Clone, Copy)]
enum ValueAt<'a> {
    Vals(&'a [u32]),
    Mem(u32, u32),
}

impl At {
    fn memory(&self) -> (u32, u32) {
        let memory = match self {
            Self::Mem(a, off) => Some((*a, *off)),
            Self::Vals(_) | Self::Globals(..) => None,
        };
        // ruled out: rooted stores reach memory through Elem/Deref;
        // counted_ref addresses a payload's borrowed base in memory.
        assert!(memory.is_some(), "the checked place must reach memory");
        memory.as_slice()[0]
    }

    fn value(&self) -> ValueAt<'_> {
        let value = match self {
            Self::Vals(ls) => Some(ValueAt::Vals(ls)),
            Self::Mem(a, off) => Some(ValueAt::Mem(*a, *off)),
            Self::Globals(..) => None,
        };
        // ruled out: place_start creates Globals only for stores; at_root
        // loads globals into locals or addresses their static slot for values.
        assert!(value.is_some(), "value walks must materialize globals");
        value.as_slice()[0]
    }
}

/// The type a reference refers to, and whether the reference is fat (a
/// trait value's).
type Referent = (Ty, bool);

/// The local index of the builder's canonical whole-local place word.
#[inline]
pub(super) fn bare_local(b: &hd_tir::ir::Body, word: u32) -> usize {
    use hd_tir::ir::place::BARE_LOCAL;
    // ruled out: callers require a whole local; the builder sets BARE_LOCAL.
    debug_assert!(
        b.decode_place(word).is_some_and(|p| {
            p.proj.is_empty()
                && matches!(p.root, PlaceRoot::Local(l) if word == (l.raw() | BARE_LOCAL))
        }),
        "expected a canonical BARE_LOCAL whole-local place"
    );
    (word & !BARE_LOCAL) as usize
}

impl Em<'_, '_, '_> {
    pub(super) fn ref_layout(&self, ty: Ty) -> StageResult<RefLayout> {
        let m = mem_of(self.pool(), self.env(), ty)?;
        let walk = self.counted(ty);
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
        })
    }

    /// Locals holding value `r`: its instruction's, or a constant's,
    /// pushed into fresh ones.
    pub(super) fn local_value_of(&mut self, r: u32) -> StageResult<Vec<u32>> {
        if r != NONE && Ref(r).as_inst().is_some() {
            return self.computed(r);
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

    /// The target and ABI width of a reference established by checked Deref.
    fn checked_ref_target(&self, t: Ty) -> Referent {
        let (pool, env) = (self.pool(), self.env());
        // ruled out: checked Deref typing preserves the reference constructor.
        let target = pool.ref_inner(crate::layout::strip(pool, env, t));
        let target = crate::layout::strip(pool, env, target);
        let fat = matches!(pool.get(target), TyData::TraitValue { .. });
        (target, fat)
    }

    /// The place in operand word `word`, which the TIR builder encoded.
    pub(super) fn place(&self, word: u32) -> Place {
        self.b.decode_place(word).expect("an encoded place")
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
    pub(super) fn path_step(&self, t: Ty, f: u32) -> StageResult<(Ty, Part)> {
        if let TyData::Adt { def, args } = self.pool().get(t)
            && let Some(base) = self.env().newtype_base(def)
        {
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
        // Checking builds `.f` only for a field `t` declares.
        Ok((fields[f as usize], parts[f as usize]))
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
    /// locals: a boxed enum, an open value, a dereference, an element.
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
                    // A checked place's payload step names a field of
                    // its variant.
                    // A `Proj::Variant` step, into an enum or an optional.
                    let at = self.enum_flat(ty)?.selection(v as usize, f as usize);
                    slots = at.iter().map(|&p| slots[p]).collect();
                    ty = self.variant_tys(ty)[v as usize][f as usize];
                    k += 2;
                }
                Proj::Deref | Proj::Elem(_) => return Ok(None),
            }
        }
        Ok(Some((ty, slots)))
    }

    /// The values of a `t` value to write after zeros in the values
    /// `zeroed`, a part's at `proj` ([`Em::zero_part`]), so that the value
    /// still says the variant the part moved out of: each enum on the way
    /// whose niche cell the zeros cover and whose niche reads 0 as another
    /// variant (a null address's). The release of the rest then walks the
    /// right variant, wherever its drop is. A counted cell gets the static
    /// empty buffer's address (`None`), which no count changes; any other
    /// the last value of its valid range, which `reserve` left just below
    /// the niche's values.
    fn niche_repairs(
        &self,
        t: Ty,
        proj: &[Proj],
        zeroed: &[usize],
    ) -> StageResult<Vec<(usize, Option<u32>)>> {
        let mut out = Vec::new();
        let mut slots: Vec<usize> = (0..self.vals(t)?.len()).collect();
        let mut ty = t;
        let mut k = 0;
        while k < proj.len() {
            let whole = self.newtype(ty) && proj[k] == Proj::Field(0);
            if !(whole || matches!(self.kind(ty), Kind::Value)) {
                return Ok(out);
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
                        return Ok(out);
                    };
                    // A `Proj::Variant` step, into an enum or an optional.
                    let flat = self.enum_flat(ty)?;
                    if let Some(d) = flat.cell()
                        && let crate::layout::Enc::Niche { start, bytes, .. } = d.enc
                        && d.enc.reads(0) != v
                        && zeroed.contains(&slots[d.value as usize])
                    {
                        let counted = match walk(self.pool(), self.env(), ty) {
                            Some(Walk::Switch(_, arms)) => arms
                                .get(v as usize)
                                .and_then(Option::as_ref)
                                .is_some_and(|arm| counts(arm, d.value)),
                            Some(_) | None => false,
                        };
                        let last =
                            start.wrapping_sub(1) & (u32::MAX >> (32 - 8 * u32::from(bytes)));
                        out.push((slots[d.value as usize], (!counted).then_some(last)));
                    }
                    let at = flat.selection(v as usize, f as usize);
                    slots = at.iter().map(|&p| slots[p]).collect();
                    ty = self.variant_tys(ty)[v as usize][f as usize];
                    k += 2;
                }
                Proj::Deref | Proj::Elem(_) => return Ok(out),
            }
        }
        Ok(out)
    }

    /// Zeros the locals `ls` of a value of type `t` that hold the bytes of
    /// its part in `slots`: the part's own, and those of other variants of
    /// an enum that overlap them (`layout::Flat`), so no copy of a moved
    /// counted address stays.
    fn zero_part(&mut self, t: Ty, ls: &[u32], slots: &[usize]) -> StageResult<()> {
        for k in self.zeroed(t, slots)? {
            self.f.zero(ls[k]);
        }
        Ok(())
    }

    /// The values of a `t` value that [`Em::zero_part`] zeros for its part
    /// in `slots`: those whose bytes overlap the part's.
    fn zeroed(&self, t: Ty, slots: &[usize]) -> StageResult<Vec<usize>> {
        let values = mem_of(self.pool(), self.env(), t)?.leaves;
        let part: Vec<Leaf> = slots.iter().map(|&k| values[k]).collect();
        Ok(crate::layout::overlapping(&values, &part))
    }

    /// The locals of place `p`'s root, when the root's value lives in
    /// them (a local in no cell, or an instruction's value), and its type.
    fn root_locals(&mut self, p: &Place) -> StageResult<Option<(Ty, Vec<u32>)>> {
        match p.root {
            PlaceRoot::Local(l) if !self.is_cell(l.idx()) => {
                Ok(Some((self.at_root_ty(p.root), self.local(l.raw())?)))
            }
            PlaceRoot::Value(r) if r.as_inst().is_some() && !self.lazy.contains_key(&r.0) => {
                Ok(Some((self.ty_of(r.0), self.computed(r.0)?)))
            }
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
        let p = self.place(self.b.data[i as usize][0]);
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
            self.zero_part(t, &ls, &slots)?;
            for (slot, value) in self.niche_repairs(t, &p.proj, &self.zeroed(t, &slots)?)? {
                let value = value.unwrap_or_else(|| crate::rt::empty_buffer(self.m));
                self.f.u32(value);
                self.f.set(ls[slot]);
            }
        }
        Ok(true)
    }

    /// `Drop [%r]` `i` of a whole value after moves or drops of its parts
    /// in the same block (the move pass keeps a base's drop whole): where this
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
            self.zero_part(t, &ls, &slots)?;
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
        let p = self.place(self.b.data[i as usize][0]);
        let Some((d, Some((target, false)))) = self.last_through(&p)? else {
            return Ok(None);
        };
        if d + 1 != p.proj.len() || !self.is_map(target) {
            return Ok(None);
        }
        let (at, _) = self.walk_place(i, &p, d, None)?;
        let reference = self.at_i32(&at)?;
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
        // A map is one header word.
        self.f.get(reference);
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        unique(self);
        self.f.set(header);
        self.f.get(reference);
        self.f.get(header);
        self.f.op(I::I32Store(mem(HEAP, 0, 2)));
        Ok(Some(header))
    }

    /// The list or map buffer at the place `word` of an `IsUnique` or a
    /// `Pin`, in a local, with the collection's type: the place resolved
    /// from its root's value ([`Em::walk_place`]), each element
    /// bounds-checked. Nothing takes a count: the buffer is the one the
    /// place holds, which a view lends. Also where the
    /// walk found it in memory, where the place keeps it ([`Em::put_at`]).
    pub(super) fn place_header(&mut self, word: u32) -> StageResult<(u32, Ty, Option<At>)> {
        let p = self.place(word);
        let (at, ty) = self.walk_place(NONE, &p, p.proj.len(), None)?;
        let h = self.at_i32(&at)?;
        Ok((h, ty, (!matches!(at, At::Vals(_))).then_some(at)))
    }

    /// Stores the buffer in local `b` at `at`, where a walk found the
    /// place's collection ([`Em::place_header`]).
    pub(super) fn put_at(&mut self, at: &At, b: u32) {
        match at.value() {
            ValueAt::Vals(ls) => {
                // ruled out: List/Map headers occupy one Wasm local.
                assert_eq!(ls.len(), 1, "collection headers must have one word");
                if ls[0] != b {
                    self.f.get(b);
                    self.f.set(ls[0]);
                }
            }
            ValueAt::Mem(a, off) => {
                self.f.get(a);
                self.f.get(b);
                self.f.op(I::I32Store(mem(HEAP, u64::from(off), 2)));
            }
        }
    }

    /// Stores the buffer in local `b`, which a change of its operand `r`
    /// left, where `r` came from: into the place a `borrow` read `r` names
    /// right after its `IsUnique`, else into `r`'s own value.
    pub(super) fn put_back(&mut self, r: u32, b: u32) -> StageResult<()> {
        if self.b.is_borrow(r) {
            return self.put_buffer(self.b.data[r as usize][0], b);
        }
        let ls = self.local_value_of(r)?;
        self.put_at(&At::Vals(ls), b);
        Ok(())
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
        let p = self.place(word);
        let (at, ty, from) = match self.last_through(&p)? {
            Some((d, Some((target, false)))) => {
                let (at, _) = self.walk_place(NONE, &p, d, None)?;
                let r = self.at_i32(&at)?;
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
        self.put_at(&at, b);
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
            // ruled out: moves::take marks only whole locals, encoded BARE_LOCAL.
            let x = bare_local(self.b, self.b.data[i as usize][0]);
            // ruled out: moves excludes frame locals; storage::slots requires
            // local frame references, whose local count cannot be taken.
            debug_assert!(!self.slots.contains_key(&x));
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
        // ruled out: moves::take marks only whole locals, encoded BARE_LOCAL.
        let x = bare_local(self.b, self.b.data[i as usize][0]);
        // ruled out: own::promote::lent_ref rejects captures and other addresses
        // for TAKES; stored_locals omits its sole promoted address.
        debug_assert!(!self.is_cell(x) && !self.slots.contains_key(&x));
        let ls = self.local(u32::try_from(x).expect("locals"))?;
        for &v in &ls {
            self.f.get(v);
        }
        self.store_owned(i)?;
        if self.tracked(self.ty(i)) {
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
    /// for a reference to one. A place with
    /// neither is in its root's storage. [`Em::ref_from`] addresses the
    /// struct field the fields after the head name.
    fn counted_ref(&mut self, i: u32) -> StageResult<()> {
        let p = self.place(self.b.data[i as usize][0]);
        // ruled out: checked Tag::Ref keeps a reference result after substitution.
        let fat = matches!(self.kind(self.ty(i)), Kind::Ref { fat: true, .. });
        let last = self.last_through(&p)?;
        let start = self.place_start(i, &p, last)?;
        let (upto, head) = match last {
            Some((d, through)) => (d, Some(through)),
            None => (0, None),
        };
        let (mut at, mut ty) = self.walk_place(i, &p, upto, start)?;
        // A capture's copy lies in the closure's environment too, which
        // outlives the code's call: a local reference points there. (A
        // receiver's counted reference points at a whole object: a copy.)
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
                (_, through) => {
                    // ruled out: last_through selects only Elem/Deref; the
                    // Elem arms leave a checked reference with its target.
                    assert!(
                        through.is_some(),
                        "last_through must state the Deref target"
                    );
                    let (target, fat) = through.as_slice()[0];
                    if fat {
                        (at, target)
                    } else {
                        (At::Mem(self.at_i32(&at)?, 0), target)
                    }
                }
            };
        }
        let Some(path) = fields(&p.proj[from..]) else {
            self.payload_ref(i, &at, ty, &p.proj[from..])?;
            return Ok(());
        };
        self.ref_from(i, at, ty, &path, fat)?;
        Ok(())
    }

    /// The rest of `Ref` `i` when `steps` go through a variant's payload
    /// (`ref.match.borrow`): the field's address in the enum at `base`, of
    /// type `ty`. A payload is a struct at one offset of its enum
    /// (`layout::Flat`), so the field is at a fixed offset from it.
    fn payload_ref(&mut self, i: u32, base: &At, parent_ty: Ty, steps: &[Proj]) -> StageResult<()> {
        let (a, off) = base.memory();
        let mut ty = parent_ty;
        let mut offset = off;
        let mut steps = steps.iter();
        while let Some(&step) = steps.next() {
            match step {
                Proj::Variant(v) => {
                    let f = place_field(steps.next());
                    // ruled out: data.enum.no-recursion/no-boxing; hd_check
                    // rejects cycles after associated payload substitution (#1554).
                    assert!(
                        !matches!(self.kind(ty), Kind::Boxed),
                        "checked enum payloads have a value layout"
                    );
                    // A checked place's payload step names a field of
                    // its variant.
                    // A `Proj::Variant` step, into an enum or an optional.
                    offset += self.enum_flat(ty)?.offsets[v as usize][f as usize];
                    ty = self.variant_tys(ty)[v as usize][f as usize];
                }
                step => {
                    let f = place_field(Some(&step));
                    let (ft, part) = self.path_step(ty, f)?;
                    offset += part.offset;
                    ty = ft;
                }
            }
        }
        self.f.get(a);
        if offset != 0 {
            self.f.u32(offset);
            self.f.op(I::I32Add);
        }
        self.store_owned(i)?;
        Ok(())
    }

    /// The rest of `Ref` `i` of the part at `off` of the value at address
    /// `at`: the address there, or the trait value a fat reference reads
    /// at it.
    fn ref_projection(
        &mut self,
        i: u32,
        at: u32,
        field: &RefLayout,
        off: u32,
        fat: bool,
    ) -> StageResult<()> {
        let at = self.offset_local(at, off);
        if fat {
            self.load_through(at, field);
        } else {
            self.f.get(at);
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
        let value = base.value();
        let has_path = !path.is_empty();
        let (mut off, mut first) = (0, 0);
        for &f in path {
            let (ft, part) = self.path_step(ty, f)?;
            off += part.offset;
            first += part.first;
            ty = ft;
        }
        let m = mem_of(self.pool(), self.env(), ty)?;
        if let ValueAt::Mem(at, _) = value
            && has_path
        {
            let field = self.ref_layout(ty)?;
            return self.ref_projection(i, at, &field, off, fat);
        }
        match value {
            ValueAt::Mem(at, _) if fat => {
                let value = self.ref_layout(ty)?;
                self.load_through(at, &value);
                self.store_owned(i)?;
                return Ok(());
            }
            ValueAt::Mem(at, _) => {
                self.f.get(at);
                if off != 0 {
                    self.f.u32(off);
                    self.f.op(I::I32Add);
                }
            }
            ValueAt::Vals(ls) if fat => {
                for l in &ls[first..first + m.leaves.len()] {
                    self.f.get(*l);
                }
            }
            ValueAt::Vals(ls) => {
                // ruled out: storage::run gives addressed mutable locals cells
                // or slots; the checker gives a mutable rvalue a local too.
                debug_assert!(!crate::layout::ref_mutable(
                    self.pool(),
                    self.env(),
                    self.ty(i)
                ));
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
                // A local reference owns the copy; a counted reference's
                // count of it is the reference's own.
                if self.local_ref(self.ty(i)) {
                    self.owned_refs.insert(i);
                }
                return self.store_owned(i);
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
        let p = self.place(self.b.data[i as usize][0]);
        let last = self.last_through(&p)?;
        let start = self.place_start(i, &p, last)?;
        if let Some((d, Some((target, fat)))) = last {
            let (at, ref_ty) = self.walk_place(i, &p, d, start)?;
            let steps = &p.proj[d + 1..];
            let fields = fields(steps);
            // A trait value has no field or payload to read.
            if fat {
                self.push_at(&at, ref_ty)?;
                self.store(i)?;
                return Ok(());
            }
            let r = self.at_i32(&at)?;
            let value = self.ref_layout(target)?;
            if fields.as_ref().is_some_and(Vec::is_empty) {
                // A loan is the leaves its place holds, or, for an enum a
                // match only decides on and binds parts of, the place.
                if self.lazy_borrow(i, target, ref_ty) {
                    // Its discriminant cell, a tag or a niche, once; an
                    // enum of one variant has none to read.
                    let word = self.f.local(Val::I32);
                    // `lazy_borrow` holds only for an enum or an optional.
                    if let Some(d) = self.enum_flat(target)?.cell() {
                        self.f.get(r);
                        self.f.op(d.leaf.load(0));
                        self.f.set(word);
                    }
                    self.lazy.insert(
                        i,
                        super::Lazy {
                            at: r,
                            off: 0,
                            word,
                            ref_ty,
                        },
                    );
                    return Ok(());
                }
                if self.loans.contains(&i) {
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
                // ruled out: steps follow last_through's final Elem/Deref.
                let (at, ty) = self.walk_steps(At::Mem(r, 0), declared, steps)?;
                self.push_at(&at, ty)?;
                self.store(i)?;
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

    /// Whether read `i` of a whole `target` through reference type
    /// `ref_ty` is an enum that matches read in place (#1422 step 12): a
    /// `borrow`, or a copy through `&` of a value that counts nothing,
    /// which nothing can change while the shared reference lives; of a
    /// flat enum every use of which decides on its tag or reaches a part
    /// of it ([`Em::part_only`]), in a body that does not suspend. Then
    /// the tag loads once and each arm's fields where they are read, as
    /// Rust's match through `&` does, not all its values first. O(1): the
    /// use fact is the body's, computed once.
    pub(super) fn lazy_borrow(&self, i: u32, target: Ty, ref_ty: Ty) -> bool {
        crate::work::use_facts(1);
        let shared_copy = !crate::layout::ref_mutable(self.pool(), self.env(), ref_ty)
            && !self.tracked(target)
            && self.drop_calls(target).is_empty();
        self.susp.is_none()
            && (self.loans.contains(&i) || shared_copy)
            && matches!(self.kind(target), Kind::Value)
            && crate::layout::variants(self.pool(), self.env(), target).is_some()
            && self.part_only[i as usize]
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
    /// names, into instruction `dest`'s value: the part's leaves at the
    /// address.
    fn read_part(&mut self, dest: u32, at: u32, value: &RefLayout, part: Part) -> StageResult<()> {
        for leaf in &value.leaves[part.first..part.first + part.count] {
            self.f.get(at);
            self.f.op(leaf.load(0));
        }
        self.store(dest)
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
        let p = self.place(self.b.data[i as usize][0]);
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
        // Checking refuses a write through a trait value reference
        // (`type-mismatch`).
        if let Some((d, Some((target, _)))) = last {
            let (at, _) = self.walk_place(i, &p, d, start)?;
            let r = self.at_i32(&at)?;
            let path = fields(&p.proj[d + 1..]);
            // ruled out: checker stores use fields, not variant paths;
            // pattern payload writes dereference their borrowed binding.
            assert!(path.is_some(), "checked stores have a field suffix");
            let fs = &path.as_slice()[0];
            if fs.is_empty() {
                self.deref_write_at(r, target, v)?;
            } else {
                self.deref_field_write_at(r, target, fs, v)?;
            }
            return Ok(());
        }
        let (at, ty) = self.walk_place(i, &p, p.proj.len(), start)?;
        let leaves;
        let d = match &at {
            At::Vals(ls) if !rooted => Dst::Locals(ls),
            At::Globals(h, first, vals) => Dst::Globals(*h, *first, vals),
            _ => {
                let (a, lv) = self.memory_leaves(&at, ty)?;
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
        let rooted = self.rooted_at(i);
        // ruled out: rooted stores have an Elem/Deref; the checker
        // borrows a pattern payload rather than storing a Variant path.
        debug_assert!(tag != Tag::Store || !rooted || last.is_some());
        let at = match (p.root, last) {
            _ if rooted => return Ok(None),
            (PlaceRoot::Value(r), Some((0, _)))
                if tag != Tag::Ref && matches!(p.proj[0], Proj::Elem(_)) =>
            {
                Some(At::Vals(vec![self.buffer(r.0)?]))
            }
            (root, Some((0, through)))
                if tag == Tag::Store || tag == Tag::Read && matches!(through, Some((_, false))) =>
            {
                let x = self.f.local(Val::I32);
                match ValueRoot::checked(root) {
                    ValueRoot::Local(l) => {
                        let ls = self.local_value(l.raw())?;
                        self.f.get(ls[0]);
                    }
                    ValueRoot::Value(r) => self.comp(r.0, 0)?,
                }
                self.f.set(x);
                Some(At::Vals(vec![x]))
            }
            (_, Some(_)) if tag != Tag::Store => return Ok(None),
            (_, None) if tag == Tag::Read => return Ok(None),
            (PlaceRoot::Local(l), None) if !self.is_cell(l.idx()) => {
                Some(At::Vals(self.local(l.raw())?))
            }
            (PlaceRoot::Local(l), None) => Some(At::Mem(self.cell_of(l.idx())?, 0)),
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
            (root, None) if tag == Tag::Ref => {
                // ruled out: address_globals reserves every Ref's global root.
                debug_assert!(
                    !matches!(root, PlaceRoot::Global(def, _) if self.def_slot(def).is_none())
                );
                return self.at_root(root).map(Some);
            }
            _ => None,
        };
        // ruled out: checked stores address a local/global or go through
        // a reference/element; inst_tag handles raw pointers.
        assert!(at.is_some(), "a checked store must have a writable start");
        Ok(at.map(|at| (at, t)))
    }

    /// A write of `v` through the reference in local `at` to a `target`.
    fn deref_write_at(&mut self, at: u32, target: Ty, v: u32) -> StageResult<()> {
        let value = self.ref_layout(target)?;
        let ls: Vec<u32> = value.leaves.iter().map(|l| self.f.local(l.val)).collect();
        // A value the store consumes moves its count into the place.
        let moved = self.take(v);
        self.write_new(Dst::Locals(&ls), &Src::Ref(v))?;
        let release = self.releases_old();
        rt::ref_store(&mut self.f, at, &ls, &value, moved, release);
        Ok(())
    }

    /// A write of `v` into the struct field `path` reaches in the `target`
    /// the reference in local `at` refers to: the value at its offset.
    fn deref_field_write_at(
        &mut self,
        at: u32,
        target: Ty,
        path: &[u32],
        v: u32,
    ) -> StageResult<()> {
        let (ft, part) = self.fold_path(target, path)?;
        let field = self.ref_layout(ft)?;
        let ls: Vec<u32> = field.leaves.iter().map(|l| self.f.local(l.val)).collect();
        // A value the store consumes moves its count into the place.
        let moved = self.take(v);
        self.write_new(Dst::Locals(&ls), &Src::Ref(v))?;
        let dst = self.offset_local(at, part.offset);
        let release = self.releases_old();
        rt::ref_store(&mut self.f, dst, &ls, &field, moved, release);
        Ok(())
    }

    /// `RefIdentity` `i` of reference `r` (std's `RefIdentity`): no base
    /// object, the address as the serial and no key, so two references
    /// share an identity exactly when they share an address.
    /// ruled out: another layout. std declares `RefIdentity`
    /// (lib/std/cmp.hd) as `base: *dyn Any`, `serial: i64`, `key: string`:
    /// a trait value's two words, an `i64`, then a string's two words.
    pub(super) fn ref_identity(&mut self, i: u32, r: u32) -> StageResult<()> {
        self.f.i32(0);
        self.f.i32(0);
        self.comp(r, 0)?;
        self.f.op(I::I64ExtendI32U);
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
        if let PlaceRoot::Global(def, t) = root {
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
                // ruled out: the materialized value ABI uses u32 local/slot indices.
                let k = u32::try_from(k).expect("k");
                ls.push(self.f.local(v));
                self.f.global_get(crate::module::Global::Storage(h, k, v));
                self.f.set(ls[ls.len() - 1]);
            }
            return Ok((At::Vals(ls), t));
        }
        match ValueRoot::checked(root) {
            ValueRoot::Local(l) => {
                Ok((At::Vals(self.local_value(l.raw())?), self.at_root_ty(root)))
            }
            // A borrow a match reads in place is its referent.
            ValueRoot::Value(r) if let Some(&z) = self.lazy.get(&r.0) => {
                Ok((At::Mem(z.at, z.off), self.ty_of(r.0)))
            }
            ValueRoot::Value(r) => Ok((At::Vals(self.local_value_of(r.0)?), self.ty_of(r.0))),
        }
    }

    /// A local holding the one 32-bit value at `at`: a list buffer, a
    /// reference, or a boxed enum's address.
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
            // ruled out: address/header callers have one word; trait loan
            // callers extract the data word separately in data_address.
            _ => return unsupported("an address step from a value of several parts"),
        };
        let x = self.f.local(Val::I32);
        self.f.get(base);
        self.f.op(leaf.load(0));
        self.f.set(x);
        Ok(x)
    }

    /// The address and leaves of a rooted store's required memory place.
    fn memory_leaves(&self, at: &At, ty: Ty) -> StageResult<(u32, Vec<Leaf>)> {
        let (a, off) = at.memory();
        let mut leaves = mem_of(self.pool(), self.env(), ty)?.leaves;
        for l in &mut leaves {
            l.offset += off;
        }
        Ok((a, leaves))
    }

    /// Pushes the value of type `ty` at `at`, with no count.
    fn push_at(&mut self, at: &At, ty: Ty) -> StageResult<()> {
        match at.value() {
            ValueAt::Vals(ls) => {
                for l in ls {
                    self.f.get(*l);
                }
            }
            ValueAt::Mem(a, off) => {
                for mut l in mem_of(self.pool(), self.env(), ty)?.leaves {
                    l.offset += off;
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
        };
        Ok((at, ft))
    }

    /// Field `f` of variant `v`'s payload of the enum at `at`: in memory,
    /// the payload's struct at its offset, in the boxed enum's cell when it
    /// has one; in locals, the values that hold its leaves.
    fn at_variant(&mut self, at: &At, ty: Ty, v: u32, f: u32) -> StageResult<(At, Ty)> {
        // A `Proj::Variant` step, into an enum or an optional.
        let flat = self.enum_flat(ty)?;
        // A checked place's payload step names a field of its variant.
        let (v, f) = (v as usize, f as usize);
        let ft = self.variant_tys(ty)[v][f];
        let field = flat.offsets[v][f];
        let at = if matches!(self.kind(ty), Kind::Boxed) {
            At::Mem(self.at_i32(at)?, field)
        } else {
            match at.value() {
                ValueAt::Mem(a, off) => At::Mem(a, off + field),
                ValueAt::Vals(ls) => {
                    At::Vals(flat.selection(v, f).iter().map(|&p| ls[p]).collect())
                }
            }
        };
        Ok((at, ft))
    }

    /// Walks the steps of `p` up to step `upto` from its root's value
    /// (`docs/compiler/tir-v2.md`, "Places"): a field picks its part, a
    /// variant its payload field, an element its slot in the buffer,
    /// bounds-checked
    /// (unchecked where the loop's range proof covers read `i`, and from the
    /// loop's cached buffer where the list is a stable list of a local).
    /// A dereference of a reference goes to its referent's address. So
    /// every dereference before
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
            // The cached buffer.
            let h = self.f.local(Val::I32);
            self.f.get(cache.buffer);
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
                self.comp(ix.0, 0)?;
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
        let mut steps = steps.iter();
        while let Some(&step) = steps.next() {
            match step {
                Proj::Field(f) => ty = self.path_step(ty, f)?.0,
                Proj::Elem(_) if self.is_map(ty) => ty = self.entry(ty)?.1,
                Proj::Elem(_) => ty = self.elem(ty)?.0,
                Proj::Deref => ty = self.checked_ref_target(ty).0,
                Proj::Variant(v) => {
                    let f = place_field(steps.next());
                    ty = self.variant_tys(ty)[v as usize][f as usize];
                }
            }
        }
        Ok(ty)
    }

    /// Walks `steps`, which hold no element, from `at` of type `ty`
    /// ([`Em::walk_place`]).
    fn walk_steps(&mut self, mut at: At, mut ty: Ty, steps: &[Proj]) -> StageResult<(At, Ty)> {
        let mut steps = steps.iter();
        while let Some(&step) = steps.next() {
            match step {
                Proj::Field(f) => {
                    (at, ty) = self.at_field(at, ty, f)?;
                }
                Proj::Variant(v) => {
                    let f = place_field(steps.next());
                    (at, ty) = self.at_variant(&at, ty, v, f)?;
                }
                step => {
                    // ruled out: walk_place splits before each Elem; other
                    // callers pass the suffix after last_through's final step.
                    assert!(matches!(step, Proj::Deref), "plain walks exclude Elem");
                    let (target, fat) = self.checked_ref_target(ty);
                    // ruled out: trait values have no fields; terminal fat
                    // reads/refs run in place_read/counted_ref, not this walk.
                    debug_assert!(!fat);
                    at = At::Mem(self.at_i32(&at)?, 0);
                    ty = target;
                }
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
        // ruled out: callers use last_through's successful thin Ref step;
        // erasing newtypes preserves its declared reference constructor.
        Ok(pool.ref_inner(crate::layout::strip(pool, env, ty)))
    }
}

/// The next checked field, paired with a Variant or in a plain suffix.
fn place_field(step: Option<&Proj>) -> u32 {
    let field = match step {
        Some(Proj::Field(f)) => Some(*f),
        _ => None,
    };
    // ruled out: checker patterns and TirBuilder::payload pair Variant/Field;
    // callers split only at Elem/Deref or the end of the place. counted_ref
    // consumes the last Elem/Deref before walking a payload's field suffix.
    assert!(
        field.is_some(),
        "a checked place suffix must name its Field"
    );
    field.as_slice()[0]
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

/// Whether walk `arm` counts the address in value `k`.
fn counts(arm: &Walk, k: u32) -> bool {
    match arm {
        Walk::Ref(x) | Walk::Known(x) => *x == k,
        Walk::Glue => false,
        Walk::Seq(ws) => ws.iter().any(|w| counts(w, k)),
        Walk::Switch(_, arms) => arms.iter().flatten().any(|w| counts(w, k)),
    }
}

/// The one value instruction `k` of `b` uses only to decide on its tag
/// or to reach a part of it, if any ([`Em::part_only`]): a `SwitchTag`'s
/// scrutinee; the root of a `Read` or `Ref` whose first step is a
/// variant's payload; the root of a `LoanEnd` or `Drop`. Every other
/// operand of `k` is a use of another kind.
pub(super) fn part_use(b: &hd_tir::ir::Body, k: usize) -> Option<u32> {
    let a = b.data[k][0];
    let tag = b.tags[k];
    match tag {
        Tag::SwitchTag => Some(a),
        Tag::Read | Tag::Ref | Tag::LoanEnd | Tag::Drop => {
            let p = b.view(a);
            let v = p.value()?;
            (matches!(tag, Tag::LoanEnd | Tag::Drop)
                || matches!(p.first(b), Some(Proj::Variant(_))))
            .then_some(v)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::bare_local;
    use hd_base::{DefId, LocalId};
    use hd_tir::ir::{BodyKind, Place, PlaceRoot, Proj, Ref, TirBuilder, place};

    #[test]
    fn bare_local_decodes_canonical_whole_locals() {
        let mut builder = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        for local in [0, 7, 1024] {
            let word = builder.place(&Place::local(LocalId::from_raw(local)));
            assert_eq!(bare_local(builder.body(), word), local as usize);
        }
    }

    #[cfg(debug_assertions)]
    #[test]
    fn bare_local_rejects_other_place_encodings() {
        let mut builder = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let record_local = builder.words_record(&[place::ROOT_LOCAL, 7]);
        let projected = builder.place(&Place {
            root: PlaceRoot::Local(LocalId::from_raw(7)),
            proj: vec![Proj::Field(0)],
        });
        let value = builder.place(&Place {
            root: PlaceRoot::Value(Ref(0)),
            proj: vec![],
        });
        for word in [record_local, projected, value, place::BARE_LOCAL - 1] {
            assert!(std::panic::catch_unwind(|| bare_local(builder.body(), word)).is_err());
        }
    }
}
