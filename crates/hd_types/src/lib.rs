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
pub use sealed::CopyDecl;

/// Measurement switch (branch measure/move-only-generics): a plain type
/// parameter and a trait's `Self` are move-only; `< Copy` makes them
/// copyable, and `List` and `Map` hold any item, copying as their items
/// do. `HD_COPY_DEFAULT=1` restores the `Copy` default.
#[must_use]
pub fn move_only_generics() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("HD_COPY_DEFAULT").is_none())
}
pub use unify::{InferTable, UnifyError, VarKind};
