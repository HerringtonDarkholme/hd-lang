//! Linear layouts (`crates/hd_wasm/DESIGN.md` §23.3): a type's Wasm
//! values in locals, parameters and results, and the same values in
//! memory, each at an offset.
//!
//! One walk gives both: a type's [`Mem`] lists its values as leaves, in
//! the order its locals hold them, each with its offset and width in
//! memory. A struct or a tuple is its fields' leaves, each field at its
//! alignment. An enum is its tag, then slots that its variants share: the
//! `k`th value of one Wasm type in a variant's payload is in that type's
//! `k`th slot, so a variant fills only the slots it needs and loads and
//! stores need no branch on the tag. A data value, a self-recursive enum
//! and a suspension are one address; a string is its address and byte
//! length; a trait value its data address and its vtable's address.
//! Lists, maps, function values and references come with later jobs.

use hd_base::StageResult;
use hd_mono::layout::{StdKind, strip_newtype};
use hd_mono::{ProgramEnv, subst};
use hd_types::{InternPool, Prim, Ty, TyData};

use crate::module::Val;
use crate::unsupported;

/// One Wasm value of a type, where it lies in memory: its offset in the
/// value, its width in bytes and whether a narrow load sign-extends it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Leaf {
    pub offset: u32,
    pub val: Val,
    pub bytes: u8,
    pub signed: bool,
}

/// A type's layout: its size and alignment in memory, and its values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mem {
    pub size: u32,
    pub align: u32,
    pub leaves: Vec<Leaf>,
}

impl Mem {
    const EMPTY: Mem = Mem {
        size: 0,
        align: 1,
        leaves: Vec::new(),
    };

    fn one(val: Val, bytes: u8, signed: bool) -> Mem {
        Mem {
            size: u32::from(bytes),
            align: u32::from(bytes),
            leaves: vec![Leaf {
                offset: 0,
                val,
                bytes,
                signed,
            }],
        }
    }

    /// An address: data, a self-recursive enum's cell, a frame.
    fn address() -> Mem {
        Mem::one(Val::I32, 4, false)
    }

    pub fn vals(&self) -> Vec<Val> {
        self.leaves.iter().map(|l| l.val).collect()
    }
}

/// One field of a struct, a tuple or a data payload: its first value's
/// index among the parent's values, its value count and its offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Part {
    pub first: usize,
    pub count: usize,
    pub offset: u32,
}

/// What a concrete type is to this backend.
pub enum Kind {
    /// Its values are in locals: a scalar, a string, an enum, a struct or
    /// tuple, a trait value.
    Value,
    /// A data value: the address of an object whose payload holds
    /// `fields` (`§23.4`).
    Data(Vec<Ty>),
    /// A self-recursive enum: the address of a cell holding the enum's
    /// value as a non-recursive enum lays it out.
    Boxed,
}

/// Which kind `t` is.
pub fn kind(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Kind {
    let t = strip(pool, env, t);
    let TyData::Adt { def, args } = pool.get(t) else {
        return Kind::Value;
    };
    if env.std_kind(def) != StdKind::Other || def == env.ref_cell() {
        return Kind::Value;
    }
    if env.enum_variants(def, args).is_some() {
        return if env.recursive_enum(def) {
            Kind::Boxed
        } else {
            Kind::Value
        };
    }
    match env.data_fields(def) {
        Some(fs) if !env.is_struct(def) => Kind::Data(
            fs.into_iter()
                .map(|f| subst(pool, env, def, args, f))
                .collect(),
        ),
        _ => Kind::Value,
    }
}

/// `t` with newtypes erased and `mut` dropped: what its layout reads.
pub fn strip(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Ty {
    let mut t = t;
    loop {
        let s = strip_newtype(pool, env, t);
        match pool.get(s) {
            TyData::Mut(inner) => t = inner,
            _ => return s,
        }
    }
}

/// The Wasm values of a concrete type.
pub fn vals(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> StageResult<Vec<Val>> {
    Ok(mem(pool, env, t)?.vals())
}

/// A concrete type's layout.
pub fn mem(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> StageResult<Mem> {
    let t = strip(pool, env, t);
    Ok(match pool.get(t) {
        TyData::Prim(p) => match p {
            Prim::I8 => Mem::one(Val::I32, 1, true),
            Prim::U8 | Prim::Bool => Mem::one(Val::I32, 1, false),
            Prim::I16 => Mem::one(Val::I32, 2, true),
            Prim::U16 => Mem::one(Val::I32, 2, false),
            Prim::I32 | Prim::U32 | Prim::Usize | Prim::Char => Mem::one(Val::I32, 4, false),
            Prim::I64 | Prim::U64 => Mem::one(Val::I64, 8, false),
            Prim::F32 => Mem::one(Val::F32, 4, false),
            Prim::F64 => Mem::one(Val::F64, 8, false),
            Prim::String => record(&[Mem::address(), Mem::address()]).0,
        },
        TyData::Never => Mem::EMPTY,
        TyData::Tuple { elems, rest: None } => {
            let parts = pool
                .list_items(elems)
                .iter()
                .map(|e| mem(pool, env, *e))
                .collect::<StageResult<Vec<_>>>()?;
            record(&parts).0
        }
        TyData::Tuple { .. } => return unsupported("tuples with a rest list (job 9)"),
        TyData::Option(inner) => enum_mem(pool, env, &[vec![], vec![inner]])?.0,
        TyData::Adt { def, args } => match env.std_kind(def) {
            StdKind::Suspend => Mem::address(),
            StdKind::List => return unsupported("lists (job 9)"),
            StdKind::Map => return unsupported("maps (job 10)"),
            StdKind::Other if def == env.ref_cell() => return unsupported("references (job 12)"),
            StdKind::Other => match kind(pool, env, t) {
                Kind::Data(_) | Kind::Boxed => Mem::address(),
                Kind::Value => match env.enum_variants(def, args) {
                    Some(variants) => enum_mem(pool, env, &variants)?.0,
                    None => struct_mem(pool, env, t)?.0,
                },
            },
        },
        TyData::Fn { .. } => return unsupported("function values (job 11)"),
        TyData::TraitValue { .. } => record(&[Mem::address(), Mem::address()]).0,
        TyData::Ref { .. } => return unsupported("references (job 12)"),
        // An instance that collection shares among reference arguments
        // (A1, `crates/hd_mono/DESIGN.md` §13.2) holds one as an address:
        // [`shares_as_address`] lets only addresses reach it.
        TyData::Canon(_) if hd_mono::is_class_ref(pool, t) => Mem::address(),
        _ => return unsupported(format!("values of type `{}`", pool.display(t))),
    })
}

/// Whether a value of type `actual` may pass where an instance's type
/// `formal` is: equal but where `formal` has the class `REF`, which
/// collection gives an argument whose Wasm GC layout is one reference
/// (A1). Here only a data value and a boxed enum are one address; a struct
/// and a wide enum or tuple, which Wasm GC also lays out as one reference,
/// are inline values, so such an instance cannot take them.
pub fn shares_as_address(pool: &InternPool, env: &dyn ProgramEnv, actual: Ty, formal: Ty) -> bool {
    let (a, f) = (strip(pool, env, actual), strip(pool, env, formal));
    if hd_mono::is_class_ref(pool, f) {
        return matches!(kind(pool, env, a), Kind::Data(_) | Kind::Boxed);
    }
    let all = |xs: &[Ty], ys: &[Ty]| {
        xs.len() == ys.len()
            && xs
                .iter()
                .zip(ys)
                .all(|(x, y)| shares_as_address(pool, env, *x, *y))
    };
    match (pool.get(a), pool.get(f)) {
        (TyData::Adt { args: x, .. }, TyData::Adt { args: y, .. })
        | (TyData::TraitValue { args: x, .. }, TyData::TraitValue { args: y, .. })
        | (TyData::Tuple { elems: x, .. }, TyData::Tuple { elems: y, .. }) => {
            all(pool.list_items(x), pool.list_items(y))
        }
        (TyData::Option(x), TyData::Option(y))
        | (TyData::Ref { inner: x, .. }, TyData::Ref { inner: y, .. }) => {
            shares_as_address(pool, env, x, y)
        }
        (
            TyData::Fn {
                params: x,
                result: rx,
                ..
            },
            TyData::Fn {
                params: y,
                result: ry,
                ..
            },
        ) => all(pool.list_items(x), pool.list_items(y)) && shares_as_address(pool, env, rx, ry),
        _ => true,
    }
}

/// The fields of a struct, a tuple or a data payload in order, each at its
/// alignment: the whole layout and each field's place in it.
fn record(fields: &[Mem]) -> (Mem, Vec<Part>) {
    let mut out = Mem::EMPTY;
    let mut parts = Vec::new();
    for f in fields {
        let offset = out.size.next_multiple_of(f.align);
        parts.push(Part {
            first: out.leaves.len(),
            count: f.leaves.len(),
            offset,
        });
        out.leaves.extend(f.leaves.iter().map(|l| Leaf {
            offset: l.offset + offset,
            ..*l
        }));
        out.size = offset + f.size;
        out.align = out.align.max(f.align);
    }
    out.size = out.size.next_multiple_of(out.align);
    (out, parts)
}

/// A struct value's or a tuple's layout and its fields' places.
pub fn struct_mem(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> StageResult<(Mem, Vec<Part>)> {
    let t = strip(pool, env, t);
    let fields = match pool.get(t) {
        TyData::Tuple { elems, rest: None } => pool.list_items(elems).to_vec(),
        TyData::Adt { def, args } if env.enum_variants(def, args).is_none() => env
            .data_fields(def)
            .unwrap_or_default()
            .into_iter()
            .map(|f| subst(pool, env, def, args, f))
            .collect(),
        _ => return unsupported("a field of a value that is not a struct or a tuple"),
    };
    payload_mem(pool, env, &fields)
}

/// The layout of a data object's payload, or of a struct's fields.
pub fn payload_mem(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    fields: &[Ty],
) -> StageResult<(Mem, Vec<Part>)> {
    let ms = fields
        .iter()
        .map(|f| mem(pool, env, *f))
        .collect::<StageResult<Vec<_>>>()?;
    Ok(record(&ms))
}

/// The order of the slots after an enum's tag: 4-byte values first, so
/// alignment pads at most once.
const SLOT_ORDER: [Val; 4] = [Val::I32, Val::F32, Val::I64, Val::F64];

/// Each variant's payload fields, each field's values as indices among
/// the enum's values.
pub type Positions = Vec<Vec<Vec<usize>>>;

/// An enum's layout from its variants' payload types: the tag, 1, 2 or 4
/// bytes by the variant count, then the shared slots, each 4 or 8 bytes
/// wide; and where each payload field's values lie.
pub fn enum_mem(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    variants: &[Vec<Ty>],
) -> StageResult<(Mem, Positions)> {
    let mut payloads = Vec::new();
    let mut slots = [0usize; 4];
    for fields in variants {
        let ms = fields
            .iter()
            .map(|f| mem(pool, env, *f))
            .collect::<StageResult<Vec<_>>>()?;
        for (k, v) in SLOT_ORDER.iter().enumerate() {
            let n = ms
                .iter()
                .flat_map(|m| &m.leaves)
                .filter(|l| l.val == *v)
                .count();
            slots[k] = slots[k].max(n);
        }
        payloads.push(ms);
    }
    let tag_bytes = match variants.len() {
        0..=256 => 1,
        257..=65536 => 2,
        _ => 4,
    };
    let mut leaves = vec![Leaf {
        offset: 0,
        val: Val::I32,
        bytes: tag_bytes,
        signed: false,
    }];
    let mut size = u32::from(tag_bytes);
    let mut align = u32::from(tag_bytes);
    let mut start = [0usize; 4];
    for (k, v) in SLOT_ORDER.iter().enumerate() {
        start[k] = leaves.len();
        let bytes: u8 = if matches!(v, Val::I64 | Val::F64) {
            8
        } else {
            4
        };
        for _ in 0..slots[k] {
            let offset = size.next_multiple_of(u32::from(bytes));
            leaves.push(Leaf {
                offset,
                val: *v,
                bytes,
                signed: false,
            });
            size = offset + u32::from(bytes);
            align = align.max(u32::from(bytes));
        }
    }
    let mut positions = Vec::new();
    for ms in payloads {
        let mut used = [0usize; 4];
        let mut fields = Vec::new();
        for m in ms {
            let mut at = Vec::new();
            for l in &m.leaves {
                let k = SLOT_ORDER.iter().position(|v| *v == l.val).unwrap_or(0);
                at.push(start[k] + used[k]);
                used[k] += 1;
            }
            fields.push(at);
        }
        positions.push(fields);
    }
    Ok((
        Mem {
            size: size.next_multiple_of(align),
            align,
            leaves,
        },
        positions,
    ))
}

/// The variants' payload types of an enum or optional type, `None` for
/// any other type.
pub fn variants(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Option<Vec<Vec<Ty>>> {
    let t = strip(pool, env, t);
    match pool.get(t) {
        // `None` is variant 0 and `Some` variant 1, as the tag says.
        TyData::Option(inner) => Some(vec![vec![], vec![inner]]),
        TyData::Adt { def, args } => env.enum_variants(def, args),
        _ => None,
    }
}

/// The width, signedness and float kind of a primitive type, as
/// arithmetic sees it: `bool` is an unsigned byte, `char` and `usize`
/// unsigned 32-bit words.
pub fn num(pool: &InternPool, t: Ty) -> (u8, bool, bool) {
    match pool.get(t) {
        TyData::Prim(p) => match p {
            Prim::I8 => (8, true, false),
            Prim::U8 | Prim::Bool => (8, false, false),
            Prim::I16 => (16, true, false),
            Prim::U16 => (16, false, false),
            Prim::I32 | Prim::String => (32, true, false),
            Prim::U32 | Prim::Usize | Prim::Char => (32, false, false),
            Prim::I64 => (64, true, false),
            Prim::U64 => (64, false, false),
            Prim::F32 => (32, true, true),
            Prim::F64 => (64, true, true),
        },
        TyData::Mut(inner) => num(pool, inner),
        _ => (32, false, false),
    }
}
