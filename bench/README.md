# Compiler benchmark inputs

These programs exercise the current compiler subset with inputs larger and
more varied than `examples/`. Run a checked-in case from the repository root:

```sh
cargo run --release -p hd_cli -- bench/tokenizer/main.hd
```

Compare its stdout with the adjacent `expected.out`. The hand-written cases
are:

- `tokenizer`: an integer-encoded tokenizer state machine;
- `math`: iterative integer kernels;
- `records`: records crossing a generic helper;
- `traits`: dispatch through generic trait bounds.

Generate the fifth case, an exact 10,000-line package spread across four
nested folders, then run it:

```sh
node bench/generated/generate.mjs
(cd bench/generated/out/package && cargo run --release --manifest-path ../../../../Cargo.toml -p hd_cli -- run)
```

Generated sources live under `generated/out/` and are intentionally ignored.
The generator accepts another output directory as its first argument.

The reusable profiler avoids benchmark item names that collide with the
prelude. It runs cold, warm, body-edit and comment-edit cases, optionally
on the pool:

```sh
cargo run --release --manifest-path bench/profile-signature/Cargo.toml -- 200
cargo run --release --manifest-path bench/profile-signature/Cargo.toml -- 200 8
```

Its `signature` mode fills the edit case that the driver's built-in
benchmark does not yet measure:

```sh
cargo run --release --manifest-path bench/profile-signature/Cargo.toml -- 200 signature
```

Two Node tools drive the Rust `hd` as a child process:

- [`metrics/`](metrics/README.md): the goal-metric scripts
  (`pnpm run metrics`), judged against the targets in
  [docs/compiler/goals.md](../docs/compiler/goals.md);
- [`micro/`](micro/README.md): runtime microbenchmarks of the same programs
  in hd, Python and Node.
- [`study/`](study/run.mjs): the runtime suite on the RC backend, below.

## Runtime Suite On The RC Backend

`study/` holds the eleven programs of the Rust model study
(docs/future-work/RUST_MODEL_STUDY.md §8 and its appendix), rewritten
from its descriptions since its sources were scratch files, each with a
JavaScript, Go and Python version that print the same output, and
`dyn_iter.hd` with Go and Python iterator references for issue #634.
The runner also times `runtime/progs`, `micro`, `tokenizer`, `math` and an empty program:

```sh
cargo build --release -p hd_cli --features wasmtime
node bench/study/run.mjs                      # every case, median of 3
node bench/study/run.mjs --only nbody,json    # some cases
node bench/study/run.mjs --python-only        # Python only, once each
```

Each hd program is built with `hd build --release --engine E` and run on
Node through `host/run.mjs` and on wasmtime as `hd FILE.wasm --engine
wasmtime`, which includes Cranelift's compile. Every output is checked
against its reference first. Times are process wall time, so start-up is
in them: see the empty program's row. Python references use plain CPython
(`python3`), with no NumPy; a missing interpreter prints a skip note.
`--python-only` checks the study's fixed expected stdout and measures each
Python program once, without a warm-up or any hd, Node or Go execution.
`--no-refs` also uses fixed expected stdout for the study cases, so hd-only
measurements do not launch a reference to obtain its output.

`--checked PATH` builds each program at a reduced input in checked mode
(crates/hd_wasm_linear/DESIGN.md §23.24) and prints the retains, releases,
allocations and heap pages it ran. The CLI has no flag for checked mode,
so PATH is an `hd` built with two local edits: `Program::packages` in
`crates/hd_cli/src/disk.rs` sets `checked` when `HD_BENCH_CHECKED` is set,
and `retained()` and `released()` in `lib/std/rt/checked.hd` put
`allocated()` and `pages()` in the high 32 bits of their results.

### Results, 2026-10-20, after #642 to #655

Commit `de67c20fb`, the same machine and tools as below (Node v24.19.0,
Go 1.24.1, CPython 3.14.7, wasmtime 49 in `hd`), the full suite with
references, median of 3 and a run over 60 s once. The one-minute load was
4 to 15. The commit holds #642 (cycle hooks only for cycle-capable
types), #643 (known block starts), #645 (borrowed parameter projections),
#649 (no collector for acyclic programs) and #655 (one-field element
stores, constant branches), not #644's wasmtime-only inline counts.
"Wasm GC era" is the GC era's hd on V8 from the table below, and
"baseline" is the first RC run below. Milliseconds.

| Program | hd V8 | hd wasmtime | Node | Go | Python | Wasm GC era hd V8 | #638 baseline hd V8 | Beats Wasm GC era? | At least Node? |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- |
| spectral-norm (n 2500) | 606 | 810 | 452 | 281 | 20,256 | 317 | 20,935 | no | no |
| fannkuch-redux (n 10) | 3,522 | 4,568 | 225 | 156 | 3,254 | 753 | 9,107 | no | no |
| mandelbrot (1600², 50) | 385 | 334 | 158 | 118 | 5,243 | 430 | 388 | yes | no |
| nbody (5M steps) | 1,313 | 2,295 | 293 | 183 | 18,512 | 262 | 63,121 | no | no |
| sieve (30M) | 566 | 607 | 106 | 73 | 1,264 | 624 | 2,990 | yes | no |
| binary-trees (depth 20) | 31,583 | 85,442 | 2,991 | 4,746 | 110,391 | 1,934 | 68,816 | no | no |
| points† (1M, 10 rounds) | 111 | 327 | 248 | 27 | 3,273 | 399 | 557 | yes | yes |
| closures† (1M, 50 rounds) | 1,865 | 5,781 | 258 | 166 | 1,825 | 272 | 16,264 | no | no |
| map-count† (2M words) | 915 | 2,249 | 460 | 123 | 436 | 497 | 2,864 | no | no |
| string-build† (3M pieces) | 593 | 1,500 | 306 | 179 | 266 | 366 | 1,443 | no | no |
| json† (400k records) | 18,133 | 33,399 | 259 | 458 | 881 | 1,007 | 92,216 | no | no |
| dyn_iter (#634, 6 variants) | 2,831 | 9,250 |  | 510 | 11,726 |  | 50,911 |  |  |
| progs/closures | 1,112 | 2,614 | 560 |  |  |  | 6,977 |  | no |
| progs/int-loop | 1,234 | 1,013 | 832 |  |  |  | 1,519 |  | no |
| progs/list-sort | 1,363 | 803 | 151 |  |  |  | 41,951 |  | no |
| progs/map-count | 49 | 235 | 39 |  |  |  | 59 |  | no |
| progs/string-build | 181 | 285 | 289 |  |  |  | 152 |  | yes |
| progs/string-builder | 50 | 193 | 45 |  |  |  | 57 |  | no |
| progs/trait-dispatch | 853 | 826 | 627 |  |  |  | 812 |  | no |
| micro/sort | 151 | 347 |  |  |  |  | 493 |  |  |
| micro/fib, sum, map, string_build | 55 to 105 | 134 to 191 |  |  |  |  | 40 to 87 |  |  |
| tokenizer, math | 308 to 312 | 158 to 189 |  |  |  |  | 40 to 87 |  |  |
| empty program | 57 | 197 |  |  |  | 36 | 44 | no |  |

Under V8's sampling profiler, the three programs furthest behind Node
spend their time on counting, not on their own code:

- **json** (70x Node): 40% in `block_of` and about 30% in releases and
  retains. Its strings and text views are interior addresses, so each
  count still finds its block through the page table.
- **fannkuch-redux** (16x): the program's own loop is 44%. Releases of
  known starts take 29%, `retain_known` 14% and `list_unique` 11%: each
  permutation copies and writes its lists, and every write checks
  uniqueness.
- **binary-trees** (11x): releases take 36% and the collector 11%
  (`suspect`, `forget`, `set_word`). `block_of` takes 8%, and allocation
  and free about 5%. Its `&Node` cells are cycle-capable, as a reference
  may close a cycle, so every shared count reaches the collector.

### Results, 2026-10-20

Commit `e8636d4b3`, Apple M3 Max (14 cores), macOS 26.6.2, Node v24.19.0,
Go 1.24.1, wasmtime 49 in `hd`. The machine was busy: the one-minute
load was 15 to 42 during the run, so a cell can be off by 2x; the gaps
below are 10x to 300x. Milliseconds, median of 3 runs; a run over 60 s
was run once. "GC era" is hd on Wasm GC and Rust on Wasm from the Rust
study (2026-10-18, load 5.6 to 15.4); a dagger marks a program whose
input this rewrite chose itself, so that row compares loosely.

The Python column was added separately with CPython 3.14.7 on 2026-10-20:
one process run per program, including startup, with matching stdout.
The other columns retain the measurements above. `dyn_iter` includes its
six variants, each with the program's own best-of-three timings; Python's
returned and local generator chains have no static-vs-erased distinction.

| Program | hd, V8 | hd, wasmtime | Node | Go | py | hd/Node | hd/Go | GC era hd, V8 | GC era hd, wasmtime | Rust, V8 | Peak RSS MB: hd V8, hd wasmtime, Go |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| spectral-norm (n 2500) | 20,935 | 23,958 | 900 | 487 | 24,751 | 23 | 43 | 317 | 822 | 227 | 55, 16, 4 |
| fannkuch-redux (n 10) | 9,107 | 17,246 | 220 | 163 | 4,218 | 41 | 56 | 753 | 414 | 279 | 57, 16, 4 |
| mandelbrot (1600², 50) | 388 | 408 | 170 | 138 | 5,547 | 2.3 | 2.8 | 430 | 224 | 243 | 55, 15, 4 |
| nbody (5M steps) | 63,121 | 118,099 | 293 | 196 | 20,297 | 216 | 322 | 262 | 339 | 408 | 59, 30, 4 |
| sieve (30M) | 2,990 | 5,978 | 140 | 84 | 1,534 | 21 | 36 | 624 | 314 | 273 | 117, 77, 33 |
| binary-trees (depth 20) | 68,816 | 174,179 | 3,015 | 4,674 | 104,449 | 23 | 15 | 1,934 | 2,913 | 3,958 | 209, 177, 148 |
| points† (1M, 10 rounds) | 557 | 1,434 | 324 | 26 | 3,322 | 1.7 | 21 | 399 | 251 | 81 | 103, 62, 67 |
| closures† (1M, 50 rounds) | 16,264 | 37,221 | 264 | 167 | 1,810 | 62 | 97 | 272 | 1,632 | 160 | 64, 35, 8 |
| map-count† (2M words) | 2,864 | 6,550 | 464 | 124 | 386 | 6.2 | 23 | 497 | 450 | 144 | 183, 142, 78 |
| string-build† (3M pieces) | 1,443 | 3,130 | 315 | 174 | 295 | 4.6 | 8.3 | 366 | 351 | 104 | 263, 222, 128 |
| json† (400k records) | 92,216 | 178,923 | 271 | 460 | 854 | 341 | 201 | 1,007 | 2,427 | 234 | 385, 344, 245 |
| dyn_iter (#634, 6 variants) | 50,911 | 117,429 |  | 642 | 12,063 |  | 79 |  |  |  | 56, 24, 9 |
| progs/closures | 6,977 | 16,063 | 547 |  |  | 13 |  |  |  |  | 261, 221 |
| progs/int-loop | 1,519 | 1,096 | 965 |  |  | 1.6 |  |  |  |  | 55, 15 |
| progs/list-sort | 41,951 | 68,106 | 202 |  |  | 208 |  |  |  |  | 56, 25 |
| progs/map-count | 59 | 208 | 41 |  |  | 1.4 |  |  |  |  | 57, 16 |
| progs/string-build | 152 | 272 | 34 |  |  | 4.5 |  |  |  |  | 56, 17 |
| progs/string-builder | 57 | 191 | 34 |  |  | 1.6 |  |  |  |  | 58, 18 |
| progs/trait-dispatch | 812 | 763 | 586 |  |  | 1.4 |  |  |  |  | 55, 15 |
| micro/sort | 493 | 1,154 |  |  |  |  |  |  |  |  | 58, 17 |
| tokenizer, math, micro/fib, sum, map, string_build | 40 to 87 | 117 to 202 |  |  |  |  |  |  |  |  | 53 to 63, 14 to 22 |
| empty program | 44 | 181 |  |  |  |  |  | 36 | 18 | 32 | 51, 14 |

`bench/micro/run.ts` (entry point only, compile excluded, best of 7) at
the same commit: fib 2.6 ms (Wasm GC baseline of 2026-10-06: 3.9), sum
6.3 (77.7), string-build 53.5 (19.0), map 6.9 (28.6), sort 456.0 (42.3);
Python 60.4, 185.1, 12.3, 13.5, 10.2; Node 6.1, 12.0, 6.3, 12.4, 18.0.

Counts in checked mode, at a reduced input (`run.mjs --checked`), per
unit of work. No program allocates per element in its hot loop except
where noted; every one counts there.

| Program | reduced input | retains per unit | allocations per unit |
| --- | --- | ---: | ---: |
| spectral-norm | n 250 | 2.0 per inner product step | 0 |
| fannkuch-redux | n 8 | 75 per permutation | 0 |
| nbody | 50k steps | 49 per body pair per step | 0 |
| sieve | 300k | 4.0 per flag | 0 |
| binary-trees | depth 10 | 7.4 per node | 1 per node |
| points | 10k | 2.9 per point per round | 0 (one list per round) |
| closures | 10k | 14.5 per item per round | 0 |
| map-count | 20k words | 67 per word | 2 per word |
| string-build | 30k pieces | 16 per piece | 2 per piece |
| json | 4k records | 8,300 per record (105 per byte of text) | 85 per record |
| dyn_iter `dyn` / `in-place` / `range` / loop | 100k | 12.3 / 9.7 / 16.3 / 0 per element | 2 / 1 / 6 / 0 per chain, 0 per element |

Releases run 0% to 13% above retains, since a new object's first count is
not a retain. Under V8's sampling profiler
(`--cpu-prof`) the three programs furthest behind Node spend this share
of their time:

| Cost | json | nbody | closures |
| --- | ---: | ---: | ---: |
| count updates (retain, release, drop glue) | 25% | 35% | 39% |
| cycle-collector hooks run on every release (`visit`, `suspect`, `takes_part`, `word`, `block_bytes`) | 29% | 29% | 27% |
| `block_of` page-table lookups and `ref_is_view` | 33% | 17% | 16% |
| size-class division in `class_bytes`, under the two rows above | 5% | 9% | 4% |
| the program's and std's own code | 7% | 7% | 12% |
| allocation and free | 0.3% | 0% | 0% |
| copy-on-write checks | 0.1% | 2% | 0% |
| polling | 0% | 0% | 0% |
