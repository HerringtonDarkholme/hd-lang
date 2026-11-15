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

/// Origin slots one type holds (`types.limit.references`): each local
/// reference, function type and suspension in it, outside function
/// types, a part counted at each of its uses
/// (`hd_check::dep_results::SlotCounts`). The dependent-result checks
/// give each slot its own origin set (`ref.dep.position`), so a type's
/// layout is as long as its count, and a type that doubles a tuple of
/// references at each level (`x1 := (x0, x0)`, ...) doubles it: 20
/// levels laid out a million slots and did not check in a minute. A
/// slot is a position the language keeps apart, so no shared encoding
/// is cheaper (#1887). A body value or a written type past it is
/// `type-too-large` (`types.limit.references.error`). rustc's
/// `type_length_limit` bounds a type's expanded length the same way.
/// Owner decision 2026-11-14.
pub const REFERENCES: u32 = 1_024;

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

/// Generic parameters one declaration has in scope
/// (`types.limit.generic-params`): its own list's and those it takes
/// from around it, a local item's enclosing lists and a trait's `Self`
/// among them. Lifetime parameters are counted apart from type, const
/// and row parameters, and each count has this limit. `hd_resolve`
/// reports the first parameter past it as `too-many-generic-parameters`.
/// A parameter past the limit is left out of the declaration (a type,
/// const or row parameter's name binds the poison type), so every
/// parameter number, a `u16` from `hd_resolve` on, stays far below
/// `u16::MAX`: without the limit the 65,535th lifetime and every later
/// one silently shared a number, and a trait's 65,535th type parameter
/// overflowed its index. Each use of a parameter looks it up by name
/// among those in scope, so the limit also bounds that work. C++ asks an
/// implementation for at least 1,024 template parameters (\[implimits\]).
pub const GENERIC_PARAMS: u32 = 1_024;
