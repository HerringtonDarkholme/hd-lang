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
(the Rust model study §8 and its appendix, in git history: `git show b3f5eed1a:docs/future-work/RUST_MODEL_STUDY.md`), rewritten
from its descriptions since its sources were scratch files, each with a
JavaScript, Go and Python version that print the same output, and
`dyn_iter.hd` with Go and Python iterator references for issue #634.
The runner also times `runtime/progs`, `micro`, `tokenizer`, `math` and an empty program:

```sh
cargo build --release -p hd_cli --features wasmtime
node bench/study/run.mjs                      # every case, median of 5
node bench/study/run.mjs --quick              # hd on V8 only, 3 runs, about a minute
node bench/study/run.mjs --only nbody,json    # some cases
node bench/study/run.mjs --python-only        # Python only, once each
```

Each study program's input is tuned so hd on V8 runs about 300 ms
(<https://matklad.github.io/2026/10/05/benchmark-milliseconds.html>):
long enough that start-up and noise are small, short enough to run
often. The input is a constant at the top of each source, identical in
the hd, JavaScript, Go and Python versions. Times are integer
milliseconds, the median of 5 fresh-process runs after one discarded
preflight process. The discarded process does not warm the measured Wasm
modules: each measured process has fresh V8 tier state. `--quick` measures hd on V8 only, 3 runs, no references.
`--backend B` passes `--backend B` to `hd build`.

Each hd program is built with `hd build --release --engine E` and run on
Node through `host/run.mjs` and on wasmtime as `hd FILE.wasm --engine
wasmtime`, which includes Cranelift's compile. Every process output, including the discarded preflight, is checked
against the expected stdout and must exit successfully. Times are process wall time, so start-up is
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

### Fresh Processes And Warm Modules

An optional Node measurement compiles each Wasm module once, runs three
unmeasured activations, then measures fresh hosts and instances of that
same module in one process:

```sh
node bench/study/run.mjs --quick --only mandelbrot --warm-module
node --test bench/study/run.test.mjs
```

The extra table labels fresh-process wall time and warm-module activation
time separately, with sample counts and the Node/V8 versions. The original
wall-time table, references, timing flags and `results.json` rows stay;
`warmModule` adds the optional measurement to each row. Warm-module time
includes host creation, instantiation, `hd.init`/`hd.poll` and host cleanup;
it excludes Wasm compilation and Node process startup. Module code and its
V8 tier state are shared; mutable Wasm globals, memory and host state belong
to each new instance. Warm-up and measured activations must all produce
exactly the expected stdout and exit successfully.

A focused run on 2026-10-22, Node v24.19.0 / V8 13.6.233.17-node.51:

| Program | fresh-process wall, ms | samples | warm-module activation, ms | samples | discarded activations |
| --- | ---: | ---: | ---: | ---: | ---: |
| mandelbrot | 317 | 3 | 97 | 3 | 3 |

Every activation produced `778803` followed by a newline. The machine was
shared (load 6–21); these numbers illustrate different measurement scopes,
not a compiler speed change. The existing wall-time column remains a
fresh-process measurement. Harness tests verify reuse of one compiled
module with distinct instances and reset mutable globals, and refuse
output or status failures beyond the discarded preflight.

### Runtime vs JavaScript, 2026-10-29

Commit `abb3d0cd4`, Apple M3 Max, Node v24.19.0 / V8
13.6.233.17-node.51. Times are integer milliseconds, the median of 15
fresh processes after a discarded preflight. The same release Wasm ran at
V8's default tier, with `--no-liftoff`, and with `--liftoff-only`; the
JavaScript reference is the clean default-tier run. Every measured output
matched exactly. The machine was shared, with one-minute load 7–16 during
the kept runs.

| Program | JS | hd default | hd/JS | hd `--no-liftoff` | hd/JS | hd `--liftoff-only` | hd/JS | Wasm bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| spectral_norm | 555 | 298 | 0.54 | 282 | 0.51 | 485 | 0.87 | 6,778 |
| fannkuch_redux | 218 | 481 | 2.20 | 214 | 0.98 | 356 | 1.63 | 6,439 |
| mandelbrot | 133 | 322 | 2.42 | 141 | 1.06 | 297 | 2.23 | 4,565 |
| nbody | 400 | 339 | 0.85 | 344 | 0.86 | 834 | 2.09 | 7,652 |
| sieve | 151 | 360 | 2.38 | 198 | 1.31 | 276 | 1.82 | 5,487 |
| binary_trees | 261 | 262 | 1.00 | 499 | 1.92 | 863 | 3.31 | 8,744 |
| points | 1,554 | 330 | 0.21 | 288 | 0.19 | 629 | 0.40 | 6,318 |
| closures | 752 | 322 | 0.43 | 317 | 0.42 | 989 | 1.31 | 6,994 |
| map_count | 531 | 284 | 0.54 | 279 | 0.53 | 462 | 0.87 | 12,809 |
| string_build | 749 | 295 | 0.39 | 305 | 0.41 | 558 | 0.74 | 6,059 |
| json | 129 | 378 | 2.94 | 448 | 3.48 | 941 | 7.32 | 70,655 |
| structure_walk | — | 184 | — | 228 | — | 540 | — | 20,446 |
| progs/closures | 663 | 295 | 0.44 | 245 | 0.37 | 758 | 1.14 | 6,303 |
| progs/int-loop | 296 | 339 | 1.14 | 314 | 1.06 | 319 | 1.08 | 4,399 |
| progs/list-sort | 179 | 342 | 1.91 | 138 | 0.77 | 342 | 1.91 | 5,778 |
| progs/map-count | 267 | 250 | 0.94 | 238 | 0.89 | 322 | 1.20 | 6,127 |
| progs/nested-display | 616 | 301 | 0.49 | 315 | 0.51 | 483 | 0.78 | 6,808 |
| progs/string-build | 34 | 333 | 9.90 | 366 | 10.87 | 340 | 10.10 | 4,473 |
| progs/string-builder | 655 | 294 | 0.45 | 277 | 0.42 | 403 | 0.61 | 6,057 |
| progs/trait-dispatch | 271 | 313 | 1.16 | 289 | 1.07 | 299 | 1.10 | 4,459 |

Across the 19 cases with JavaScript references, the geometric-mean hd/JS
ratio is 0.98 at the default tier, 0.85 with `--no-liftoff`, and 1.47
with `--liftoff-only`. Excluding `progs/string-build`, the deliberately
naive quadratic `out + s` loop, the ratios are 0.87, 0.74, and 1.32.
`structure_walk` has no JavaScript reference and is excluded from both
geometric means.

The quadratic string case remains the largest ratio; its root cause and
the owner-approved builder direction are already recorded in issue #165
and #317. JSON is the largest remaining default-tier gap at 2.94x, ahead
of mandelbrot at 2.42x and sieve at 2.38x. Fannkuch, mandelbrot, and
list-sort reaching or beating JavaScript with `--no-liftoff` confirms that
their default gap is V8 tiering of one long first call, not their optimized
Wasm body.

A default-tier V8 profile of JSON had 350 ticks: 247 (70.6%) in Wasm, 82
(23.4%) in C++, one (0.3%) in GC, and the rest shared or unaccounted. Of
the Wasm ticks, 104 were in named `std.json` bodies, 118 in generated or
unnamed bodies, 17 in `std.rt.alloc`, five in `std.serde`, and three in
`main`. No one function dominates: the largest generated body is 8.9% of
the Wasm ticks, `Parser.validate` is 7.7%, `TextWriter.start` is 4.0%, and
`TextReader.next_item` is 3.6%. The cost is the JSON and generated
structure work itself, not GC or one allocator helper.

The ceiling retained one compiled module, discarded three activations,
then timed 15 fresh hosts and instances. Its median activation is 397 ms
at the default tier, 339 ms with `--no-liftoff`, and 898 ms with
`--liftoff-only`, against fresh-process medians of 378, 448, and 941 ms in
the table. Removing process startup and compilation recovers none of the
default gap; even the eager-TurboFan ceiling remains 2.63x JavaScript.
There is no local compiler fix to build from this measurement. JSON and
generated-structure performance remain in issue #929, whose owner-approved
direction and measured ceilings already cover those mechanisms.

### Returned Iterator Cost, 2026-10-22

Issue #634, commit base `6278705a6`, Apple M3 Max, Node v24.19.0 / V8
13.6.233.17-node.51, wasmtime 49.0.2, Go 1.24.1. The machine was shared;
observed one-minute load ranged from 4 to 30. Each callback sums `x * 2`
for `x` in `0..10_000_000` where `x % 3 != 0`, using optimized hd programs.
Times are the best of three in-program calls, excluding process startup
and compilation; these are distinct from the runner's process-wall table.
All six checksums matched exactly on Node, wasmtime and Go.

| Variant | hd Node, ms | ns/element | hd wasmtime, ms | ns/element | Go, ms | ns/element |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| returned `&mut dyn Iterator[i64]` | 143 | 14.3 | 289 | 28.9 | 34 | 3.4 |
| same concrete chain in place | 72 | 7.2 | 161 | 16.1 | 28 | 2.8 |
| plain `while` loop | 5 | 0.5 | 10 | 1.0 | 9 | 0.9 |
| range adapter chain | 83 | 8.3 | 175 | 17.5 | | |
| returned chain, 100k calls of 100 elements | 155 | 15.5 | 303 | 30.3 | 42 | 4.2 |
| in-place chain, 100k calls of 100 elements | 79 | 7.9 | 173 | 17.3 | 27 | 2.7 |

The Go range-labelled case reuses its interface pipeline, so it is not a
separate range-adapter comparison. Long-case checksums are
`66666653333334`; the short-chain aggregate is `653400000`.

```sh
node bench/study/run.mjs --hd target/debug/hd --only dyn_iter --no-refs --runs 1
go build -o target/dyn-iter-go bench/study/go/dyn_iter.go
target/dyn-iter-go
```

The block counts below came from a probe of release builds' allocation
total, which #938 removed; checked builds count allocations
(`std.rt.checked.allocated`).

| Variant | blocks per call, Node | blocks per call, wasmtime | per element |
| --- | ---: | ---: | ---: |
| returned trait-value chain | 2 | 2 | 0 |
| concrete chain in place | 1 | 1 | 0 |
| plain loop | 0 | 0 | 0 |

These are slopes across one, two and eight calls, at both 100 and 100,000
elements. Fixed probe/entry overhead is nine blocks for either chain and
eight for the loop; it is excluded from the per-call numbers. Both input
lengths give identical slopes. Erasure adds one block per chain, rather
than per element.

**Recommendation:** keep the interface-return model for now. Erasure is
now a material 1.99x on Node and 1.80x on wasmtime, but the concrete chain
still costs 14–16x the loop. Opaque return syntax could remove roughly
half the measured chain time while leaving that larger gap. Profile the
remaining adapter/dispatch cost through existing mechanisms before adding
`impl Trait`; per-element allocation is not the cause shown here.

### Results, 2026-10-21, after the RC optimization round

Commit `f7549b935`, Apple M3 Max, Node v24.19.0, Go 1.24.1, CPython
3.14.7, wasmtime 49 in `hd`. The machine was shared (one-minute load 5 to
15, Spotlight indexing), so a cell can be off by 1.3x; the full run took
5 min 51 s. Integer milliseconds, median of 5. spectral_norm and points on
V8 are from a `--quick` re-run (the full run's 291 and 349 came during a
load spike). "hd V8, 10-20" is the previous table's run at `c163ee2c0`.

"Wasm GC era" is hd on V8 built by `84095aa9e`, the last commit whose CLI
has `--backend wasm-gc`, running the same sources at the same sizes
(`run.mjs --backend wasm-gc`, median of 5, once). `dyn_iter` does not
compile there (no `Iterator` trait), so it has no GC-era number. The
owner's RC target is hd V8 at most 1.2x the Wasm GC era.

`closures` is not like for like with its GC-era number: #712 replaced
`List.iter`'s step closure with direct indexing after that run, and
`closures.js` is idiomatic JavaScript: the `Int32Array`'s own `map`,
`filter` and `reduce` with arrow functions (eager; it was one hand-fused
loop). Its Node number is the best of three re-runs after that change, at
load 23 to 25. Go keeps a `fold` over three function values, as Go has no
`map`/`filter`.

| Program | Input | hd V8 | hd V8, 10-20 | hd wasmtime | Node | Go | Python | Wasm GC era hd V8 | hd / Wasm GC | Meets 1.2x? |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| spectral_norm | n 2000 | 225 | 252 | 567 | 318 | 181 | 12,729 | 508 | 0.44 | yes |
| fannkuch_redux | n 9 | 118 | 341 | 214 | 55 | 21 | 312 | 117 | 1.01 | yes |
| mandelbrot | 1400², 50 iterations | 309 | 311 | 287 | 137 | 93 | 3,990 | 356 | 0.87 | yes |
| nbody | 1.15M steps | 289 | 314 | 497 | 92 | 49 | 4,372 | 1,277 | 0.23 | yes |
| sieve | 16M | 267 | 318 | 306 | 68 | 34 | 646 | 365 | 0.73 | yes |
| binary_trees | depth 14 | 127 | 360 | 338 | 59 | 59 | 402 | 108 | 1.18 | yes |
| points | 3.3M records, 10 rounds | 222 | 288 | 446 | 738 | 71 | 11,242 | 1,537 | 0.14 | yes |
| closures | 170k items, 50 rounds | 152 | 363 | 314 | 226 | 36 | 340 | 132 | 1.15 | yes (std changed) |
| map_count | 650k words | 170 | 316 | 357 | 167 | 47 | 136 | 239 | 0.71 | yes |
| string_build | 1.5M pieces | 165 | 336 | 338 | 173 | 94 | 147 | 278 | 0.59 | yes |
| json | 6k records | 122 | 341 | 225 | 38 | 13 | 33 | 149 | 0.82 | yes |
| dyn_iter | 1M elements, 10k calls | 233 | 339 | 412 |  | 58 | 1,220 |  |  |  |

All eleven programs with a GC-era number meet the target; binary_trees
(1.18) and closures (1.15) with the least margin. Against Node, hd is
level or faster on spectral_norm, points, closures, map_count and
string_build, and 2.1x to 3.9x slower on the rest.
The GC-era numbers for nbody and points are far above the earlier
scratch-source GC-era figures, since these sources copy more.

The other cases the runner times, which this retune left alone (hd V8,
hd wasmtime, Node): progs/closures 1,124, 1,894, 573; progs/int-loop
1,241, 1,164, 837; progs/list-sort 1,367, 865, 210; progs/map-count 46,
169, 36; progs/string-build 152, 320, 33; progs/string-builder 43, 134,
35; progs/trait-dispatch 791, 1,444, 583; micro/* 58 to 187 on V8;
tokenizer 52, math 54, empty program 55 (wasmtime 170).
