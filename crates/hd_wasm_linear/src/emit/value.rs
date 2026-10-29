//! Structs, tuples, enums and data values (`crates/hd_wasm_linear/DESIGN.md`
//! §23.3, job 6). A struct, a tuple and a non-recursive enum are their
//! values in locals ([`crate::layout::mem`]): construction pushes them, a
//! field read picks some. A data value and a self-recursive enum are the
//! address of an object in the heap, whose payload holds the same values
//! at their offsets: construction allocates it and stores them, a field
//! read loads them, a field write stores them.

use crate::op::Op as I;
use hd_base::StageResult;
use hd_mono::layout::{CanonMemo, canon};
use hd_types::{Ty, TyData};

use super::Em;
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
        self.f.op(l.store(base));
        Ok(())
    }

    /// A new counted object whose payload is laid out as `m`, with the
    /// counted references `walk` (§23.4): a local holding its payload's
    /// address. Its descriptor names the release glue of its payload, and
    /// says whether it takes part in cycle collection (`cyclic`, from
    /// `crate::cycle`).
    pub(super) fn object(
        &mut self,
        m: &Mem,
        walk: Option<&Walk>,
        cyclic: bool,
        recursive: bool,
        target: Option<Ty>,
    ) -> u32 {
        let at = self.f.local(Val::I32);
        self.f.u32(m.size);
        let glue = walk.map(|w| {
            Sym::Helper(Helper::DropFields {
                leaves: m.leaves.clone(),
                walk: w.clone(),
            })
        });
        let weak_key = target.and_then(|ty| {
            let key = canon(
                self.pool(),
                &|d| self.env().path_hash(d),
                &mut CanonMemo::default(),
                ty,
            );
            (self.m.open_world || self.m.weak_types.contains(&key)).then_some(key)
        });
        if let Some(key) = weak_key {
            crate::rt::push_typed_desc_recursive(&mut self.f, glue, cyclic, recursive, key);
        } else {
            crate::rt::push_desc_recursive(&mut self.f, glue, cyclic, recursive);
        }
        if self.input.runtime.is_some_and(|r| r.checked.is_none())
            && m.size
                .checked_add(crate::module::HEADER)
                .is_some_and(|n| n <= 2048)
        {
            self.f.helper(Helper::NewFixed { size: m.size });
        } else {
            self.f.helper(Helper::New);
        }
        self.f.set(at);
        at
    }

    /// The leaves of a payload of `tys`, laid out as `parts`, that hold a
    /// collection a count copies ([`Em::copied`]).
    pub(super) fn part_colls(&self, tys: &[Ty], parts: &[Part]) -> Vec<u32> {
        let mut out = Vec::new();
        for (t, p) in tys.iter().zip(parts) {
            let first = u32::try_from(p.first).expect("leaves");
            out.extend(self.copied(*t).into_iter().map(|c| first + c));
        }
        out
    }

    /// One more count on each counted reference that a new object's
    /// construction copied into its payload: each collection, at the
    /// leaves `colls`, the copy glue's result stored back.
    pub(super) fn retain_fields(&mut self, at: u32, m: &Mem, walk: Option<&Walk>, colls: &[u32]) {
        for &c in colls {
            let l = m.leaves[c as usize];
            self.f.get(at);
            self.f.get(at);
            self.f.op(l.load(0));
            self.f.helper(Helper::CollCopy);
            self.f.op(I::I32Store(mem(HEAP, u64::from(l.offset), 2)));
        }
        if let Some(w) = walk.and_then(|w| w.without(colls)) {
            self.f.get(at);
            self.f.helper(Helper::RetainFields {
                leaves: m.leaves.clone(),
                walk: w,
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
                // Construction consumes its parts (§23.25).
                let srcs = self.count_refs(&fields, &tys)?;
                for (s, ft) in srcs.iter().zip(&tys) {
                    for k in 0..self.vals(*ft)?.len() {
                        self.src(s, k)?;
                    }
                }
            }
            Kind::Data(tys) => {
                if tys.is_empty() {
                    let at = self.empty_object();
                    self.f.static_addr(at);
                } else {
                    let (m, parts) = payload_mem(self.pool(), self.env(), &tys)?;
                    let walk = self.payload_walk(&tys);
                    let srcs = self.count_refs(&fields, &tys)?;
                    let cyclic = self.cyclic_object(t);
                    let recursive = self.recursive_object(t);
                    let at = self.object(&m, walk.as_ref(), cyclic, recursive, Some(t));
                    for (s, p) in srcs.iter().zip(&parts) {
                        self.store_part(at, &m, *p, s)?;
                    }
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
        if self.counted(t).is_none() {
            return self.write_new(d, &Src::Ref(v));
        }
        let s = self.consumed(v)?;
        let old = self.old_locals(d);
        self.load_old(d, &old);
        self.release_locals(&old, t)?;
        self.write_new(d, &s)
    }

    /// Replaces the `t` value at `d` with `s`, which holds its count
    /// already, as [`Em::replace`] does, but releases the old value, read
    /// into `old` ([`Em::old_locals`]), after the write.
    pub(super) fn replace_late(&mut self, d: Dst, t: Ty, s: &Src, old: &[u32]) -> StageResult<()> {
        let counted = self.counted(t).is_some();
        if counted {
            self.load_old(d, old);
        }
        self.write_new(d, s)?;
        if counted {
            self.release_locals(old, t)?;
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
            Dst::Mem(at, base, leaves) => {
                for (k, l) in leaves.iter().enumerate() {
                    self.f.get(at);
                    self.src(s, k)?;
                    self.f.op(l.store(base));
                }
            }
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
                // The copy takes a count of each kept field, and each
                // replacement moves its count in or takes one (§23.25).
                let from = self.vals.get(&a).cloned();
                let mut srcs = Vec::new();
                for (f, (ft, p)) in tys.iter().zip(&parts).enumerate() {
                    srcs.push(match (replaced(f), &from) {
                        (Some(v), _) => Some(self.consumed(v)?),
                        (None, Some(ls)) => {
                            let kept = &ls[p.first..p.first + p.count];
                            Some(Src::Locals(self.copy_locals(kept, *ft)?))
                        }
                        (None, None) => None,
                    });
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
                    self.f.static_addr(at);
                } else {
                    let (m, parts) = payload_mem(self.pool(), self.env(), &tys)?;
                    let walk = self.payload_walk(&tys);
                    let srcs: Vec<Option<Src>> =
                        (0..tys.len()).map(|f| replaced(f).map(Src::Ref)).collect();
                    let cyclic = self.cyclic_object(t);
                    let recursive = self.recursive_object(t);
                    let at = self.object(&m, walk.as_ref(), cyclic, recursive, Some(t));
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
                    let colls = self.part_colls(&tys, &parts);
                    self.retain_fields(at, &m, walk.as_ref(), &colls);
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
            && walk(self.pool(), self.env(), t).and_then(|w| w.single()) != Some(0)
        {
            return unsupported("identity of a value that is not a data value");
        }
        self.comp(a, 0)?;
        self.comp(b, 0)?;
        self.f.op(I::I32Eq);
        self.store(i)
    }

    /// Whether enum or optional type `t` is a nullable address
    /// ([`crate::layout::nullable`]): its tag is whether the address is
    /// not null.
    pub(super) fn nullable(&self, t: Ty) -> bool {
        variants(self.pool(), self.env(), t)
            .is_some_and(|vs| crate::layout::nullable(self.pool(), self.env(), &vs))
    }

    /// Pushes the tag of enum value `a`, loaded from its cell when it is
    /// boxed.
    pub(super) fn tag(&mut self, a: u32) -> StageResult<()> {
        let t = self.ty_of(a);
        if self.nullable(t) {
            self.comp(a, 0)?;
            self.f.i32(0);
            self.f.op(I::I32Ne);
            return Ok(());
        }
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
        // Construction consumes its parts (§23.25).
        let fields = self.count_parts(fields, &tys)?;
        // A nullable enum's value 0 is the payload's address, or null.
        let push = |em: &mut Self, p: usize| -> StageResult<()> {
            if let Some((f, k)) = from[p] {
                em.src(&fields[f], k)
            } else if p == 0 {
                em.f.u32(variant);
                Ok(())
            } else {
                em.zero(m.leaves[p].val);
                Ok(())
            }
        };
        if matches!(self.kind(t), Kind::Boxed) {
            let walk = self.cell_walk(t);
            let cyclic = self.cyclic_object(t);
            let recursive = self.recursive_object(t);
            let cell = self.object(&m, walk.as_ref(), cyclic, recursive, Some(t));
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
            self.f.get(cell);
            return self.store_owned(i);
        }
        for p in 0..m.leaves.len() {
            push(self, p)?;
        }
        self.store_owned(i)
    }
}
