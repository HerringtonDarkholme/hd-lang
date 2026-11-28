# Random hd programs

This hd package generates deterministic, bounded programs and compares their
checksums across dev, checked, and release profiles on Wasmtime. It follows the
typed generation and checksum approach of [Csmith](https://github.com/csmith-project/csmith)
and [YARPGen](https://github.com/intel/yarpgen). Expression trees carry one of all nine integer types. Ordinary masked
arithmetic stays in range; wrapping arithmetic explores the full range. Selected
seeds deliberately overflow in dev and checked builds and compare release output
with an explicitly wrapping reference. List indices and loop bounds are valid
by construction.

Run from the repository root with an **existing Wasmtime-enabled hd**:

```sh
scripts/fuzz-programs.sh --count 1000
HD=/absolute/path/to/hd scripts/fuzz-programs.sh --seed 123 --count 1
scripts/fuzz-programs.sh --count 10000 --minutes 10 --keep-going
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
`--count` to 1000, and `--minutes` to 0 (no additional time bound). With both
limits, the first one reached stops the campaign. The time bound includes
reduction; saved original findings and counts survive cancellation.
`--keep-going` continues after findings. `--hd` can select a compiler under test
separately from the driver. `--directory` defaults to `../../target/fuzz`,
relative to `tools/fuzz/`; use an absolute path to avoid ambiguity.

Every seed shuffles twenty-one construct families and adds up to seven extra
blocks. They combine struct/enum/tuple/optional values, lists, maps, UTF-8
strings, loops, generic `F < Fn` calls, borrowed mutation and box aliasing with:

- generic packets over nested packet, tuple, optional, list and map types;
- generic and dynamic trait dispatch;
- closures captured in loops, stored in structs, copied and invoked later;
- a random cyclic function graph, with alternate and self edges;
- ready and pending bang calls, `join!`, virtual-clock sleeps and wide captured parameter cells;
- named local generic functions, captured defaults, and local types/traits/impls;
- repeated indexed reads in nested loops around borrowed mutation.

Deep random expression trees are interleaved with those blocks. Every program
uses `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64` and `usize`, and later
blocks consume earlier witnesses. Tree budgets sample 63–191 nodes and depth 6–20;
planning and rendering use explicit worklists. Recursive generated calls decrease
a fuel argument and execute one successor per level. These are sampling bounds,
not limits of the hd language.

For ordinary seeds, dev and release execute the same source. Checked captures
the same `main` through `MemoryConsole` and asserts its output equals dev's
checksum. Exactly one checked case must pass. Every eighth seed deliberately
adds one to its chosen integer type's maximum: dev and checked must report
`integer-overflow`, while release must match a fourth execution whose probe uses
`wrapping_add`. Other panics, missing panics and differing checksums are findings.
There is no engine comparison.

For the requested three-hour hunt, set the outer engine deadline beyond the
campaign's own deadline, so its cancellation cleanup can save the final counts:

```sh
HD_FUZZ_TIME_LIMIT=181min scripts/fuzz-programs.sh --count 10000 --minutes 180 --keep-going
```

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

Reduction preserves the failure signature, following the property-preserving
approach of [C-Reduce](https://github.com/csmith-project/creduce). It tries
hierarchical line deletion, then replaces each recorded
expression with a zero of its own type. It preserves the failure signature,
including panic categories and the overflow oracle's raw/wrapping probe pair. It tries typed AST subtree cuts without storing copies of every subtree. Two
line passes surround expression simplification. They visit at most
`4L - 2 + E + N` candidates for L original lines, E legacy expressions and
N expression-tree nodes;
candidate rendering and compilation cost their actual source size. There is
no recursive traversal or unbounded fixpoint. The generator's work is
proportional to tree nodes, emitted text and recorded expressions. Each planned
node is visited once; each rendered node pushes at most five pieces. Counted
work tests cover 1,001 and 10,001-node budgets at depth 20. Campaign work is one
check and three profile runs per seed, with one additional wrapping-reference
run for intentional overflow, plus reduction for findings. Output
writes are proportional to the current seed, without rescanning the corpus.

Focused package checks, from this directory:

```sh
hd check --all
hd lint --all
hd test --engine wasmtime
hd fmt --check
```
