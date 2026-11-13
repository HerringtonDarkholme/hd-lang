//! The checker's limits on input-shaped work (`crates/hd_tir/DESIGN.md`
//! §4.15): each counts language-level units and reports its diagnostic
//! past its value, so a program passes or fails the same way on every
//! run. A walk bounded by one of these stops at it only on input the
//! checker has already rejected.

/// Type constructors on the longest path from a type down to a leaf
/// (`hd_types::Types::depth`, kept in each type's meta word). A body
/// whose value has a deeper type is `type-too-large`. Every recursive
/// walk over a type's structure in `hd_check` and `hd_types` stops at
/// this depth. A written type nests at most as deep as the parser's
/// nesting limit (160) allows; only inference grows one past it, as
/// `x1 := [x0]`, `x2 := [x1]`, ... does.
pub const TYPE_DEPTH: u32 = 256;
