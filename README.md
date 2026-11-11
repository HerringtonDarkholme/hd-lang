# Daily goal metrics

Finished: 2026-11-11T03:47:38.044Z. Commit: 3374ec3d785dffd4e0c9753d53c53c37363966f1.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V74 80-Core Processor).
Compiler: target/release/hd.

96 passed, 35 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 34 ms → pass 41 ms
- edit-latency: p95 check after an edit: pass 36 ms → pass 44 ms
- cold-check: median cold check, 10k lines: pass 143 ms → pass 178 ms
- test-latency: median edit-then-test: pass 279 ms → fail 347 ms
- pathological: overlapping-from: time at 4x: pass 82 ms → pass 53 ms
- pathological: deep-blocks: time at 4x: pass 24 ms → pass 39 ms
- pathological: wide-list-literal: time at 4x: pass 35 ms → pass 44 ms
- pathological: large-enum-match: time at 4x: pass 27 ms → pass 30 ms
- pathological: deep-generic: time at 4x: pass 48 ms → pass 24 ms
- pathological: big-file: time at 4x: pass 125 ms → pass 158 ms
- fmt: hd fmt, 10k lines: pass 18 ms → pass 24 ms
- dev-speed: geomean dev / optimized: pass 0.76x → fail -
- dev-speed: worst case dev / optimized: pass 1.31x → fail -
- long-session: RSS growth, REPL inputs 100 to 1000: fail 83.8 MB → fail 94.6 MB
- concurrency: p95 latency at N = 4 vs N = 1: fail 2.06x → fail 2.34x
- concurrency: p95 latency at N = 16 vs N = 1: fail 8.99x → fail 28.04x
- parallel-speedup: speedup, 50k lines, 4 threads vs 1: fail 2.21x → fail 1.97x
- proptest-perf: property cases per second: fail 67k/s → fail 41k/s
- proptest-perf: time to shrink a known failure: pass 64 ms → pass 86 ms
- unit-test-perf: 1,000 unit tests, warm: fail 1077 ms → fail 1266 ms
- unit-test-perf: overhead per unit test: pass 0.6 ms → pass 0.7 ms
- integration-test-perf: setup per integration test program: fail 318 ms → fail 362 ms
- integration-test-perf: total time growth in programs: fail 1.006 → pass 0.995
- integration-test-perf: cost per doc test: fail 221 ms → fail 258 ms
- runtime: geomean hd / Node: fail - → pass 0.63x
- runtime: fib: hd / Node: fail - → pass 0.05x
- runtime: sum: hd / Node: pass 1.43x → pass 2.15x
- runtime: string-build: hd / Node: pass 0.58x → pass 0.31x
- runtime: map: hd / Node: fail 3.75x → fail 4.15x
- allocations: list loop, bytes per iteration: pass 0.00 B/iter → pass 0.02 B/iter
- allocations: map loop, bytes per iteration: pass 0.00 B/iter → pass 0.02 B/iter
- size-startup-heap: tiny program, start to first output: fail 113 ms → fail 131 ms
- size-startup-heap: peak heap, string-build: hd / Node: pass 0.19x → pass 0.21x
- host-call-overhead: Console println, per call: fail 6.0 µs → fail 6.9 µs
- host-call-overhead: Fs read_text!, per call: fail 15 µs → fail 18 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 26 µs → fail 33 µs
- suspension-overhead: await of a ready value, per await: pass 3.8 ns → pass 1.6 ns
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.33x → fail 0.24x
- text-throughput: string build, hd / Node MB/s: pass 6.86x → pass 4.21x
- text-throughput: split, hd / Node MB/s: pass 4.27x → pass 2.16x
- conformance: pass rate, known failures excluded: pass 100.0% → fail 100.0%

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 41 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 44 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 178 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 347 ms | ≤ 300 ms | FAIL |  |
| mistakes | programs with exactly one diagnostic | 98.6% | ≥ 95.0% | PASS | 68 of 69 |
| mistakes | one diagnostic with the expected code | 98.6% | ≥ 95.0% | PASS | 68 of 69; first misses: some-on-result |
| mistakes | fix-it makes hd check pass | 33.3% | ≥ 80.0% | FAIL | 23 of 69 resolved; 23 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 92 tokens | ≤ 60 tokens | FAIL | largest: some-on-result; median 43 tokens, 171 B; 66 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 53 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 25.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 39 → 53 ms, start-up 20 ms |
| pathological | deep-blocks: time at 4x | 39 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 20.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 39 ms, start-up 20 ms |
| pathological | deep-parens: time at 4x | 23 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-parens: peak RSS at 4x | 20.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 23 ms, start-up 20 ms |
| pathological | method-chain: time at 4x | 24 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | method-chain: peak RSS at 4x | 21.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 24 ms, start-up 20 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 44 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 24.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 31 → 44 ms, start-up 20 ms |
| pathological | large-enum-match: time at 4x | 30 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 23.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 33 → 30 ms, start-up 20 ms |
| pathological | deep-generic: time at 4x | 24 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 20.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 21 → 24 ms, start-up 20 ms |
| pathological | question-chain: time at 4x | 28 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | question-chain: peak RSS at 4x | 23.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 28 → 28 ms, start-up 20 ms |
| pathological | big-file: time at 4x | 158 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 52.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.15 | ≤ 1.30 | PASS | 2x→4x; 82 → 158 ms, start-up 20 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 23 of 23 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 24 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 2.07x | ≤ 1.30x | FAIL | checked 659 ms, unchecked 318 ms |
| dev-speed | geomean dev / optimized | - | ≤ 4.00x | FAIL | optimized work is within noise: string-build (38.0 ms) |
| dev-speed | worst case dev / optimized | - | ≤ 10.00x | FAIL | optimized work is within noise: string-build (38.0 ms) |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 30 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 25.3 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.15 CPU-s, 34.7 MB | completes ≤ 30 s | PASS | wall 96 ms |
| resources | check, small, warm | 0.01 CPU-s, 18.2 MB | completes ≤ 30 s | PASS | wall 17 ms |
| resources | test, small, cold | 3.19 CPU-s, 125.3 MB | completes ≤ 30 s | PASS | wall 1226 ms |
| resources | test, small, warm | 2.47 CPU-s, 121.5 MB | completes ≤ 30 s | PASS | wall 906 ms |
| resources | build, small, cold | 0.72 CPU-s, 80.1 MB | completes ≤ 30 s | PASS | wall 278 ms |
| resources | build, small, warm | 0.02 CPU-s, 30.9 MB | completes ≤ 30 s | PASS | wall 28 ms |
| resources | check, 10k, cold | 0.39 CPU-s, 56.9 MB | completes ≤ 30 s | PASS | wall 182 ms |
| resources | check, 10k, warm | 0.03 CPU-s, 25.3 MB | completes ≤ 30 s | PASS | wall 35 ms |
| resources | test, 10k, cold | 3.84 CPU-s, 131.1 MB | completes ≤ 30 s | PASS | wall 1511 ms |
| resources | test, 10k, warm | 2.69 CPU-s, 131.0 MB | completes ≤ 30 s | PASS | wall 1076 ms |
| resources | build, 10k, cold | 0.97 CPU-s, 102.0 MB | completes ≤ 30 s | PASS | wall 343 ms |
| resources | build, 10k, warm | 0.04 CPU-s, 34.6 MB | completes ≤ 30 s | PASS | wall 45 ms |
| resources | check, 50k, cold | 1.51 CPU-s, 159.9 MB | completes ≤ 150 s | PASS | wall 582 ms |
| resources | check, 50k, warm | 0.13 CPU-s, 55.3 MB | completes ≤ 150 s | PASS | wall 113 ms |
| resources | test, 50k, cold | 7.74 CPU-s, 273.7 MB | completes ≤ 150 s | PASS | wall 3572 ms |
| resources | test, 50k, warm | 4.27 CPU-s, 169.1 MB | completes ≤ 150 s | PASS | wall 1722 ms |
| resources | build, 50k, cold | 2.14 CPU-s, 204.7 MB | completes ≤ 150 s | PASS | wall 739 ms |
| resources | build, 50k, warm | 0.14 CPU-s, 64.6 MB | completes ≤ 150 s | PASS | wall 125 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 94.6 MB | ≤ 22.0 MB | FAIL | RSS 93.2 MB at input 100, 187.8 MB at input 1000; build per input 75 ms at 50, 759 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.1 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.9 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 14 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 16.3 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.34x | ≤ 1.50x | FAIL | p95 97 ms at N = 1; 4 cores; total CPU 740 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 28.04x | ≤ 6.00x | FAIL | p95 97 ms at N = 1; 4 cores; total CPU 3160 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.10 | &lt; 1.00 (sublinear) | FAIL | 150 ms at N = 1, 3160 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 19.2 MB | ≤ 10.0 MB | FAIL | artifacts 7802 B in 10 files; toolchain 19.2 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 10.8 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.97x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 9 files | the files of one check's cache | FAIL | one check alone: 7 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 4.5 MB | ≤ 5.0 MB | PASS | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 79 | ≤ 1 | FAIL | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 41k/s | ≥ 100k/s | FAIL | 6000 extra cases took 147 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 86 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 1266 ms | ≤ 1000 ms | FAIL | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.7 ms | ≤ 1.0 ms | PASS | 100 tests: 677 ms |
| integration-test-perf | setup per integration test program | 362 ms | ≤ 20 ms | FAIL | 10 programs: 3897 ms for 21 tests, 186 ms per test |
| integration-test-perf | total time growth in programs | 0.995 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 730 ms at 2, 3622 ms at 10 programs |
| integration-test-perf | cost per doc test | 258 ms | ≤ 20 ms | FAIL | 10 doc tests: 2863 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 0.63x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.05x | ≤ 3.00x | PASS | hd 0.4 ms (p10 -1.1, p90 10), Node 8.5 ms, Python 76 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 2.15x | ≤ 3.00x | PASS | hd 18 ms (p10 0.2, p90 43), Node 8.2 ms, Python 238 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.31x | ≤ 3.00x | PASS | hd 2.9 ms (p10 -2.8, p90 8.1), Node 9.3 ms, Python 12 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 4.15x | ≤ 3.00x | FAIL | hd 38 ms (p10 25, p90 45), Node 9.0 ms, Python 14 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.76x | ≤ 3.00x | PASS | hd 15 ms (p10 3.8, p90 24), Node 19 ms, Python 15 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.02 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.02 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 1627 B | ≤ 2048 B | PASS | benchmarks: fib 5411 B, sum 5223 B, string-build 8570 B, map 6628 B, sort 13881 B |
| size-startup-heap | tiny program, start to first output | 131 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.91x | ≤ 2.00x | PASS | hd +13.0 MB, Node +14.3 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.21x | ≤ 2.00x | PASS | hd +4.3 MB, Node +20.8 MB over an empty program |
| host-call-overhead | Console println, per call | 6.9 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 18 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 33 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 1.6 ns | ≤ 100 ns | PASS | 1.6 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 15.9M/s | ≥ 1.0M/s | PASS | 126 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 12.8M/s | ≥ 1.0M/s | PASS | 156 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.24x | ≥ 0.50x | FAIL | hd 90 MB/s (45488 B document), Node 367 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.30x | ≥ 0.50x | FAIL | hd 55 MB/s (45488 B document), Node 183 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 4.21x | ≥ 0.50x | PASS | hd 494 MB/s, Node 117 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 2.16x | ≥ 0.50x | PASS | hd 610 MB/s, Node 283 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.10x | ≥ 0.50x | FAIL | hd 8.3 MB/s, Node 83 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 10956 B | ≤ 10240 B | FAIL | 17395 B at 1075 lines, 116013 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 908 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 21922000 requests; RSS 99.4 MB after warm-up, 99.4 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | FAIL | 7346 of 7347 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

- [2026-11-11T03](daily/2026-11-11T03.json)
- [2026-11-10T19](daily/2026-11-10T19.json)
- [2026-11-10T05](daily/2026-11-10T05.json)
- [2026-11-09T22](daily/2026-11-09T22.json)
- [2026-11-09T03](daily/2026-11-09T03.json)
- [2026-11-08T18](daily/2026-11-08T18.json)
- [2026-11-08T03](daily/2026-11-08T03.json)
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
