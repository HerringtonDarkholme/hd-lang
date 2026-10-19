//! Linear layouts (`crates/hd_wasm/DESIGN.md` §23.3): a type's Wasm
//! values in locals, parameters and results.

use hd_base::StageResult;
use hd_types::{InternPool, Prim, Ty, TyData};

use crate::module::Val;
use crate::unsupported;

/// The Wasm values of a concrete type.
pub fn vals(pool: &InternPool, t: Ty) -> StageResult<Vec<Val>> {
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
            Prim::String => return unsupported("strings (job 7)"),
        },
        TyData::Never => vec![],
        TyData::Tuple { elems, rest: None } if pool.list_items(elems).is_empty() => vec![],
        TyData::Mut(inner) => return vals(pool, inner),
        TyData::Adt { .. } | TyData::Tuple { .. } | TyData::Option(_) => {
            return unsupported("data, enum, tuple and option values (job 6)");
        }
        TyData::Fn { .. } => return unsupported("function values (job 11)"),
        TyData::TraitValue { .. } => return unsupported("`dyn` values (job 11)"),
        TyData::Ref { .. } => return unsupported("references (job 12)"),
        _ => return unsupported(format!("values of type `{}`", pool.display(t))),
    })
}
