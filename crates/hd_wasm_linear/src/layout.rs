//! Linear layouts (`crates/hd_wasm/DESIGN.md` §23.3): a type's Wasm
//! values in locals, parameters and results.
//!
//! Job 3 lays out the values its programs pass: scalars, a string as its
//! address and byte length, an enum or an optional as its tag followed by
//! every variant's payload values in declaration order, a trait value as
//! its data address and its vtable's address, and a suspension as its
//! frame's address. Data, tuples, lists, maps and function values come
//! with later jobs.

use hd_base::StageResult;
use hd_mono::ProgramEnv;
use hd_mono::layout::{StdKind, strip_newtype};
use hd_types::{InternPool, Prim, Ty, TyData};

use crate::module::Val;
use crate::unsupported;

/// The Wasm values of a concrete type.
pub fn vals(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> StageResult<Vec<Val>> {
    let t = strip_newtype(pool, env, t);
    Ok(match pool.get(t) {
        TyData::Prim(p) => match p {
            Prim::I8
            | Prim::I16
            | Prim::I32
            | Prim::U8
            | Prim::U16
            | Prim::U32
            | Prim::Usize
            | Prim::Bool
            | Prim::Char => vec![Val::I32],
            Prim::I64 | Prim::U64 => vec![Val::I64],
            Prim::F32 => vec![Val::F32],
            Prim::F64 => vec![Val::F64],
            Prim::String => vec![Val::I32, Val::I32],
        },
        TyData::Never => vec![],
        TyData::Tuple { elems, rest: None } if pool.list_items(elems).is_empty() => vec![],
        TyData::Mut(inner) => return vals(pool, env, inner),
        TyData::Option(inner) => [vec![Val::I32], vals(pool, env, inner)?].concat(),
        TyData::Adt { def, args } => match env.std_kind(def) {
            StdKind::Suspend => vec![Val::I32],
            StdKind::List => return unsupported("lists (job 9)"),
            StdKind::Map => return unsupported("maps (job 10)"),
            StdKind::Other => {
                let Some(variants) = env.enum_variants(def, args) else {
                    return unsupported("data values (job 6)");
                };
                if env.recursive_enum(def) {
                    return unsupported("recursive enums (job 6)");
                }
                let mut out = vec![Val::I32];
                for payload in variants.iter().flatten() {
                    out.extend(vals(pool, env, *payload)?);
                }
                out
            }
        },
        TyData::Tuple { .. } => return unsupported("tuples (job 6)"),
        TyData::Fn { .. } => return unsupported("function values (job 11)"),
        TyData::TraitValue { .. } => vec![Val::I32, Val::I32],
        TyData::Ref { .. } => return unsupported("references (job 12)"),
        _ => return unsupported(format!("values of type `{}`", pool.display(t))),
    })
}

/// Where field `field` of variant `variant` of the enum or optional type
/// `t` lies among `t`'s values: its first value's index and its count.
pub fn payload_at(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    t: Ty,
    variant: u32,
    field: u32,
) -> StageResult<(usize, usize)> {
    let t = strip_newtype(pool, env, t);
    let variants = match pool.get(t) {
        TyData::Mut(inner) => return payload_at(pool, env, inner, variant, field),
        // `None` is variant 0 and `Some` variant 1, as the tag says.
        TyData::Option(inner) => vec![vec![], vec![inner]],
        TyData::Adt { def, args } => match env.enum_variants(def, args) {
            Some(v) => v,
            None => return unsupported("a payload of a value that is not an enum"),
        },
        _ => return unsupported("a payload of a value that is not an enum"),
    };
    let mut at = 1;
    for (k, payload) in variants.iter().enumerate() {
        for (f, ty) in payload.iter().enumerate() {
            let n = vals(pool, env, *ty)?.len();
            if k == variant as usize && f == field as usize {
                return Ok((at, n));
            }
            at += n;
        }
    }
    unsupported("a payload field the enum does not have")
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
