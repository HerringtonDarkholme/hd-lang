#![forbid(unsafe_code)]
//! `hd_types`: the type interner (`InternPool`), rows, unification, poison,
//! the trait solver core with its memo, and the entry-local tables that
//! carry types and paths into cache entries (design-overview.md §2.1,
//! data-structures.md §3.4, §3.6, §3.9.2, §3.20.2; type-checking.md §3;
//! trait-solver.md).

pub mod pool;
pub mod solver;
pub mod unify;
pub mod wire;

pub use pool::{
    InternPool, LocalPool, ParamRef, Prim, RowData, RowId, RowParamRef, Ty, TyData, TyList, Types,
};
pub use unify::{InferTable, UnifyError, VarKind};
