//! Structs, tuples and enums (`crates/hd_wasm/DESIGN.md` §23.3,
//! job 6). A struct, tuple or enum is its values in locals
//! ([`crate::layout::mem`]): construction pushes them, a field read picks
//! some. Recursive enum links are explicit references; the enum's own
//! fields remain inline.

use crate::op::Op as I;
use hd_base::StageResult;
use hd_types::AdtId;
use hd_types::{Ty, TyData, TyList};
use std::rc::Rc;

use super::Em;
use crate::layout::{
    Discr, Enc, Flat, Is, IsOps as _, Kind, Leaf, LeafOps as _, Mem, Part, Step, Test, kind,
    variant_steps, variants,
};
use crate::module::{HEAP, Sym, Val};
use crate::rt::Helper;

/// Where a payload field's values come from: a TIR value, or locals.
#[derive(Clone)]
pub enum Src {
    Ref(u32),
    Locals(Vec<u32>),
}

/// Where [`Em::replace`] writes a value.
#[derive(Clone, Copy)]
pub(super) enum Dst<'a> {
    /// The Wasm locals that hold it.
    Locals(&'a [u32]),
    /// Top-level binding `h`'s Wasm globals from value `first` on, of the
    /// types `vals`: `(h, first, vals)`.
    Globals(hd_base::Hash128, usize, &'a [Val]),
    /// The leaves of the memory at the address in a local, some bytes
    /// before them: `(at, base, leaves)`.
    Mem(u32, u32, &'a [Leaf]),
}

/// Wasm global `k` of top-level binding `h`, of type `v`.
fn storage(h: hd_base::Hash128, k: usize, v: Val) -> crate::module::Global {
    crate::module::Global::Storage(h, u32::try_from(k).expect("k"), v)
}

impl Em<'_, '_, '_> {
    pub(super) fn kind(&self, t: Ty) -> Kind {
        kind(self.pool(), self.env(), t)
    }

    /// Whether `t` is a nominal newtype, which erases to its base.
    pub(super) fn newtype(&self, t: Ty) -> bool {
        matches!(self.pool().get(t), TyData::Adt(AdtId::Declared(def), _)
            if self.env().newtype_base(def).is_some())
    }

    /// An enum's (or an optional's) layout as values, whether or not it is
    /// boxed, and where each payload field lies in it; `None` for a type
    /// that is neither.
    pub(super) fn flat(&self, t: Ty) -> StageResult<Option<Rc<Flat>>> {
        if let Some(f) = self.flats.borrow().get(&t) {
            return Ok(Some(f.clone()));
        }
        let f = crate::layout::flat(self.pool(), self.env(), t)?;
        if let Some(f) = &f {
            self.flats.borrow_mut().insert(t, f.clone());
        }
        Ok(f)
    }

    /// [`Em::flat`] of a type its caller knows is an enum or an optional:
    /// a value a `SwitchTag`, a payload read, a variant's construction or a
    /// place's `as .v .f` step reaches, each of which the checker types
    /// only on an enum or an optional (each caller names its own).
    pub(super) fn enum_flat(&self, t: Ty) -> StageResult<Rc<Flat>> {
        // Every caller decides on, reads a payload of, builds or steps
        // into a variant of a value the checker typed as an enum or an
        // optional (`Tag::SwitchTag`, `Tag::Payload`, `Tag::NewVariant`,
        // `Proj::Variant`), or names an optional type itself.
        // ruled out: a type that is no enum, by the checker's typing above.
        Ok(self.flat(t)?.expect("an enum or an optional"))
    }

    /// Carries out the steps `steps` ([`Step`]): `leaf` pushes field
    /// `f`'s leaf `k`.
    pub(super) fn run_steps(
        &mut self,
        steps: &[Step],
        leaf: &mut dyn FnMut(&mut Self, usize, usize) -> StageResult<()>,
    ) -> StageResult<()> {
        for s in steps {
            match *s {
                Step::Leaf(f, k) => leaf(self, f, k)?,
                Step::Op(op) => self.f.op(op),
            }
        }
        Ok(())
    }

    /// The payload types of each variant of enum (or optional) `t`.
    pub(super) fn variant_tys(&self, t: Ty) -> Rc<Vec<Vec<Ty>>> {
        let present = self.cache_variant_tys(t);
        // ruled out: checked Proj::Variant steps name enum or optional payloads;
        // other callers select enum_flat first. Traversal probes remain optional.
        debug_assert!(present);
        self.variant_tys.borrow()[&t].clone()
    }

    /// Populate the existing body-local cache without cloning its payload.
    fn cache_variant_tys(&self, t: Ty) -> bool {
        if self.variant_tys.borrow().contains_key(&t) {
            return true;
        }
        let Some(vs) = variants(self.pool(), self.env(), t) else {
            return false;
        };
        self.variant_tys.borrow_mut().insert(t, vs);
        true
    }

    /// Pushes a zero of `v`.
    pub(super) fn zero(&mut self, v: Val) {
        self.f.op(match v {
            Val::I32 => I::I32Const(0),
            Val::I64 => I::I64Const(0),
            Val::F32 => I::F32Const(wasm_encoder::Ieee32::new(0)),
            Val::F64 => I::F64Const(wasm_encoder::Ieee64::new(0)),
        });
    }

    /// Pushes value `k` of a source.
    pub(super) fn src(&mut self, s: &Src, k: usize) -> StageResult<()> {
        match s {
            Src::Ref(r) => self.comp(*r, k),
            Src::Locals(ls) => {
                self.f.get(ls[k]);
                Ok(())
            }
        }
    }

    /// Stores into leaf `l` at `address + base` the value that `value`
    /// pushes.
    pub(super) fn store_leaf(
        &mut self,
        address: &dyn Fn(&mut Self) -> StageResult<()>,
        base: u32,
        l: Leaf,
        value: &mut dyn FnMut(&mut Self) -> StageResult<()>,
    ) -> StageResult<()> {
        address(self)?;
        value(self)?;
        self.f.op(l.store(base));
        Ok(())
    }

    /// A new counted object whose payload is laid out as `m` and holds
    /// `parts`, each by its first leaf and how its type counts: a local
    /// holding its payload's address. Its descriptor names the payload's
    /// release glue, [`Helper::DropParts`], and says whether it takes
    /// part in cycle collection (`cyclic`, from `crate::cycle`).
    pub(super) fn object_parts(
        &mut self,
        m: &Mem,
        parts: Vec<(u32, crate::rt::Counted)>,
        cyclic: bool,
        recursive: bool,
    ) -> u32 {
        let glue = (!parts.is_empty()).then(|| {
            Sym::Helper(Helper::DropParts {
                leaves: m.leaves.clone(),
                parts,
            })
        });
        self.new_object(m.size, glue, cyclic, recursive)
    }

    /// A new counted object of `size` payload bytes whose descriptor
    /// names release glue `glue`: a local holding its payload's address.
    pub(super) fn new_object(
        &mut self,
        size: u32,
        glue: Option<Sym>,
        cyclic: bool,
        recursive: bool,
    ) -> u32 {
        let at = self.f.local(Val::I32);
        self.f.u32(size);
        crate::rt::push_desc_recursive(&mut self.f, glue, cyclic, recursive);
        if self.input.runtime.checked.is_none()
            && size
                .checked_add(crate::module::HEADER)
                .is_some_and(|n| n <= 2048)
        {
            self.f.helper(Helper::NewFixed { size });
        } else {
            self.f.helper(Helper::New);
        }
        self.f.set(at);
        at
    }

    /// The counted-block intrinsics of `std.rt.counted` (#2369). A block
    /// is one counted object, `[count | descriptor | H | pad | tail]`, and
    /// a `*H` names it by its payload's address: a copy is a retain, a drop
    /// a release. Blocks are 16-aligned, so a payload is 8 past a multiple
    /// of 16 and the tail at alignment `a <= 16` starts at the constant
    /// [`Em::tail_offset`].
    /// - `alloc_counted[H](header, tail_bytes, align)`: a new block of
    ///   count 1 holding `header`, whose descriptor is an `H` object's:
    ///   `H`'s release glue (its `Drop` hook drops the tail once, at count
    ///   0, and the collector's trace runs it once per block) and cycle bit.
    /// - `tail_of[H](block, align)`: the tail's first byte.
    /// - `is_unique[H](block)`: the count is 1; a static block's is 0.
    pub(super) fn counted_block(
        &mut self,
        i: u32,
        key: &str,
        targs: TyList,
        args: &[u32],
    ) -> StageResult<()> {
        // ruled out: the intrinsic table gives each `counted_` key one type
        // parameter, `H`, which instances fill.
        let targs = self.pool().list_items(targs);
        let t = targs[targs.len() - 1];
        let m = crate::layout::mem(self.pool(), self.env(), t)?;
        match key {
            "counted_is_unique" => {
                self.push(args[0])?;
                self.f.u32(crate::module::HEADER);
                self.f.op(I::I32Sub);
                crate::rt::unique_count(&mut self.f, self.m);
                self.f.i32(1);
                self.f.op(I::I32Eq);
            }
            "counted_tail" => {
                self.push(args[0])?;
                self.tail_offset(m.size, args[1])?;
                self.f.op(I::I32Add);
            }
            // ruled out: the intrinsic table's `counted_` keys are these three.
            _ => {
                let header = self.retain_src(&Src::Ref(args[0]), t)?;
                let parts = self.typed_parts(t)?;
                let glue = (!parts.is_empty()).then(|| {
                    Sym::Helper(Helper::DropParts {
                        leaves: m.leaves.clone(),
                        parts,
                    })
                });
                let (cyclic, recursive) = (self.cyclic_object(t), self.recursive_object(t));
                let at = self.f.local(Val::I32);
                self.tail_offset(m.size, args[2])?;
                self.push(args[1])?;
                self.f.op(I::I32Add);
                crate::rt::push_desc_recursive(&mut self.f, glue, cyclic, recursive);
                self.f.helper(Helper::New);
                self.f.set(at);
                self.write_new(Dst::Mem(at, 0, &m.leaves), &header)?;
                self.f.get(at);
            }
        }
        self.store(i)
    }

    /// Pushes the offset from a block's payload to its tail at alignment
    /// `align` past an `H` of `size` bytes: `round_up(8 + size, align) - 8`,
    /// a constant when `align` is one, as after inlining.
    fn tail_offset(&mut self, size: u32, align: u32) -> StageResult<()> {
        let header = crate::module::HEADER;
        if let Some(a) = self.const_u32(align).filter(|a| a.is_power_of_two()) {
            self.f.u32((header + size).next_multiple_of(a) - header);
            return Ok(());
        }
        self.f.u32(header + size - 1);
        self.push(align)?;
        self.f.op(I::I32Add);
        self.f.i32(0);
        self.push(align)?;
        self.f.op(I::I32Sub);
        self.f.op(I::I32And);
        self.f.u32(header);
        self.f.op(I::I32Sub);
        Ok(())
    }

    /// The parts of a payload that is one `t` value: the value whole,
    /// when its type counts anything.
    pub(super) fn typed_parts(&mut self, t: Ty) -> StageResult<Vec<(u32, crate::rt::Counted)>> {
        let counted = crate::rt::counted_of(self.input, self.m, t)?;
        Ok(if counted.counts() {
            vec![(0, counted)]
        } else {
            Vec::new()
        })
    }

    /// The parts of a payload of fields `tys`, laid out as `parts`, that
    /// count anything. A field of `None` is a shared capture's cell, one
    /// known address.
    pub(super) fn payload_parts(
        &mut self,
        tys: &[Option<Ty>],
        parts: &[Part],
    ) -> StageResult<Vec<(u32, crate::rt::Counted)>> {
        let mut out = Vec::new();
        for (t, p) in tys.iter().zip(parts) {
            let counted = match t {
                Some(t) => crate::rt::counted_of(self.input, self.m, *t)?,
                None => crate::rt::Counted::Address {
                    index: 0,
                    known: true,
                },
            };
            if counted.counts() {
                out.push((p.first_leaf(), counted));
            }
        }
        Ok(out)
    }

    /// One more count on each of `parts` (by first leaf and how its type
    /// counts) that a new object's construction, at payload `at` laid out
    /// as `m`, copied into its payload.
    pub(super) fn retain_fields(
        &mut self,
        at: u32,
        m: &Mem,
        parts: Vec<(u32, crate::rt::Counted)>,
    ) {
        if !parts.is_empty() {
            self.f.get(at);
            self.f.helper(Helper::RetainFields {
                leaves: m.leaves.clone(),
                parts,
            });
        }
    }

    /// `NewStruct`: a struct pushes its fields' values; a newtype is its
    /// one value.
    pub(super) fn new_data(&mut self, i: u32, bw: u32) -> StageResult<()> {
        let fields = self.rec(bw);
        let t = self.ty(i);
        if self.newtype(t) {
            // A newtype is constructed from its one base value.
            let r = fields[0];
            self.take(r);
            self.push(r)?;
            return self.store(i);
        }
        // A struct literal names a struct, whose values are in locals.
        let tys = self.struct_fields(t);
        // Construction consumes its parts (§23.25).
        let srcs = if self.b.is_borrowed_coercion(i) {
            fields.iter().copied().map(Src::Ref).collect()
        } else {
            self.count_refs(&fields, &tys)?
        };
        for (s, ft) in srcs.iter().zip(&tys) {
            for k in 0..self.vals(*ft)?.len() {
                self.src(s, k)?;
            }
        }
        // The parts' counts are the new value's.
        self.store(i)
    }

    /// The parts `srcs`, of types `tys`, of a new value, each moved in or a
    /// copy with counts of its own ([`Em::retain_src`]).
    pub(super) fn count_parts(&mut self, srcs: &[Src], tys: &[Ty]) -> StageResult<Vec<Src>> {
        srcs.iter()
            .zip(tys)
            .map(|(s, t)| self.retain_src(s, *t))
            .collect()
    }

    /// The parts `rs`, of types `tys`, of a new value ([`Em::count_parts`]).
    fn count_refs(&mut self, rs: &[u32], tys: &[Ty]) -> StageResult<Vec<Src>> {
        let srcs: Vec<Src> = rs.iter().map(|r| Src::Ref(*r)).collect();
        self.count_parts(&srcs, tys)
    }

    /// Pushes every value of `s`.
    pub(super) fn push_src(&mut self, s: &Src) -> StageResult<()> {
        match s {
            Src::Ref(r) => self.push(*r),
            Src::Locals(ls) => {
                for l in ls {
                    self.f.get(*l);
                }
                Ok(())
            }
        }
    }

    /// Stores the values of `s` as the part `p` of the object at local `at`
    /// whose payload is laid out as `m`.
    pub(super) fn store_part(&mut self, at: u32, m: &Mem, p: Part, s: &Src) -> StageResult<()> {
        let leaves = &m.leaves[p.first..p.first + p.count];
        self.write_new(Dst::Mem(at, 0, leaves), s)
    }

    /// Replaces the `t` value at `d` with value `v`: a counted place takes
    /// a count of the new value ([`Em::consumed`]; a copy of each
    /// collection a place holds, `types.coll.value`) and releases the old
    /// one's before the write.
    pub(super) fn replace(&mut self, d: Dst, t: Ty, v: u32) -> StageResult<()> {
        if !self.needs_drop(t) {
            self.write_new(d, &Src::Ref(v))?;
            return Ok(());
        }
        let s = self.consumed(v)?;
        if self.releases_old() {
            let old = self.old_locals(d);
            self.load_old(d, &old);
            self.release_old_drop(&old, t)?;
        }
        self.write_new(d, &s)?;
        Ok(())
    }

    /// Replaces the `t` value at `d` with `s`, which holds its count
    /// already, as [`Em::replace`] does, but releases the old value, read
    /// into `old` ([`Em::old_locals`]), after the write.
    pub(super) fn replace_late(&mut self, d: Dst, t: Ty, s: &Src, old: &[u32]) -> StageResult<()> {
        let drops = self.needs_drop(t) && self.releases_old();
        if drops {
            self.load_old(d, old);
        }
        self.write_new(d, s)?;
        if drops {
            self.release_old_drop(old, t)?;
        }
        Ok(())
    }

    /// The locals the value at `d` is read into before a write: `d`'s own
    /// when it is locals.
    pub(super) fn old_locals(&mut self, d: Dst) -> Vec<u32> {
        match d {
            Dst::Locals(ls) => ls.to_vec(),
            Dst::Globals(_, _, vals) => vals.iter().map(|v| self.f.local(*v)).collect(),
            Dst::Mem(_, _, leaves) => leaves.iter().map(|l| self.f.local(l.val)).collect(),
        }
    }

    /// Reads the values at `d` into the locals `old`.
    fn load_old(&mut self, d: Dst, old: &[u32]) {
        for (k, o) in old.iter().enumerate() {
            match d {
                Dst::Locals(_) => return,
                Dst::Globals(h, first, vals) => self.f.global_get(storage(h, first + k, vals[k])),
                Dst::Mem(at, base, leaves) => {
                    self.f.get(at);
                    self.f.op(leaves[k].load(base));
                }
            }
            self.f.set(*o);
        }
    }

    /// Writes the values of `s` to `d`.
    pub(super) fn write_new(&mut self, d: Dst, s: &Src) -> StageResult<()> {
        match d {
            Dst::Locals(ls) => {
                self.push_src(s)?;
                for x in ls.iter().rev() {
                    self.f.set(*x);
                }
            }
            Dst::Globals(h, first, vals) => {
                self.push_src(s)?;
                for (k, v) in vals.iter().enumerate().rev() {
                    self.f.global_set(storage(h, first + k, *v));
                }
            }
            Dst::Mem(at, base, leaves) => self.store_src(HEAP, at, base, leaves, s)?,
        }
        Ok(())
    }

    /// Stores each of `leaves` of `s` in memory `memory` at the address in
    /// local `at`, `base` bytes before the value.
    pub(super) fn store_src(
        &mut self,
        memory: u32,
        at: u32,
        base: u32,
        leaves: &[Leaf],
        s: &Src,
    ) -> StageResult<()> {
        for (k, l) in leaves.iter().enumerate() {
            self.f.get(at);
            self.src(s, k)?;
            self.f.op(l.store_in(memory, base));
        }
        Ok(())
    }

    /// A field read: field `idx` of `a`, picked from a struct's values.
    pub(super) fn field(&mut self, i: u32, a: u32, idx: u32) -> StageResult<()> {
        let t = self.ty_of(a);
        if self.newtype(t) {
            self.push(a)?;
            return self.store(i);
        }
        // A field read of a value reads a struct's field; one through a
        // reference is a `DerefField` step.
        let p = crate::layout::members(self.pool(), self.env(), t)?.parts[idx as usize];
        for k in p.first..p.first + p.count {
            self.comp(a, k)?;
        }
        self.store(i)
    }

    /// The field types of struct type `t`, substituted.
    pub(super) fn struct_fields(&self, t: Ty) -> Vec<Ty> {
        let (pool, env) = (self.pool(), self.env());
        let Some((def, args)) = pool.adt(crate::layout::strip_newtype(pool, env, t)) else {
            return Vec::new();
        };
        env.data_fields(def)
            .unwrap_or_default()
            .into_iter()
            .map(|f| hd_mono::subst(pool, env, def, args, f))
            .collect()
    }

    /// A tuple element read: element `idx`'s values.
    pub(super) fn tuple_get(&mut self, i: u32, a: u32, idx: u32) -> StageResult<()> {
        let p = crate::layout::members(self.pool(), self.env(), self.ty_of(a))?.parts[idx as usize];
        for k in p.first..p.first + p.count {
            self.comp(a, k)?;
        }
        self.store(i)
    }

    /// `a is b` (`expr.is.*`): two values refer to the same object.
    pub(super) fn is(&mut self, i: u32, a: u32, b: u32) -> StageResult<()> {
        // Each operand's first word is its identity
        // (`ref.identity.same-place`): a reference's is the address of the
        // place it refers to, a trait value reference's its data word.
        // Checking validates the identity-bearing operand types
        // (`expr.is.value-operand`, `identity-requires-references`).
        self.comp(a, 0)?;
        self.comp(b, 0)?;
        self.f.op(I::I32Eq);
        self.store(i)
    }

    /// Pushes the cell that says enum value `a`'s variant, a tag or a
    /// niche ([`Flat::cell`]): from its locals, or as a borrow through a
    /// reference read it in place
    /// ([`Em::lazy_borrow`]). An enum of one variant has no cell; its
    /// tests need no read ([`Em::is_variant`]).
    pub(super) fn discr_cell(&mut self, a: u32, d: Discr) -> StageResult<()> {
        if let Some(l) = self.lazy.get(&a) {
            self.f.get(l.word);
            return Ok(());
        }
        self.comp(a, d.value as usize)
    }

    /// Pushes an `i32` that is nonzero exactly when enum value `a` is
    /// variant `v` (`read_discr` against `v`): a compare of its cell, or a
    /// constant for an enum of one variant.
    pub(super) fn is_variant(&mut self, a: u32, v: u32) -> StageResult<()> {
        // A `SwitchTag`'s scrutinee, an enum or an optional.
        let flat = self.enum_flat(self.ty_of(a))?;
        match flat.test(v) {
            Test::Const(b) => self.f.i32(i32::from(b)),
            // A direct two-variant tag is already the bool for variant
            // one. Use the type's domain even when that variant's target
            // reads a payload and cannot use `switch_two_arms`.
            Test::Read(d, Is::Eq(1)) if d.enc == Enc::Direct && flat.positions.len() == 2 => {
                self.discr_cell(a, d)?;
            }
            Test::Read(d, is) => {
                self.discr_cell(a, d)?;
                for op in is.ops() {
                    self.f.op(op);
                }
            }
        }
        Ok(())
    }

    /// Payload read `i` of field `f` of variant `v` of enum value `a` read
    /// in place through a reference ([`Em::lazy_borrow`]), when the field
    /// is itself an enum a match only decides on and reads parts of: it
    /// is read in place too, at its offset in `a`, with whether its
    /// discriminant cell is `a`'s, as the payload enums an untagged
    /// variant's niche holds share it (`i32??`). Each arm then loads only
    /// its own fields, as for `a`.
    pub(super) fn nested_lazy(
        &self,
        i: u32,
        a: u32,
        v: u32,
        f: u32,
    ) -> StageResult<Option<(super::Lazy, bool)>> {
        let Some(&z) = self.lazy.get(&a) else {
            return Ok(None);
        };
        let t = self.ty(i);
        if self.flat(t)?.is_none() || !self.lazy_borrow(i, t, z.ref_ty) {
            return Ok(None);
        }
        // A payload read's operand, an enum or an optional, and a checked
        // payload step names a field of its variant.
        let parent = self.enum_flat(self.ty_of(a))?;
        let off = z.off + parent.offsets[v as usize][f as usize];
        let same = match (parent.cell(), self.enum_flat(t)?.cell()) {
            (Some(pc), Some(c)) => {
                z.off + pc.leaf.offset == off + c.leaf.offset
                    && pc.leaf.bytes == c.leaf.bytes
                    && pc.leaf.val == c.leaf.val
            }
            _ => false,
        };
        Ok(Some((super::Lazy { off, ..z }, same)))
    }

    /// A payload read: field `field` of variant `variant` of enum value `a`.
    pub(super) fn payload(&mut self, i: u32, a: u32, variant: u32, field: u32) -> StageResult<()> {
        if let Some((z, same)) = self.nested_lazy(i, a, variant, field)? {
            // Its own cell, unless it shares its parent's, loads once here.
            let mut word = z.word;
            if !same && let Some(d) = self.enum_flat(self.ty(i))?.cell() {
                word = self.f.local(Val::I32);
                self.f.get(z.at);
                self.f.op(d.leaf.load(z.off));
                self.f.set(word);
            }
            self.lazy.insert(i, super::Lazy { word, ..z });
            return Ok(());
        }
        let t = self.ty_of(a);
        // A payload read's operand, an enum or an optional.
        let flat = self.enum_flat(t)?;
        // A checked payload step names a field of its variant.
        let (v, f) = (variant as usize, field as usize);
        if let Some(&z) = self.lazy.get(&a) {
            // A borrow through a reference reads the field in place.
            for l in &flat.leaves[v][f] {
                self.f.get(z.at);
                self.f.op(l.load(z.off));
            }
        } else {
            for k in flat.selection(v, f) {
                self.comp(a, k)?;
            }
        }
        self.store(i)
    }

    /// [`Em::variant`] of copies of `fields`.
    pub(super) fn new_variant(&mut self, i: u32, variant: u32, fields: &[Src]) -> StageResult<()> {
        self.variant(i, variant, fields, false)
    }

    /// Variant `v` of `i`'s enum with the payload `fs`, moved in if `moved`:
    /// its cells ([`variant_steps`]).
    pub(super) fn variant(&mut self, i: u32, v: u32, fs: &[Src], moved: bool) -> StageResult<()> {
        let t = self.ty(i);
        // A variant's construction, of an enum or an optional.
        let flat = self.enum_flat(t)?;
        let variants = self.variant_tys(t);
        // ruled out: a checked constructor names one of this enum's variants.
        debug_assert!((v as usize) < variants.len());
        let tys = &variants[v as usize];
        // Construction consumes its parts (§23.25).
        let fields = if moved || self.short_origins.contains(&i) || self.b.is_borrowed_coercion(i) {
            fs.to_vec()
        } else {
            self.count_parts(fs, tys)?
        };
        // The checker builds a variant of its enum with each of its
        // fields, as the backend's own constructions do.
        let mut leaf = |em: &mut Self, f: usize, k: usize| em.src(&fields[f], k);
        let m = &flat.mem;
        // data.enum.no-recursion and data.enum.no-boxing: enum values stay
        // inline; recursive links are explicit references, built by box/ref.
        let values = variant_steps(&flat, v);
        for (c, steps) in values.iter().enumerate() {
            let steps = crate::layout::value_or_zero(&m.leaves, c, steps.as_ref());
            self.run_steps(&steps, &mut leaf)?;
        }
        self.store(i)
    }
}
