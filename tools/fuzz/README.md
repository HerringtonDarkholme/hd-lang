# Random hd programs

This hd package generates deterministic, bounded programs and compares their
checksums across dev, checked, and release profiles on Wasmtime. It follows the
typed generation and checksum approach of [Csmith](https://github.com/csmith-project/csmith)
and [YARPGen](https://github.com/intel/yarpgen). Integer operations that can
overflow use `wrapping_*`; list indices and loop bounds are valid by construction.

Run from the repository root with an **existing Wasmtime-enabled hd**:

```sh
scripts/fuzz-programs.sh --count 1000
HD=/absolute/path/to/hd scripts/fuzz-programs.sh --seed 123 --count 1
scripts/fuzz-programs.sh --minutes 10 --keep-going
# Optional package-manager alias for the same shell launcher:
pnpm fuzz:programs --count 1000
```

The launcher selects `HD` when supplied, otherwise `target/release/hd`, then
`target/debug/hd`, then `hd` on PATH. It never builds or installs the compiler.
The launcher, generator, and generated programs run natively through hd and
Wasmtime. An incompatible compiler fails the three-profile preflight.
The generator package uses hd's normal cache; `HD_CACHE` overrides its location.
`HD_FUZZ_TIME_LIMIT` changes the outer hd deadline (default `1h`). Each child
compiler command has a 30-second deadline and each program a 5-second deadline.

To bypass the launcher, run inside `tools/fuzz/`:

```sh
hd run --engine wasmtime --time-limit 1h -- --hd /absolute/path/to/hd --count 1000
```

Options are derived with `std.cli`; `--help` lists them. `--seed` defaults to 1,
`--count` to 1000, and `--minutes` to 0 (count mode). A positive `--minutes`
replaces the count limit and bounds the entire campaign, including reduction.
`--keep-going` continues after findings. `--hd` can select a compiler under test
separately from the driver. `--directory` defaults to `../../target/fuzz`,
relative to `tools/fuzz/`; use an absolute path to avoid ambiguity.

Every seed shuffles eleven construct families and adds up to seven extra blocks:
struct value copies, enums and match, nested tuples, optionals and let-else,
list and map copies/mutations, UTF-8 strings, bounded loops with break/continue,
direct and generic `F < Fn` closures, borrowed mutation, and shared boxes.
Later blocks use earlier values. Expression generation is iterative, with at
most three extra layers. Every program prints one signed 64-bit checksum.

The dev and release runs execute the same source. Checked uses the existing
`hd test` profile: a wrapper captures the same `main` through `MemoryConsole`
and asserts its output equals dev's checksum. A passing checked test must report
exactly one passing case. There is no engine comparison.

The output directory contains:

- `corpus/seed-N.hd`: the exact generated source; checker rejections also have
  `seed-N.rejected.txt`. Rejections are generator defects, counted separately.
- `findings/seed-N.original.hd` and `.original.txt`: evidence saved before reduction.
- `findings/seed-N.hd` and `.txt`: reduced source, compiler, signature, and outputs.
- `summary.txt` and `compiler.txt`: campaign counts and compiler identity.

Reproduce a seed with `HD=/absolute/path/to/hd scripts/fuzz-programs.sh --seed N --count 1`.
Reproduce a saved source with `hd check --format json FILE`, then
`hd FILE --engine wasmtime` and `hd FILE --release --engine wasmtime`.
The campaign's `checked.hd` contains the most recent checked wrapper.
Exit status is 1 for findings, 2 for setup errors or a rejection rate at least
10%, and 0 otherwise. File each reproducible compiler or std defect with its
reduced source and a failing stage case; also file defects encountered while
implementing or using the fuzzer.

Reduction first tries hierarchical line deletion, then replaces recorded
expressions with `0i64`, preserving the original failure signature. It visits
at most `2L - 1 + E` candidates for L original lines and E expression sites;
candidate rendering and compilation cost their actual source size. There is
no recursive traversal or unbounded fixpoint. The generator's work is
proportional to emitted text and recorded expressions; campaign work is one
check and three profile runs per seed, plus reduction for findings. Output
writes are proportional to the current seed, without rescanning the corpus.

Focused package checks, from this directory:

```sh
hd check --all
hd lint --all
hd test --engine wasmtime
hd fmt --check
```
