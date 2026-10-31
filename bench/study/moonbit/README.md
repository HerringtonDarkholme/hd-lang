# hd vs MoonBit (linear-memory wasm and wasm-gc backends)

MoonBit ports of every hd benchmark program (11 in `bench/study`, 9 in
`bench/runtime/progs`) plus `empty`, and a runner that compares hd's release
Wasm with `moon build --target wasm` output, or `--target wasm-gc`. Same algorithm, sizes and stdout
as the hd source; `compare.ts` fails if the two stdouts differ.

Packages: `study_<name>` (from `bench/study/<name>.hd`), `progs_<name>` (from
`bench/runtime/progs/<name>.hd`, dashes become underscores), `empty`.

Versions measured: moon 0.1.20261023 (3778465 2026-10-23), node v24.19.0.

## Build

```sh
cargo build --release -p hd_cli -j 4            # target/release/hd
node bench/study/moonbit/compare.ts --build    # hd packages into work/, then moon build
# the MoonBit half alone:
cd bench/study/moonbit && /Users/hd/.moon/bin/moon build --target wasm --release
```

## Run

```sh
node bench/study/moonbit/compare.ts                        # 11 runs, all programs
node bench/study/moonbit/compare.ts --runs 5 --only study_json,empty
node bench/study/moonbit/compare.ts --build --target wasm-gc   # then run with --target wasm-gc
```

Each run is a fresh `node` process; one discarded warm-up, then `--runs`
measured runs with hd and MoonBit interleaved, at two V8 tiers (default and
`--no-liftoff`). Run time is `performance.now()` around the entry call after
instantiation (hd: `host.run(instance, "hd.init", "hd.poll")` on hd's own
`host/node.ts`; MoonBit: `_start()`). Wall time is the whole process.
The MoonBit host is only `wasi_snapshot_preview1.fd_write` (wasm) or
`spectest.print_char` (wasm-gc). Medians are
printed as markdown tables with the geometric mean of hd/MBT (empty excluded)
and the load average.

Ports use core `Array`/`Map`/`StringBuilder`, `for .. in` ranges and
iterators. `moon fmt` rewrites `Int` as `Int32`; they are the same type.
(`moon fmt` writes `Int32`).

## Nested Display

`progs_nested_display` formats 200,000 orders. Each contains three lines,
with a money amount in each line. The three `Show.to_string` implementations
mirror hd's nested `Display` interpolation; the JS twin uses `toString`.
Every character is ASCII. All three fold the complete text into the same
rolling checksum, `563962784`.

The benchmark is registered in both hd runners and this comparison:

```sh
node bench/runtime/run.ts target/bench-nested-display-runtime --release --hd target/debug/hd --only nested-display
node bench/study/run.ts --only progs/nested-display --engines node --hd target/debug/hd --work target/bench-nested-display-study
node bench/study/moonbit/compare.ts --build --only progs_nested_display --hd target/debug/hd
node bench/study/moonbit/compare.ts --only progs_nested_display --runs 5
```

`--hd` selects an existing compiler, including a dev compiler. `--only` also
limits the comparison's build step; it leaves other built programs in place.
