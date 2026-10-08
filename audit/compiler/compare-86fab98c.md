# Side-by-side: TS prototype versus new compiler (`86fab98c`)

Date: 2026-10-07. Report only; no behavior changed. Measured at
`86fab98c` (origin/main) with the rerunnable script
`compiler/bench/compare/run.mjs` (`node compiler/bench/compare/run.mjs [WORK_DIR]`).
TS is `node bin/hd.js` from the repo (`src/`); new is
`compiler/target/release/hd` built from this commit
(`cargo build --release -p hd_cli` in `compiler/`, 8 s).

## Method

Idle machine, `uptime` load 7.05 at start and 4.81 at end (8 users, no
other full check running). 5 runs each; p50 = median of 5, p95 = max of
5. Cold TS = `rm -rf build` before each run; warm TS = build dir reused
after one warmup. Cold new = fresh empty `HD_CACHE` per run; warm new =
shared `HD_CACHE` after one warmup. All times are wall ms of one CLI
process, including Node startup for TS. Inputs both accept: `tiny.hd`
(the footprint tiny program), `mid.hd` (the `bench/math` kernel fixed
with `let` reassignables and `pub fn main() -> void $ Console`), and
`test1/30/300.hd` (1, 30, 300 `assert_equal` cases). `calc.hd` is
`examples/dogfood/calc.hd` (248 lines), TS-only. The checked-in
`bench/tokenizer|math|records|traits` programs fail on both (see
Skips), so `mid.hd` stands in for them.

## Startup (nearest no-op; new has no `--version`)

| Compiler | Command | p50 / p95 (ms) |
| --- | --- | ---: |
| TS | `hd help` | 76 / 83 |
| new | `hd --help` | 2 / 3 |

## `hd check` (TS only; new has no `check` command)

| Input | Cold p50 / p95 (ms) | Warm p50 / p95 (ms) |
| --- | ---: | ---: |
| tiny | 351 / 360 | 347 / 383 |
| mid | 352 / 356 | 349 / 352 |
| calc | 542 / 570 | 538 / 557 |

## `hd build`

| Input | TS cold | TS warm | new cold | new warm |
| --- | ---: | ---: | ---: | ---: |
| tiny | 672 / 678 | 648 / 657 | 46 / 47 | 8 / 8 |
| mid | 686 / 759 | 676 / 701 | 48 / 62 | 8 / 8 |
| calc | 895 / 939 (TS only) | n/a | fails (see Skips) | — |

## `hd run` (bare FILE / `run FILE`; same sources, same stdout)

| Input | TS cold | TS warm | new cold | new warm |
| --- | ---: | ---: | ---: | ---: |
| tiny | 664 / 677 | 635 / 649 | 78 / 98 | 40 / 50 |
| mid | 641 / 647 | 643 / 651 | 75 / 112 | 39 / 40 |
| calc | 897 / 930 (TS only) | n/a | fails (see Skips) | — |

`tiny` and `mid` print identical stdout on both compilers
(`ready`; `21, 832040, 5831, 483152`).

## `hd test` (package; total ms, per-case ms)

| Cases | TS total | TS per-case | new total | new per-case |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 963 / 983 | 963.2 / 982.8 | 38 / 41 | 37.5 / 40.6 |
| 30 | 1004 / 1053 | 33.5 / 35.1 | 52 / 55 | 1.7 / 1.8 |
| 300 | 1284 / 1313 | 4.3 / 4.4 | 69 / 75 | 0.2 / 0.2 |

Both report all cases passed (TS `N passed`; new `N passed; 0 failed; 0 ignored; 0 unsupported`).

## Skips

- New has no `check` and no `--version` (bare `--help` prints the four-form usage, exit 0).
- New rejects `calc.hd`: `unknown-import it_prop in std.testing` plus `unsupported: Body: a list spread`.
- New rejects `bench/tokenizer` and `bench/math`: `missing-requirement: this needs $ Console` (their `fn main():` predates the effect row).
- New rejects `bench/records` and `bench/traits`: `unknown-module src.domain.readings` / `src.metrics.shapes` (the `pkg.*` layout is not a package it discovers).
- TS rejects `bench/tokenizer` and `bench/math` on check: `non-reassignable-binding` (`:=` bindings are not reassignable) plus a `private-main` warning.
- TS rejects `bench/records` and `bench/traits` as single files: `unknown-module: 'pkg' names no module` (a single-file program may use only std); TS `build FILE` outside a package is an error, so build/run comparisons use package dirs.
- Compared inputs are exactly the intersection both accept; everything else is TS-only (`calc.hd`) or broken on both (checked-in bench programs).

## Conclusion

- The new compiler is 8-80x faster per command on shared inputs (build 46 vs 672 ms cold, 8 vs 648 ms warm; test-300 total 69 vs 1284 ms), with startup 2 vs 76 ms.
- TS time is flat across input size (check 351 vs 352 ms tiny/vs/mid; run ~640 ms) because Node startup and the TS pipeline dominate; new scales with work but stays under 120 ms everywhere measured.
- The comparison covers only the small shared subset: `calc.hd` and the checked-in bench programs run nowhere side-by-side until the new compiler gains `it_prop`/spread/`$ Console` tolerance and the bench inputs are fixed for `:=`.
