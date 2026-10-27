# Daily goal metrics

Finished: 2026-10-27T18:05:57.528Z. Commit: 42c9eae075bf5782d72c46cd807d549ec603e76a.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 7763 64-Core Processor).
Compiler: target/release/hd.

97 passed, 34 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 35 ms → pass 47 ms
- edit-latency: p95 check after an edit: pass 37 ms → pass 50 ms
- cold-check: median cold check, 10k lines: pass 138 ms → pass 185 ms
- test-latency: median edit-then-test: pass 165 ms → pass 223 ms
- pathological: overlapping-from: time at 4x: pass 42 ms → pass 56 ms
- pathological: deep-blocks: time at 4x: pass 16 ms → pass 21 ms
- pathological: deep-parens: time at 4x: pass 15 ms → pass 20 ms
- pathological: wide-list-literal: time at 4x: pass 39 ms → pass 58 ms
- pathological: large-enum-match: time at 4x: pass 366 ms → pass 458 ms
- pathological: deep-generic: time at 4x: pass 18 ms → pass 22 ms
- pathological: question-chain: time at 4x: pass 22 ms → pass 31 ms
- pathological: big-file: time at 4x: pass 135 ms → pass 180 ms
- fmt: hd fmt, 10k lines: pass 18 ms → pass 33 ms
- dev-speed: worst case dev / optimized: pass 1.01x → pass 1.27x
- resources: warm check, 10k lines: CPU time: pass 30 ms → pass 60 ms
- long-session: RSS growth, REPL inputs 100 to 1000: fail 88.6 MB → fail 114.8 MB
- startup: hd help (no version command): wall time: pass 2.9 ms → pass 5.0 ms
- startup: hd check empty.hd: wall time: pass 11 ms → pass 18 ms
- concurrency: p95 latency at N = 16 vs N = 1: fail 8.63x → fail 7.43x
- proptest-perf: property cases per second: fail 34k/s → fail 21k/s
- proptest-perf: time to shrink a known failure: pass 107 ms → pass 140 ms
- unit-test-perf: 1,000 unit tests, warm: pass 854 ms → fail 1245 ms
- unit-test-perf: overhead per unit test: pass 0.7 ms → fail 1.1 ms
- integration-test-perf: setup per integration test program: fail 104 ms → fail 145 ms
- integration-test-perf: cost per doc test: fail 90 ms → fail 130 ms
- runtime: fib: hd / Node: pass 0.59x → pass 0.29x
- runtime: sum: hd / Node: pass 1.47x → pass 2.17x
- runtime: string-build: hd / Node: pass 1.27x → pass 0.98x
- runtime: map: hd / Node: pass 1.88x → pass 2.09x
- size-startup-heap: tiny program, start to first output: fail 40 ms → fail 55 ms
- host-call-overhead: Console println, per call: fail 7.5 µs → fail 9.6 µs
- host-call-overhead: Fs read_text!, per call: fail 19 µs → fail 28 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 34 µs → fail 50 µs
- suspension-overhead: await of a ready value, per await: pass 14 ns → pass 20 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 9.6M/s → pass 8.1M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 8.4M/s → pass 6.2M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.06x → fail 0.05x
- text-throughput: split, hd / Node MB/s: pass 2.25x → pass 1.90x
- dead-code: Wasm bytes per 1,000 lines: pass 6412 B → pass 5647 B
- long-run-memory: RSS growth after warm-up, 1 min: pass 0.0 MB → pass 2.6 MB
- conformance: listed known failures: fail 5 → fail 4

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 47 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 50 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 185 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 223 ms | ≤ 300 ms | PASS |  |
| mistakes | programs with exactly one diagnostic | 95.7% | ≥ 95.0% | PASS | 67 of 70 |
| mistakes | one diagnostic with the expected code | 95.7% | ≥ 95.0% | PASS | 67 of 70; first misses: mut-on-binding-type, mut-on-parameter-type, some-on-result |
| mistakes | fix-it makes hd check pass | 34.3% | ≥ 80.0% | FAIL | 24 of 70 resolved; 24 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 93 tokens | ≤ 60 tokens | FAIL | largest: mut-on-parameter-type; median 43 tokens, 171 B; 65 of 70 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 56 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 21.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 38 → 56 ms, start-up 20 ms |
| pathological | deep-blocks: time at 4x | 21 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 16.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 21 → 21 ms, start-up 20 ms |
| pathological | deep-parens: time at 4x | 20 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 16.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 20 ms, start-up 20 ms |
| pathological | method-chain: time at 4x | - | ≤ 2000 ms | FAIL | 4x: hd check failed: thread '&lt;unknown>' (7519) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 4x: hd check failed: thread '&lt;unknown>' (7519) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 1x→2x; 22 → 23 ms, start-up 20 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 58 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 20.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 51 → 58 ms, start-up 20 ms |
| pathological | large-enum-match: time at 4x | 458 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 19.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | 2.19 | ≤ 1.30 | FAIL | 2x→4x; 116 → 458 ms, start-up 20 ms |
| pathological | deep-generic: time at 4x | 22 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 16.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 22 ms, start-up 20 ms |
| pathological | question-chain: time at 4x | 31 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 19.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 24 → 31 ms, start-up 20 ms |
| pathological | big-file: time at 4x | 180 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 50.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.07 | ≤ 1.30 | PASS | 2x→4x; 96 → 180 ms, start-up 20 ms |
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
| check-cost | checked run time / unchecked run time | 2.88x | ≤ 1.30x | FAIL | checked 1217 ms, unchecked 423 ms |
| dev-speed | geomean dev / optimized | 1.05x | ≤ 4.00x | PASS | fib 0.97x, sum 0.97x, string-build 1.27x, map 1.04x, sort 1.01x |
| dev-speed | worst case dev / optimized | 1.27x | ≤ 10.00x | PASS | string-build |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 60 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 24.0 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.14 CPU-s, 27.2 MB | completes ≤ 30 s | PASS | wall 89 ms |
| resources | check, small, warm | 0.00 CPU-s, 15.1 MB | completes ≤ 30 s | PASS | wall 18 ms |
| resources | test, small, cold | 1.38 CPU-s, 81.2 MB | completes ≤ 30 s | PASS | wall 593 ms |
| resources | test, small, warm | 0.80 CPU-s, 70.7 MB | completes ≤ 30 s | PASS | wall 339 ms |
| resources | build, small, cold | 0.59 CPU-s, 79.8 MB | completes ≤ 30 s | PASS | wall 262 ms |
| resources | build, small, warm | 0.03 CPU-s, 25.4 MB | completes ≤ 30 s | PASS | wall 30 ms |
| resources | check, 10k, cold | 0.42 CPU-s, 49.4 MB | completes ≤ 30 s | PASS | wall 185 ms |
| resources | check, 10k, warm | 0.06 CPU-s, 24.0 MB | completes ≤ 30 s | PASS | wall 41 ms |
| resources | test, 10k, cold | 2.47 CPU-s, 101.0 MB | completes ≤ 30 s | PASS | wall 1266 ms |
| resources | test, 10k, warm | 1.46 CPU-s, 78.8 MB | completes ≤ 30 s | PASS | wall 814 ms |
| resources | build, 10k, cold | 0.92 CPU-s, 96.5 MB | completes ≤ 30 s | PASS | wall 339 ms |
| resources | build, 10k, warm | 0.06 CPU-s, 31.2 MB | completes ≤ 30 s | PASS | wall 61 ms |
| resources | check, 50k, cold | 1.76 CPU-s, 146.6 MB | completes ≤ 150 s | PASS | wall 652 ms |
| resources | check, 50k, warm | 0.24 CPU-s, 62.6 MB | completes ≤ 150 s | PASS | wall 149 ms |
| resources | test, 50k, cold | 15.49 CPU-s, 256.6 MB | completes ≤ 150 s | PASS | wall 11920 ms |
| resources | test, 50k, warm | 12.23 CPU-s, 117.0 MB | completes ≤ 150 s | PASS | wall 10203 ms |
| resources | build, 50k, cold | 2.30 CPU-s, 233.6 MB | completes ≤ 150 s | PASS | wall 861 ms |
| resources | build, 50k, warm | 0.21 CPU-s, 69.2 MB | completes ≤ 150 s | PASS | wall 187 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 114.8 MB | ≤ 22.0 MB | FAIL | RSS 108.3 MB at input 100, 223.1 MB at input 1000; build per input 56 ms at 50, 771 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 5.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.5 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 18 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 13.2 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.08x | ≤ 1.50x | FAIL | p95 104 ms at N = 1; 4 cores; total CPU 780 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 7.43x | ≤ 6.00x | FAIL | p95 104 ms at N = 1; 4 cores; total CPU 3180 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.10 | &lt; 1.00 (sublinear) | FAIL | 150 ms at N = 1, 3180 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 15.4 MB | ≤ 10.0 MB | FAIL | artifacts 7973 B in 10 files; toolchain 15.4 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 8.8 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.82x | ≥ 2.40x | FAIL | by --jobs |
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
| proptest-perf | property cases per second | 21k/s | ≥ 100k/s | FAIL | 6000 extra cases took 287 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 140 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 1245 ms | ≤ 1000 ms | FAIL | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 1.1 ms | ≤ 1.0 ms | FAIL | 100 tests: 265 ms |
| integration-test-perf | setup per integration test program | 145 ms | ≤ 20 ms | FAIL | 10 programs: 1601 ms for 21 tests, 76 ms per test |
| integration-test-perf | total time growth in programs | 0.965 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 309 ms at 2, 1461 ms at 10 programs |
| integration-test-perf | cost per doc test | 130 ms | ≤ 20 ms | FAIL | 10 doc tests: 1428 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 1.08x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.29x | ≤ 3.00x | PASS | hd 2.9 ms (p10 -2.3, p90 8.9), Node 10 ms, Python 119 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 2.17x | ≤ 3.00x | PASS | hd 20 ms (p10 13, p90 23), Node 9.4 ms, Python 354 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.98x | ≤ 3.00x | PASS | hd 13 ms (p10 9.4, p90 16), Node 13 ms, Python 16 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 2.09x | ≤ 3.00x | PASS | hd 24 ms (p10 16, p90 32), Node 12 ms, Python 19 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 1.16x | ≤ 3.00x | PASS | hd 30 ms (p10 25, p90 41), Node 25 ms, Python 22 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 4522 B | ≤ 2048 B | FAIL | benchmarks: fib 4179 B, sum 4114 B, string-build 6458 B, map 5916 B, sort 6587 B |
| size-startup-heap | tiny program, start to first output | 55 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.95x | ≤ 2.00x | PASS | hd +13.4 MB, Node +14.1 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.36x | ≤ 2.00x | PASS | hd +7.8 MB, Node +21.8 MB over an empty program |
| host-call-overhead | Console println, per call | 9.6 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 28 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 50 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 20 ns | ≤ 100 ns | PASS | 20 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 8.1M/s | ≥ 1.0M/s | PASS | 248 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 6.2M/s | ≥ 1.0M/s | PASS | 323 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.05x | ≥ 0.50x | FAIL | hd 15 MB/s (45488 B document), Node 266 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.09x | ≥ 0.50x | FAIL | hd 12 MB/s (45488 B document), Node 145 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 1.55x | ≥ 0.50x | PASS | hd 150 MB/s, Node 96 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 1.90x | ≥ 0.50x | PASS | hd 437 MB/s, Node 230 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.10x | ≥ 0.50x | FAIL | hd 6.7 MB/s, Node 70 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 5647 B | ≤ 10240 B | PASS | 12576 B at 1075 lines, 63406 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 837 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 2.6 MB | ≤ 10.0 MB | PASS | 10902000 requests; RSS 69.2 MB after warm-up, 71.8 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 5408 of 5408 unlisted cases passed |
| conformance | listed known failures | 4 | ≤ 0 | FAIL | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

- [2026-10-27T18](daily/2026-10-27T18.json)
- [2026-10-27T02](daily/2026-10-27T02.json)
- [2026-10-26T18](daily/2026-10-26T18.json)
- [2026-10-26T03](daily/2026-10-26T03.json)
- [2026-10-25](daily/2026-10-25.json)
