# Daily goal metrics

Finished: 2026-11-10T19:44:31.501Z. Commit: 1c3f29ac31e762d4857aaa5f5b26caacfcdbfa4a.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V45 96-Core Processor).
Compiler: target/release/hd.

97 passed, 34 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 40 ms → pass 34 ms
- edit-latency: p95 check after an edit: pass 71 ms → pass 36 ms
- cold-check: median cold check, 10k lines: pass 176 ms → pass 143 ms
- test-latency: median edit-then-test: fail 338 ms → pass 279 ms
- pathological: overlapping-from: time at 4x: pass 164 ms → pass 82 ms
- pathological: overlapping-from: growth exponent: fail 6.69 → pass noise
- pathological: deep-parens: time at 4x: pass 25 ms → pass 21 ms
- pathological: method-chain: time at 4x: pass 25 ms → pass 22 ms
- pathological: wide-list-literal: time at 4x: pass 45 ms → pass 35 ms
- pathological: large-enum-match: time at 4x: pass 31 ms → pass 27 ms
- pathological: deep-generic: time at 4x: pass 24 ms → pass 48 ms
- pathological: big-file: time at 4x: pass 160 ms → pass 125 ms
- fmt: hd fmt, 10k lines: pass 27 ms → pass 18 ms
- dev-speed: geomean dev / optimized: pass 0.96x → pass 0.76x
- parallel-speedup: speedup, 50k lines, 4 threads vs 1: fail 1.93x → fail 2.21x
- proptest-perf: property cases per second: fail 38k/s → fail 67k/s
- proptest-perf: time to shrink a known failure: pass 76 ms → pass 64 ms
- unit-test-perf: 1,000 unit tests, warm: fail 1225 ms → fail 1077 ms
- integration-test-perf: setup per integration test program: fail 360 ms → fail 318 ms
- integration-test-perf: cost per doc test: fail 259 ms → fail 221 ms
- runtime: string-build: hd / Node: pass 0.46x → pass 0.58x
- runtime: sort: hd / Node: pass 0.57x → pass 0.78x
- allocations: counted loop, bytes per iteration: pass 0.01 B/iter → pass 0.00 B/iter
- allocations: map loop, bytes per iteration: pass 0.02 B/iter → pass 0.00 B/iter
- size-startup-heap: tiny program, start to first output: fail 136 ms → fail 113 ms
- size-startup-heap: peak heap, map: hd / Node: pass 0.79x → pass 0.88x
- size-startup-heap: peak heap, string-build: hd / Node: pass 0.13x → pass 0.19x
- host-call-overhead: Console println, per call: fail 7.1 µs → fail 6.0 µs
- host-call-overhead: Fs read_text!, per call: fail 18 µs → fail 15 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 32 µs → fail 26 µs
- suspension-overhead: await of a ready value, per await: pass 11 ns → pass 3.8 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 14.1M/s → pass 16.6M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 10.8M/s → pass 13.3M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.26x → fail 0.33x
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.26x → fail 0.32x
- text-throughput: string build, hd / Node MB/s: pass 4.27x → pass 6.86x
- text-throughput: split, hd / Node MB/s: pass 2.60x → pass 4.27x
- text-throughput: regex find_all, hd / Node MB/s: fail 0.09x → fail 0.10x

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 34 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 36 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 143 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 279 ms | ≤ 300 ms | PASS |  |
| mistakes | programs with exactly one diagnostic | 98.6% | ≥ 95.0% | PASS | 68 of 69 |
| mistakes | one diagnostic with the expected code | 98.6% | ≥ 95.0% | PASS | 68 of 69; first misses: some-on-result |
| mistakes | fix-it makes hd check pass | 33.3% | ≥ 80.0% | FAIL | 23 of 69 resolved; 23 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 92 tokens | ≤ 60 tokens | FAIL | largest: some-on-result; median 43 tokens, 171 B; 66 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 82 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 25.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 72 → 82 ms, start-up 34 ms |
| pathological | deep-blocks: time at 4x | 24 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-blocks: peak RSS at 4x | 20.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 24 ms, start-up 34 ms |
| pathological | deep-parens: time at 4x | 21 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 19.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 18 → 21 ms, start-up 34 ms |
| pathological | method-chain: time at 4x | 22 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 22.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 22 ms, start-up 34 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 35 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 23.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 27 → 35 ms, start-up 34 ms |
| pathological | large-enum-match: time at 4x | 27 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 23.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 29 → 27 ms, start-up 34 ms |
| pathological | deep-generic: time at 4x | 48 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 20.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 21 → 48 ms, start-up 34 ms |
| pathological | question-chain: time at 4x | 26 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | question-chain: peak RSS at 4x | 23.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 134 → 26 ms, start-up 34 ms |
| pathological | big-file: time at 4x | 125 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 52.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 68 → 125 ms, start-up 34 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 23 of 23 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 18 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.98x | ≤ 1.30x | FAIL | checked 472 ms, unchecked 239 ms |
| dev-speed | geomean dev / optimized | 0.76x | ≤ 4.00x | PASS | fib 0.83x, sum 0.89x, string-build 0.29x, map 0.90x, sort 1.31x |
| dev-speed | worst case dev / optimized | 1.31x | ≤ 10.00x | PASS | sort |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 30 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 25.9 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.11 CPU-s, 34.7 MB | completes ≤ 30 s | PASS | wall 81 ms |
| resources | check, small, warm | 0.00 CPU-s, 17.9 MB | completes ≤ 30 s | PASS | wall 15 ms |
| resources | test, small, cold | 2.78 CPU-s, 124.4 MB | completes ≤ 30 s | PASS | wall 1066 ms |
| resources | test, small, warm | 2.16 CPU-s, 125.4 MB | completes ≤ 30 s | PASS | wall 763 ms |
| resources | build, small, cold | 0.54 CPU-s, 81.2 MB | completes ≤ 30 s | PASS | wall 222 ms |
| resources | build, small, warm | 0.02 CPU-s, 31.2 MB | completes ≤ 30 s | PASS | wall 25 ms |
| resources | check, 10k, cold | 0.30 CPU-s, 57.6 MB | completes ≤ 30 s | PASS | wall 142 ms |
| resources | check, 10k, warm | 0.03 CPU-s, 25.9 MB | completes ≤ 30 s | PASS | wall 29 ms |
| resources | test, 10k, cold | 3.30 CPU-s, 131.8 MB | completes ≤ 30 s | PASS | wall 1282 ms |
| resources | test, 10k, warm | 2.33 CPU-s, 130.7 MB | completes ≤ 30 s | PASS | wall 832 ms |
| resources | build, 10k, cold | 0.76 CPU-s, 101.9 MB | completes ≤ 30 s | PASS | wall 280 ms |
| resources | build, 10k, warm | 0.03 CPU-s, 34.4 MB | completes ≤ 30 s | PASS | wall 38 ms |
| resources | check, 50k, cold | 1.22 CPU-s, 161.1 MB | completes ≤ 150 s | PASS | wall 511 ms |
| resources | check, 50k, warm | 0.11 CPU-s, 56.1 MB | completes ≤ 150 s | PASS | wall 111 ms |
| resources | test, 50k, cold | 6.05 CPU-s, 275.0 MB | completes ≤ 150 s | PASS | wall 2688 ms |
| resources | test, 50k, warm | 3.58 CPU-s, 166.6 MB | completes ≤ 150 s | PASS | wall 1261 ms |
| resources | build, 50k, cold | 1.64 CPU-s, 205.9 MB | completes ≤ 150 s | PASS | wall 581 ms |
| resources | build, 50k, warm | 0.11 CPU-s, 65.4 MB | completes ≤ 150 s | PASS | wall 98 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 83.8 MB | ≤ 22.0 MB | FAIL | RSS 95.6 MB at input 100, 179.4 MB at input 1000; build per input 63 ms at 50, 542 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.1 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.8 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 13 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 16.6 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.06x | ≤ 1.50x | FAIL | p95 84 ms at N = 1; 4 cores; total CPU 640 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.99x | ≤ 6.00x | FAIL | p95 84 ms at N = 1; 4 cores; total CPU 2800 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.14 | &lt; 1.00 (sublinear) | FAIL | 120 ms at N = 1, 2800 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 19.1 MB | ≤ 10.0 MB | FAIL | artifacts 7802 B in 10 files; toolchain 19.0 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 10.8 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 2.21x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 9 files | the files of one check's cache | FAIL | one check alone: 7 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 4.4 MB | ≤ 5.0 MB | PASS | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 79 | ≤ 1 | FAIL | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 67k/s | ≥ 100k/s | FAIL | 6000 extra cases took 89 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 64 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 1077 ms | ≤ 1000 ms | FAIL | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.6 ms | ≤ 1.0 ms | PASS | 100 tests: 577 ms |
| integration-test-perf | setup per integration test program | 318 ms | ≤ 20 ms | FAIL | 10 programs: 3392 ms for 21 tests, 162 ms per test |
| integration-test-perf | total time growth in programs | 1.006 | exponent &lt; 1 (sublinear) | FAIL | beyond the 0-program run: 626 ms at 2, 3160 ms at 10 programs |
| integration-test-perf | cost per doc test | 221 ms | ≤ 20 ms | FAIL | 10 doc tests: 2452 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | - | ≤ 1.50x | FAIL | 1 of 5 cases not measured |
| runtime | fib: hd / Node | - | ≤ 3.00x | FAIL | no work measured beyond start-up; hd -3.3 ms (p10 -8.6, p90 11), Node 5.5 ms, Python 51 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 1.43x | ≤ 3.00x | PASS | hd 9.5 ms (p10 -13.1, p90 17), Node 6.6 ms, Python 176 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.58x | ≤ 3.00x | PASS | hd 3.9 ms (p10 -3.8, p90 10), Node 6.7 ms, Python 8.3 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 3.75x | ≤ 3.00x | FAIL | hd 27 ms (p10 22, p90 36), Node 7.2 ms, Python 11 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.78x | ≤ 3.00x | PASS | hd 12 ms (p10 8.4, p90 38), Node 16 ms, Python 10 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 1627 B | ≤ 2048 B | PASS | benchmarks: fib 5414 B, sum 5226 B, string-build 8603 B, map 6631 B, sort 13928 B |
| size-startup-heap | tiny program, start to first output | 113 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.88x | ≤ 2.00x | PASS | hd +12.5 MB, Node +14.2 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.19x | ≤ 2.00x | PASS | hd +3.9 MB, Node +20.9 MB over an empty program |
| host-call-overhead | Console println, per call | 6.0 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 15 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 26 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 3.8 ns | ≤ 100 ns | PASS | 3.8 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 16.6M/s | ≥ 1.0M/s | PASS | 120 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 13.3M/s | ≥ 1.0M/s | PASS | 151 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.33x | ≥ 0.50x | FAIL | hd 166 MB/s (45488 B document), Node 506 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.32x | ≥ 0.50x | FAIL | hd 79 MB/s (45488 B document), Node 247 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 6.86x | ≥ 0.50x | PASS | hd 1039 MB/s, Node 152 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 4.27x | ≥ 0.50x | PASS | hd 1511 MB/s, Node 354 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.10x | ≥ 0.50x | FAIL | hd 11 MB/s, Node 106 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 11002 B | ≤ 10240 B | FAIL | 17443 B at 1075 lines, 116475 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 908 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 27790000 requests; RSS 99.4 MB after warm-up, 99.5 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 7249 of 7249 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
- [2026-10-28T03](daily/2026-10-28T03.json)
