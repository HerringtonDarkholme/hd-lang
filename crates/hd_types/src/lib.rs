#![forbid(unsafe_code)]
//! `hd_types`: the type interner (`InternPool`), rows, unification, poison,
//! the trait solver core with its memo, and the entry-local tables that
//! carry types and paths into cache entries (`docs/compiler/design-overview.md` §2.1,
//! `docs/compiler/data-structures.md` §3.4, §3.6, §3.9.2, §3.20.2; `crates/hd_check/DESIGN.md` (type checking) §3;
//! `crates/hd_check/DESIGN.md` (trait solver)).

pub mod lookup;
pub mod pool;
pub mod sealed;
pub mod solver;
pub mod unify;
pub mod wire;

pub use pool::{
    ConstValue, FnDeps, Inputs, InternPool, LocalPool, ParamRef, Prim, RefCells, RowData, RowId,
    RowParamRef, Ty, TyData, TyList, Types, with_assoc_args,
};
pub use unify::{InferTable, UnifyError, VarKind};
