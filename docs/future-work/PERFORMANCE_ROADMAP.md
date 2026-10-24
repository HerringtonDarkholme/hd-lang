# Performance Roadmap

hd is a statically typed language, so a program that runs slower than
the same program in JavaScript points at the compiler or the library,
not at the language. This roadmap finds where time and memory go today
and ranks the fixes, for the optimization umbrella #426. It changes no
code and measured nothing new: every number comes from a record it
cites.

Speed comes after correctness, ergonomics and usability, and runtime
speed is not a gate today. These rules shape the plan:

- An API that forces a cost no implementation can remove is a design
  bug, not a tuning task.
- Strings follow Go: immutable UTF-8 bytes, text built with
  `StringBuilder` or `join`, and `+` copies.
- Intrinsics stay low-level, one machine operation each; algorithms are
  std hd code. Where hd is slow, the fix is the missing low-level
  primitive (#514), not moving the algorithm into the compiler.
- A speed fix never grows hello.wasm. It may grow programs that use the
  feature when it gains at least 5% speed for each 10% of size.
- Allocation removal (escape analysis, scalar replacement, bounded
  inlining) runs in the optimized pipeline; constant hoisting and trivial
  inlining run in both. The allocations metric reports both pipelines.
- Float text and Unicode case mapping stay host primitives until a
  measurement says otherwise.

Contents:

- [1. Summary](#1-summary)
- [2. Where Time And Memory Go Today](#2-where-time-and-memory-go-today)
- [3. Candidate Fixes By Mechanism](#3-candidate-fixes-by-mechanism)
- [4. Ranked Steps](#4-ranked-steps)
- [5. What Stays Parked](#5-what-stays-parked)

## 1. Summary

The standard library review of 2026-10-17 found that hd is not slow
because of one bad algorithm. Time went to four habits: float text on
BigInt in the host, per-byte abstractions that allocate or call through a
closure, intermediate structures (a JSON tree, a copied regex haystack),
and setup repeated per call. Eight fixes landed the same day and removed
most of the first and third habits: native float text, number parsers on
bytes, JSON encode straight to text, JSON parse on the input's bytes,
the regex VM on bytes with reused state, a regex literal prefilter,
Two-Way substring search, and a keyed map hash. On the metric's
document, JSON encode went from 26 MB/s to 47 MB/s and decode from
22 MB/s to 44 to 46 MB/s; regex `find_all` went from 10 MB/s to 36 MB/s.

What is left falls into five groups:

1. **Allocation per element.** No compiler pass removes an allocation
   today. Inlining, escape analysis and scalar replacement are declared
   in the optimized pipeline but report `Skipped`. The `allocations`
   metric passes only because its loops carry `i64` values. A record
   returned from a helper, a constant enum value that needs a box, a
   closure made per element, derived metadata built on every decode, and a time value
   each allocate.
2. **String building per piece.** Integer text prepends one digit string
   at a time. Each string literal use calls a getter. A small piece costs
   about 8 ns to push and join. Text building is 40% integer text by
   profile.
3. **Arithmetic written as loops.** `wrapping_add`, `count_ones` and
   their kin loop over bits. This makes SHA-256 slow, and it holds back
   the fastest keyed string hash, so the flood-safe map pays 25% to 35%
   on short-key lookups.
4. **Setup per call.** Derived decoding builds member metadata on every
   call, the property runner builds a host and an instance per case, and
   each host file call resolves its path twice.
5. **Indirect calls in iterator chains,** mostly on wasmtime, where a
   closure call costs a libcall that V8 inlines away.

Memory is in better shape. Peak heap is below Node's on both measured
programs. The open memory items are shapes: a `Set` stores a `bool` per
entry, `Deque` and `Heap` store `T?`, and time values are heap objects.

The plan in one line: measure again on today's code, fix the cheap std
and lowering items that each remove an allocation or a call per element,
then build the compiler passes that remove allocation per element in
user code. SIMD and the string layout stay parked until the scans go
through the approved primitives.

## 2. Where Time And Memory Go Today

### 2.1 The record

| Source | What it measured | Caveat |
| --- | --- | --- |
| Standard library implementation review, `git show 2816e1f48:docs/audit/stdlib-impl-review-2026-10-17.md` | 40 micro-benchmarks against Node, with V8 CPU profiles, at `caa69d93a` | V8 only; scripts in `target/stdlib-b/`, not committed |
| Commit messages of the fixes | before and after on each fixed case: `79abfba8b` (#352), `a66c3c1c8` (#356), `813bb1f15` (#358), `b9e0256df` (#359), `7c95d1eb7` (#360), `7cfa0bc1c` (#354), `b2b264f22` (#355), `8d8240a8f` and `2a274b8ad` (#353, #511) | each measures its own case, not the metric suite |
| `0b1429b47` (#317) | `StringBuilder` and string equality after `concat_all` | Node startup inside each figure |
| #426 comments | the redesign's unmeasured findings; keyed hash costs | |
| #376, #377, #357 | piece push cost, derived metadata, integer text | |
| [SIMD_PRIMITIVES.md section 10](SIMD_PRIMITIVES.md#10-probe-results-2026-10-17) | scans and copies over `(array i8)` and `(array v128)` on both engines | hand-written kernels, not hd output |
| [crates/hd_opt/DESIGN.md](../../crates/hd_opt/DESIGN.md) sections 7 and 8, #65 | closure call cost per engine | #65's layer cost predates closure specialization |

### 2.2 The failing runtime rows, then and now

"Recorded" is the metrics run. "Since" is the latest figure on record
for the same row or its closest micro-benchmark.

| Row | Recorded | Goal | Since | Where the time goes |
| --- | --- | --- | --- | --- |
| JSON encode | 26 MB/s, 0.05x Node | 0.5x | 47 MB/s (`b9e0256df`, whose before figure, 35 MB/s, already had the native float printer of `a66c3c1c8`) | before: floats 39%, the `Json` tree 29% (both fixed). Now: pushing pieces and `concat_all` is about 12 ms of 24 ms on the no-float case, the walk 5 ms (#376); integers go through the per-digit path (#357) |
| JSON decode | 22 MB/s, 0.08x | 0.5x | 44 to 46 MB/s, from 26 MB/s at that commit's start (`7c95d1eb7`) | `from_json` gained only 1.4x: the derived `build` allocates a `Member`, its `Facts`, a `Members`, a closure, an `Option` and a `Key` per field per call, and a `List[VariantInfo]` per value (#377) |
| text build | 54 MB/s, 0.19x | 0.5x | not re-measured; 0.36x on equal work in the review (#429) | integer text 40% (`digit_text` 22%, `unsigned_text` 18.5%), GC 17%, `StringBuilder.push` and `build` 13% |
| regex `find_all` | 9.8 MB/s, 0.05x | 0.5x | 36 MB/s, 0.16x (`7cfa0bc1c`); the prefilter does not help this pattern (99 to 100 ms, `b2b264f22`) | the Pike VM steps every live thread per byte; no profile on record since the rewrite |
| runtime `sum` | 4.77x Node (36 ms against 7.5 ms; p10 25, p90 66) | 3x | not re-measured | not profiled. `allocations` shows 0 bytes per iteration on counted loops, so it is not allocation |
| runtime `string-build` | 3.18x | 3x | not re-measured | the program interpolates an integer per piece, which takes std's per-digit path unless the integer is a `usize` (review fix 3) |
| property cases | 28k/s | 100k/s | not re-measured | per case: host JavaScript rebuilding the import object 27.5%, `instantiate` 20%, GC 9%, the Wasm itself 24.5% |
| `read_text!`, `list_dir!` | 30 µs, 40 µs | 10 µs, 5 µs | not re-measured | `realpath` 27%, `readdir` 26%, `open` and `close` 21%; encoding the reply about 3%. Serde is not the cost; the floor with one `realpath` is about 20 µs |
| tiny program size | 2399 B | 2048 B | hello.wasm 4257 B (dev, the job gate) | literals go through the pool and its getter; the host fast path for a literal (`memory.init`) is not built (crates/hd_wasm_linear/DESIGN.md §15.4); no per-section breakdown is on record |
| Wasm per 1,000 lines | 17413 B | 10240 B | not re-measured | not broken down; `hd build --size-report` is planned, not built |
| start to first output | 40 ms | 5 ms | not re-measured | not profiled; §15.6 expects process start to dominate |
| `check-cost` | 1.77x (988 against 559 ms) | 1.3x | not re-measured | not profiled; the known costly check is `i64` multiply, which divides back (crates/hd_mono/DESIGN.md §12.5) |
| `long-run-memory` | no result | 10 MB | not re-measured | the harness sees no progress line, because `println` output does not reach the pipe before exit |

Rows that pass and bound the work: `fib` 0.24x, `map` 1.02x, `sort` 1.22x,
`split` 0.90x, the runtime geomean 1.35x, `println` 0.84 µs per call,
and await, `all!` and `race!` far inside their goals.

### 2.3 Micro numbers still open

From the review (V8, release, median of 3), for cases that no fix since
has touched:

| Case | hd | Node | Cause | Review fix |
| --- | ---: | ---: | --- | --- |
| 10⁶ `"${i}"` with `i: i64` | 91 ms | - | one digit string, one literal call and one copy per digit; the `usize` path through the runtime's `IntToStr` takes 27 ms | 3 (#357) |
| SHA-256 of 1 MiB | 128 ms | 0.4 ms (native) | `wrapping_add` as a carry loop, 53% of the time | 16 (#362) |
| `Heap`, 3 × 200,000 push and pop | 167 ms | - | `List[T?]`: an option match per read; `less` and `element` 67% | 17 |
| `sorted_by`, 3 × 200,000 `i64` | 132 ms | - | three new lists per merge level | 18 |
| sort 3 × 100,000 strings | 136 ms | 56.4 ms | as above | 18 |
| `to_utf8` and `from_utf8`, 5 × 1.09 MB | 56 ms | 1.4 ms | a closure call and a push per byte; an element copy instead of `array.copy` | 14 |
| base64 round trip, 3 × 1 MiB | 96 ms | 0.8 ms (native) | a push per byte, symbols by comparison chains, output revalidated | 20 |
| map with `(i32, i32)` keys | 190 ns per op | - | a `List[u8]` per integer through `dyn Hasher`; an `i64` key takes 22 ns | 21 (#434) |
| derived `Eq` / hand-written, 10⁶ | 16 / 5 ms | - | field-handle walkers | 22 |
| derived `Ord` sort / hand comparator | 55 / 27 ms | - | a variant probe walk before each compare | 22 |
| `debug` of a record, 100,000 | 78 ms | 50.5 ms | one string per character in `escape_text`, 40% | 19 |
| iterate `chars()`, 5 × 1.09 MB | 40 ms | 15.3 ms | a closure call per item | iterator chains |

Later records:

- **Keyed map hash** (#353, #426 comment). On 1M short keys, insert and
  lookup went from 49 / 41 ms (unkeyed FNV-1a) to 63 / 52-57 ms
  (HalfSipHash-1-3). SipHash-1-3 with a native `i64.add` hashes 400k
  short inputs in 3 ms, as fast as FNV, against 13 ms with adds built from
  32-bit halves. So the regression comes from the missing one-instruction
  wrapping add.
- **Closures.** The closures bench runs in 292 ms on V8 and 781 ms on
  wasmtime in release, with closure specialization on
  (crates/hd_opt/DESIGN.md §8). Before specialization, #65 measured about
  29 ns per lazy adapter layer per item on wasmtime. That has not been
  re-measured.
- **Strings.** `StringBuilder` with 1M parts takes 92 ms against Node's
  67 ms, startup included (`0b1429b47`). Comparing two 800 KB strings 400
  times takes 226 ms, eight bytes per step.

### 2.4 What the SIMD probe settled

The probe (SIMD_PRIMITIVES.md section 10) answers where scans and copies
stand:

- A byte scan gains 22x on V8 and 41x on wasmtime at 1 MiB, but only with
  a `(array v128)` string layout. Native loops over today's `(array i8)`
  gain almost nothing on V8 and about 10% on wasmtime.
- `array.copy` on `(array i8)` already lowers to a native memmove on both
  engines, so concatenation and `concat_all` copy at memory speed. Their
  cost is per piece, not per byte.
- The wider layout makes byte reads 1.2x to 1.8x slower and short strings
  4 to 12 bytes larger. The owner kept `(array i8)`.

So scanning is not where the time goes now. Allocation, per-piece work
and calls per element are.

### 2.5 Memory

- **Peak heap.** The map program peaks at +10.4 MB against Node's
  +12.2 MB (0.85x), string building at +17.7 MB against +18.9 MB (0.93x).
- **Per-value shapes** (crates/hd_wasm_linear/DESIGN.md §15.2). A string is a
  12-byte view in a field or a list element, and allocates nothing when
  sliced. An `Option` or `Result` within the layout bound allocates
  nothing. A `data` value is one struct. An erased `i32` is an `i31ref`
  when it fits; an erased `i64`, float or string is always a box. A
  3-byte string's array is a 32-byte heap object (the probe).
- **Shapes that waste memory.** `Set[T]` wraps `Map[T, bool]`, a `bool`
  per entry. `Deque` and `Heap` store `List[T?]`, which doubles the
  arrays of a scalar `T`. `Duration`, `Timestamp` and `Instant` are
  one-field data over an `i64`, one heap object each, where a newtype
  would be free.
- **Retention.** A small slice keeps its whole backing array alive, as in
  Go. This is a known risk with no mitigation in the first release;
  `long-run-memory` would watch it once its harness reports.
- **Compiler memory** (`hd check` startup RSS 13.8 MB against 10 MB,
  250 MB for a cold `hd test` at 50k lines) is compiler-side, indexed
  by #49, and outside this roadmap.

### 2.6 Measurements that are missing

| Question | Probe that answers it |
| --- | --- |
| The metric table after the eight fixes | a full `pnpm run metrics -- --pillar 3` on HEAD at low load (step 1) |
| Allocations per element outside `i64` loops | new loops in `bench/metrics/scripts/allocations.ts`: a record returned and destructured, a 3-tuple returned, an `Option` of a record, a boxed constant enum value, a closure made per element, an `i64` through a `dyn` generic method, integer interpolation, `Duration` arithmetic (step 3) |
| Why `sum` is 4.77x | the release WAT of `bench/micro/sum.hd`, its time per iteration on V8 with and without Liftoff and on wasmtime, against a hand-written Wasm loop (step 4) |
| The GC share after the fixes | V8 CPU profiles of JSON encode and decode and text build, as the review took them (step 1) |
| wasmtime figures for text, JSON and regex | the review's cases run under `--engine wasmtime` (step 1) |
| The A1 cast cost on wasmtime | catalog says "unmeasured"; a list-read kernel with and without the cast |
| What fills the tiny program and the 17 KB per 1,000 lines | bytes per section and per function of a release build; the planned `hd build --size-report` would give it |
| What `check-cost` pays for | a profile of the checked and unchecked builds of its program |
| Iterator layer cost after closure specialization | the closures bench on wasmtime, as #65 asks (step 23) |

## 3. Candidate Fixes By Mechanism

Each fix lists its expected gain (the review's estimate unless it says
measured), its cost in code churn, compiler size and compile time, its
risk, and what it waits on. Compiler size matters because the web
compiler has a 2 MB gzip gate, and it stands at about 1.58 MB
(`2a274b8ad`). Compile time matters because the budget is per function
and the dev pipeline is what `hd run` and `hd test` use.

### 3.1 Allocation per element

**Today.** The optimized pipeline lists `inline_plan`, `escape` and
`scalar_replace`, and each reports `Skipped` (crates/hd_opt/DESIGN.md
§1.3). The planned `hoist-constants`, `inline-trivial` and `const-fold`
are not built either (docs/compiler/lowering-catalog.md, Optimization
Passes). So every `data` value built is a heap struct, even one returned
and destructured at once. A constant enum or tuple value that needs a
box (past the layout bound, or self-recursive) allocates on each
evaluation. A payloadless variant of an enum with the subtype box
allocates a base struct (§15.2, "Implementation differs"). Derived
decoding builds its metadata per call (#377), and a downcast compares
`TypeId` key strings (`lib/std/inspect.hd`, #426 comment).

| Fix | Gain | Cost | Risk | Waits on |
| --- | --- | --- | --- | --- |
| **Constant hoisting** (both pipelines): a box, enum or tuple whose payloads are constants becomes an immutable global; payloadless subtype variants become singletons | one allocation per evaluation of a constant value | small, in `hd_wasm` emission; one global per constant | low: identity-free values allow it (§15.2, "Representation equivalence") | nothing |
| **Derived metadata once per type** (#377) | `from_json` at least 1.5x (the issue's target) | `hd_structure` templates, `lib/std/structure.hd`, constant records in lowering | medium: every derive template | nothing; hoisting helps |
| **Trivial inlining** (both pipelines): callees of at most 8 TIR instructions, no loop, no suspension, no closure | a call per small callee; makes the passes below see through accessors | a pass in `hd_opt`; inlined TIR hashes join the code key; `hd.lines` must record inlined call sites | medium: panic sites and cache keys | nothing |
| **Escape analysis** (optimized): an escape bit per allocation site | none alone; it counts what scalar replacement can take | a linear pass, an estimated 1 to 5 ms per 10k lines (§2.3 of hd_opt) | low | nothing |
| **Scalar replacement** (optimized): non-escaping records, boxes, closures and cells become locals | an allocation each; #427's done check | the largest item here; code usually shrinks | high: mutable data, values saved in suspension frames, `defer`, and panics all bound what "does not escape" means | escape analysis; across calls, inlining |
| **Bounded inlining** (optimized): budgets of crates/hd_mono/DESIGN.md §12.8 | a call, and values that stop escaping once inlined | code size up to 2x or 2 KB per caller; more Cranelift time | medium | trivial inlining |
| **Time types as newtypes**: `Duration`, `Timestamp`, `Instant` | one allocation per time value; a newtype emits nothing | std only; the redesign says no API change | low, if a newtype can keep its constructor private as the field is today; check first | nothing |
| **Integer type ids**: compare one `i64` per type, as §15.2 designs it | a string compare per downcast | lowering of `TypeId::of`; `inspect.hd` | low | nothing |

The local `^` mode of [Parallelism And Locality](PARALLELISM_AND_LOCALITY.md)
gives a later option: a `^T` parameter cannot escape its callee, so
escape analysis gets facts across calls without inlining.

### 3.2 Boxing of numbers and options

**Today.** `Option` and `Result` within the bound of 4 Wasm values never
allocate. An `i32` crossing an erased position is an `i31ref` when it
fits (`730d7754d`); an `i64`, a float or a string is always boxed. A
tuple or enum over the bound is boxed in every position, and a string
counts 2 toward the bound. std adds boxes the layout does not need:
`Deque` and `Heap` store `T?`, and `Set` stores a `bool` per entry.

| Fix | Gain | Cost | Risk | Waits on |
| --- | --- | --- | --- | --- |
| **`Heap` on `List[T]`** with `pop` and a hole (review fix 17) | heap 167 to about 50 ms (3x) | std only | low | nothing |
| **`Deque` on plain slots**: the ring over `List[T]` with `pop` and truncation (the redesign's finding) | a tag test per access; half the arrays for a scalar `T` | std only | low; a removed slot keeps its value alive until overwritten, as a ring over `T?` does not | nothing |
| **`Set` without the `bool`**: `Map[T, void]` | one byte per entry; a values array less | needs `Array[void]` lowering | low | #180 or a smaller map change |
| **`i64` as `i31ref` when it fits**, as `i32` does | a box per small erased `i64` | two runtime helpers | low; no hot path on record needs it | nothing |
| **Known-vtable devirtualization**: a `dyn` call on a constant vtable global becomes direct | an indirect call; on wasmtime it opens inlining, `mut dyn Hasher` above all | a small pass; vtables are already globals (§15.4) | low | nothing |

### 3.3 String building and concatenation

**Today.** `join` and `StringBuilder.build` copy each byte once through
`concat_all` (`0b1429b47`). `StringBuilder` keeps a `List[string]` of
pieces and a `List[u8]` for characters. The costs left are per piece and
per digit: about 8 ns per small piece and 3 ns per byte (#376); integer
text that prepends a digit string per digit; a getter call per literal
use; one new string per nesting level of `Display`; and `+` chains in
loops in std (`cli.usage`, `Timestamp.format`, `to_rfc3339`,
`zero_padded`, `path.join`, regex `expand`, `DebugWriter.margin`).

| Fix | Gain | Cost | Risk | Waits on |
| --- | --- | --- | --- | --- |
| **`format_int`** (#357, owner-approved): integer `Display` through the runtime's `IntToStr` | `i64` text 91 to about 27 ms (the helper's measured speed); string build about 2x | one intrinsic body; the fixture and property test on `park/357-int-text` | low; the helper links only where an integer is shown, and hello must stay 4257 B (the parked hd version grew it by 537 B) | nothing |
| **A literal without a call per use** (#361) | 22% of integer parsing before #358; 5% to 20% of string-heavy code (estimate) | `hd_wasm` getter and link: fill the tables at init, or hoist getter calls out of loops (`cse-getters`) | medium: §15.4 keeps literal numbers out of code so an edit recompiles nothing else; either fix must keep that | nothing |
| **Cheaper piece push and `concat_all`** (#376) | at least 2x per piece (the issue's target); JSON encode follows | `hd_wasm` list push for two-array elements, the `flush` per push | low | nothing |
| **`Display.write` into a builder** (#436, approved API) | one string per nested value per `Display`; interpolation pushes in place | std, every `Display` impl's default, interpolation lowering | medium | `format_int`, so integers push without a string |
| **UTF-8 conversions in bulk** (review fix 14): `string_to_bytes` with one `array.copy`, `array.copy` in `string_from_list`, an ASCII fast loop in `from_utf8` | `to_utf8` plus `from_utf8` 56 to about 10 ms (5x) | one new primitive (question 1), one lowering change, std | low | question 1 |
| **Builders in std's `+` loops** (the redesign's findings, review fix 25) | O(n²) to O(n) in each | std only, about eight call sites | low | nothing |
| **`MemoryFs.append_text!` in place** | O(n²) to O(n) over many appends | std only | low | nothing |

### 3.4 List growth and bulk copies

**Today.** `push` grows to 2n + 4 with `array.copy`, which is right. The
costs are elsewhere: builders that know their size still double from
empty; `insert` and `remove_at` shift one element per step;
`slice_list` pushes per element; `sorted_by` allocates three lists per
level and copies one-element lists through `filter` at the leaves;
`chunks` and `windows` copy every element; `group_by` copies its map;
`most_common(n)` sorts every entry.

| Fix | Gain | Cost | Risk | Waits on |
| --- | --- | --- | --- | --- |
| **Stable sort with one buffer** and insertion sort for short runs (review fix 18) | `sorted_by` 133 to about 55 ms (2.4x) | std only | low; the conformance sort cases pin stability | nothing |
| **List copy and reserve primitives**: one `array.copy` for a slice, an insert or a removal; a private reserve | an element per step becomes one memmove | two primitives (question 1), std call sites | low | question 1 |
| **`group_by` returns its first map; `most_common` keeps a size-n heap** | a map copy; O(m log m) to O(m log n) | std only | low | `Heap` on `List[T]` |
| **`counts` with one probe** | a second hash and probe per item | a map upsert primitive, which needs owner approval (`c46604c56`) | low | question 1 |

### 3.5 Closure environments and iterator chains

**Today.** A closure is one struct per construction plus one cell per
mutable capture; a capture-free closure is a constant. In the optimized
pipeline, a closure made in the same body is called directly (A1), and a
callee of up to 64 instructions gets one copy per closure literal passed
to it (A2). What stays indirect is a closure read from a data field: each
iterator adapter stores its step closure in `Iterator.step`, so a chain
pays one indirect call per layer per item. V8 inlines a monomorphic
indirect call; wasmtime pays a libcall to read the function reference,
which is why the closures bench is 781 ms there against 292 ms on V8.

| Fix | Gain | Cost | Risk | Waits on |
| --- | --- | --- | --- | --- |
| **Re-measure #65** at HEAD on both engines | decides between the two fixes below | none | none | nothing |
| **Closure specialization by inlining** (crates/hd_mono/DESIGN.md §12.6): inline the adapter, scalar-replace the `Iterator`, call the known body, repeat | a chain becomes a loop with no allocation; about 0.3 ns per element on V8 against 1 to 5 ns (catalog estimate) | 50 to 150 bytes per stage, at most 512 bytes per chain site | medium | bounded inlining, scalar replacement, devirtualization |
| **Chain fusion** (#65 option B): a pattern pass over `fold`, `collect` and `for` on known adapter shapes | projected about 550 ms on wasmtime for the closures bench (1.4x V8) | a pass per shape with a proof of call order and `take` | medium | the re-measure |
| **A range's `iter()` as a counted loop** | a closure per range iterator | lowering, as `for` over a range already is | low | nothing |
| **A function table on wasmtime** (option E2): `call_indirect` with no libcall | the libcall per closure call on wasmtime | a table per module; a new lowering profile field | medium | a check of a newer wasmtime for a fast path first |

### 3.6 Map and hash

**Today.** `Map` is an insertion-ordered table with linear probing, the
right design. Since #353 and #511 every bucket hash is keyed by the
run's seed, which stopped hash flooding and cost 25% to 35% on short
string-key lookups. A composite key hashes through `Hash` impls that
build a `List[u8]` per integer and call through `dyn Hasher`.

| Fix | Gain | Cost | Risk | Waits on |
| --- | --- | --- | --- | --- |
| **Wrapping and bit-count methods as instructions** (review fix 16) | SHA-256 128 to about 45 ms (2.8x); `checked_*` and `saturating_*` lose a 64-step loop per call | intrinsic bodies (question 1); signatures stay plain hd | low | question 1 |
| **SipHash-1-3 on a native `i64.add`** for string keys | short keys as fast as FNV, long keys 1.7x faster (#426 comment); recovers the regression | std only (`std.hash`) | low; the collision tests of #353 and #511 stay | the wrapping methods |
| **`Hasher.write_u64` and `write_str`** (#434, approved) | tuple-key map 190 to about 70 ns per op; no list per integer | std, derived `Hash` | low | nothing |
| **`BucketHasher` as plain locals** | 15% to 25% on short keys, which `string_bucket_hash` now gets by repeating code (`2a274b8ad`) | none once scalar replacement exists | low | scalar replacement |

### 3.7 JSON parse and encode

**Today**, after #359 and #360: encode writes text as the walk goes, and
parse reads the input's bytes and shares unescaped strings. What is left
is shared with the groups above: piece pushing (#376), integer text
(#357), derived metadata per call (#377), and `Members.find` scanning all
members per key (`structure.hd:201`).

| Fix | Gain | Cost | Risk | Waits on |
| --- | --- | --- | --- | --- |
| The shared fixes above | the review projected 0.3x encode and 0.2x decode with all of its JSON fixes | listed above | - | - |
| **A member index per type** from the compiler | O(m²) to O(m) member lookups per decoded value | `hd_structure` | low | #377 |
| **`bytes_index_json`** through a native loop, later a SIMD body | the escape and string scans; the probe says a native loop gains little on V8 | one helper | low | the owner's deferral of native loops, and the layout for SIMD |
| **Float text in hd** (Ryu and Eisel-Lemire) | about 30 ns per float against about 250 ns with the host crossing (review estimate) | moves three host primitives into std; needs wide multiplication | medium | question 4 |
| **A streaming deserializer** that skips the tree | decode without building `Json` | a reader design | medium | #377 and the reader work |

### 3.8 Startup and module size

**Today.** The tiny program is 2399 B in release against 2048 B; hello is
4257 B in dev. Literals are pooled behind one getter, and the host fast
path that sends a literal to `println` with `memory.init` is not built.
Start to first output is 40 ms against 5 ms, unprofiled. Each property
case builds a fresh host and instance.

| Fix | Gain | Cost | Risk | Waits on |
| --- | --- | --- | --- | --- |
| **The literal host fast path** (§15.4) | no string array for a literal passed to the host; the tiny program toward 2 KB | `hd_wasm` span function per module | low | nothing |
| **Reuse the host import object per test, instantiate synchronously** (review fix 15) | 28k to about 45k cases per second | `host/cases.mjs`, `host/core.mjs` | low | nothing |
| **Many cases per instance** | reaches the 100k goal (review estimate) | runner change | weakens isolation between cases | question 5 |
| **One `realpath` per host file call** (review fix 24) | `list_dir!` 41 to about 25 µs, `read_text!` 32 to about 20 µs | `host/node.mjs`, the wasmtime host | low; a `realpath` cache would be unsafe | nothing |
| **A size report**, then targeted cuts | finds what fills 17 KB per 1,000 lines | the planned `hd build --size-report` | low | nothing |

## 4. Ranked Steps

Each step is about an hour of agent work and names the measurement it
reports before and after. Every step that touches the compiler or std
also reports hello.wasm (4257 B unless the step explains a change) and,
for compiler changes, the web compiler's gzip size. Measurements run on
a release build at a 1-minute load under 8, as medians of interleaved
runs, on V8 and wasmtime unless the step says otherwise.

The order: measure first, then owner-approved fixes with the largest
measured or estimated gain, then the compiler passes that #427 needs,
then the rest.

| # | Step | Issue | Report before and after |
| --- | --- | --- | --- |
| 1 | Re-run pillar 3 on HEAD; profile JSON encode, decode and text build on V8; run the review's cases on wasmtime. A new metrics record, no code | #426 | the whole pillar 3 table; GC share per profile |
| 2 | Make the text-build row compare equal work | #429 | both ratios, old and equal-work |
| 3 | Add per-element shapes to the `allocations` metric (section 2.6) | #427 | bytes per iteration for each shape; this is #427's baseline |
| 4 | Find why `sum` is 4.77x | - | ns per iteration on V8 with and without Liftoff, on wasmtime, and for a hand-written loop; the WAT of the hot loop |
| 5 | Integer `Display` through `format_int` | #357 | 10⁶ `i64` interpolations (91 ms), `to_string` (72 ms), text-build MB/s, runtime `string-build` ratio, integer-interpolation bytes per iteration |
| 6 | A literal without a getter call per use | #361 | a loop of 10⁶ literal compares; text build with a constant piece (24 ms); share of the literal getter in a dev profile; functions recompiled after a one-literal edit |
| 7 | Wrapping and bit-count methods as instructions | #362 | SHA-256 of 1 MiB (128 ms), 10⁶ `wrapping_add` (12 ms), `checked_add` in a loop |
| 8 | SipHash-1-3 on native adds for string bucket hashes | #353 follow-up | 1M short keys insert and lookup (63 / 52-57 ms), 200k 63-byte keys (19 / 17 ms), the colliding-key tests (stay near 0) |
| 9 | `Hasher.write_u64` and `write_str` | #434 | `(i32, i32)`-key map (190 ns per op), `hash_of(string)` 100k (23 ms), `(string, i32)` keys 200k |
| 10 | Cheaper piece push and `concat_all` | #376 | 1M small-piece pushes plus one `concat_all` (8 ns per piece), 1M byte pushes (3 ns per byte), no-float JSON encode (23-25 ms) |
| 11 | Derived metadata once per type | #377 | `from_json` steady state (about 122 ms per 500), bytes per decoded record |
| 12 | `Display.write` into a builder | #436 | `debug` of a record 100k (78 ms), `Display` of a three-level nested value, text build |
| 13 | Constant hoisting and payloadless singletons | #427 | the boxed-constant shape (bytes per iteration to 0), a match over a subtype enum, Wasm bytes per 1,000 lines |
| 14 | `Heap` on `List[T]`; `Deque` on plain slots | #362 | heap 3 × 200,000 push and pop (167 ms), deque (22 ms), peak heap of a 1M-element deque |
| 15 | Stable sort with one buffer | #362 | `sorted_by` 3 × 200,000 (132 ms), 3 × 100,000 strings (136 ms), runtime `sort` ratio |
| 16 | Time types as newtypes | #426 | the `Duration` shape's bytes per iteration, `to_rfc3339` 10⁵ times |
| 17 | Builders in std's `+` loops, `MemoryFs` append in place | #362 | the growth exponent of each fixed call at 2x and 4x input |
| 18 | UTF-8 conversions in bulk | #362 | `to_utf8` plus `from_utf8` 5 × 1.09 MB (56 ms) |
| 19 | Escape analysis and scalar replacement design note in crates/hd_opt/DESIGN.md: pass order, what counts as an escape, suspension frames, `defer`, panics, and how `^` borrows will feed it | #427 | none (docs) |
| 20 | Trivial inlining in both pipelines | #427 | runtime suite (`bench/runtime`) on both engines, Wasm bytes per 1,000 lines, cold build of 10k lines (0.36 CPU-s), web compiler gzip |
| 21 | Escape analysis as a fact only: count non-escaping sites | #427 | non-escaping sites in std and the examples; Wasm digests unchanged (`HD_CONFORMANCE_WASM_DIGEST=check`) |
| 22 | Scalar replacement of non-escaping records, tuples and enum boxes | #427 | step 3's shapes (records, tuples, `Option` of a record to 0 bytes per iteration), runtime suite, Wasm size |
| 23 | Re-measure iterator chains on both engines | #65 | closures bench (292 ms V8, 781 ms wasmtime), ns per layer per item; the choice between specialization and fusion |
| 24 | Scalar replacement of closures and capture cells | #427 | the closure-per-element shape, closures bench |
| 25 | Property runner: one import object per test, synchronous instantiation | #426 | cases per second (28k) |
| 26 | One `realpath` per host file call, in both hosts | #426 | `list_dir!` (41 µs), `read_text!` (32 µs) |
| 27 | The literal host fast path | #426 | tiny program release bytes (2399 B), hello.wasm, start to first output |

Bounded inlining, known-vtable devirtualization and the closure
specialization schedule follow step 24. Each is larger than an hour, so
each needs its own split once steps 20 to 24 show their real cost.
Several steps wait on a question: 7, 8 and 18 on question 1, and 22 and
24 on question 2. Steps 8 and 9 change std hashing, so they rerun the
#353 and #511 collision tests.

## 5. What Stays Parked

- **The string layout** (`(array v128)` or `(array i64)`). The owner kept
  `(array i8)`. Revisit once JSON and text scanning go through the
  approved primitives and real programs can be measured, as
  SIMD_PRIMITIVES.md section 11 says.
- **Native loops for the SIMD primitives.** Deferred by the owner; the
  probe found almost no gain on V8 and about 10% on wasmtime.
- **SIMD bodies.** They need the layout change.
- **A lazy DFA for regex.** The Pike VM with a prefilter is linear; a DFA
  is a larger design for a row at 0.16x. Text and JSON come first.
- **`Map` in hd** (#180). Not a speed item. `Set` without its `bool`
  waits for it.
- **`a + b + c` as one allocation.** Go's model says loops use a builder;
  #317 left it optional.
- **`wasm-opt` and other Wasm-level passes.** They would break the code
  offsets that `hd.lines` and trap mapping read (§15.5).
- **Cranelift inlining.** Rejected (crates/hd_opt/DESIGN.md, option D): it
  slowed the rest of the suite and dropped frames from release panics.
- **Bounds-check hoisting and loop-invariant motion.** The engines do part
  of it; the catalog leaves them out.
- **Escape facts from `^` borrows.** They wait for the ownership
  prototype to settle.
- **List views for `chunks` and `windows`, `flat_map` over `Iterable`.**
  These are API changes in the stdlib redesign, not speed jobs.
- **The `list_dir!` 5 µs goal.** One `readdir` costs more. The metric
  should time a host call with no I/O if it means to measure the serde
  boundary; that is a metrics change.
- **Compiler time and memory** (pathological inputs, parallel speedup,
  startup RSS, recheck precision). Indexed by #49.
