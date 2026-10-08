# Q22: Runtime vs Node (`6572b51d`)

Six small user-style programs the new compiler runs today, each with an
equivalent hand-written JavaScript program. Rerunnable:
`node compiler/bench/runtime/run.mjs [workdir]` (sources in
`compiler/bench/runtime/progs/`).

- hd commit: `6572b51d`; binary `compiler/target/release/hd build`
  (debug profile, rebuilt from this commit before measuring)
- runner: `node v24.19.0 compiler/host/run.mjs` for hd, `node` directly
  for JS; same machine, load ~4.9
- method: build once per program; 1 warmup + 5 measured runs each;
  p50 = median, p95 = max; each program prints a checksum, and the
  script refuses to time a program whose hd and JS outputs differ
  (all six matched)

| Program | What it does | hd p50 / p95 (ms) | JS p50 / p95 (ms) | hd/JS |
| --- | --- | --- | --- | --- |
| closures | fill 20M list, `map`/`filter`/`fold` with closures | 361 / 362 | 529 / 536 | 0.68x |
| int-loop | 240M iterations of `%`-heavy i32 accumulation | 2329 / 2338 | 797 / 811 | 2.92x |
| list-sort | insertion sort of 30k pseudorandom i32 | 1309 / 1355 | 170 / 173 | 7.72x |
| map-count | 200k inserts + 5k `get_or` reads on `Map[i32, i32]` | 1964 / 2069 | 34 / 36 | 58.47x |
| string-build | 40k `out + "abcdefgh"` appends (320 KB result) | 2495 / 2555 | 30 / 35 | 82.18x |
| trait-dispatch | 80M alternating generic calls through a `Step` bound | 883 / 890 | 579 / 631 | 1.53x |

Geomean hd/JS: **6.95x**. A second full run reproduced every ratio
within a few percent (geomean 6.85x).

## Flagged (over 1.5x), with a guess at the cause

- **string-build, 82x — allocation (quadratic concat).** Each `+`
  copies the whole accumulated string (`bytes_concat` of two flat
  strings), so 40k appends copy ~6 GB total. V8 builds a cons-string
  rope and flattens once. Fix direction is a rope or builder string
  representation (architecture issue: representation-runtime.md).
- **map-count, 55–58x — boxing + hashing overhead.** Every key, value
  and `Option` result crosses as a heap object, and each `get`/insert
  pays a full hash + equality call. V8's `Map` uses inlined caches and
  unboxed small integers. Likely implementation slip (specialize
  small-int keys, avoid per-op allocation) before representation work.
- **list-sort, 7.7x — boxing + checked indexing.** Insertion sort is
  all indexed get/set; each `items[j]` plausibly boxes the i32 and
  bounds-checks twice (get then set). V8 keeps the array unboxed
  (Smi/double backing store). Implementation slip if elements box
  per access; worth one profile before redesigning layout.
- **int-loop, 2.9x — checked arithmetic.** Every `*`/`+`/`%` carries
  the debug-build overflow check (`types.arith.checked`); V8 JITs the
  same loop on unboxed doubles/ints with no checks. Expected cost of
  the specified semantics, not a bug; a release profile without
  overflow checks would say how much is checks vs codegen.
- **trait-dispatch, 1.5x — call overhead, barely over the line.**
  Alternating generic calls through a bound cost ~50% over V8's
  monomorphic inline caches. Plausibly vtable-per-call or
  dictionary lookup where a per-instance selection cache would do.
  Not atrocious; measure again after the bigger gaps close.

## Not flagged

- **closures, 0.68x — hd is faster.** The hd iterator adapters fuse
  `map`/`filter`/`fold` with no intermediate allocation, while the
  equivalent JS allocates three 20M-element arrays. Evidence the
  lazy-adapter design pays at run time.

## Limits

- Debug-profile Wasm only; no `--release` comparison yet.
- Node startup (~30 ms) is inside every sample; negligible next to
  the 0.3–2.6 s workloads except for the JS side of string-build and
  map-count (there the hd side dominates 50–80x anyway).
- `for x in map` iteration is unemitted (`MapIter` unsupported), so
  map-count verifies with `get_or` reads instead of a traversal;
  write and read costs are both in the number.
