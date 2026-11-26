# Keep a constant scalar list literal as static data through the builder

Rule: SWITCH.md risk 2; `static-scalar-list-literals.wat`.

Today: a literal of constant `Copy` scalars with the builtin list is one
data segment and no allocation. Through `FromListLiteral` it becomes
`alloc_counted` and N stores.

Fix: lower a built literal whose pushes are all constants of a type
with no drop glue to a static block with count 0 (as `empty_block`):
the header (`cap` = `len` = N) and the values in a data segment. A
change copies it on its cold path (count 0 is never unique), as the
empty block is today. Price: the bench programs' literal sites, before
and after, in bytes and allocations.

Where: the builder lowering (`crates/hd_check/src/expr.rs`
`built_literal`), `std.rt.empty_block`'s lowering in `hd_wasm`.

Done: `static-scalar-list-literals.wat` keeps no allocation on the std
list; bench sizes no larger.
