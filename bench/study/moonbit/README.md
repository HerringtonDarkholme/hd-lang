# hd vs MoonBit (linear-memory wasm backend)

MoonBit ports of the 18 hd benchmark programs (11 in `bench/study`, 7 in
`bench/runtime/progs`) plus `empty`, and a runner that compares hd's release
Wasm with `moon build --target wasm` output. Same algorithm, sizes and stdout
as the hd source; `compare.mjs` fails if the two stdouts differ.

Packages: `study_<name>` (from `bench/study/<name>.hd`), `progs_<name>` (from
`bench/runtime/progs/<name>.hd`, dashes become underscores), `empty`.

Versions measured: moon 0.1.20261023 (3778465 2026-10-23), node v24.19.0.

## Build

```sh
cargo build --release -p hd_cli -j 4            # target/release/hd
node bench/study/moonbit/compare.mjs --build    # hd packages into work/, then moon build
# the MoonBit half alone:
cd bench/study/moonbit && /Users/hd/.moon/bin/moon build --target wasm --release
```

## Run

```sh
node bench/study/moonbit/compare.mjs                        # 11 runs, all programs
node bench/study/moonbit/compare.mjs --runs 5 --only study_json,empty
```

Each run is a fresh `node` process; one discarded warm-up, then `--runs`
measured runs with hd and MoonBit interleaved, at two V8 tiers (default and
`--no-liftoff`). Run time is `performance.now()` around the entry call after
instantiation (hd: `host.run(instance, "hd.init", "hd.poll")` on hd's own
`host/node.mjs`; MoonBit: `_start()`). Wall time is the whole process.
The MoonBit host is only `wasi_snapshot_preview1.fd_write`. Medians are
printed as markdown tables with the geometric mean of hd/MBT (empty excluded)
and the load average.

Ports use core `Array`/`Map`/`StringBuilder`, `for .. in` ranges and
iterators. `moon fmt` rewrites `Int` as `Int32`; they are the same type.
(`moon fmt` writes `Int32`).
