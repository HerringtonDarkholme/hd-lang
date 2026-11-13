//! The checker's limits on input-shaped work (`crates/hd_tir/DESIGN.md`
//! §4.15): each counts language-level units and reports its diagnostic
//! past its value, so a program passes or fails the same way on every
//! run. A walk bounded by one of these stops at it only on input the
//! checker has already rejected.

/// Type constructors on the longest path from a type down to a leaf
/// (`hd_types::Types::depth`, kept in each type's meta word). A body
/// value or a written type that nests deeper is `type-too-large`
/// (`types.limit.type-size.error`). Every recursive walk over a type's
/// structure in `hd_check` and `hd_types` stops at this depth, which
/// the browser build's engine stack holds: V8 overflowed between 1,500
/// and 1,750 levels of an inferred type. A written type reaches it
/// through aliases (`type A1 = List[A0]`, ...); inference grows one as
/// `x1 := [x0]`, `x2 := [x1]`, ... does. Owner decision 2026-11-13.
pub const TYPE_DEPTH: u32 = 1_000;
