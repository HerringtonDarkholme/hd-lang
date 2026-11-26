# #2369 step 3: the switch, planned (lane 294, 2026-11-25)

Base: `lane/counted-3` at lane 294's tip. Census: lane 285's
(`notes/lane285/CENSUS.md`), refreshed against the branch with the same
marker set (`known.list|map`, `StdKind::List|Map`, `IntrinsicOp::List*|Map*`,
`Tag::NewList|NewMap`, `Proj::Elem`, `Tag::IsUnique`, `Indexed::List|Map`,
`Rt::List*|Map*`, `LList|LMap`).

Compiler src today: main 360,553 lines, branch 361,987 (+1,434). Std:
`list.hd` 627, `map.hd` 128, `plain_list.hd` 329, `plain_map.hd` 332.

## 1. Sites and their fate

Fates: **D** delete; **S** route through std (trait or std body);
**K** keep as an intrinsic (only `ListPush` and the four `std.rt.counted`
intrinsics: `alloc_counted`, `tail_of`, `is_unique`, `make_unique`); **R** re-key (same code, names the std type's `DefId`).

| Crate | Markers (branch) | Sites | Fate | Lines out |
| --- | --- | --- | --- | --- |
| hd_resolve | 17 | `known.rs` `list_is_empty`, `map_key_hash`, `map_string_hash`, `map_integer_hash`, `LIST_MODULE`; `lower.rs` 8 (rest param must be `List`, inherent-impl permission, variance slots); `lower/decls.rs` 2; `header.rs` 1 | `known.list`/`known.map` **R** (literal default types and the spec-named rest type `List[T]...`). Inherent-impl permission, variance slots, `list_is_empty`, `map_*_hash` **D** (a std struct gets them from its declaration; integers hash through std's `hash_one`) | -60 |
| hd_check | 163 | `expr.rs` 36 (`Indexed::List/Map`, `NewList`/`NewMap` literal paths, `convert_spread`, `rest_tuple_value`, `collection_loop`'s list/map half, `map_loop_entry`), `call.rs` 19 (`spread_arg`, `rest_of`), `dep_results/constraints/exclusive.rs` 22 and `constraints.rs` 12 (element loans), `elem_refs.rs` (284 lines, `for p in &xs`), `coll.rs` (243), `refs.rs` `elem_index`, `access.rs` `element_of`, `body.rs` 11, `newtype.rs` (`ListConversion`), `fnref.rs`, `moves.rs`, `render.rs`, `pat.rs`, `facts.rs`, `join.rs`, `ty.rs`, `repair.rs`, `wire_crossing.rs` | Literals, index, spread, patterns **S** (the branch's builder, `Index`/`IndexMut`/`IndexSet` and `Iterable` routes are already the general path). Element loans **D** (owner: an element loan borrows the whole list): `elem_refs.rs`, the element half of `exclusive.rs`. `for p in &xs` **S** (`Iterable for &List`). Rest `List[T]...` **R**. `ListConversion` **S** (std `map`) | -1,500 |
| hd_types | 0 | | | 0 |
| hd_mono | 16 | `layout.rs` `StdKind::List/Map`, `inline_map_key`; `lib.rs` `MapKey{hash,eq}`, `MapInline` targets | **D** | -180 |
| hd_tir | 235 | `ir.rs` 25 (`IntrinsicOp` `ListLen ListReserve MapLen MapGet MapRemove MapSet MapSlots MapSlotNext MapSlotEntry MapSlotEntryMut MapEntry`, `NewList`, `NewMap`, `Proj::Elem`), `own/hoist.rs` 47, `ir/owned.rs` 19, `own.rs` 18, `own/derive.rs` 13, `own/short.rs` 12, `own/insert.rs` 11, `own/map_value.rs` (286), `own/map_key.rs` (316), `ir/root.rs`, `own/late.rs`, `own/frame.rs`, `own/retains.rs` | List/Map ops **D** but `ListPush` **K**. `NewList`/`NewMap` **D**. `Proj::Elem` list places **D** (strings keep `StrIndex`). `map_value.rs`, `map_key.rs` **D**. `IsUnique` stays only as `ListPush`'s own check, and `hoist.rs`'s candidate becomes the make-unique diamond of std's `is_unique` (issue 1 below), which replaces its collection-place half | -1,400 |
| hd_opt | 31 | `prove.rs` 20 (`Lists`: bounds proofs on `ListLen` + `Elem`, `root_at_headers`, about 1,100 of its 2,060 lines), `fact_specialize.rs` 6, `prove/loop_loads.rs` 3 | `Lists` **D** once `loop_loads` decides a write loop's guards (issue 1). `loop_loads`' `ListLen` arm **D** | -1,150 |
| hd_layout | 2 | `coll_kind`, `Mem::nonnull` | **D** (a struct over one `*H` is nonnull and counted by its field) | -40 |
| hd_wasm | 75 + list/map files | `emit/map.rs` (705), `map.rs` (1,353), `emit/list.rs` (492; `ListPush` lowering stays, about 150), `rt.rs` list helpers (`list_truncate`, `list_reserve`, `list_extend`, `reserve_room`, `drop_elems`, `copy_elems`; `list_buf`, `list_unique`, `list_copy`, `list_grow` stay for push), map helpers (`MapSeed`..`MapStep`, `DropEntries`), `emit.rs` 16, `emit/refs.rs` 12, `emit/borrow.rs` 4, `emit/iter.rs` (87), `cycle.rs` list arm, `boundary.rs` 13 (`LList`/`LMap`) | **D** all but push. `boundary.rs` **R**: the wire codec reads the std block, whose `ListHead` keeps the builtin offsets (`cap` 0, `len` 4, elements at payload + 16), so the host ABI and the JS host (`host/wire.ts` reads only the wire form) are unchanged. `checked-report`'s "list/map buffers/headers" kinds **R** | -3,000 |
| hd_structure | 22 | `lib.rs` 12 (generated `List` types for choices, members, rests), `dbg.rs` 8, `unique.rs` 2 | `lib.rs` **R**. `dbg.rs` **S** (std `Debug`). `unique.rs` **S** (one counted-block case) | -80 |
| hd_driver | 26 | `own.rs` 7 (`StdKind` facts, `coll_kind`), `own/test_hooks.rs` 14, `lib.rs` 2 | **D** | -150 |
| hd_lint | 0 markers; `map_store.rs` (480) | map store lint | **R** to `Map`'s `IndexMut` by `DefId` (owner call: keep or drop; drop is -480) | 0 |
| host ABI | 3 | `hd_host_abi/native_rust.rs`, `hd_run_wasmtime/native/wire.rs` | wire form unchanged | 0 |
| std | | `list.hd` 627 (inherent methods over intrinsics), `map.hd` 128 | **S**: `plain_list.hd` becomes `list.hd` with the old API ported over the block (52 public functions; 13 exist); `plain_map.hd` becomes `map.hd` (12; 9 exist) | std +250 net |

Deletion total, compiler: about **-7,560**; the branch's own additions
stay (they are the general builder, index-trait and counted-block paths).

## 2. End state

- Compiler src: 361,987 - 7,560 = about **354,400**, **-6,100 net over
  main** (the branch's +1,434 included).
- Std: about +250 net (two ported files replace `list.hd`/`map.hd`'s
  intrinsic bodies).
- Builtin List/Map lowerings left: one, `ListPush`, plus the three
  `std.rt.counted` intrinsics.

## 3. Commit order (each green, each deleting what it replaces)

| # | Step | Deletes in the same commit | Net |
| --- | --- | --- | --- |
| 0a | **Done (lane 296, 3bc45f396).** `std.rt.make_unique(&mut list)`, a fourth counted intrinsic (owner, 2026-11-25: Swift's `array.make_mutable`), lowers to the builtin's `IsUnique` of the list place; its copy is `rt:list_unique` with the block head's glue, the one `ListPush` on `PlainList` uses. Hoist extensions: inlined-`self` aliases of a parameter, pointer-derived stores below their place, a panic writes nothing | none | +181 |
| 0b | **Done (lane 296).** `loop_loads`: a store or `Ptr.write` through a `tail_of` pointer writes no header; loads below a hoisted check's `[*$p]` read its value; the check keeps a block list's length, so `i < n` decides the bounds test | none | +113 |
| 0a' | **Done (lane 297, 01e214374).** Local-list form: `own::local_refs` forwards `$self = &$xs` for every local type (reference propagation), `make_unique` lowers first so its `IsUnique` is forwarded too; the value passes read that check as a change. A local list's write loop has no test, load or make-unique, as the builtin's. Left: forward parameter-reference copies there and delete `hoist::aliases` (lane 297 issue 1) | none | +18 |
| 0b' | **Done (lane 297, 77e44cde5).** A check that keeps a load's value (the length) reads as apart from it in `loop_loads`, so the length leaves the loop before the check, as the builtin's. Sieve's loop matches but for the panic call (lane 297 issue 2) | none | +4 |
| 0c | Lane 288 issue 5 (the known checked-build failure) | its KNOWN_FAILURES row | ~0 |
| 1 | **List switch**: `known.list` names the std struct (`plain_list.hd` renamed `list.hd`, API ported); `List.push` is `ListPush` | list `IntrinsicOp`s but push, `NewList`, list `Proj::Elem`, `Indexed::List`, `convert_spread`, `collection_loop` list half, `elem_refs.rs`, element-loan half of `exclusive.rs`, `prove.rs` `Lists`, `emit/list.rs` but push, `rt.rs` list helpers but push's, `emit/iter.rs` list arm, `StdKind::List`, `coll_kind` list, driver list facts, resolve list permissions; spec: element loans borrow the whole list, two-element-loan conformance cases become rejections | about -3,900 |
| 2 | **Map switch**: `known.map` names the std struct | `map.rs`, `emit/map.rs`, map `IntrinsicOp`s, `NewMap`, `Indexed::Map`, `map_value.rs`, `map_key.rs`, `MapKey`/`MapInline` in mono, `map_*_hash` knowns, `collection_loop` (whole), lint retarget | about -3,500 |
| 3 | Sweep: `IsUnique` only where `ListPush` needs it, `Proj::Elem` remains (strings), docs (`tir-v2.md`, `hd_wasm/DESIGN.md`, lowering catalog) | the dead remains | about -350 |

The branch lands only after step 1 at the earliest (already net negative
there: +1,434 +230 -3,900 = about -2,200). Steps 0a-0c can land on the branch
first; they are the price of keeping step 1's WAT identity.

## 4. Risks

1. **Generated code must not regress.** Check: WAT identity on the bench
   programs (`bench/runtime/progs`, `bench/study`, `bench/micro`) before and
   after each switch step, plus both count scripts. The known gap today:
   a std list's **write loop** keeps its bounds and uniqueness tests (issue
   1); `sieve`, `fannkuch_redux`, `spectral_norm`, `list-sort`,
   `route-search` and `json-decode` have indexed writes in loops, so step 1
   without 0a/0b regresses them. A builtin-list spread calls
   `ListIter.next` per item (lane 293 issue 2): gone at step 1, where the
   std cursor (`@inline(.Always)`) replaces it. The builtin `for` over a
   list is a counted loop; the std `for` goes through `PlainListItems`,
   whose loop is at parity today (lane 293's `for` stage case).
2. **Default literal type.** `[a, b]` with no expectation stays `List[T]`
   through `known.list` (now the std struct): the builder path picks it as
   the default, as Swift's `Array` for `ExpressibleByArrayLiteral`. The
   static literal path (`static-scalar-list-literals.wat`: a constant
   literal is a data segment, no allocation) has no builder equivalent
   yet; step 1 must keep it (a constant literal's block as static data with
   count 0, as `empty_block` is) or show the cost.
3. **Map ordering.** The builtin map iterates in insertion order with
   removed entries skipped (tombstones). `plain_map.hd` keeps the entry
   array in write order with `GONE` hashes, the same order; its iteration,
   drain and the snapshot rule (`flow.for.snapshot`) need the port's
   conformance run (`std/collections`, `lang/control-flow`).
4. **Host knowledge of layout.** `hd_wasm/boundary.rs` reads list and map
   blocks in memory for the wire codec; the JS host (`host/wire.ts`) and
   the Rust native hosts read only the wire form. `ListHead` keeps the
   builtin's word offsets, and `MapHead` must keep `map.rs`'s table words
   (entries, then the group index), so the codec only re-keys its type
   test. `checked-report.ts` names allocation kinds "list/map buffers" and
   "list/map headers": the std blocks must keep reporting under those kinds.
5. **Element loans.** Programs holding two element loans of one list at
   once (`elem_refs.rs` today) become errors (owner decision). The
   conformance cases change from accept to reject in step 1; the guide and
   site examples that do it need a rewrite.
6. **Growth until step 1.** The branch is +1,434 over main; 0a/0b add about
   +230. It must not merge before step 1.

## 5. Lane 298 (2026-11-26)

Landed on `lane/counted-3`:

- **Element loans borrow the whole list** (owner decision). The
  exclusivity check reads `&xs[i]` as a `&` of `xs` and `&mut xs[i]` as a
  `&mut` of it (the path up to its first element step), so `ref.excl`
  covers it and the element-loan half of `exclusive.rs` is gone (the
  `Elem` loan kind, its argument/conflict/message paths, the per-key map
  store exemption, the frozen-element case, `source_path`). A reference
  read through a local reference held in an element keeps that
  reference's source (Rust's loan stops at a `&` dereference). Spec:
  `ref.elem.access.*`, `flow.for.ref.conflict`; four accept cases became
  rejections. Maps follow the same rule (one rule for both). This is the
  semantics the switch's `Index`/`IndexMut` path gives, so the switch now
  changes no program's acceptance.
- **Lane 297 issue 2, decided:** the std message stays (the better
  diagnostic) in one never-returning cold function per module,
  `out_of_bounds()`, so a bounds-test site is `call $out_of_bounds` (2
  bytes), the builtin's `call $rt:panic` size, not 5 constants and a
  `call_indirect` (15 bytes). Price without it, at the switch: up to 912
  bench bounds sites x 13 bytes = 11.9 KB of 975 KB over the 93 bench
  programs (1.2%; json 2.8 KB of 80 KB, 3.6%), over the 0.5% size gate.
  With it: one function (about 25 bytes) per module. `loop_loads` reads a
  call typed `never` as a panic, so the guard stays decided.

### What is left of step 1 (census by reading, lane 298)

The switch is one atomic commit in the checker: once `known.list` names
the std struct, every builtin-list path keyed by `known.list` misfires on
it, so they go together. By group:

1. **List construction inside the checker and generated code**
   (`Tag::NewList`): `literals.rs` (template parts), `call.rs` (rest
   arguments), `expr.rs` (literal, tuple rest value, `convert_spread`,
   `collection_loop` accumulator), `facts.rs` (2), `newtype.rs`
   (`ListConversion`), `hd_structure` (`lib.rs` 5: choices, members,
   keys; `dbg.rs` 1). Each needs one helper that builds a std list from
   evaluated values through `FromListLiteral` (`builder(n)`, `push`,
   `build`), as `built_literal` does for a literal node; `hd_structure`
   needs the same as TIR calls.
2. **Static literal path** (risk 2): a constant scalar literal is a data
   segment today. Through the builder it is an allocation and N stores.
   Keep: a constant literal of `Copy` scalars lowers to a static block
   with count 0 (as `empty_block`), which the builder's first push copies
   on its cold path; or price it on `static-scalar-list-literals.wat` and
   the bench programs.
3. **`for p in &xs` / `&mut xs`** (`elem_refs.rs`): with whole-list
   borrows (landed), the loop is exactly `for i in 0..xs.len(): p :=
   &mut xs[i]` over `IndexMut`; one desugaring for any type with `len()`
   and `Index[usize]`/`IndexMut[usize]` replaces the list half.
4. **Indexing, `len`, patterns**: `index_kind`'s `Indexed::List`
   (`expr.rs`, `refs.rs`, `body.rs`), `builtin_method` (`len` goes, push
   stays), `pat.rs` and rest tuples (`types.tuple.rest`: the rest member
   is a `List[T]` built from values).
5. **std duplicates**: `ops.hd`'s builtin `Index`/`IndexMut`/`IndexSet`
   for `List`, `iter.hd`'s `Iterable for List` (`ListIter`) and
   `PlainList`'s own; `view.hd`, `list.hd` use `list_reserve`,
   `list_truncate`, `list_extend` (become block methods).
6. **Types**: `seed.rs` builtin `List` item, `hd_types` `solver.rs`
   `item("List")`, `sealed.rs` list/map, `join.rs` variance (`[1]` stays
   as the struct's declared variance), `header.rs`, `ty.rs`, `fnref.rs`.
7. **Lower layers** (dead once the checker emits no list op): `StdKind::
   List`, `coll_kind`, `prove.rs` `Lists`, `emit/list.rs` but push,
   `rt.rs` list helpers but push's, `emit/iter.rs` list arm, driver list
   facts.
8. **Runtime-made lists** (`string.split`, `chars`, args, fs reads):
   `rt` builds buffers with the builtin layout (cap 0, len 4, elements
   at 16), which `ListHead` keeps; their descriptors must be the
   `ListHead` glue's (lane 288's `list_buf` descriptor sharing) before
   the std drop path frees them.
