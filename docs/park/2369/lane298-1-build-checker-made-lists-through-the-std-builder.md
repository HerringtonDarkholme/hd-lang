# Build every checker-made list through the std builder, not `NewList`

Rule: #2369 step 1 (SWITCH.md §5 group 1): once `known.list` names the
std struct, no stage may emit `Tag::NewList` for it.

Today: the checker and the structure generator build lists with
`Tag::NewList` outside list literals: `literals.rs` (template string
parts and values), `call.rs` (rest arguments, `rest_of`), `expr.rs`
(tuple rest values, `convert_spread`'s accumulator, `collection_loop`),
`facts.rs` (2), `newtype.rs` (`ListConversion`), and
`hd_structure/src/lib.rs` (5: choices, members, keys) and `dbg.rs` (1).
`built_literal` already builds a literal node through
`FromListLiteral::{builder, push, build}`.

Where: `crates/hd_check/src/expr.rs` (`built_literal`, `literal_builder`),
the sites above, `crates/hd_structure/src/lib.rs`.

Fix: one checker helper, `build_list(values: &[Ref], elem: Ty, at)`,
that emits the builder calls over evaluated values (the literal path
calls it after evaluating each item), and its TIR twin in
`hd_structure` (calls by `DefId` of the three trait methods). Delete
`Tag::NewList` from the checker in the switch commit.

Done: `rg "Tag::NewList" crates/hd_check crates/hd_structure` is empty
on the switch commit; `lang/expressions`, `lang/data`, `std/collections`
conformance pass in three modes; WAT identity on the bench programs.
