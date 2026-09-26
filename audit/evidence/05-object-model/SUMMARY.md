# 5.1 to 5.3: Object Model, Allocation, Generic Erasure

Commit `bd985d7`, 2026-09-25. The worker could not write report files; the
coordinator wrote this file from the worker's final report. The coordinator
reproduced F-500 independently
(`audit/probes/coordinator/prim-partialeq.hd`:
`missing-trait-implementation: type 'i32' does not implement PartialEq`).

## Headline

- **Maps are linear.** `get` costs 22 ns at 10 entries and 2,490 ns at
  10,000; building a 10,000-entry map takes 131 ms (quadratic).
- **String accumulation is quadratic.** 100k appends take 3.3 to 3.5 s.
- **Generic sum is slower than concrete:** 3.0 to 4.2 times for `i32` (one
  outlier run at 6.9), and 3.0 to 7.2 times for `f64`.
- **Concrete sort gains nothing.** Concrete insertion sort is no faster than
  generic (ratio 0.26 to 1.02), because `list[i32]` re-boxes on every store.
- **Dictionaries allocate per call:** 2 allocations for a bounded call, 3
  through a supertrait, 8 through a blanket implementation.
- **Value and identity correctness:** 33 of 33 checks pass (NaN, -0,
  extremes, `is` rules).
- **Frames:** suspension frames hold 5 to 15 slots when only 0 to 3 are live.

## Method

`audit/scripts/arch/alloc-lib.ts` compiles each probe and wraps every
`struct.new*` and `array.new*` in a function body with a per-type exported
counter. It assembles the result with the repository's `assembleWat`. It then
replaces `WebAssembly.instantiate` for one call, so the counted build gets
exactly the host imports that `compiler.instantiate` builds. Boxing is an
inline `struct.new $hd.box-*`, so box counts appear by type.

Scripts, all passing oxlint, in `audit/scripts/arch/`: `alloc-lib`,
`alloc-static`, `alloc-loops`, `alloc-frames`, `om-build`, `om-timing`,
`erasure-bench`, `erasure-values`, `erasure-excerpts`.

Evidence, in this directory:

- `wat/*.wat`: WAT excerpts;
- `cli-runs.txt`;
- `static-alloc-sites.tsv`, `alloc-static.tsv`, `alloc-per-iteration.tsv`,
  `frames.tsv`;
- `timing-map-string.txt`, `erasure-bench*.txt`, `erasure-values.txt`,
  `failures-f160-f500.txt`.

## 5.1 Object Model

| Construct            | Representation                                                  | Allocations per creation                    | Casts per access              | Notes                                                    |
| -------------------- | --------------------------------------------------------------- | ------------------------------------------- | ----------------------------- | -------------------------------------------------------- |
| data                 | typed struct; every field `mut`                                  | 1, plus 1 per string literal (re-allocated each evaluation) | 0                 |                                                          |
| generic data         | one erased struct with `anyref` fields                          | 1, plus a box per primitive                 | `ref.cast` and unbox per read | applies in concrete code too                             |
| enum                 | one flat struct per enum: `i32` tag plus the union of all variant fields | 1 per payload variant               | 0                             | `match` is a linear if-chain on the tag; no `br_table`, no subtypes |
| fieldless variant    | global singleton, including generic `None`                      | 0                                           |                               | fieldless *data* allocates per `Unit {}`, as spec identity rules require |
| `T?`, `Result`       | shared `$hd.variant {tag, anyref}`                              | `nil` allocates; a present `i32` costs 2    | 1 per unwrap                  | `nil` is neither a null ref nor a singleton              |
| tuple                | `anyref` array, shared with list storage                        | 1, plus a box per scalar                    | `array.get` and `ref.cast`    |                                                          |
| `list[T]`            | vector struct plus `anyref` array                               | 2, plus a box per element                   | 1                             | growth 4 then doubling; indexing runs `vector_get`'s explicit check, the Wasm bounds check, and a cast; `for` allocates one variant per element |
| `map[K, V]`          | parallel key and value arrays; no hashing                       |                                             |                               | `get` scans O(n) and allocates a variant; `remove` shifts O(n) |
| string               | UTF-8 `(array (mut i8))`                                        |                                             |                               | `len` is O(n); interpolation is a pairwise concat chain  |
| closure              | `{funcref, env}` plus a typed env struct                        | 2, even with no captures                    | 1 env cast per captured read  |                                                          |
| dynamic trait value  | `{value, bounds, m0..mk}`                                       | 1 per conversion; method table copied into each value |                     |                                                          |
| stored `Suspend[T]`  | frame plus a 4-field wrapper                                    | 2                                           |                               | a poll runs `call_ref`, a cast adapter, then the poll function, and boxes the result; a direct `f!()` skips the wrapper |

**Verdict against "Representation Shortcuts".**

- Every listed shortcut matches the emitted code.
- Erasure reaches beyond generic positions: concrete lists, maps, tuples,
  optionals, and `Result` payloads are also erased and boxed. Dictionaries
  are trait-value structs.
- Shortcuts that are not listed: tuples as `anyref` arrays, an allocated
  `nil`, and a method table copied into each trait value.
- Minor costs found here (see also 5.7): every `i < n` becomes a three-way
  compare, and checked arithmetic goes through calls.

## 5.2 Allocation

Allocations per loop iteration:

| Loop body                                                                                  | Allocations |
| ------------------------------------------------------------------------------------------ | ----------- |
| empty loop; closure called; requirement-bearing call; `$.with`; generic call returning erased `i32` | 0 |
| `for` over `list[i32]`, concrete or generic                                                | 2 (1 variant, plus 1 box from building the list) |
| closure created                                                                            | 2           |
| string interpolation                                                                       | 7           |
| bounded generic call                                                                       | 2           |
| call through a supertrait                                                                  | 3           |
| call through a blanket implementation                                                      | 8           |
| `fn!` that completes immediately                                                           | 1           |

Suspension frame slots against maximum live values (live sets derived by
hand):

| Function      | Slots | Max live | Notes                                  |
| ------------- | ----- | -------- | -------------------------------------- |
| `wide!`       | 7     | 0        |                                        |
| `accumulate!` | 15    | 3        |                                        |
| `heap!`       | 6     | 0        | retains a string, a list, and a tuple  |
| `phases!`     | 5     | 1        |                                        |

Liveness analysis would remove 75 to 100% of frame slots.

## 5.3 Generic Erasure

- **Cost:** see the headline ratios. Generic code pays boxing, unboxing, and
  casts on every element. Concrete lists already pay most of that, so
  concrete code has little advantage.
- **Correctness:** NaN, -0, `f64` max, the smallest subnormal, and `i32`
  min and max survive identity, `Box`, list, tuple, optional, and closure
  round trips bit for bit. Generic equality agrees with concrete equality.
  Trait-value identity follows chapter 05.
- **Dictionaries:** built on every call and never hoisted. Each bound method
  call inside a generic body also builds a `$trait` value (HIR `trait-call`
  on `trait-bound`).
- **Evolution:** HIR fields that could carry selective monomorphization:
  - `HirFunction.genericParameters` and `genericBounds`;
  - on calls: `arguments[].type`, `type`, `erasedParameterTypes` and
    `erasedResultType`, and `bounds[].dictionary`
    (`HirTraitDictionaryPlan.implementationIndex`);
  - `trait-bound.boundIndex`, together with
    `HirTraitImplementation.methodFunctions`;
  - `erasedFieldType(s)` on data, match, and pattern nodes.

  The gaps are that calls carry no explicit type-argument list, and that
  `ValueType` is a string (F-607). A HIR-to-HIR specialization pass looks
  feasible (`UNVERIFIED`).
- **Verdict:** uniform erasure was the right MVP choice. As implemented, it
  is not the right long-term default. It needs:
  - static dictionaries;
  - unboxed optional, tuple, and scalar-list storage where no generic
    sharing occurs;
  - selective specialization, with erasure kept as the fallback.

## Findings

| ID    | Severity | Title                                                                      |
| ----- | -------- | -------------------------------------------------------------------------- |
| F-500 | major    | primitives do not satisfy `T: PartialEq` or `T: PartialOrd`                |
| F-501 | major    | `map` has no hashing: O(n) `get` and insert, O(n²) build                  |
| F-502 | major    | bounded generic calls rebuild dictionaries every call, plus a trait value per method call |
| F-503 | minor    | interpolation concatenates pairwise and re-allocates literals each time    |
| F-504 | minor    | the optional carrier allocates for each `nil`, each `for` step, and each `map.get` |
| F-505 | note     | concrete `list[i32]` stores boxes                                          |
| F-506 | note     | suspension frames keep dead locals and dead heap references                |

Also reproduced: F-160 (any `f64` `<`, `>`, `<=`, `>=` crashes WAT emission).

## Phase-7 Questions

1. Chapter 04 makes erasure and boxing normative, while chapter 09 says the
   compiler "may monomorphize". Which one governs?
2. Does any spec rule force erased tuple or optional storage? Not directly.
   Uniform erasure combined with list identity does:
   `erasure-shared-storage.hd` shows generic code mutating a concrete
   `list[(i32, i32)]` in place. Should specialized layouts be allowed for
   values without identity?
3. Should map operation complexity be specified?
4. Is distinct identity for fieldless data intended?
5. Does "`Suspend[T]` is dynamic" force a wrapper on every stored suspension?
6. Should `string.len()` be allowed to be O(n)?

## Not Done

- Non-canonical NaN: hd source cannot produce one.
- `f64` sort and `PartialOrd`-bounded sort: blocked by F-160 and F-500.
- An unboxed baseline: impossible to write in hd today.
- Live sets were derived by hand.
- Timings are noisy (contended CPU).
