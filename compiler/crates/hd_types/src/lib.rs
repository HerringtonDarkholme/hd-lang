#![forbid(unsafe_code)]
//! `hd_types`: the type interner (`InternPool`), rows, unification, poison
//! and the trait solver core with its memo (design-overview.md §2.1,
//! data-structures.md §3.4, §3.6, §3.9.2; type-checking.md §3;
//! trait-solver.md).

pub mod pool;
pub mod solver;
pub mod unify;

pub use pool::{InternPool, ParamRef, Prim, RowData, RowId, RowParamRef, Ty, TyData, TyList};
pub use unify::{InferTable, UnifyError, VarKind};
