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
mod walk_memo;
pub mod wire;
pub mod work;

/// `n`, an index into or the length of one of this crate's tables (the
/// pool's columns and records, the solver's memo and impl rows, the
/// inference variables, the wire tables), as the `u32` word that indexes
/// them.
pub(crate) fn word(n: usize) -> u32 {
    // Every such table is indexed by `u32` ids (`Ty`, `TyList`, `RowId`,
    // `InferVar`, memo and wire rows), as rustc's `newtype_index!` ids
    // are, whose `Idx::new` asserts the same bound.
    // ruled out: no table is built past `u32::MAX` entries, since its ids
    // could not name the entries beyond.
    u32::try_from(n).expect("a type table indexed past u32::MAX")
}

/// The low 32 bits of `w`, as an encoding that splits a 64-bit value
/// into two words reads them.
pub(crate) fn low32(w: u64) -> u32 {
    let [a, b, c, d, ..] = w.to_le_bytes();
    u32::from_le_bytes([a, b, c, d])
}

/// The low 16 bits of `w`: a `u16` the pool stored in a word.
pub(crate) fn low16(w: u32) -> u16 {
    let [a, b, ..] = w.to_le_bytes();
    u16::from_le_bytes([a, b])
}

pub use pool::{
    ANON_SUM, AdtId, ConstValue, FnDeps, Inputs, InternPool, LocalPool, ParamRef, Prim, RowData,
    RowId, RowParamRef, Ty, TyData, TyList, TyMap, Types, with_assoc_args,
};
pub use sealed::CopyDecl;
pub use unify::{InferTable, UnifyError, VarKind, holds};

#[cfg(test)]
mod word_tests {
    #[test]
    fn the_low_halves_read_back_what_was_stored() {
        assert_eq!(super::low32(0x1234_5678_9abc_def0), 0x9abc_def0);
        assert_eq!(super::low32(u64::from(u32::MAX)), u32::MAX);
        assert_eq!(super::low16(u32::from(u16::MAX)), u16::MAX);
        assert_eq!(super::low16(0x0001_0002), 2);
        assert_eq!(super::word(7), 7);
    }
}
