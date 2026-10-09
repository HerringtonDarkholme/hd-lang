//! Boundary codecs (runtime-and-host.md §17.4): the encoders and decoders
//! of the values that cross the exchange buffer, generated per boundary
//! type from its structure, as a derive would be. A `BTy` describes a type
//! by its parts and its Wasm layout, with no pool, so a codec helper is
//! keyed by it alone.
//!
//! Each encoder is `(off, values...) -> off'`: it writes the value at
//! `off`, growing the buffer first, and returns the end. Each decoder is
//! `(off) -> (values..., off')`: it reads the value at `off`, checking
//! every length against the buffer's size, and returns its values and the
//! end. A malformed or out-of-range result is the `host-contract` panic.

use hd_base::StageResult;
use hd_base::wire::{Reader, Writer};
use hd_mono::subst;
use hd_types::{Prim, Ty, TyData};

use crate::asm::{Asm, mem};
use crate::layout::{Lay, OptShape, Shape};
use crate::rt::{Helper, OptForm};
use crate::{Code, Sym, VT, WTy, decode_vts, encode_vts, unsupported};

/// How deep boundary types may nest: a recursive type does not cross.
const MAX_DEPTH: u32 = 32;

/// A boundary type (runtime-and-host.md §17.4).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BTy {
    /// `void`: no bytes.
    Void,
    /// An integer, `bool` (one bit) or `char` (21 bits): LEB128, zigzag
    /// when signed. Values wider than 32 bits are an `i64`.
    Int {
        bits: u8,
        signed: bool,
    },
    /// Raw IEEE 754 little-endian bytes.
    F32,
    F64,
    /// A string: its length, then its viewed bytes.
    Str,
    /// `List[u8]` (its list type): its length, then its bytes.
    Bytes(WTy),
    /// Any other list: its count, then its elements.
    List {
        ty: WTy,
        elem: Box<BTy>,
    },
    /// `T?`: 0, or 1 and the value.
    Opt {
        form: OptForm,
        inner: Box<BTy>,
    },
    /// A data type (a newtype too): its fields in declaration order.
    Data {
        ty: WTy,
        fields: Vec<BTy>,
    },
    /// An enum: the variant index, then the payload fields.
    Enum(Box<EnumB>),
}

/// An enum's layout (`layout::EnumShape`) with its payloads' boundary types.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EnumB {
    pub slots: Vec<VT>,
    pub boxed: Option<WTy>,
    pub subtypes: Vec<Option<WTy>>,
    /// Per variant, per payload field: its slots and its boundary type.
    pub variants: Vec<Vec<(Vec<usize>, BTy)>>,
}

impl BTy {
    /// The boundary type of `t`, or `unsupported` for a type that does not
    /// cross (a function, a trait value, a map, a tuple, a recursive type).
    pub fn of(lay: &Lay<'_>, t: Ty) -> StageResult<BTy> {
        Self::of_at(lay, t, 0)
    }

    fn of_at(lay: &Lay<'_>, t: Ty, depth: u32) -> StageResult<BTy> {
        let pool = lay.pool;
        let t = lay.strip(t);
        let what = || format!("`{}` at the host boundary", pool.display(t));
        if depth > MAX_DEPTH {
            return unsupported(what());
        }
        let int = |bits, signed| BTy::Int { bits, signed };
        Ok(match lay.shape(t)? {
            Shape::Void => BTy::Void,
            Shape::Scalar(_) => match pool.get(t) {
                TyData::Prim(p) => match p {
                    Prim::F32 => BTy::F32,
                    Prim::F64 => BTy::F64,
                    Prim::I8 => int(8, true),
                    Prim::I16 => int(16, true),
                    Prim::I32 => int(32, true),
                    Prim::I64 => int(64, true),
                    Prim::U8 => int(8, false),
                    Prim::U16 => int(16, false),
                    Prim::U32 | Prim::Usize => int(32, false),
                    Prim::U64 => int(64, false),
                    Prim::Bool => int(1, false),
                    Prim::Char => int(21, false),
                    Prim::String | Prim::Void => return unsupported(what()),
                },
                _ => return unsupported(what()),
            },
            Shape::Str => BTy::Str,
            Shape::List { ty, .. } => {
                let TyData::Adt { args, .. } = pool.get(t) else {
                    return unsupported(what());
                };
                let [e] = pool.list_items(args)[..] else {
                    return unsupported(what());
                };
                if matches!(pool.get(lay.strip(e)), TyData::Prim(Prim::U8)) {
                    BTy::Bytes(ty)
                } else {
                    BTy::List {
                        ty,
                        elem: Box::new(Self::of_at(lay, e, depth + 1)?),
                    }
                }
            }
            Shape::Opt(o, _) => {
                let TyData::Option(inner) = pool.get(t) else {
                    return unsupported(what());
                };
                BTy::Opt {
                    form: match o {
                        OptShape::NullRef(_) => OptForm::NullRef,
                        OptShape::NullStr => OptForm::NullStr,
                        OptShape::Tagged(_) => OptForm::Tagged,
                        OptShape::Boxed(b, _) => OptForm::Boxed(b),
                    },
                    inner: Box::new(Self::of_at(lay, inner, depth + 1)?),
                }
            }
            Shape::Data { ty, fields } => {
                let TyData::Adt { def, args } = pool.get(t) else {
                    return unsupported(what());
                };
                if fields.is_empty() {
                    return unsupported(what());
                }
                let mut fs = Vec::new();
                for f in lay.env.data_fields(def).unwrap_or_default() {
                    fs.push(Self::of_at(
                        lay,
                        subst(pool, lay.env, def, args, f),
                        depth + 1,
                    )?);
                }
                BTy::Data { ty, fields: fs }
            }
            Shape::Enum(e) => {
                let TyData::Adt { def, args } = pool.get(t) else {
                    return unsupported(what());
                };
                let vs = lay.env.enum_variants(def, args).unwrap_or_default();
                let mut variants = Vec::new();
                for (tys, fs) in vs.iter().zip(&e.fields) {
                    let mut v = Vec::new();
                    for (ft, (slots, _)) in tys.iter().zip(fs) {
                        v.push((slots.clone(), Self::of_at(lay, *ft, depth + 1)?));
                    }
                    variants.push(v);
                }
                BTy::Enum(Box::new(EnumB {
                    slots: e.slots,
                    boxed: e.boxed,
                    subtypes: e.subtypes,
                    variants,
                }))
            }
            _ => return unsupported(what()),
        })
    }

    /// The type's Wasm values (`Lay::vts`).
    #[must_use]
    pub fn vts(&self) -> Vec<VT> {
        match self {
            BTy::Void => vec![],
            BTy::Int { bits, .. } => vec![if *bits > 32 { VT::I64 } else { VT::I32 }],
            BTy::F32 => vec![VT::F32],
            BTy::F64 => vec![VT::F64],
            BTy::Str => vec![VT::r(WTy::Bytes), VT::I64],
            BTy::Bytes(t) | BTy::List { ty: t, .. } | BTy::Data { ty: t, .. } => {
                vec![VT::r(t.clone())]
            }
            BTy::Opt { form, inner } => {
                let iv = inner.vts();
                match form {
                    OptForm::NullRef => iv.iter().take(1).map(VT::dflt).collect(),
                    OptForm::NullStr => vec![VT::rn(WTy::Bytes), VT::I64],
                    OptForm::Tagged => std::iter::once(VT::I32)
                        .chain(iv.iter().map(VT::dflt))
                        .collect(),
                    OptForm::Boxed(b) => vec![VT::rn(b.clone())],
                }
            }
            BTy::Enum(e) => match &e.boxed {
                Some(b) => vec![VT::r(b.clone())],
                None => std::iter::once(VT::I32)
                    .chain(e.slots.iter().cloned())
                    .collect(),
            },
        }
    }

    /// An enum whose payloads are at most one payload-free enum each, such
    /// as `Result[void, ConsoleError]`: its encoding is one or two variant
    /// indices, which `rt::host_poll` reads as bytes.
    #[must_use]
    pub fn is_tags(&self) -> bool {
        let BTy::Enum(e) = self else {
            return false;
        };
        e.boxed.is_none()
            && e.variants.len() < 128
            && e.variants.iter().all(|v| match v.as_slice() {
                [] | [(_, BTy::Void)] => true,
                [(_, BTy::Enum(p))] => {
                    p.boxed.is_none()
                        && p.variants.len() < 128
                        && p.variants.iter().all(Vec::is_empty)
                }
                _ => false,
            })
    }

    pub(crate) fn encode(&self, w: &mut Writer) {
        match self {
            BTy::Void => w.u8(0),
            BTy::Int { bits, signed } => {
                w.u8(1);
                w.u8(*bits);
                w.u8(u8::from(*signed));
            }
            BTy::F32 => w.u8(2),
            BTy::F64 => w.u8(3),
            BTy::Str => w.u8(4),
            BTy::Bytes(t) => {
                w.u8(5);
                enc_wty(t, w);
            }
            BTy::List { ty, elem } => {
                w.u8(6);
                enc_wty(ty, w);
                elem.encode(w);
            }
            BTy::Opt { form, inner } => {
                w.u8(7);
                form.encode(w);
                inner.encode(w);
            }
            BTy::Data { ty, fields } => {
                w.u8(8);
                enc_wty(ty, w);
                w.len_of(fields);
                for f in fields {
                    f.encode(w);
                }
            }
            BTy::Enum(e) => {
                w.u8(9);
                encode_vts(&e.slots, w);
                match &e.boxed {
                    Some(b) => {
                        w.u8(1);
                        enc_wty(b, w);
                    }
                    None => w.u8(0),
                }
                w.len_of(&e.subtypes);
                for s in &e.subtypes {
                    match s {
                        Some(t) => {
                            w.u8(1);
                            enc_wty(t, w);
                        }
                        None => w.u8(0),
                    }
                }
                w.len_of(&e.variants);
                for v in &e.variants {
                    w.len_of(v);
                    for (slots, b) in v {
                        w.len_of(slots);
                        for s in slots {
                            w.u32(u32::try_from(*s).unwrap_or(u32::MAX));
                        }
                        b.encode(w);
                    }
                }
            }
        }
    }

    pub(crate) fn decode(r: &mut Reader<'_>) -> Option<BTy> {
        Some(match r.u8() {
            0 => BTy::Void,
            1 => BTy::Int {
                bits: r.u8(),
                signed: r.u8() == 1,
            },
            2 => BTy::F32,
            3 => BTy::F64,
            4 => BTy::Str,
            5 => BTy::Bytes(dec_wty(r)?),
            6 => BTy::List {
                ty: dec_wty(r)?,
                elem: Box::new(BTy::decode(r)?),
            },
            7 => BTy::Opt {
                form: OptForm::decode(r)?,
                inner: Box::new(BTy::decode(r)?),
            },
            8 => BTy::Data {
                ty: dec_wty(r)?,
                fields: (0..r.count())
                    .map(|_| BTy::decode(r))
                    .collect::<Option<Vec<_>>>()?,
            },
            9 => {
                let slots = decode_vts(r, 0)?;
                let boxed = if r.u8() == 1 { Some(dec_wty(r)?) } else { None };
                let subtypes = (0..r.count())
                    .map(|_| {
                        if r.u8() == 1 {
                            dec_wty(r).map(Some)
                        } else {
                            Some(None)
                        }
                    })
                    .collect::<Option<Vec<_>>>()?;
                let variants = (0..r.count())
                    .map(|_| {
                        (0..r.count())
                            .map(|_| {
                                let slots: Vec<usize> =
                                    (0..r.count()).map(|_| r.u32() as usize).collect();
                                Some((slots, BTy::decode(r)?))
                            })
                            .collect::<Option<Vec<_>>>()
                    })
                    .collect::<Option<Vec<_>>>()?;
                BTy::Enum(Box::new(EnumB {
                    slots,
                    boxed,
                    subtypes,
                    variants,
                }))
            }
            _ => return None,
        })
    }
}

fn enc_wty(t: &WTy, w: &mut Writer) {
    encode_vts(&[VT::r(t.clone())], w);
}

fn dec_wty(r: &mut Reader<'_>) -> Option<WTy> {
    match decode_vts(r, 0)?.pop()? {
        VT::Ref(t, _) => Some(std::sync::Arc::unwrap_or_clone(t)),
        _ => None,
    }
}

fn u32_of(n: usize) -> u32 {
    u32::try_from(n).expect("index")
}

/// The `host-contract` panic of a result that does not decode.
fn malformed(a: &mut Asm) {
    a.call(Sym::Helper(Helper::Panic(
        "host-contract: a host result does not decode".into(),
    )));
    a.s().unreachable();
}

/// Pushes the exchange buffer's size in bytes.
fn buffer_size(a: &mut Asm) {
    a.s().memory_size(0);
    a.i32(16);
    a.s().i32_shl();
}

/// Panics unless `n` bytes from offset `off` lie inside the buffer.
fn check_room(a: &mut Asm, off: u32, n: u32) {
    a.get(n);
    buffer_size(a);
    a.get(off);
    a.s().i32_sub().i32_gt_u();
    a.if_();
    malformed(a);
    a.end();
}

/// `(end)`: grows the exchange buffer to at least `end` bytes
/// (runtime-and-host.md §17.3); a refused growth is `heap-exhausted`.
#[must_use]
pub fn fit_code() -> Code {
    let mut a = Asm::new(vec![VT::I32]);
    a.get(0);
    buffer_size(&mut a);
    a.s().i32_gt_u();
    a.if_();
    a.get(0);
    a.i32(65535);
    a.s().i32_add();
    a.i32(16);
    a.s().i32_shr_u().memory_size(0).i32_sub().memory_grow(0);
    a.i32(-1);
    a.s().i32_eq();
    a.if_();
    a.call(Sym::Helper(Helper::Panic(
        "heap-exhausted: the exchange buffer cannot grow".into(),
    )));
    a.s().unreachable();
    a.end();
    a.end();
    a.finish(vec![])
}

/// `(off, v: i64) -> off'`: `v` as unsigned LEB128.
#[must_use]
pub fn leb_put_code() -> Code {
    let mut a = Asm::new(vec![VT::I32, VT::I64]);
    let b = a.local(VT::I32);
    a.get(0);
    a.i32(10);
    a.s().i32_add();
    a.call(Sym::Helper(Helper::Fit));
    a.loop_();
    a.get(1);
    a.s().i32_wrap_i64();
    a.i32(127);
    a.s().i32_and();
    a.set(b);
    a.get(1);
    a.i64(7);
    a.s().i64_shr_u();
    a.set(1);
    a.get(1);
    a.i64(0);
    a.s().i64_ne();
    a.if_();
    a.get(b);
    a.i32(128);
    a.s().i32_or();
    a.set(b);
    a.end();
    a.get(0);
    a.get(b);
    a.mem8_store(0);
    a.get(0);
    a.i32(1);
    a.s().i32_add();
    a.set(0);
    a.get(1);
    a.i64(0);
    a.s().i64_ne();
    a.br_if(0);
    a.end();
    a.get(0);
    a.finish(vec![VT::I32])
}

/// `(off) -> (v: i64, off')`: an unsigned LEB128 number.
#[must_use]
pub fn leb_get_code() -> Code {
    let mut a = Asm::new(vec![VT::I32]);
    let (r, sh, b) = (a.local(VT::I64), a.local(VT::I64), a.local(VT::I32));
    a.loop_();
    a.get(0);
    buffer_size(&mut a);
    a.s().i32_ge_u();
    a.if_();
    malformed(&mut a);
    a.end();
    a.get(0);
    a.mem8_load(0);
    a.set(b);
    a.get(0);
    a.i32(1);
    a.s().i32_add();
    a.set(0);
    a.get(r);
    a.get(b);
    a.i32(127);
    a.s().i32_and().i64_extend_i32_u();
    a.get(sh);
    a.s().i64_shl().i64_or();
    a.set(r);
    a.get(sh);
    a.i64(7);
    a.s().i64_add();
    a.set(sh);
    a.get(b);
    a.i32(128);
    a.s().i32_and();
    a.br_if(0);
    a.end();
    a.get(r);
    a.get(0);
    a.finish(vec![VT::I64, VT::I32])
}

/// Calls `LebGet` at local `off`, sets `off` to the end, and leaves the
/// number on the stack as an `i32`.
fn get_len(a: &mut Asm, off: u32) {
    a.get(off);
    a.call(Sym::Helper(Helper::LebGet));
    a.set(off);
    a.s().i32_wrap_i64();
}

/// Calls `LebPut` with local `off` and the `i64` on the stack's top
/// pushed by `push`, and sets `off` to the end.
fn put(a: &mut Asm, off: u32, push: impl FnOnce(&mut Asm)) {
    a.get(off);
    push(a);
    a.call(Sym::Helper(Helper::LebPut));
    a.set(off);
}

/// The encoder of `b`: `(off, values...) -> off'`.
pub fn enc_code(b: &BTy) -> StageResult<Code> {
    let mut params = vec![VT::I32];
    params.extend(b.vts());
    let mut a = Asm::new(params);
    match b {
        BTy::Void => {}
        BTy::Int { bits, signed } => {
            let x = a.local(VT::I64);
            a.get(1);
            if *bits <= 32 {
                if *signed {
                    a.s().i64_extend_i32_s();
                } else {
                    a.s().i64_extend_i32_u();
                }
            }
            a.set(x);
            put(&mut a, 0, |a| {
                a.get(x);
                if *signed {
                    a.i64(1);
                    a.s().i64_shl();
                    a.get(x);
                    a.i64(63);
                    a.s().i64_shr_s().i64_xor();
                }
            });
        }
        BTy::F32 | BTy::F64 => {
            let n = if *b == BTy::F64 { 8 } else { 4 };
            a.get(0);
            a.i32(n);
            a.s().i32_add();
            a.call(Sym::Helper(Helper::Fit));
            a.get(0);
            a.get(1);
            if n == 8 {
                a.s().f64_store(mem(0));
            } else {
                a.s().f32_store(mem(0));
            }
            a.get(0);
            a.i32(n);
            a.s().i32_add();
            a.set(0);
        }
        BTy::Str => {
            let (len, start, k) = (a.local(VT::I32), a.local(VT::I32), a.local(VT::I32));
            a.get(2);
            a.i64(32);
            a.s().i64_shr_u().i32_wrap_i64();
            a.set(len);
            a.get(2);
            a.s().i32_wrap_i64();
            a.set(start);
            put(&mut a, 0, |a| {
                a.get(len);
                a.s().i64_extend_i32_u();
            });
            copy_out(&mut a, 0, len, k, |a| {
                a.get(1);
                a.get(start);
                a.get(k);
                a.s().i32_add();
                a.array_get(&WTy::Bytes);
            });
        }
        BTy::Bytes(t) => {
            let arr = WTy::Array(VT::I32);
            let (n, k) = (a.local(VT::I32), a.local(VT::I32));
            a.get(1);
            a.struct_get(t, 0);
            a.set(n);
            put(&mut a, 0, |a| {
                a.get(n);
                a.s().i64_extend_i32_u();
            });
            copy_out(&mut a, 0, n, k, |a| {
                a.get(1);
                a.struct_get(t, 1);
                a.get(k);
                a.array_get(&arr);
            });
        }
        BTy::List { ty, elem } => {
            let ev = elem.vts();
            let arrays = list_arrays(ty, ev.len())?;
            let (n, i) = (a.local(VT::I32), a.local(VT::I32));
            a.get(1);
            a.struct_get(ty, 0);
            a.set(n);
            put(&mut a, 0, |a| {
                a.get(n);
                a.s().i64_extend_i32_u();
            });
            a.block();
            a.loop_();
            a.get(i);
            a.get(n);
            a.s().i32_ge_u();
            a.br_if(1);
            a.get(0);
            for (k, (arr, v)) in arrays.iter().zip(&ev).enumerate() {
                a.get(1);
                a.struct_get(ty, 1 + u32_of(k));
                a.get(i);
                a.array_get(arr);
                let WTy::Array(st) = arr else {
                    return unsupported("a list field that is not an array");
                };
                a.conv(st, v);
            }
            a.call(Sym::Helper(Helper::Enc((**elem).clone())));
            a.set(0);
            a.get(i);
            a.i32(1);
            a.s().i32_add();
            a.set(i);
            a.br(0);
            a.end();
            a.end();
        }
        BTy::Opt { form, inner } => {
            let iv = inner.vts();
            a.get(1);
            match form {
                OptForm::Tagged => {
                    a.s().i32_eqz();
                }
                _ => {
                    a.s().ref_is_null();
                }
            }
            a.if_();
            put(&mut a, 0, |a| a.i64(0));
            a.get(0);
            a.s().return_();
            a.end();
            put(&mut a, 0, |a| a.i64(1));
            a.get(0);
            match form {
                OptForm::NullRef => {
                    a.get(1);
                    a.conv(&iv[0].dflt(), &iv[0]);
                }
                OptForm::NullStr => {
                    a.get(1);
                    a.s().ref_as_non_null();
                    a.get(2);
                }
                OptForm::Tagged => {
                    for (k, v) in iv.iter().enumerate() {
                        a.get(2 + u32_of(k));
                        a.conv(&v.dflt(), v);
                    }
                }
                OptForm::Boxed(bx) => {
                    for (k, v) in iv.iter().enumerate() {
                        a.get(1);
                        a.s().ref_as_non_null();
                        a.struct_get(bx, u32_of(k));
                        a.conv(&v.dflt(), v);
                    }
                }
            }
            a.call(Sym::Helper(Helper::Enc((**inner).clone())));
            a.set(0);
        }
        BTy::Data { ty, fields } => {
            let mut first = 0;
            for f in fields {
                let n = f.vts().len();
                a.get(0);
                for k in 0..n {
                    a.get(1);
                    a.struct_get(ty, u32_of(first + k));
                }
                a.call(Sym::Helper(Helper::Enc(f.clone())));
                a.set(0);
                first += n;
            }
        }
        BTy::Enum(e) => {
            let tag = a.local(VT::I32);
            a.get(1);
            if let Some(bx) = &e.boxed {
                a.struct_get(bx, 0);
            }
            a.set(tag);
            put(&mut a, 0, |a| {
                a.get(tag);
                a.s().i64_extend_i32_u();
            });
            for (v, fields) in e.variants.iter().enumerate() {
                if fields.is_empty() {
                    continue;
                }
                a.get(tag);
                a.i32(i32::try_from(v).unwrap_or(i32::MAX));
                a.s().i32_eq();
                a.if_();
                let sub = match (&e.boxed, e.subtypes.get(v)) {
                    (Some(_), Some(Some(sub))) => Some(sub),
                    _ => None,
                };
                for (slots, fb) in fields {
                    a.get(0);
                    if let Some(sub) = sub {
                        for s in slots {
                            a.get(1);
                            a.ref_cast(sub, false);
                            a.struct_get(sub, u32_of(*s + 1));
                        }
                    } else {
                        for (s, want) in slots.iter().zip(fb.vts()) {
                            let st = &e.slots[*s];
                            match &e.boxed {
                                Some(bx) => {
                                    a.get(1);
                                    a.struct_get(bx, u32_of(*s + 1));
                                }
                                None => a.get(2 + u32_of(*s)),
                            }
                            a.conv(st, &want);
                        }
                    }
                    a.call(Sym::Helper(Helper::Enc(fb.clone())));
                    a.set(0);
                }
                a.end();
            }
        }
    }
    a.get(0);
    Ok(a.finish(vec![VT::I32]))
}

/// Grows the buffer to `off + n`, writes `n` bytes, each pushed by
/// `byte` with `k` its index, and sets `off` to the end.
fn copy_out(a: &mut Asm, off: u32, n: u32, k: u32, byte: impl Fn(&mut Asm)) {
    a.get(off);
    a.get(n);
    a.s().i32_add();
    a.call(Sym::Helper(Helper::Fit));
    a.block();
    a.loop_();
    a.get(k);
    a.get(n);
    a.s().i32_ge_u();
    a.br_if(1);
    a.get(off);
    a.get(k);
    a.s().i32_add();
    byte(a);
    a.mem8_store(0);
    a.get(k);
    a.i32(1);
    a.s().i32_add();
    a.set(k);
    a.br(0);
    a.end();
    a.end();
    a.get(off);
    a.get(n);
    a.s().i32_add();
    a.set(off);
}

/// Reads `n` bytes from `off` into the array in local `arr` of type `t`,
/// and sets `off` to the end.
fn copy_in(a: &mut Asm, off: u32, n: u32, k: u32, arr: u32, t: &WTy) {
    a.block();
    a.loop_();
    a.get(k);
    a.get(n);
    a.s().i32_ge_u();
    a.br_if(1);
    a.get(arr);
    a.get(k);
    a.get(off);
    a.get(k);
    a.s().i32_add();
    a.mem8_load(0);
    a.array_set(t);
    a.get(k);
    a.i32(1);
    a.s().i32_add();
    a.set(k);
    a.br(0);
    a.end();
    a.end();
    a.get(off);
    a.get(n);
    a.s().i32_add();
    a.set(off);
}

/// The element arrays of a list type.
fn list_arrays(ty: &WTy, n: usize) -> StageResult<Vec<WTy>> {
    let fields = ty.fields();
    if fields.len() != n + 1 {
        return unsupported("a list type without one array per element value");
    }
    fields[1..]
        .iter()
        .map(|f| match f {
            VT::Ref(t, _) => Ok((**t).clone()),
            _ => unsupported("a list field that is not an array"),
        })
        .collect()
}

/// Calls the decoder of `b` at local `off`, sets `off` to the end, and
/// stores the values into fresh locals, which it returns.
fn dec_into(a: &mut Asm, off: u32, b: &BTy) -> Vec<u32> {
    a.get(off);
    a.call(Sym::Helper(Helper::Dec(b.clone())));
    a.set(off);
    let locals: Vec<u32> = b.vts().into_iter().map(|v| a.local(v)).collect();
    for l in locals.iter().rev() {
        a.set(*l);
    }
    locals
}

/// Builds variant `v` of an enum, whose tag is on the stack, from its
/// decoded payload fields in `vals`, into the result locals `res`, as
/// `Emit::new_variant` builds one.
fn build_variant(a: &mut Asm, e: &EnumB, v: usize, vals: &[Vec<u32>], res: &[u32]) {
    if let (Some(base), Some(sub)) = (&e.boxed, e.subtypes.get(v)) {
        // A subtype box: the tag, then the payload's exact values; a
        // payloadless variant is the base alone.
        if let Some(sub) = sub {
            for l in vals.iter().flatten() {
                a.get(*l);
            }
            a.struct_new(sub);
        } else {
            a.struct_new(base);
        }
        a.set(res[0]);
        return;
    }
    let fields = &e.variants[v];
    for (s, st) in e.slots.iter().enumerate() {
        let src = fields.iter().zip(vals).find_map(|((slots, fb), ls)| {
            slots
                .iter()
                .position(|x| *x == s)
                .map(|k| (ls[k], fb.vts()[k].clone()))
        });
        if let Some((l, exact)) = src {
            a.get(l);
            a.conv(&exact, st);
        } else {
            a.zero(st);
        }
    }
    if let Some(bx) = &e.boxed {
        a.struct_new(bx);
        a.set(res[0]);
    } else {
        for r in res.iter().rev() {
            a.set(*r);
        }
    }
}

/// The decoder of `b`: `(off) -> (values..., off')`.
pub fn dec_code(b: &BTy) -> StageResult<Code> {
    let mut a = Asm::new(vec![VT::I32]);
    let vts = b.vts();
    match b {
        BTy::Void => {}
        BTy::Int { bits, signed } => {
            let x = a.local(VT::I64);
            a.get(0);
            a.call(Sym::Helper(Helper::LebGet));
            a.set(0);
            a.set(x);
            if *signed {
                a.get(x);
                a.i64(1);
                a.s().i64_shr_u();
                a.i64(0);
                a.get(x);
                a.i64(1);
                a.s().i64_and().i64_sub().i64_xor();
                a.set(x);
            }
            if *bits < 64 {
                let rest = i64::from(64 - *bits);
                if *signed {
                    a.get(x);
                    a.i64(rest);
                    a.s().i64_shl();
                    a.i64(rest);
                    a.s().i64_shr_s();
                    a.get(x);
                    a.s().i64_ne();
                } else {
                    a.get(x);
                    a.i64(i64::from(*bits));
                    a.s().i64_shr_u();
                    a.i64(0);
                    a.s().i64_ne();
                }
                a.if_();
                a.call(Sym::Helper(Helper::Panic(
                    "host-contract: a host result is out of range".into(),
                )));
                a.s().unreachable();
                a.end();
            }
            a.get(x);
            if *bits <= 32 {
                a.s().i32_wrap_i64();
            }
        }
        BTy::F32 | BTy::F64 => {
            let n = if *b == BTy::F64 { 8 } else { 4 };
            let size = a.local(VT::I32);
            a.i32(n);
            a.set(size);
            check_room(&mut a, 0, size);
            a.get(0);
            if n == 8 {
                a.s().f64_load(mem(0));
            } else {
                a.s().f32_load(mem(0));
            }
            a.get(0);
            a.i32(n);
            a.s().i32_add();
            a.set(0);
        }
        BTy::Str => {
            let (len, arr, k) = (
                a.local(VT::I32),
                a.local(VT::r(WTy::Bytes)),
                a.local(VT::I32),
            );
            get_len(&mut a, 0);
            a.set(len);
            check_room(&mut a, 0, len);
            a.get(len);
            a.array_new_default(&WTy::Bytes);
            a.set(arr);
            copy_in(&mut a, 0, len, k, arr, &WTy::Bytes);
            a.get(arr);
            a.get(len);
            a.s().i64_extend_i32_u();
            a.i64(32);
            a.s().i64_shl();
        }
        BTy::Bytes(t) => {
            let at = WTy::Array(VT::I32);
            let (n, arr, k) = (
                a.local(VT::I32),
                a.local(VT::r(at.clone())),
                a.local(VT::I32),
            );
            get_len(&mut a, 0);
            a.set(n);
            check_room(&mut a, 0, n);
            a.get(n);
            a.array_new_default(&at);
            a.set(arr);
            copy_in(&mut a, 0, n, k, arr, &at);
            a.get(n);
            a.get(arr);
            a.struct_new(t);
        }
        BTy::List { ty, elem } => {
            let ev = elem.vts();
            let arrays = list_arrays(ty, ev.len())?;
            let (n, i) = (a.local(VT::I32), a.local(VT::I32));
            let arrs: Vec<u32> = arrays.iter().map(|t| a.local(VT::r(t.clone()))).collect();
            get_len(&mut a, 0);
            a.set(n);
            // Every element takes at least one byte.
            check_room(&mut a, 0, n);
            for (t, l) in arrays.iter().zip(&arrs) {
                a.get(n);
                a.array_new_default(t);
                a.set(*l);
            }
            a.block();
            a.loop_();
            a.get(i);
            a.get(n);
            a.s().i32_ge_u();
            a.br_if(1);
            let vals = dec_into(&mut a, 0, elem);
            for ((t, l), v) in arrays.iter().zip(&arrs).zip(&vals) {
                a.get(*l);
                a.get(i);
                a.get(*v);
                a.array_set(t);
            }
            a.get(i);
            a.i32(1);
            a.s().i32_add();
            a.set(i);
            a.br(0);
            a.end();
            a.end();
            a.get(n);
            for l in &arrs {
                a.get(*l);
            }
            a.struct_new(ty);
        }
        BTy::Opt { form, inner } => {
            let tag = a.local(VT::I32);
            let res: Vec<u32> = vts.iter().map(|v| a.local(v.clone())).collect();
            get_len(&mut a, 0);
            a.set(tag);
            a.get(tag);
            a.i32(1);
            a.s().i32_gt_u();
            a.if_();
            malformed(&mut a);
            a.end();
            a.get(tag);
            a.if_();
            let vals = dec_into(&mut a, 0, inner);
            match form {
                OptForm::NullRef | OptForm::NullStr => {
                    for (r, v) in res.iter().zip(&vals) {
                        a.get(*v);
                        a.set(*r);
                    }
                }
                OptForm::Tagged => {
                    a.i32(1);
                    a.set(res[0]);
                    for (r, v) in res[1..].iter().zip(&vals) {
                        a.get(*v);
                        a.set(*r);
                    }
                }
                OptForm::Boxed(bx) => {
                    for v in &vals {
                        a.get(*v);
                    }
                    a.struct_new(bx);
                    a.set(res[0]);
                }
            }
            a.end();
            for r in &res {
                a.get(*r);
            }
        }
        BTy::Data { ty, fields } => {
            let mut vals = Vec::new();
            for f in fields {
                vals.extend(dec_into(&mut a, 0, f));
            }
            for v in &vals {
                a.get(*v);
            }
            a.struct_new(ty);
        }
        BTy::Enum(e) => {
            let tag = a.local(VT::I32);
            let res: Vec<u32> = vts.iter().map(|v| a.local(v.clone())).collect();
            get_len(&mut a, 0);
            a.set(tag);
            a.get(tag);
            a.i32(i32::try_from(e.variants.len()).unwrap_or(i32::MAX));
            a.s().i32_ge_u();
            a.if_();
            malformed(&mut a);
            a.end();
            for (v, fields) in e.variants.iter().enumerate() {
                let vi = i32::try_from(v).unwrap_or(i32::MAX);
                a.get(tag);
                a.i32(vi);
                a.s().i32_eq();
                a.if_();
                let vals: Vec<Vec<u32>> = fields
                    .iter()
                    .map(|(_, fb)| dec_into(&mut a, 0, fb))
                    .collect();
                a.i32(vi);
                build_variant(&mut a, e, v, &vals, &res);
                a.end();
            }
            for r in &res {
                a.get(*r);
            }
        }
    }
    a.get(0);
    let mut results = vts;
    results.push(VT::I32);
    Ok(a.finish(results))
}
