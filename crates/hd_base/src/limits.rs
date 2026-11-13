//! The compiler's limits on input-shaped work (`crates/hd_tir/DESIGN.md`
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

/// Associated-type bindings one normalization follows, one after another
/// (`trait.assoc.projection.depth`): normalizing `T::Out` through `impl
/// Walk for A: type Out = U::Out` takes one, and a chain that needs more
/// than this many is `trait-resolution-depth`, as a bound proof past its
/// depth is (`trait.bound.depth.limit`). The checker's normalizers and
/// `hd_types::solver::normalize_concrete` stop at it; rustc's
/// `recursion_limit` (E0275) plays the same part.
pub const PROJECTION_DEPTH: u32 = 64;

/// The work of one match's coverage and reachability checks
/// (`flow.match.limit`): each state of the usefulness work stack
/// (`hd_check` `Ck::useful`) costs one plus the matrix rows it holds, as
/// rustc's pattern complexity counts. Usefulness is exponential in the
/// worst case (a match over a tuple of `bool`s can encode a SAT
/// formula), and a match past this is `match-too-complex`, reported on
/// the match. The work also charges the body's fuel. Flat matches cost a
/// few rows per arm; the largest of std's 560 matches costs 239.
pub const MATCH_WORK: u32 = 1 << 22;

/// Bytes in one source file (`lex.limit.file-size`). `hd_syntax::lex`
/// reports a larger file as `file-too-large` and lexes none of it, so
/// every offset, span and token index a stage keeps fits its `u32`, with
/// room to spare for the browser build's memory. The largest file in std
/// is far below it.
pub const FILE_SIZE: u32 = 16 << 20;
