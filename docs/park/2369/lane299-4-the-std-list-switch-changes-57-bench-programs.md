# The std List switch changes 57 of 93 bench programs (+87.7 KB, +9.0%)

Rule: #2369 evidence: identical generated code on the 93 bench programs
(`bench/runtime/progs`, `bench/study`, `bench/micro`) against 73270e86c.

Today (lane 299 WIP `lane299/switch-wip`, release WAT, `target/lane299/wat-4`
against `target/lane299/wat-base`): 36 identical, 57 differ, total
975,434 -> 1,063,169 bytes. Largest: json +29,758, tuple_ordering_shared
+16,739, regex-scan +13,834, route-search +1,741, log-stats +1,537,
structure_walk +1,533, nbody +1,442, fannkuch_redux +1,422, sieve +1,068,
spectral_norm +1,022. Smaller: list_get_i32 -325, list_get_string -251.

Causes, by the functions each program links that 73270e86c did not
(count of programs):

1. An empty literal `[]` (and a comprehension's accumulator) is
   `builder(0)` -> `empty_block()`, an out-of-line call (40 programs),
   where `NewList` with no items was the static empty buffer's address.
   The first push then cannot see a fresh block, so `rt:list_unique`
   and `rt:list_copy` are linked and called (18).
2. A list's block comes from std `List.alloc` (40), `grown` (13) and
   `reserve` (13) out of line, where the builtin used `rt:list_buf` and
   `rt:list_reserve`; a comprehension reserves through std `reserve`.
3. Element drop is std `ListHead.drop` glue (26) instead of
   `rt:drop_elems` (21 lose it), with `release_value`/`retain_known`
   linked beside it.
4. `out_of_bounds` (30): index reads and writes in loops whose guards
   `loop_loads` decided for `Proj::Elem` keep their test through
   `Index`/`IndexMut` (fannkuch_redux, sieve, spectral_norm).
5. Generated lists (`hd_structure`: Structure members, variants,
   shared pairs; `dbg!`) are builder calls per item, where `NewList`
   was one buffer of N stores (tuple_ordering_shared, json,
   structure_walk; json and regex-scan not yet read in detail).
6. `fact_specialize` and the driver's facts fold read `Tag::NewList`;
   a built facts list no longer folds (`Facts.items.is_empty()`).

Where: `std.rt.empty_block` (an `@inline(.Never)` std fn), the
ownership passes' freshness proof (333d0c61b covers calls that make a
block, not `empty_block`), `crates/hd_opt/src/prove/loop_loads.rs`,
`crates/hd_structure/src/lib.rs` `new_list`, `crates/hd_opt/src/fact_specialize.rs`.

Fix order: (1) the static empty block as a constant the passes know is
never unique and empty (its push is the cold path, as the builtin's);
(2)/(3) push's own grow/copy helpers for std blocks, as `ListPush`
already does; (4) `loop_loads` over the `Index` bodies' guard; (5) the
builder's room tests fold to N stores; (6) facts fold over a built list.

Done: WAT identity on all 93, or each remaining difference named and
smaller; both count scripts with no increase.
