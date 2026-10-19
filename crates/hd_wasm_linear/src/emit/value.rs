//! Structs, tuples, enums and data values (`crates/hd_wasm/DESIGN.md`
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
use crate::layout::{Kind, Leaf, Mem, Part, enum_mem, kind, payload_mem, struct_mem, variants};
use crate::module::{HEADER, HEAP, Val, mem};
use crate::rt::Helper;
use crate::unsupported;

/// Where a payload field's values come from: a TIR value, or locals.
pub enum Src {
    Ref(u32),
    Locals(Vec<u32>),
}

impl Em<'_, '_, '_> {
    fn kind(&self, t: Ty) -> Kind {
        kind(self.pool(), self.env(), t)
    }

    /// Whether `t` is a nominal newtype, which erases to its base.
    fn newtype(&self, t: Ty) -> bool {
        matches!(self.pool().get(self.strip_mut(t)), TyData::Adt { def, .. }
            if self.env().newtype_base(def).is_some())
    }

    /// An enum's (or an optional's) layout as values, whether or not it is
    /// boxed, and where each payload field lies in it.
    fn flat(&self, t: Ty) -> StageResult<(Mem, Vec<Vec<Vec<usize>>>)> {
        let Some(vs) = variants(self.pool(), self.env(), t) else {
            return unsupported("a variant of a type that is not an enum");
        };
        enum_mem(self.pool(), self.env(), &vs)
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
    fn src(&mut self, s: &Src, k: usize) -> StageResult<()> {
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
        let m = mem(
            HEAP,
            u64::from(base + l.offset),
            u32::from(l.bytes).trailing_zeros(),
        );
        self.f.op(match (l.val, l.bytes, l.signed) {
            (Val::I32, 1, true) => I::I32Load8S(m),
            (Val::I32, 1, false) => I::I32Load8U(m),
            (Val::I32, 2, true) => I::I32Load16S(m),
            (Val::I32, 2, false) => I::I32Load16U(m),
            (Val::I32, _, _) => I::I32Load(m),
            (Val::I64, _, _) => I::I64Load(m),
            (Val::F32, _, _) => I::F32Load(m),
            (Val::F64, _, _) => I::F64Load(m),
        });
        Ok(())
    }

    /// Stores into leaf `l` at `address + base` the value that `value`
    /// pushes.
    fn store_leaf(
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

    /// A new object of `size` payload bytes: a local holding its payload's
    /// address. Its header is reserved; counting fills it (job 8).
    fn object(&mut self, size: u32) -> u32 {
        let at = self.f.local(Val::I32);
        self.f.u32(HEADER + size);
        self.f.helper(Helper::Alloc);
        self.f.u32(HEADER);
        self.f.op(I::I32Add);
        self.f.set(at);
        at
    }

    /// The address of the one static object of every data type without
    /// fields: a header and no payload.
    fn empty_object(&mut self) -> u32 {
        self.m.static_bytes(&[0; HEADER as usize], 8) + HEADER
    }

    /// `NewData`: a struct pushes its fields' values; a data value stores
    /// them in a new object; a newtype is its one value.
    pub(super) fn new_data(&mut self, i: u32, bw: u32) -> StageResult<()> {
        let fields = self.rec(bw);
        let t = self.ty(i);
        if self.newtype(t) {
            let [r] = fields[..] else {
                return unsupported("a newtype construction without one value");
            };
            self.push(r)?;
            return self.store(i);
        }
        match self.kind(t) {
            Kind::Value => {
                for r in fields {
                    self.push(r)?;
                }
            }
            Kind::Data(tys) => {
                if tys.is_empty() {
                    let at = self.empty_object();
                    self.f.u32(at);
                } else {
                    let (m, parts) = payload_mem(self.pool(), self.env(), &tys)?;
                    let at = self.object(m.size);
                    for (r, p) in fields.iter().zip(&parts) {
                        self.store_part(at, &m, *p, &Src::Ref(*r))?;
                    }
                    self.f.get(at);
                }
            }
            Kind::Boxed => return unsupported("a data literal of an enum"),
        }
        self.store(i)
    }

    /// Stores the values of `s` as the part `p` of the object at local `at`
    /// whose payload is laid out as `m`.
    fn store_part(&mut self, at: u32, m: &Mem, p: Part, s: &Src) -> StageResult<()> {
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

    /// `CopyData`: a copy of `a` with the fields of the `(field, value)`
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
                for (f, p) in parts.iter().enumerate() {
                    match replaced(f) {
                        Some(v) => self.push(v)?,
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
                    let at = self.object(m.size);
                    for (f, p) in parts.iter().enumerate() {
                        match replaced(f) {
                            Some(v) => self.store_part(at, &m, *p, &Src::Ref(v))?,
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
                    self.f.get(at);
                }
            }
            Kind::Boxed => return unsupported("a copy-update of an enum"),
        }
        self.store(i)
    }

    /// `Field`: field `idx` of `a`, picked from a struct's values or loaded
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
        }
        self.store(i)
    }

    /// `FieldSet`: a store into a data value's field. The checker writes a
    /// struct's field as a copy-update of the place, never as a store.
    pub(super) fn field_set(&mut self, a: u32, idx: u32, v: u32) -> StageResult<()> {
        if v != NONE && self.diverges(v) {
            self.f.op(I::Unreachable);
            return Ok(());
        }
        let Kind::Data(tys) = self.kind(self.ty_of(a)) else {
            return unsupported("a field write to a value that is not a data value");
        };
        let (m, parts) = payload_mem(self.pool(), self.env(), &tys)?;
        let Some(p) = parts.get(idx as usize).copied() else {
            return unsupported("a field index outside the data type");
        };
        let at = self.f.local(Val::I32);
        self.comp(a, 0)?;
        self.f.set(at);
        self.store_part(at, &m, p, &Src::Ref(v))
    }

    /// `TupleGet`: element `idx`'s values.
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

    /// `a is b` (`expr.is.*`): two data values are the same object.
    pub(super) fn is(&mut self, i: u32, a: u32, b: u32) -> StageResult<()> {
        if !matches!(self.kind(self.ty_of(a)), Kind::Data(_) | Kind::Boxed) {
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

    /// `Payload`: field `field` of variant `variant` of enum value `a`.
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
        for p in at {
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
            let cell = self.object(m.size);
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
        } else {
            for p in 0..m.leaves.len() {
                push(self, p)?;
            }
        }
        self.store(i)
    }
}
