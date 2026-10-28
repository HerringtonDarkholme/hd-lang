# Daily goal metrics

Finished: 2026-10-28T18:30:28.389Z. Commit: 0d4fc192f2efac8d46417cb547164fbf7ec001d1.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 7763 64-Core Processor).
Compiler: target/release/hd.

97 passed, 34 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 47 ms → fail 74 ms
- edit-latency: p95 check after an edit: pass 48 ms → pass 76 ms
- cold-check: median cold check, 10k lines: pass 131 ms → pass 211 ms
- test-latency: median edit-then-test: pass 195 ms → pass 299 ms
- pathological: overlapping-from: time at 4x: pass 36 ms → pass 57 ms
- pathological: deep-blocks: time at 4x: pass 15 ms → pass 21 ms
- pathological: deep-parens: time at 4x: pass 14 ms → pass 20 ms
- pathological: wide-list-literal: time at 4x: pass 40 ms → pass 61 ms
- pathological: large-enum-match: time at 4x: pass 343 ms → pass 474 ms
- pathological: deep-generic: time at 4x: pass 16 ms → pass 25 ms
- pathological: question-chain: time at 4x: pass 20 ms → pass 30 ms
- pathological: big-file: time at 4x: pass 118 ms → pass 197 ms
- fmt: hd fmt, 10k lines: pass 19 ms → pass 33 ms
- check-cost: checked run time / unchecked run time: fail 3.45x → fail 2.80x
- resources: warm check, 10k lines: CPU time: pass 50 ms → pass 80 ms
- long-session: RSS growth, REPL inputs 100 to 1000: fail 106.6 MB → fail 120.3 MB
- startup: hd help (no version command): wall time: pass 2.8 ms → pass 4.6 ms
- startup: hd check empty.hd: wall time: pass 11 ms → pass 16 ms
- concurrency: p95 latency at N = 4 vs N = 1: fail 1.88x → fail 2.19x
- concurrency: p95 latency at N = 16 vs N = 1: fail 7.47x → fail 8.81x
- proptest-perf: property cases per second: fail 55k/s → fail 24k/s
- proptest-perf: time to shrink a known failure: pass 83 ms → pass 139 ms
- unit-test-perf: 1,000 unit tests, warm: pass 752 ms → fail 1184 ms
- unit-test-perf: overhead per unit test: pass 0.6 ms → fail 1.0 ms
- integration-test-perf: setup per integration test program: fail 99 ms → fail 157 ms
- integration-test-perf: cost per doc test: fail 96 ms → fail 141 ms
- runtime: fib: hd / Node: pass 0.60x → pass 0.50x
- runtime: sum: hd / Node: pass 1.47x → pass 2.08x
- runtime: sort: hd / Node: pass 1.05x → pass 1.34x
- size-startup-heap: tiny program, start to first output: fail 31 ms → fail 50 ms
- host-call-overhead: Console println, per call: fail 5.4 µs → fail 8.4 µs
- host-call-overhead: Fs read_text!, per call: fail 15 µs → fail 27 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 27 µs → fail 48 µs
- suspension-overhead: await of a ready value, per await: pass 10 ns → pass 28 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 16.3M/s → pass 9.9M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 13.3M/s → pass 6.8M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.05x → fail 0.06x
- long-run-memory: RSS growth after warm-up, 1 min: pass 0.4 MB → pass 2.9 MB
- conformance: listed known failures: fail 1 → pass 0

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 74 ms | ≤ 50 ms | FAIL |  |
| edit-latency | p95 check after an edit | 76 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 211 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 299 ms | ≤ 300 ms | PASS |  |
| mistakes | programs with exactly one diagnostic | 95.7% | ≥ 95.0% | PASS | 67 of 70 |
| mistakes | one diagnostic with the expected code | 95.7% | ≥ 95.0% | PASS | 67 of 70; first misses: mut-on-binding-type, mut-on-parameter-type, some-on-result |
| mistakes | fix-it makes hd check pass | 34.3% | ≥ 80.0% | FAIL | 24 of 70 resolved; 24 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 93 tokens | ≤ 60 tokens | FAIL | largest: mut-on-parameter-type; median 43 tokens, 171 B; 65 of 70 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 57 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 21.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 41 → 57 ms, start-up 20 ms |
| pathological | deep-blocks: time at 4x | 21 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 16.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 21 → 21 ms, start-up 20 ms |
| pathological | deep-parens: time at 4x | 20 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 16.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 20 ms, start-up 20 ms |
| pathological | method-chain: time at 4x | - | ≤ 2000 ms | FAIL | 4x: hd check failed: thread '&lt;unknown>' (7660) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 4x: hd check failed: thread '&lt;unknown>' (7660) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 1x→2x; 23 → 24 ms, start-up 20 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 61 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 19.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 49 → 61 ms, start-up 20 ms |
| pathological | large-enum-match: time at 4x | 474 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 18.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | 2.30 | ≤ 1.30 | FAIL | 2x→4x; 112 → 474 ms, start-up 20 ms |
| pathological | deep-generic: time at 4x | 25 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 16.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 25 ms, start-up 20 ms |
| pathological | question-chain: time at 4x | 30 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 19.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 26 → 30 ms, start-up 20 ms |
| pathological | big-file: time at 4x | 197 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 50.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.13 | ≤ 1.30 | PASS | 2x→4x; 101 → 197 ms, start-up 20 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 69 of 70; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 24 of 24 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 33 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 2.80x | ≤ 1.30x | FAIL | checked 1212 ms, unchecked 432 ms |
| dev-speed | geomean dev / optimized | 1.02x | ≤ 4.00x | PASS | fib 1.09x, sum 1.05x, string-build 0.92x, map 1.02x, sort 1.01x |
| dev-speed | worst case dev / optimized | 1.09x | ≤ 10.00x | PASS | fib |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 80 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 30.8 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.14 CPU-s, 27.9 MB | completes ≤ 30 s | PASS | wall 93 ms |
| resources | check, small, warm | 0.01 CPU-s, 16.3 MB | completes ≤ 30 s | PASS | wall 21 ms |
| resources | test, small, cold | 1.42 CPU-s, 84.6 MB | completes ≤ 30 s | PASS | wall 648 ms |
| resources | test, small, warm | 0.79 CPU-s, 69.0 MB | completes ≤ 30 s | PASS | wall 366 ms |
| resources | build, small, cold | 0.65 CPU-s, 84.4 MB | completes ≤ 30 s | PASS | wall 296 ms |
| resources | build, small, warm | 0.06 CPU-s, 37.1 MB | completes ≤ 30 s | PASS | wall 69 ms |
| resources | check, 10k, cold | 0.45 CPU-s, 54.4 MB | completes ≤ 30 s | PASS | wall 215 ms |
| resources | check, 10k, warm | 0.08 CPU-s, 30.8 MB | completes ≤ 30 s | PASS | wall 70 ms |
| resources | test, 10k, cold | 2.53 CPU-s, 109.8 MB | completes ≤ 30 s | PASS | wall 1336 ms |
| resources | test, 10k, warm | 1.48 CPU-s, 76.5 MB | completes ≤ 30 s | PASS | wall 925 ms |
| resources | build, 10k, cold | 0.99 CPU-s, 104.3 MB | completes ≤ 30 s | PASS | wall 400 ms |
| resources | build, 10k, warm | 0.12 CPU-s, 43.7 MB | completes ≤ 30 s | PASS | wall 118 ms |
| resources | check, 50k, cold | 1.93 CPU-s, 170.3 MB | completes ≤ 150 s | PASS | wall 810 ms |
| resources | check, 50k, warm | 0.38 CPU-s, 92.6 MB | completes ≤ 150 s | PASS | wall 288 ms |
| resources | test, 50k, cold | 15.61 CPU-s, 289.5 MB | completes ≤ 150 s | PASS | wall 12086 ms |
| resources | test, 50k, warm | 12.30 CPU-s, 114.4 MB | completes ≤ 150 s | PASS | wall 10256 ms |
| resources | build, 50k, cold | 2.52 CPU-s, 263.1 MB | completes ≤ 150 s | PASS | wall 1055 ms |
| resources | build, 50k, warm | 0.39 CPU-s, 110.3 MB | completes ≤ 150 s | PASS | wall 376 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 120.3 MB | ≤ 22.0 MB | FAIL | RSS 104.5 MB at input 100, 224.8 MB at input 1000; build per input 86 ms at 50, 714 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 4.6 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.5 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 16 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 13.7 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.19x | ≤ 1.50x | FAIL | p95 95 ms at N = 1; 4 cores; total CPU 720 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.81x | ≤ 6.00x | FAIL | p95 95 ms at N = 1; 4 cores; total CPU 3190 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.13 | &lt; 1.00 (sublinear) | FAIL | 140 ms at N = 1, 3190 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 15.5 MB | ≤ 10.0 MB | FAIL | artifacts 6819 B in 10 files; toolchain 15.5 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 8.9 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.69x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 3 files | the files of one check's cache | PASS | one check alone: 3 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 4.5 MB | ≤ 5.0 MB | PASS | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 79 | ≤ 1 | FAIL | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 24k/s | ≥ 100k/s | FAIL | 6000 extra cases took 247 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 139 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 1184 ms | ≤ 1000 ms | FAIL | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 1.0 ms | ≤ 1.0 ms | FAIL | 100 tests: 275 ms |
| integration-test-perf | setup per integration test program | 157 ms | ≤ 20 ms | FAIL | 10 programs: 1730 ms for 21 tests, 82 ms per test |
| integration-test-perf | total time growth in programs | 0.987 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 322 ms at 2, 1577 ms at 10 programs |
| integration-test-perf | cost per doc test | 141 ms | ≤ 20 ms | FAIL | 10 doc tests: 1565 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 1.32x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.50x | ≤ 3.00x | PASS | hd 5.1 ms (p10 0.4, p90 12), Node 10 ms, Python 119 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 2.08x | ≤ 3.00x | PASS | hd 20 ms (p10 14, p90 31), Node 9.4 ms, Python 345 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 1.24x | ≤ 3.00x | PASS | hd 14 ms (p10 12, p90 18), Node 11 ms, Python 19 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 2.35x | ≤ 3.00x | PASS | hd 26 ms (p10 24, p90 32), Node 11 ms, Python 19 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 1.34x | ≤ 3.00x | PASS | hd 33 ms (p10 29, p90 35), Node 25 ms, Python 22 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 2321 B | ≤ 2048 B | FAIL | benchmarks: fib 3656 B, sum 3610 B, string-build 5329 B, map 5203 B, sort 5694 B |
| size-startup-heap | tiny program, start to first output | 50 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.95x | ≤ 2.00x | PASS | hd +13.4 MB, Node +14.1 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.38x | ≤ 2.00x | PASS | hd +8.2 MB, Node +21.8 MB over an empty program |
| host-call-overhead | Console println, per call | 8.4 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 27 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 48 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 28 ns | ≤ 100 ns | PASS | 28 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 9.9M/s | ≥ 1.0M/s | PASS | 201 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 6.8M/s | ≥ 1.0M/s | PASS | 292 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.06x | ≥ 0.50x | FAIL | hd 17 MB/s (45488 B document), Node 265 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.09x | ≥ 0.50x | FAIL | hd 13 MB/s (45488 B document), Node 143 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 1.48x | ≥ 0.50x | PASS | hd 133 MB/s, Node 89 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 2.20x | ≥ 0.50x | PASS | hd 470 MB/s, Node 213 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.10x | ≥ 0.50x | FAIL | hd 6.8 MB/s, Node 66 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 5343 B | ≤ 10240 B | PASS | 10073 B at 1075 lines, 58168 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 664 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 2.9 MB | ≤ 10.0 MB | PASS | 11876000 requests; RSS 67.1 MB after warm-up, 70.1 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 5415 of 5415 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

- [2026-10-28T18](daily/2026-10-28T18.json)
- [2026-10-28T03](daily/2026-10-28T03.json)
- [2026-10-27T18](daily/2026-10-27T18.json)
- [2026-10-27T02](daily/2026-10-27T02.json)
- [2026-10-26T18](daily/2026-10-26T18.json)
- [2026-10-26T03](daily/2026-10-26T03.json)
- [2026-10-25](daily/2026-10-25.json)
