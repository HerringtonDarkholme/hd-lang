# Daily goal metrics

Finished: 2026-10-29T18:34:19.264Z. Commit: c30cb2957c5e54acaf0ebf4c4a706a2b221144e1.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V74 80-Core Processor).
Compiler: target/release/hd.

98 passed, 33 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: fail 62 ms → fail 77 ms
- edit-latency: p95 check after an edit: pass 64 ms → pass 80 ms
- cold-check: median cold check, 10k lines: pass 176 ms → pass 225 ms
- test-latency: median edit-then-test: pass 273 ms → fail 336 ms
- pathological: overlapping-from: time at 4x: pass 48 ms → pass 61 ms
- pathological: deep-blocks: time at 4x: pass 20 ms → pass 25 ms
- pathological: deep-parens: time at 4x: pass 19 ms → pass 24 ms
- pathological: wide-list-literal: time at 4x: pass 51 ms → pass 61 ms
- pathological: large-enum-match: time at 4x: pass 378 ms → pass 483 ms
- pathological: large-enum-match: growth exponent: fail 2.90 → fail 2.32
- pathological: deep-generic: time at 4x: pass 22 ms → pass 28 ms
- pathological: question-chain: time at 4x: pass 27 ms → pass 34 ms
- pathological: big-file: time at 4x: pass 164 ms → pass 207 ms
- pathological: big-file: growth exponent: fail 1.71 → pass 1.13
- fmt: hd fmt, 10k lines: pass 28 ms → pass 35 ms
- resources: warm check, 10k lines: CPU time: pass 60 ms → pass 80 ms
- long-session: RSS growth, REPL inputs 100 to 1000: fail 119.6 MB → pass 14.8 MB
- startup: hd help (no version command): wall time: pass 4.0 ms → pass 4.8 ms
- startup: hd check empty.hd: wall time: pass 15 ms → pass 19 ms
- proptest-perf: property cases per second: fail 34k/s → fail 27k/s
- proptest-perf: time to shrink a known failure: pass 105 ms → pass 133 ms
- unit-test-perf: 1,000 unit tests, warm: pass 948 ms → fail 1113 ms
- unit-test-perf: overhead per unit test: pass 0.8 ms → pass 0.9 ms
- integration-test-perf: setup per integration test program: fail 129 ms → fail 159 ms
- integration-test-perf: cost per doc test: fail 117 ms → fail 150 ms
- runtime: geomean hd / Node: pass 1.16x → pass 1.30x
- runtime: fib: hd / Node: pass 0.43x → pass 0.81x
- runtime: sum: hd / Node: pass 1.57x → pass 1.39x
- runtime: string-build: hd / Node: pass 1.12x → pass 1.46x
- runtime: map: hd / Node: pass 2.30x → pass 2.05x
- allocations: map loop, bytes per iteration: pass 0.00 B/iter → pass 0.21 B/iter
- size-startup-heap: tiny program, start to first output: fail 42 ms → fail 50 ms
- host-call-overhead: Console println, per call: fail 7.5 µs → fail 8.9 µs
- host-call-overhead: Fs read_text!, per call: fail 20 µs → fail 24 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 34 µs → fail 43 µs
- suspension-overhead: await of a ready value, per await: pass 10 ns → pass 7.3 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 11.6M/s → pass 9.5M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 8.7M/s → pass 7.2M/s
- conformance: listed known failures: fail 18 → pass 0

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 77 ms | ≤ 50 ms | FAIL |  |
| edit-latency | p95 check after an edit | 80 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 225 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 336 ms | ≤ 300 ms | FAIL |  |
| mistakes | programs with exactly one diagnostic | 95.7% | ≥ 95.0% | PASS | 66 of 69 |
| mistakes | one diagnostic with the expected code | 95.7% | ≥ 95.0% | PASS | 66 of 69; first misses: mut-on-binding-type, mut-on-parameter-type, some-on-result |
| mistakes | fix-it makes hd check pass | 34.8% | ≥ 80.0% | FAIL | 24 of 69 resolved; 24 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 93 tokens | ≤ 60 tokens | FAIL | largest: mut-on-parameter-type; median 44 tokens, 175 B; 64 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 61 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 22.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 43 → 61 ms, start-up 24 ms |
| pathological | deep-blocks: time at 4x | 25 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 17.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 24 → 25 ms, start-up 24 ms |
| pathological | deep-parens: time at 4x | 24 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 17.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 24 → 24 ms, start-up 24 ms |
| pathological | method-chain: time at 4x | - | ≤ 2000 ms | FAIL | 4x: hd check failed: thread '&lt;unknown>' (6875) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 4x: hd check failed: thread '&lt;unknown>' (6875) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 1x→2x; 26 → 28 ms, start-up 24 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 61 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 21.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 43 → 61 ms, start-up 24 ms |
| pathological | large-enum-match: time at 4x | 483 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 21.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | 2.32 | ≤ 1.30 | FAIL | 2x→4x; 116 → 483 ms, start-up 24 ms |
| pathological | deep-generic: time at 4x | 28 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 18.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 26 → 28 ms, start-up 24 ms |
| pathological | question-chain: time at 4x | 34 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 20.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 30 → 34 ms, start-up 24 ms |
| pathological | big-file: time at 4x | 207 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 51.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.13 | ≤ 1.30 | PASS | 2x→4x; 107 → 207 ms, start-up 24 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 24 of 24 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 35 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 2.72x | ≤ 1.30x | FAIL | checked 1231 ms, unchecked 453 ms |
| dev-speed | geomean dev / optimized | 1.01x | ≤ 4.00x | PASS | fib 1.01x, sum 0.99x, string-build 1.05x, map 1.01x, sort 1.00x |
| dev-speed | worst case dev / optimized | 1.05x | ≤ 10.00x | PASS | string-build |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 80 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 31.7 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.16 CPU-s, 30.7 MB | completes ≤ 30 s | PASS | wall 108 ms |
| resources | check, small, warm | 0.01 CPU-s, 17.3 MB | completes ≤ 30 s | PASS | wall 24 ms |
| resources | test, small, cold | 1.51 CPU-s, 105.8 MB | completes ≤ 30 s | PASS | wall 698 ms |
| resources | test, small, warm | 0.82 CPU-s, 69.2 MB | completes ≤ 30 s | PASS | wall 393 ms |
| resources | build, small, cold | 0.70 CPU-s, 89.5 MB | completes ≤ 30 s | PASS | wall 331 ms |
| resources | build, small, warm | 0.08 CPU-s, 38.8 MB | completes ≤ 30 s | PASS | wall 81 ms |
| resources | check, 10k, cold | 0.46 CPU-s, 56.3 MB | completes ≤ 30 s | PASS | wall 228 ms |
| resources | check, 10k, warm | 0.08 CPU-s, 31.7 MB | completes ≤ 30 s | PASS | wall 74 ms |
| resources | test, 10k, cold | 2.54 CPU-s, 131.4 MB | completes ≤ 30 s | PASS | wall 1338 ms |
| resources | test, 10k, warm | 1.49 CPU-s, 76.2 MB | completes ≤ 30 s | PASS | wall 907 ms |
| resources | build, 10k, cold | 1.03 CPU-s, 112.1 MB | completes ≤ 30 s | PASS | wall 433 ms |
| resources | build, 10k, warm | 0.14 CPU-s, 46.0 MB | completes ≤ 30 s | PASS | wall 130 ms |
| resources | check, 50k, cold | 1.86 CPU-s, 173.2 MB | completes ≤ 150 s | PASS | wall 810 ms |
| resources | check, 50k, warm | 0.38 CPU-s, 94.1 MB | completes ≤ 150 s | PASS | wall 299 ms |
| resources | test, 50k, cold | 15.52 CPU-s, 288.1 MB | completes ≤ 150 s | PASS | wall 12060 ms |
| resources | test, 50k, warm | 12.61 CPU-s, 119.5 MB | completes ≤ 150 s | PASS | wall 10450 ms |
| resources | build, 50k, cold | 2.58 CPU-s, 270.0 MB | completes ≤ 150 s | PASS | wall 1122 ms |
| resources | build, 50k, warm | 0.45 CPU-s, 112.8 MB | completes ≤ 150 s | PASS | wall 424 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 14.8 MB | ≤ 22.0 MB | PASS | RSS 166.5 MB at input 100, 165.1 MB at input 1000; build per input 99 ms at 50, 770 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 4.8 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.5 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 19 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 14.9 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.06x | ≤ 1.50x | FAIL | p95 109 ms at N = 1; 4 cores; total CPU 820 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 7.85x | ≤ 6.00x | FAIL | p95 109 ms at N = 1; 4 cores; total CPU 3280 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.09 | &lt; 1.00 (sublinear) | FAIL | 160 ms at N = 1, 3280 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 16.0 MB | ≤ 10.0 MB | FAIL | artifacts 6822 B in 10 files; toolchain 16.0 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 9.8 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.73x | ≥ 2.40x | FAIL | by --jobs |
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
| proptest-perf | property cases per second | 27k/s | ≥ 100k/s | FAIL | 6000 extra cases took 220 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 133 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 1113 ms | ≤ 1000 ms | FAIL | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.9 ms | ≤ 1.0 ms | PASS | 100 tests: 273 ms |
| integration-test-perf | setup per integration test program | 159 ms | ≤ 20 ms | FAIL | 10 programs: 1794 ms for 21 tests, 85 ms per test |
| integration-test-perf | total time growth in programs | 0.962 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 344 ms at 2, 1617 ms at 10 programs |
| integration-test-perf | cost per doc test | 150 ms | ≤ 20 ms | FAIL | 10 doc tests: 1676 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 1.30x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.81x | ≤ 3.00x | PASS | hd 8.8 ms (p10 3.2, p90 14), Node 11 ms, Python 116 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 1.39x | ≤ 3.00x | PASS | hd 15 ms (p10 12, p90 18), Node 11 ms, Python 355 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 1.46x | ≤ 3.00x | PASS | hd 17 ms (p10 12, p90 21), Node 12 ms, Python 16 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 2.05x | ≤ 3.00x | PASS | hd 24 ms (p10 20, p90 27), Node 12 ms, Python 20 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 1.12x | ≤ 3.00x | PASS | hd 28 ms (p10 25, p90 31), Node 25 ms, Python 22 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.21 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 2337 B | ≤ 2048 B | FAIL | benchmarks: fib 3656 B, sum 3610 B, string-build 5329 B, map 5203 B, sort 5694 B |
| size-startup-heap | tiny program, start to first output | 50 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.95x | ≤ 2.00x | PASS | hd +13.3 MB, Node +14.1 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.37x | ≤ 2.00x | PASS | hd +8.1 MB, Node +21.7 MB over an empty program |
| host-call-overhead | Console println, per call | 8.9 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 24 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 43 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 7.3 ns | ≤ 100 ns | PASS | 7.3 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 9.5M/s | ≥ 1.0M/s | PASS | 211 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 7.2M/s | ≥ 1.0M/s | PASS | 277 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.06x | ≥ 0.50x | FAIL | hd 17 MB/s (45488 B document), Node 277 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.11x | ≥ 0.50x | FAIL | hd 16 MB/s (45488 B document), Node 141 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 1.48x | ≥ 0.50x | PASS | hd 133 MB/s, Node 90 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 1.63x | ≥ 0.50x | PASS | hd 364 MB/s, Node 223 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.11x | ≥ 0.50x | FAIL | hd 6.9 MB/s, Node 64 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 5343 B | ≤ 10240 B | PASS | 10089 B at 1075 lines, 58182 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 664 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 3.2 MB | ≤ 10.0 MB | PASS | 11728000 requests; RSS 64.5 MB after warm-up, 67.7 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 5569 of 5569 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

- [2026-10-29T18](daily/2026-10-29T18.json)
- [2026-10-29T03](daily/2026-10-29T03.json)
- [2026-10-28T18](daily/2026-10-28T18.json)
- [2026-10-28T03](daily/2026-10-28T03.json)
- [2026-10-27T18](daily/2026-10-27T18.json)
- [2026-10-27T02](daily/2026-10-27T02.json)
- [2026-10-26T18](daily/2026-10-26T18.json)
- [2026-10-26T03](daily/2026-10-26T03.json)
- [2026-10-25](daily/2026-10-25.json)
