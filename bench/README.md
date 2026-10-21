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
milliseconds, the median of 5 runs after a warm-up run (Python: 3 runs,
no warm-up). `--quick` measures hd on V8 only, 3 runs, no references.
`--backend B` passes `--backend B` to `hd build`.

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

### Results, 2026-10-20, at the 300 ms sizes

Commit `c163ee2c0` plus the retuned inputs, Apple M3 Max, Node v24.19.0,
Go 1.24.1, CPython 3.14.7, wasmtime 49 in `hd`. The machine was shared
(one-minute load 10 to 100), so a cell can be off by 1.5x or more; the
full run took 7 min 18 s and `--quick` 48 s. Integer milliseconds,
median of 5. spectral-norm and nbody on V8 are from a re-run, as the
full run's first reading of nbody (610) came during a load spike.

"Wasm GC era" is hd on V8 built by `84095aa9e`, the last commit whose CLI
has `--backend wasm-gc`, running the same sources at the same sizes
(`run.mjs --backend wasm-gc`, median of 5, once). `dyn_iter` does not
compile there (no `Iterator` trait), so it has no GC-era number. The
owner's RC target is hd V8 at most 1.2x the Wasm GC era.

The table below records the run before [#712](https://github.com/HerringtonDarkholme/hd-lang/issues/712).
That std change replaces `List.iter`'s step closure with direct indexing.
Later `closures` / Wasm GC ratios include this std change: the GC-era run
used closure stepping. Judge #712 with paired hd before/after measurements.

| Program | Input | hd V8 | hd wasmtime | Node | Go | Python | Wasm GC era hd V8 | hd / Wasm GC | Meets 1.2x? |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| spectral_norm | n 2000 | 252 | 497 | 303 | 216 | 13,072 | 508 | 0.50 | yes |
| fannkuch_redux | n 9 | 341 | 267 | 48 | 20 | 306 | 117 | 2.9 | no |
| mandelbrot | 1400², 50 iterations | 311 | 327 | 131 | 92 | 4,100 | 356 | 0.87 | yes |
| nbody | 1.15M steps | 314 | 659 | 97 | 53 | 4,429 | 1,277 | 0.25 | yes |
| sieve | 16M | 318 | 393 | 69 | 35 | 673 | 365 | 0.87 | yes |
| binary_trees | depth 14 | 360 | 1,018 | 64 | 60 | 433 | 108 | 3.3 | no |
| points | 3.3M records, 10 rounds | 288 | 522 | 987 | 75 | 11,637 | 1,537 | 0.19 | yes |
| closures | 170k items, 50 rounds | 363 | 896 | 72 | 34 | 330 | 132 | 2.8 | no |
| map_count | 650k words | 316 | 817 | 166 | 45 | 139 | 239 | 1.3 | no |
| string_build | 1.5M pieces | 336 | 692 | 440 | 94 | 147 | 278 | 1.21 | no |
| json | 6k records | 341 | 694 | 300 | 13 | 38 | 149 | 2.3 | no |
| dyn_iter | 1M elements, 10k calls | 339 | 754 |  | 63 | 1,227 |  |  |  |

Six of eleven programs miss the target: fannkuch_redux, binary_trees,
closures and json by more than 2x, map_count and string_build narrowly.
The GC-era numbers for nbody and points are far above the earlier
scratch-source GC-era figures, since these sources copy more.

The other cases the runner times, which this retune left alone (hd V8,
hd wasmtime, Node): progs/closures 1,124, 1,894, 573; progs/int-loop
1,241, 1,164, 837; progs/list-sort 1,367, 865, 210; progs/map-count 46,
169, 36; progs/string-build 152, 320, 33; progs/string-builder 43, 134,
35; progs/trait-dispatch 791, 1,444, 583; micro/* 58 to 187 on V8;
tokenizer 52, math 54, empty program 55 (wasmtime 170).
