# Daily goal metrics

Finished: 2026-10-29T03:26:11.320Z. Commit: 10197d0631420c1d6f54c3db2c4bf14bc1d3d482.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V74 80-Core Processor).
Compiler: target/release/hd.

97 passed, 34 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: fail 74 ms → fail 62 ms
- edit-latency: p95 check after an edit: pass 76 ms → pass 64 ms
- cold-check: median cold check, 10k lines: pass 211 ms → pass 176 ms
- pathological: overlapping-from: time at 4x: pass 57 ms → pass 48 ms
- pathological: wide-list-literal: time at 4x: pass 61 ms → pass 51 ms
- pathological: large-enum-match: time at 4x: pass 474 ms → pass 378 ms
- pathological: large-enum-match: growth exponent: fail 2.30 → fail 2.90
- pathological: deep-generic: time at 4x: pass 25 ms → pass 22 ms
- pathological: big-file: time at 4x: pass 197 ms → pass 164 ms
- pathological: big-file: growth exponent: pass 1.13 → fail 1.71
- fmt: hd fmt, 10k lines: pass 33 ms → pass 28 ms
- resources: warm check, 10k lines: CPU time: pass 80 ms → pass 60 ms
- startup: hd help (no version command): wall time: pass 4.6 ms → pass 4.0 ms
- proptest-perf: property cases per second: fail 24k/s → fail 34k/s
- proptest-perf: time to shrink a known failure: pass 139 ms → pass 105 ms
- unit-test-perf: 1,000 unit tests, warm: fail 1184 ms → pass 948 ms
- unit-test-perf: overhead per unit test: fail 1.0 ms → pass 0.8 ms
- integration-test-perf: setup per integration test program: fail 157 ms → fail 129 ms
- integration-test-perf: cost per doc test: fail 141 ms → fail 117 ms
- runtime: geomean hd / Node: pass 1.32x → pass 1.16x
- runtime: fib: hd / Node: pass 0.50x → pass 0.43x
- runtime: sum: hd / Node: pass 2.08x → pass 1.57x
- runtime: sort: hd / Node: pass 1.34x → pass 1.19x
- size-startup-heap: tiny program, start to first output: fail 50 ms → fail 42 ms
- host-call-overhead: Console println, per call: fail 8.4 µs → fail 7.5 µs
- host-call-overhead: Fs read_text!, per call: fail 27 µs → fail 20 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 48 µs → fail 34 µs
- suspension-overhead: await of a ready value, per await: pass 28 ns → pass 10 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 9.9M/s → pass 11.6M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 6.8M/s → pass 8.7M/s
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.09x → fail 0.10x
- text-throughput: split, hd / Node MB/s: pass 2.20x → pass 1.71x
- conformance: listed known failures: pass 0 → fail 18

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 62 ms | ≤ 50 ms | FAIL |  |
| edit-latency | p95 check after an edit | 64 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 176 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 273 ms | ≤ 300 ms | PASS | Timing change ≤10%: noise. |
| mistakes | programs with exactly one diagnostic | 95.7% | ≥ 95.0% | PASS | 66 of 69 |
| mistakes | one diagnostic with the expected code | 95.7% | ≥ 95.0% | PASS | 66 of 69; first misses: mut-on-binding-type, mut-on-parameter-type, some-on-result |
| mistakes | fix-it makes hd check pass | 34.8% | ≥ 80.0% | FAIL | 24 of 69 resolved; 24 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 93 tokens | ≤ 60 tokens | FAIL | largest: mut-on-parameter-type; median 44 tokens, 175 B; 64 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 48 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 22.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 34 → 48 ms, start-up 48 ms |
| pathological | deep-blocks: time at 4x | 20 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-blocks: peak RSS at 4x | 16.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 20 ms, start-up 48 ms |
| pathological | deep-parens: time at 4x | 19 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-parens: peak RSS at 4x | 16.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 24 → 19 ms, start-up 48 ms |
| pathological | method-chain: time at 4x | - | ≤ 2000 ms | FAIL | 4x: hd check failed: thread '&lt;unknown>' (6747) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 4x: hd check failed: thread '&lt;unknown>' (6747) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 1x→2x; 46 → 22 ms, start-up 48 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 51 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 20.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 32 → 51 ms, start-up 48 ms |
| pathological | large-enum-match: time at 4x | 378 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 19.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | 2.90 | ≤ 1.30 | FAIL | 2x→4x; 92 → 378 ms, start-up 48 ms |
| pathological | deep-generic: time at 4x | 22 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 17.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 22 ms, start-up 48 ms |
| pathological | question-chain: time at 4x | 27 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | question-chain: peak RSS at 4x | 19.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 23 → 27 ms, start-up 48 ms |
| pathological | big-file: time at 4x | 164 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 51.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.71 | ≤ 1.30 | FAIL | 2x→4x; 84 → 164 ms, start-up 48 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 24 of 24 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 28 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 2.93x | ≤ 1.30x | FAIL | checked 948 ms, unchecked 324 ms |
| dev-speed | geomean dev / optimized | 1.06x | ≤ 4.00x | PASS | fib 1.09x, sum 1.06x, string-build 1.13x, map 0.98x, sort 1.02x |
| dev-speed | worst case dev / optimized | 1.13x | ≤ 10.00x | PASS | string-build |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 60 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 31.1 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.11 CPU-s, 29.2 MB | completes ≤ 30 s | PASS | wall 82 ms |
| resources | check, small, warm | 0.01 CPU-s, 16.8 MB | completes ≤ 30 s | PASS | wall 18 ms |
| resources | test, small, cold | 1.11 CPU-s, 102.6 MB | completes ≤ 30 s | PASS | wall 529 ms |
| resources | test, small, warm | 0.63 CPU-s, 69.1 MB | completes ≤ 30 s | PASS | wall 302 ms |
| resources | build, small, cold | 0.49 CPU-s, 86.0 MB | completes ≤ 30 s | PASS | wall 234 ms |
| resources | build, small, warm | 0.05 CPU-s, 37.3 MB | completes ≤ 30 s | PASS | wall 59 ms |
| resources | check, 10k, cold | 0.34 CPU-s, 55.0 MB | completes ≤ 30 s | PASS | wall 175 ms |
| resources | check, 10k, warm | 0.06 CPU-s, 31.1 MB | completes ≤ 30 s | PASS | wall 57 ms |
| resources | test, 10k, cold | 1.92 CPU-s, 121.6 MB | completes ≤ 30 s | PASS | wall 1029 ms |
| resources | test, 10k, warm | 1.24 CPU-s, 76.1 MB | completes ≤ 30 s | PASS | wall 726 ms |
| resources | build, 10k, cold | 0.77 CPU-s, 105.2 MB | completes ≤ 30 s | PASS | wall 331 ms |
| resources | build, 10k, warm | 0.10 CPU-s, 44.4 MB | completes ≤ 30 s | PASS | wall 104 ms |
| resources | check, 50k, cold | 1.49 CPU-s, 171.7 MB | completes ≤ 150 s | PASS | wall 661 ms |
| resources | check, 50k, warm | 0.32 CPU-s, 93.0 MB | completes ≤ 150 s | PASS | wall 251 ms |
| resources | test, 50k, cold | 11.96 CPU-s, 283.0 MB | completes ≤ 150 s | PASS | wall 9319 ms |
| resources | test, 50k, warm | 9.86 CPU-s, 132.7 MB | completes ≤ 150 s | PASS | wall 8192 ms |
| resources | build, 50k, cold | 1.95 CPU-s, 265.4 MB | completes ≤ 150 s | PASS | wall 866 ms |
| resources | build, 50k, warm | 0.34 CPU-s, 111.0 MB | completes ≤ 150 s | PASS | wall 320 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 119.6 MB | ≤ 22.0 MB | FAIL | RSS 107.5 MB at input 100, 227.0 MB at input 1000; build per input 104 ms at 50, 601 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 4.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.6 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 15 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 14.3 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.12x | ≤ 1.50x | FAIL | p95 82 ms at N = 1; 4 cores; total CPU 580 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.10x | ≤ 6.00x | FAIL | p95 82 ms at N = 1; 4 cores; total CPU 2510 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.10 | &lt; 1.00 (sublinear) | FAIL | 120 ms at N = 1, 2510 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 15.6 MB | ≤ 10.0 MB | FAIL | artifacts 6822 B in 10 files; toolchain 15.6 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 9.0 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.69x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 3 files | the files of one check's cache | PASS | one check alone: 3 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 4.2 MB | ≤ 5.0 MB | PASS | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 79 | ≤ 1 | FAIL | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 34k/s | ≥ 100k/s | FAIL | 6000 extra cases took 179 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 105 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 948 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.8 ms | ≤ 1.0 ms | PASS | 100 tests: 223 ms |
| integration-test-perf | setup per integration test program | 129 ms | ≤ 20 ms | FAIL | 10 programs: 1422 ms for 21 tests, 68 ms per test |
| integration-test-perf | total time growth in programs | 0.991 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 262 ms at 2, 1289 ms at 10 programs |
| integration-test-perf | cost per doc test | 117 ms | ≤ 20 ms | FAIL | 10 doc tests: 1320 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 1.16x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.43x | ≤ 3.00x | PASS | hd 3.6 ms (p10 1.4, p90 6.6), Node 8.5 ms, Python 91 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 1.57x | ≤ 3.00x | PASS | hd 13 ms (p10 9.5, p90 22), Node 8.2 ms, Python 275 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 1.12x | ≤ 3.00x | PASS | hd 12 ms (p10 10, p90 18), Node 11 ms, Python 13 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 2.30x | ≤ 3.00x | PASS | hd 22 ms (p10 20, p90 28), Node 9.6 ms, Python 17 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 1.19x | ≤ 3.00x | PASS | hd 24 ms (p10 19, p90 28), Node 20 ms, Python 19 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 2337 B | ≤ 2048 B | FAIL | benchmarks: fib 3656 B, sum 3610 B, string-build 5329 B, map 5203 B, sort 5694 B |
| size-startup-heap | tiny program, start to first output | 42 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.97x | ≤ 2.00x | PASS | hd +13.5 MB, Node +13.9 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.38x | ≤ 2.00x | PASS | hd +8.3 MB, Node +21.8 MB over an empty program |
| host-call-overhead | Console println, per call | 7.5 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 20 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 34 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 10 ns | ≤ 100 ns | PASS | 10 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 11.6M/s | ≥ 1.0M/s | PASS | 172 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 8.7M/s | ≥ 1.0M/s | PASS | 230 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.06x | ≥ 0.50x | FAIL | hd 21 MB/s (45488 B document), Node 363 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.10x | ≥ 0.50x | FAIL | hd 19 MB/s (45488 B document), Node 183 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 1.40x | ≥ 0.50x | PASS | hd 156 MB/s, Node 111 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 1.71x | ≥ 0.50x | PASS | hd 478 MB/s, Node 279 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.10x | ≥ 0.50x | FAIL | hd 8.2 MB/s, Node 79 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 5343 B | ≤ 10240 B | PASS | 10089 B at 1075 lines, 58182 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 664 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 3.0 MB | ≤ 10.0 MB | PASS | 16024000 requests; RSS 66.1 MB after warm-up, 69.1 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 5438 of 5438 unlisted cases passed |
| conformance | listed known failures | 18 | ≤ 0 | FAIL | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

- [2026-10-29T03](daily/2026-10-29T03.json)
- [2026-10-28T18](daily/2026-10-28T18.json)
- [2026-10-28T03](daily/2026-10-28T03.json)
- [2026-10-27T18](daily/2026-10-27T18.json)
- [2026-10-27T02](daily/2026-10-27T02.json)
- [2026-10-26T18](daily/2026-10-26T18.json)
- [2026-10-26T03](daily/2026-10-26T03.json)
- [2026-10-25](daily/2026-10-25.json)
