# Daily goal metrics

Finished: 2026-11-07T17:57:06.998Z. Commit: 453b07fdce755f754a3850a47fef6f0e2e4a1bf3.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V74 80-Core Processor).
Compiler: target/release/hd.

99 passed, 32 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- test-latency: median edit-then-test: fail 320 ms → pass 297 ms
- pathological: big-file: growth exponent: fail 1.36 → pass 1.17
- proptest-perf: property cases per second: fail 35k/s → fail 48k/s
- unit-test-perf: overhead per unit test: pass 0.6 ms → pass 0.7 ms
- integration-test-perf: total time growth in programs: fail 1.006 → pass 0.994
- runtime: geomean hd / Node: pass 0.63x → pass 0.49x
- runtime: fib: hd / Node: pass 0.58x → pass 0.03x
- runtime: sum: hd / Node: pass 2.25x → pass 1.06x
- runtime: string-build: hd / Node: pass 0.04x → pass 0.63x
- runtime: sort: hd / Node: pass 0.64x → pass 0.41x
- size-startup-heap: peak heap, map: hd / Node: pass 0.79x → pass 0.93x
- size-startup-heap: peak heap, string-build: hd / Node: pass 0.21x → pass 0.28x
- suspension-overhead: await of a ready value, per await: pass 14 ns → pass 20 ns
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.22x → fail 0.27x
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.26x → fail 0.30x
- text-throughput: string build, hd / Node MB/s: pass 4.06x → pass 4.77x
- text-throughput: split, hd / Node MB/s: pass 3.00x → pass 1.83x

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 41 ms | ≤ 50 ms | PASS | Timing change ≤10%: noise. |
| edit-latency | p95 check after an edit | 43 ms | ≤ 200 ms | PASS | Timing change ≤10%: noise. |
| cold-check | median cold check, 10k lines | 174 ms | ≤ 1000 ms | PASS | Timing change ≤10%: noise. |
| test-latency | median edit-then-test | 297 ms | ≤ 300 ms | PASS | Timing change ≤10%: noise. |
| mistakes | programs with exactly one diagnostic | 95.7% | ≥ 95.0% | PASS | 66 of 69 |
| mistakes | one diagnostic with the expected code | 95.7% | ≥ 95.0% | PASS | 66 of 69; first misses: mut-on-binding-type, mut-on-parameter-type, some-on-result |
| mistakes | fix-it makes hd check pass | 33.3% | ≥ 80.0% | FAIL | 23 of 69 resolved; 23 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 93 tokens | ≤ 60 tokens | FAIL | largest: mut-on-parameter-type; median 43 tokens, 171 B; 64 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 46 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | overlapping-from: peak RSS at 4x | 25.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 33 → 46 ms, start-up 18 ms |
| pathological | deep-blocks: time at 4x | 19 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-blocks: peak RSS at 4x | 19.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 18 → 19 ms, start-up 18 ms |
| pathological | deep-parens: time at 4x | 18 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-parens: peak RSS at 4x | 19.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 18 → 18 ms, start-up 18 ms |
| pathological | method-chain: time at 4x | 23 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | method-chain: peak RSS at 4x | 21.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 23 ms, start-up 18 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 45 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | wide-list-literal: peak RSS at 4x | 23.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 32 → 45 ms, start-up 18 ms |
| pathological | large-enum-match: time at 4x | 29 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | large-enum-match: peak RSS at 4x | 22.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 23 → 29 ms, start-up 18 ms |
| pathological | deep-generic: time at 4x | 21 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-generic: peak RSS at 4x | 19.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 21 ms, start-up 18 ms |
| pathological | question-chain: time at 4x | 26 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | question-chain: peak RSS at 4x | 22.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 26 ms, start-up 18 ms |
| pathological | big-file: time at 4x | 164 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | big-file: peak RSS at 4x | 53.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.17 | ≤ 1.30 | PASS | 2x→4x; 83 → 164 ms, start-up 18 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 23 of 23 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 23 ms | ≤ 200 ms | PASS | Timing change ≤10%: noise. |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.90x | ≤ 1.30x | FAIL | checked 653 ms, unchecked 344 ms |
| dev-speed | geomean dev / optimized | - | ≤ 4.00x | FAIL | optimized work is within noise: string-build (34.9 ms) |
| dev-speed | worst case dev / optimized | - | ≤ 10.00x | FAIL | optimized work is within noise: string-build (34.9 ms) |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 40 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 27.2 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.13 CPU-s, 35.1 MB | completes ≤ 30 s | PASS | wall 93 ms |
| resources | check, small, warm | 0.00 CPU-s, 17.9 MB | completes ≤ 30 s | PASS | wall 16 ms |
| resources | test, small, cold | 2.86 CPU-s, 120.8 MB | completes ≤ 30 s | PASS | wall 1070 ms |
| resources | test, small, warm | 2.15 CPU-s, 119.6 MB | completes ≤ 30 s | PASS | wall 765 ms |
| resources | build, small, cold | 0.69 CPU-s, 81.2 MB | completes ≤ 30 s | PASS | wall 267 ms |
| resources | build, small, warm | 0.03 CPU-s, 31.3 MB | completes ≤ 30 s | PASS | wall 28 ms |
| resources | check, 10k, cold | 0.38 CPU-s, 57.3 MB | completes ≤ 30 s | PASS | wall 175 ms |
| resources | check, 10k, warm | 0.04 CPU-s, 27.2 MB | completes ≤ 30 s | PASS | wall 37 ms |
| resources | test, 10k, cold | 3.83 CPU-s, 129.3 MB | completes ≤ 30 s | PASS | wall 1686 ms |
| resources | test, 10k, warm | 2.61 CPU-s, 130.4 MB | completes ≤ 30 s | PASS | wall 1098 ms |
| resources | build, 10k, cold | 0.95 CPU-s, 101.3 MB | completes ≤ 30 s | PASS | wall 338 ms |
| resources | build, 10k, warm | 0.05 CPU-s, 36.5 MB | completes ≤ 30 s | PASS | wall 52 ms |
| resources | check, 50k, cold | 1.48 CPU-s, 163.8 MB | completes ≤ 150 s | PASS | wall 568 ms |
| resources | check, 50k, warm | 0.19 CPU-s, 67.7 MB | completes ≤ 150 s | PASS | wall 128 ms |
| resources | test, 50k, cold | 13.70 CPU-s, 276.4 MB | completes ≤ 150 s | PASS | wall 9775 ms |
| resources | test, 50k, warm | 10.66 CPU-s, 174.3 MB | completes ≤ 150 s | PASS | wall 8123 ms |
| resources | build, 50k, cold | 2.10 CPU-s, 209.1 MB | completes ≤ 150 s | PASS | wall 716 ms |
| resources | build, 50k, warm | 0.18 CPU-s, 76.9 MB | completes ≤ 150 s | PASS | wall 161 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 92.4 MB | ≤ 22.0 MB | FAIL | RSS 92.9 MB at input 100, 185.3 MB at input 1000; build per input 71 ms at 50, 723 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.2 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 4.0 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 15 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 16.0 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.07x | ≤ 1.50x | FAIL | p95 98 ms at N = 1; 4 cores; total CPU 750 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.61x | ≤ 6.00x | FAIL | p95 98 ms at N = 1; 4 cores; total CPU 3120 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.12 | &lt; 1.00 (sublinear) | FAIL | 140 ms at N = 1, 3120 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 18.4 MB | ≤ 10.0 MB | FAIL | artifacts 7847 B in 10 files; toolchain 18.3 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 11.1 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.91x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 3 files | the files of one check's cache | PASS | one check alone: 3 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 4.4 MB | ≤ 5.0 MB | PASS | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 79 | ≤ 1 | FAIL | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 48k/s | ≥ 100k/s | FAIL | 6000 extra cases took 126 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 81 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 Timing change ≤10%: noise. |
| unit-test-perf | 1,000 unit tests, warm | 1204 ms | ≤ 1000 ms | FAIL | the run reports 1000 of 1000 passed Timing change ≤10%: noise. |
| unit-test-perf | overhead per unit test | 0.7 ms | ≤ 1.0 ms | PASS | 100 tests: 588 ms |
| integration-test-perf | setup per integration test program | 319 ms | ≤ 20 ms | FAIL | 10 programs: 3465 ms for 21 tests, 165 ms per test Timing change ≤10%: noise. |
| integration-test-perf | total time growth in programs | 0.994 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 651 ms at 2, 3227 ms at 10 programs |
| integration-test-perf | cost per doc test | 225 ms | ≤ 20 ms | FAIL | 10 doc tests: 2490 ms; the run reports 11 of 11 passed Timing change ≤10%: noise. |
| runtime | geomean hd / Node | 0.49x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.03x | ≤ 3.00x | PASS | hd 0.2 ms (p10 -4.2, p90 6.7), Node 8.5 ms, Python 78 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 1.06x | ≤ 3.00x | PASS | hd 8.7 ms (p10 -3.4, p90 19), Node 8.2 ms, Python 238 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.63x | ≤ 3.00x | PASS | hd 5.9 ms (p10 0.8, p90 9.5), Node 9.3 ms, Python 11 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 3.61x | ≤ 3.00x | FAIL | hd 33 ms (p10 26, p90 37), Node 9.1 ms, Python 15 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.41x | ≤ 3.00x | PASS | hd 8.0 ms (p10 -2.8, p90 15), Node 19 ms, Python 15 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 1628 B | ≤ 2048 B | PASS | benchmarks: fib 5414 B, sum 5226 B, string-build 8628 B, map 6631 B, sort 13506 B |
| size-startup-heap | tiny program, start to first output | 120 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm Timing change ≤10%: noise. |
| size-startup-heap | peak heap, map: hd / Node | 0.93x | ≤ 2.00x | PASS | hd +13.3 MB, Node +14.2 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.28x | ≤ 2.00x | PASS | hd +5.9 MB, Node +20.8 MB over an empty program |
| host-call-overhead | Console println, per call | 6.8 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm Timing change ≤10%: noise. |
| host-call-overhead | Fs read_text!, per call | 18 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 32 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 20 ns | ≤ 100 ns | PASS | 20 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 13.7M/s | ≥ 1.0M/s | PASS | 146 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 10.6M/s | ≥ 1.0M/s | PASS | 188 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.27x | ≥ 0.50x | FAIL | hd 99 MB/s (45488 B document), Node 362 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.30x | ≥ 0.50x | FAIL | hd 55 MB/s (45488 B document), Node 182 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 4.77x | ≥ 0.50x | PASS | hd 496 MB/s, Node 104 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 1.83x | ≥ 0.50x | PASS | hd 604 MB/s, Node 331 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.11x | ≥ 0.50x | FAIL | hd 9.0 MB/s, Node 82 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 10676 B | ≤ 10240 B | FAIL | 17132 B at 1075 lines, 113231 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 908 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 24598000 requests; RSS 98.5 MB after warm-up, 98.5 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 6722 of 6722 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

- [2026-11-07T17](daily/2026-11-07T17.json)
- [2026-11-07T03](daily/2026-11-07T03.json)
- [2026-11-06T19](daily/2026-11-06T19.json)
- [2026-11-06T04](daily/2026-11-06T04.json)
- [2026-11-05T20](daily/2026-11-05T20.json)
- [2026-11-05T04](daily/2026-11-05T04.json)
- [2026-11-04T19](daily/2026-11-04T19.json)
- [2026-11-04T04](daily/2026-11-04T04.json)
- [2026-11-03T19](daily/2026-11-03T19.json)
- [2026-11-03T04](daily/2026-11-03T04.json)
- [2026-11-02T21](daily/2026-11-02T21.json)
- [2026-11-02T03](daily/2026-11-02T03.json)
- [2026-11-01T18](daily/2026-11-01T18.json)
- [2026-11-01T03](daily/2026-11-01T03.json)
- [2026-10-31T17](daily/2026-10-31T17.json)
- [2026-10-31T03](daily/2026-10-31T03.json)
- [2026-10-30T18](daily/2026-10-30T18.json)
- [2026-10-30T03](daily/2026-10-30T03.json)
- [2026-10-29T18](daily/2026-10-29T18.json)
- [2026-10-29T03](daily/2026-10-29T03.json)
- [2026-10-28T18](daily/2026-10-28T18.json)
- [2026-10-28T03](daily/2026-10-28T03.json)
- [2026-10-27T18](daily/2026-10-27T18.json)
- [2026-10-27T02](daily/2026-10-27T02.json)
- [2026-10-26T18](daily/2026-10-26T18.json)
- [2026-10-26T03](daily/2026-10-26T03.json)
- [2026-10-25](daily/2026-10-25.json)
