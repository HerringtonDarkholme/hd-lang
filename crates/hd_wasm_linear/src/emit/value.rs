//! Structs, tuples, enums and data values (`crates/hd_wasm_linear/DESIGN.md`
//! §23.3, job 6). A struct, a tuple and a non-recursive enum are their
//! values in locals ([`crate::layout::mem`]): construction pushes them, a
//! field read picks some. A data value and a self-recursive enum are the
//! address of an object in the heap, whose payload holds the same values
//! at their offsets: construction allocates it and stores them, a field
//! read loads them, a field write stores them.

use hd_base::StageResult;
use hd_tir::ir::NONE;
use hd_types::{Ty, TyData};
use wasm_encoder::Instruction as I;

use super::Em;
use super::refs::Start;
use crate::layout::{
    Kind, Leaf, Mem, Part, Walk, enum_mem, fields_walk, kind, payload_mem, struct_mem, variants,
    walk,
};
use crate::module::{HEADER, HEAP, Sym, Val, mem};
use crate::rt::Helper;
use crate::unsupported;

/// Where a payload field's values come from: a TIR value, or locals.
pub enum Src {
    Ref(u32),
    Locals(Vec<u32>),
}

impl Em<'_, '_, '_> {
    pub(super) fn kind(&self, t: Ty) -> Kind {
        kind(self.pool(), self.env(), t)
    }

    /// Whether `t` is a nominal newtype, which erases to its base.
    pub(super) fn newtype(&self, t: Ty) -> bool {
        matches!(self.pool().get(self.strip_mut(t)), TyData::Adt { def, .. }
            if self.env().newtype_base(def).is_some())
    }

    /// An enum's (or an optional's) layout as values, whether or not it is
    /// boxed, and where each payload field lies in it.
    pub(super) fn flat(&self, t: Ty) -> StageResult<(Mem, Vec<Vec<Vec<usize>>>)> {
        let Some(vs) = variants(self.pool(), self.env(), t) else {
            return unsupported("a variant of a type that is not an enum");
        };
        enum_mem(self.pool(), self.env(), &vs)
    }

    /// The counted references of a boxed enum's cell: its value's, as the
    /// enum lays it out unboxed.
    fn cell_walk(&self, t: Ty) -> Option<Walk> {
        self.input.runtime?;
        let vs = variants(self.pool(), self.env(), t)?;
        crate::layout::switch_walk(self.pool(), self.env(), &vs)
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

    /// Pushes the value of leaf `l` at `address + base`.
    fn load(
        &mut self,
        address: &dyn Fn(&mut Self) -> StageResult<()>,
        base: u32,
        l: Leaf,
    ) -> StageResult<()> {
        address(self)?;
        self.f.op(l.load(base));
        Ok(())
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
        let m = mem(
            HEAP,
            u64::from(base + l.offset),
            u32::from(l.bytes).trailing_zeros(),
        );
        self.f.op(match (l.val, l.bytes) {
            (Val::I32, 1) => I::I32Store8(m),
            (Val::I32, 2) => I::I32Store16(m),
            (Val::I32, _) => I::I32Store(m),
            (Val::I64, _) => I::I64Store(m),
            (Val::F32, _) => I::F32Store(m),
            (Val::F64, _) => I::F64Store(m),
        });
        Ok(())
    }

    /// A new counted object whose payload is laid out as `m`, with the
    /// counted references `walk` (§23.4): a local holding its payload's
    /// address. Its descriptor names the release glue of its payload, and
    /// says whether it takes part in cycle collection (`cyclic`, from
    /// `crate::cycle`).
    pub(super) fn object(&mut self, m: &Mem, walk: Option<&Walk>, cyclic: bool) -> u32 {
        let at = self.f.local(Val::I32);
        self.f.u32(m.size);
        let glue = walk.map(|w| {
            Sym::Helper(Helper::DropFields {
                leaves: m.leaves.clone(),
                walk: w.clone(),
            })
        });
        crate::rt::push_desc(&mut self.f, glue, cyclic);
        if self.input.runtime.is_some_and(|r| r.checked.is_none())
            && m.size
                .checked_add(crate::module::HEADER)
                .and_then(|n| n.checked_add(if cyclic { crate::module::CYCLE_WORD } else { 0 }))
                .is_some_and(|n| n <= 2048)
        {
            self.f.helper(Helper::NewFixed {
                size: m.size,
                cyclic,
            });
        } else {
            self.f.helper(Helper::New);
        }
        self.f.set(at);
        at
    }

    /// One more count on each counted reference that a new object's
    /// construction copied into its payload.
    pub(super) fn retain_fields(&mut self, at: u32, m: &Mem, walk: Option<&Walk>) {
        if let Some(w) = walk {
            self.f.get(at);
            self.f.helper(Helper::RetainFields {
                leaves: m.leaves.clone(),
                walk: w.clone(),
            });
        }
    }

    /// The counted references of a data payload of `fields`, when the
    /// program counts.
    fn payload_walk(&self, fields: &[Ty]) -> Option<Walk> {
        self.input.runtime?;
        fields_walk(self.pool(), self.env(), fields)
    }

    /// The address of the one static object of every data type without
    /// fields: a header and no payload.
    fn empty_object(&mut self) -> u32 {
        let at = self.m.static_bytes(&[0; HEADER as usize], 8) + HEADER;
        self.m.label_static(at, "empty-object");
        self.f.mark_static(at);
        at
    }

    /// `NewStruct`: a struct pushes its fields' values; a data value stores
    /// them in a new object; a newtype is its one value.
    pub(super) fn new_data(&mut self, i: u32, bw: u32) -> StageResult<()> {
        let fields = self.rec(bw);
        let t = self.ty(i);
        if self.newtype(t) {
            let [r] = fields[..] else {
                return unsupported("a newtype construction without one value");
            };
            let moved = self.take(r);
            self.push(r)?;
            return self.store_as(i, moved);
        }
        match self.kind(t) {
            Kind::Value => {
                let tys = self.struct_fields(t);
                let mut srcs = Vec::new();
                for (r, ft) in fields.iter().zip(&tys) {
                    srcs.push(self.slot_src(*r, *ft, false)?);
                }
                // Construction consumes its parts (§23.25).
                self.count_parts(&srcs, &tys)?;
                for (s, ft) in srcs.iter().zip(&tys) {
                    for k in 0..self.vals(*ft)?.len() {
                        self.src(s, k)?;
                    }
                }
            }
            Kind::Data(tys) => {
                if tys.is_empty() {
                    let at = self.empty_object();
                    self.f.u32(at);
                } else {
                    let (m, parts) = payload_mem(self.pool(), self.env(), &tys)?;
                    let walk = self.payload_walk(&tys);
                    let mut srcs = Vec::new();
                    for (r, ft) in fields.iter().zip(&tys) {
                        srcs.push(self.slot_src(*r, *ft, true)?);
                    }
                    let cyclic = self.cyclic_object(t);
                    let at = self.object(&m, walk.as_ref(), cyclic);
                    for (s, p) in srcs.iter().zip(&parts) {
                        self.store_part(at, &m, *p, s)?;
                    }
                    self.count_parts(&srcs, &tys)?;
                    self.f.get(at);
                }
                // A new object arrives with its count.
                return self.store_owned(i);
            }
            Kind::Boxed => return unsupported("a data literal of an enum"),
            Kind::Ref { .. } => return unsupported("a reference's closure form"),
        }
        // The parts' counts are the new value's.
        self.store_owned(i)
    }

    /// One count for a new value on each part of `srcs`, of types `tys`,
    /// that does not move its own count in ([`Em::retain_src`]).
    pub(super) fn count_parts(&mut self, srcs: &[Src], tys: &[Ty]) -> StageResult<()> {
        for (s, t) in srcs.iter().zip(tys) {
            if self.counted(*t).is_some() {
                self.retain_src(s, *t)?;
            }
        }
        Ok(())
    }

    /// Stores the values of `s` as the part `p` of the object at local `at`
    /// whose payload is laid out as `m`.
    pub(super) fn store_part(&mut self, at: u32, m: &Mem, p: Part, s: &Src) -> StageResult<()> {
        for (k, l) in m.leaves[p.first..p.first + p.count]
            .iter()
            .copied()
            .enumerate()
        {
            self.store_leaf(
                &|em: &mut Self| {
                    em.f.get(at);
                    Ok(())
                },
                0,
                l,
                &mut |em: &mut Self| em.src(s, k),
            )?;
        }
        Ok(())
    }

    /// `CopyStruct`: a copy of `a` with the fields of the `(field, value)`
    /// pairs `bw` replaced; a data value's copy is a new object.
    pub(super) fn copy_data(&mut self, i: u32, a: u32, bw: u32) -> StageResult<()> {
        let words = self.rec(bw);
        let repl: Vec<(u32, u32)> = words.chunks(2).map(|w| (w[0], w[1])).collect();
        let t = self.ty(i);
        let replaced = |f: usize| repl.iter().find(|(k, _)| *k as usize == f).map(|(_, v)| *v);
        if self.newtype(t) {
            match replaced(0) {
                Some(v) => self.push(v)?,
                None => self.push(a)?,
            }
            return self.store(i);
        }
        match self.kind(t) {
            Kind::Value => {
                let (_, parts) = struct_mem(self.pool(), self.env(), t)?;
                let tys = self.struct_fields(t);
                let mut srcs = Vec::new();
                for (f, ft) in tys.iter().enumerate() {
                    srcs.push(match replaced(f) {
                        Some(v) => Some(self.slot_src(v, *ft, false)?),
                        None => None,
                    });
                }
                // The copy takes a count of each kept field, and each
                // replacement moves its count in or takes one (§23.25).
                let from = self.vals.get(&a).cloned();
                for ((s, ft), p) in srcs.iter().zip(&tys).zip(&parts) {
                    if self.counted(*ft).is_none() {
                        continue;
                    }
                    match (s, &from) {
                        (Some(s), _) => self.retain_src(s, *ft)?,
                        (None, Some(ls)) => {
                            self.retain_locals(&ls[p.first..p.first + p.count], *ft)?;
                        }
                        (None, None) => {}
                    }
                }
                for (f, p) in parts.iter().enumerate() {
                    match &srcs[f] {
                        Some(s) => {
                            for k in 0..p.count {
                                self.src(s, k)?;
                            }
                        }
                        None => {
                            for k in p.first..p.first + p.count {
                                self.comp(a, k)?;
                            }
                        }
                    }
                }
            }
            Kind::Data(tys) => {
                if tys.is_empty() {
                    let at = self.empty_object();
                    self.f.u32(at);
                } else {
                    let (m, parts) = payload_mem(self.pool(), self.env(), &tys)?;
                    let walk = self.payload_walk(&tys);
                    let mut srcs = Vec::new();
                    for (f, ft) in tys.iter().enumerate() {
                        srcs.push(match replaced(f) {
                            Some(v) => Some(self.slot_src(v, *ft, true)?),
                            None => None,
                        });
                    }
                    let cyclic = self.cyclic_object(t);
                    let at = self.object(&m, walk.as_ref(), cyclic);
                    for (f, p) in parts.iter().enumerate() {
                        match &srcs[f] {
                            Some(s) => self.store_part(at, &m, *p, s)?,
                            None => {
                                for l in m.leaves[p.first..p.first + p.count].iter().copied() {
                                    self.store_leaf(
                                        &|em: &mut Self| {
                                            em.f.get(at);
                                            Ok(())
                                        },
                                        0,
                                        l,
                                        &mut |em: &mut Self| {
                                            em.load(&|em: &mut Self| em.comp(a, 0), 0, l)
                                        },
                                    )?;
                                }
                            }
                        }
                    }
                    self.retain_fields(at, &m, walk.as_ref());
                    self.f.get(at);
                }
                return self.store_owned(i);
            }
            Kind::Boxed => return unsupported("a copy-update of an enum"),
            Kind::Ref { .. } => return unsupported("a reference's closure form"),
        }
        self.store_owned(i)
    }

    /// A field read: field `idx` of `a`, picked from a struct's values or loaded
    /// from a data value's object.
    pub(super) fn field(&mut self, i: u32, a: u32, idx: u32) -> StageResult<()> {
        let t = self.ty_of(a);
        if self.newtype(t) {
            self.push(a)?;
            return self.store(i);
        }
        match self.kind(t) {
            Kind::Value => {
                let (_, parts) = struct_mem(self.pool(), self.env(), t)?;
                let Some(p) = parts.get(idx as usize).copied() else {
                    return unsupported("a field index outside the struct");
                };
                for k in p.first..p.first + p.count {
                    self.comp(a, k)?;
                }
            }
            Kind::Data(tys) => {
                let (m, parts) = payload_mem(self.pool(), self.env(), &tys)?;
                let Some(p) = parts.get(idx as usize).copied() else {
                    return unsupported("a field index outside the data type");
                };
                for l in m.leaves[p.first..p.first + p.count].iter().copied() {
                    self.load(&|em: &mut Self| em.comp(a, 0), 0, l)?;
                }
            }
            Kind::Boxed => return unsupported("a field of an enum"),
            Kind::Ref { .. } => return unsupported("a reference's closure form"),
        }
        self.store(i)
    }

    /// A `Store` into a data field (`[*%o .k]`): a store into field `idx`
    /// of the data value at `obj`.
    pub(super) fn field_set(&mut self, obj: Start, idx: u32, v: u32) -> StageResult<()> {
        if v != NONE && self.diverges(v) {
            self.f.op(I::Unreachable);
            return Ok(());
        }
        let Kind::Data(tys) = self.kind(self.start_ty(obj)) else {
            return unsupported("a field write to a value that is not a data value");
        };
        let (m, parts) = payload_mem(self.pool(), self.env(), &tys)?;
        let Some(p) = parts.get(idx as usize).copied() else {
            return unsupported("a field index outside the data type");
        };
        let ft = tys[idx as usize];
        let s = self.slot_src(v, ft, true)?;
        let at = self.f.local(Val::I32);
        self.push_start(obj, Some(0))?;
        self.f.set(at);
        // The field takes a count of the new value and drops the old one's.
        if self.counted(ft).is_some() {
            self.retain_src(&s, ft)?;
            let old: Vec<u32> = m.leaves[p.first..p.first + p.count]
                .iter()
                .map(|l| self.f.local(l.val))
                .collect();
            for (l, o) in m.leaves[p.first..p.first + p.count].iter().zip(&old) {
                self.f.get(at);
                self.f.op(l.load(0));
                self.f.set(*o);
            }
            self.release_locals(&old, ft)?;
        }
        self.store_part(at, &m, p, &s)
    }

    /// The field types of struct type `t`, substituted.
    pub(super) fn struct_fields(&self, t: Ty) -> Vec<Ty> {
        let (pool, env) = (self.pool(), self.env());
        let TyData::Adt { def, args } = pool.get(crate::layout::strip(pool, env, t)) else {
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
        let (_, parts) = struct_mem(self.pool(), self.env(), self.ty_of(a))?;
        let Some(p) = parts.get(idx as usize).copied() else {
            return unsupported("a tuple index outside the tuple");
        };
        for k in p.first..p.first + p.count {
            self.comp(a, k)?;
        }
        self.store(i)
    }

    /// `a is b` (`expr.is.*`): two values refer to the same object.
    pub(super) fn is(&mut self, i: u32, a: u32, b: u32) -> StageResult<()> {
        // `ref.identity.same-place`: two references compare the places
        // they refer to, which are their addresses; two references to
        // trait values compare their data.
        let t = self.ty_of(a);
        if matches!(self.kind(t), Kind::Ref { .. }) {
            self.comp(a, 0)?;
            self.f.helper(Helper::RefAddress);
            self.comp(b, 0)?;
            self.f.helper(Helper::RefAddress);
            self.f.op(I::I64Eq);
            return self.store(i);
        }
        // A data value, a boxed enum, a list, a map and a trait value's
        // data are each one counted address first; a trait value that
        // boxed a value compares its boxes (`ref.identity.same-place`).
        let t = self.ty_of(a);
        if !matches!(self.kind(t), Kind::Data(_) | Kind::Boxed)
            && walk(self.pool(), self.env(), t) != Some(Walk::Ref(0))
        {
            return unsupported("identity of a value that is not a data value");
        }
        self.comp(a, 0)?;
        self.comp(b, 0)?;
        self.f.op(I::I32Eq);
        self.store(i)
    }

    /// Pushes the tag of enum value `a`, loaded from its cell when it is
    /// boxed.
    pub(super) fn tag(&mut self, a: u32) -> StageResult<()> {
        let t = self.ty_of(a);
        if matches!(self.kind(t), Kind::Boxed) {
            let (m, _) = self.flat(t)?;
            return self.load(&|em: &mut Self| em.comp(a, 0), 0, m.leaves[0]);
        }
        self.comp(a, 0)
    }

    /// A payload read: field `field` of variant `variant` of enum value `a`.
    pub(super) fn payload(&mut self, i: u32, a: u32, variant: u32, field: u32) -> StageResult<()> {
        let t = self.ty_of(a);
        let (m, positions) = self.flat(t)?;
        let Some(at) = positions
            .get(variant as usize)
            .and_then(|fs| fs.get(field as usize))
            .cloned()
        else {
            return unsupported("a payload field the enum does not have");
        };
        let boxed = matches!(self.kind(t), Kind::Boxed);
        for &p in &at {
            if boxed {
                self.load(&|em: &mut Self| em.comp(a, 0), 0, m.leaves[p])?;
            } else {
                self.comp(a, p)?;
            }
        }
        self.store(i)
    }

    /// A new value of variant `variant` of `i`'s enum type with the payload
    /// `fields`: the tag, then each slot's value from the payload or a zero;
    /// a boxed enum stores them in a new cell.
    pub(super) fn new_variant(&mut self, i: u32, variant: u32, fields: &[Src]) -> StageResult<()> {
        let t = self.ty(i);
        let (m, positions) = self.flat(t)?;
        let Some(at) = positions.get(variant as usize) else {
            return unsupported("a variant the enum does not have");
        };
        // The source of each slot: a field and its value's index.
        let mut from: Vec<Option<(usize, usize)>> = vec![None; m.leaves.len()];
        for (f, ps) in at.iter().enumerate() {
            for (k, p) in ps.iter().enumerate() {
                from[*p] = Some((f, k));
            }
        }
        let tys = crate::layout::variants(self.pool(), self.env(), t)
            .and_then(|vs| vs.get(variant as usize).cloned())
            .unwrap_or_default();
        if tys.len() != fields.len() {
            return unsupported("a variant built from other fields than it has");
        }
        let push = |em: &mut Self, p: usize| -> StageResult<()> {
            if p == 0 {
                em.f.u32(variant);
                return Ok(());
            }
            if let Some((f, k)) = from[p] {
                em.src(&fields[f], k)
            } else {
                em.zero(m.leaves[p].val);
                Ok(())
            }
        };
        if matches!(self.kind(t), Kind::Boxed) {
            let walk = self.cell_walk(t);
            let cyclic = self.cyclic_object(t);
            let cell = self.object(&m, walk.as_ref(), cyclic);
            for (p, l) in m.leaves.iter().enumerate() {
                self.store_leaf(
                    &|em: &mut Self| {
                        em.f.get(cell);
                        Ok(())
                    },
                    0,
                    *l,
                    &mut |em: &mut Self| push(em, p),
                )?;
            }
            if walk.is_some() {
                self.count_parts(fields, &tys)?;
            }
            self.f.get(cell);
            return self.store_owned(i);
        }
        // Construction consumes its parts (§23.25).
        self.count_parts(fields, &tys)?;
        for p in 0..m.leaves.len() {
            push(self, p)?;
        }
        self.store_owned(i)
    }
}
