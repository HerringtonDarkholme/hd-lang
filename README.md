# Daily goal metrics

Finished: 2026-10-27T02:55:03.573Z. Commit: 3b0e4cb4b07b02185415089a52174927e0444d92.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V74 80-Core Processor).
Compiler: target/release/hd.

99 passed, 32 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 45 ms → pass 35 ms
- edit-latency: p95 check after an edit: pass 49 ms → pass 37 ms
- cold-check: median cold check, 10k lines: pass 178 ms → pass 138 ms
- test-latency: median edit-then-test: pass 209 ms → pass 165 ms
- pathological: overlapping-from: time at 4x: pass 53 ms → pass 42 ms
- pathological: deep-blocks: time at 4x: pass 19 ms → pass 16 ms
- pathological: deep-parens: time at 4x: pass 19 ms → pass 15 ms
- pathological: wide-list-literal: time at 4x: pass 53 ms → pass 39 ms
- pathological: large-enum-match: time at 4x: pass 450 ms → pass 366 ms
- pathological: deep-generic: time at 4x: pass 22 ms → pass 18 ms
- pathological: question-chain: time at 4x: pass 27 ms → pass 22 ms
- pathological: big-file: time at 4x: pass 175 ms → pass 135 ms
- fmt: hd fmt, 10k lines: pass 15 ms → pass 18 ms
- resources: warm check, 10k lines: CPU time: pass 50 ms → pass 30 ms
- startup: hd help (no version command): wall time: pass 3.3 ms → pass 2.9 ms
- startup: hd check empty.hd: wall time: pass 14 ms → pass 11 ms
- proptest-perf: property cases per second: fail 18k/s → fail 34k/s
- proptest-perf: time to shrink a known failure: pass 131 ms → pass 107 ms
- unit-test-perf: 1,000 unit tests, warm: fail 1086 ms → pass 854 ms
- unit-test-perf: overhead per unit test: pass 0.9 ms → pass 0.7 ms
- integration-test-perf: setup per integration test program: fail 127 ms → fail 104 ms
- integration-test-perf: cost per doc test: fail 112 ms → fail 90 ms
- runtime: sum: hd / Node: pass 2.18x → pass 1.47x
- runtime: string-build: hd / Node: pass 1.12x → pass 1.27x
- size-startup-heap: tiny program, release Wasm size: fail 6518 B → fail 4452 B
- size-startup-heap: tiny program, start to first output: fail 48 ms → fail 40 ms
- host-call-overhead: Console println, per call: fail 5.1 µs → fail 7.5 µs
- host-call-overhead: Fs read_text!, per call: fail 27 µs → fail 19 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 49 µs → fail 34 µs
- suspension-overhead: await of a ready value, per await: pass 12 ns → pass 14 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 8.0M/s → pass 9.6M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 5.7M/s → pass 8.4M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.05x → fail 0.06x
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.06x → fail 0.10x
- text-throughput: split, hd / Node MB/s: pass 1.75x → pass 2.25x
- text-throughput: regex find_all, hd / Node MB/s: fail 0.06x → fail 0.11x
- long-run-memory: RSS growth after warm-up, 1 min: pass 2.8 MB → pass 0.0 MB
- conformance: listed known failures: fail 4 → fail 5

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 35 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 37 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 138 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 165 ms | ≤ 300 ms | PASS |  |
| mistakes | programs with exactly one diagnostic | 95.7% | ≥ 95.0% | PASS | 67 of 70 |
| mistakes | one diagnostic with the expected code | 95.7% | ≥ 95.0% | PASS | 67 of 70; first misses: mut-on-binding-type, mut-on-parameter-type, some-on-result |
| mistakes | fix-it makes hd check pass | 34.3% | ≥ 80.0% | FAIL | 24 of 70 resolved; 24 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 93 tokens | ≤ 60 tokens | FAIL | largest: mut-on-parameter-type; median 43 tokens, 171 B; 65 of 70 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 42 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 21.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 28 → 42 ms, start-up 15 ms |
| pathological | deep-blocks: time at 4x | 16 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 16.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 15 → 16 ms, start-up 15 ms |
| pathological | deep-parens: time at 4x | 15 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 15.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 15 → 15 ms, start-up 15 ms |
| pathological | method-chain: time at 4x | - | ≤ 2000 ms | FAIL | 4x: hd check failed: thread '&lt;unknown>' (7237) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 4x: hd check failed: thread '&lt;unknown>' (7237) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 1x→2x; 16 → 18 ms, start-up 15 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 39 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 19.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 30 → 39 ms, start-up 15 ms |
| pathological | large-enum-match: time at 4x | 366 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 19.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | 2.31 | ≤ 1.30 | FAIL | 2x→4x; 86 → 366 ms, start-up 15 ms |
| pathological | deep-generic: time at 4x | 18 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 16.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 16 → 18 ms, start-up 15 ms |
| pathological | question-chain: time at 4x | 22 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 18.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 22 ms, start-up 15 ms |
| pathological | big-file: time at 4x | 135 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 51.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.05 | ≤ 1.30 | PASS | 2x→4x; 73 → 135 ms, start-up 15 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 69 of 70; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 24 of 24 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 18 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 2.84x | ≤ 1.30x | FAIL | checked 944 ms, unchecked 332 ms |
| dev-speed | geomean dev / optimized | 0.99x | ≤ 4.00x | PASS | fib 1.01x, sum 0.98x, string-build 0.98x, map 1.01x, sort 0.98x |
| dev-speed | worst case dev / optimized | 1.01x | ≤ 10.00x | PASS | map |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 30 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 24.0 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.10 CPU-s, 27.0 MB | completes ≤ 30 s | PASS | wall 70 ms |
| resources | check, small, warm | 0.00 CPU-s, 14.7 MB | completes ≤ 30 s | PASS | wall 14 ms |
| resources | test, small, cold | 1.03 CPU-s, 82.3 MB | completes ≤ 30 s | PASS | wall 441 ms |
| resources | test, small, warm | 0.58 CPU-s, 74.2 MB | completes ≤ 30 s | PASS | wall 242 ms |
| resources | build, small, cold | 0.42 CPU-s, 81.7 MB | completes ≤ 30 s | PASS | wall 187 ms |
| resources | build, small, warm | 0.02 CPU-s, 24.7 MB | completes ≤ 30 s | PASS | wall 23 ms |
| resources | check, 10k, cold | 0.31 CPU-s, 49.1 MB | completes ≤ 30 s | PASS | wall 141 ms |
| resources | check, 10k, warm | 0.03 CPU-s, 24.0 MB | completes ≤ 30 s | PASS | wall 32 ms |
| resources | test, 10k, cold | 1.81 CPU-s, 101.8 MB | completes ≤ 30 s | PASS | wall 926 ms |
| resources | test, 10k, warm | 1.08 CPU-s, 82.9 MB | completes ≤ 30 s | PASS | wall 608 ms |
| resources | build, 10k, cold | 0.64 CPU-s, 100.0 MB | completes ≤ 30 s | PASS | wall 250 ms |
| resources | build, 10k, warm | 0.04 CPU-s, 31.5 MB | completes ≤ 30 s | PASS | wall 45 ms |
| resources | check, 50k, cold | 1.28 CPU-s, 146.4 MB | completes ≤ 150 s | PASS | wall 486 ms |
| resources | check, 50k, warm | 0.17 CPU-s, 62.8 MB | completes ≤ 150 s | PASS | wall 113 ms |
| resources | test, 50k, cold | 11.43 CPU-s, 263.3 MB | completes ≤ 150 s | PASS | wall 8861 ms |
| resources | test, 50k, warm | 9.27 CPU-s, 117.3 MB | completes ≤ 150 s | PASS | wall 7705 ms |
| resources | build, 50k, cold | 1.64 CPU-s, 243.9 MB | completes ≤ 150 s | PASS | wall 621 ms |
| resources | build, 50k, warm | 0.16 CPU-s, 69.2 MB | completes ≤ 150 s | PASS | wall 145 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 88.6 MB | ≤ 22.0 MB | FAIL | RSS 108.2 MB at input 100, 181.4 MB at input 1000; build per input 50 ms at 50, 574 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 2.9 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.5 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 11 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 13.2 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.14x | ≤ 1.50x | FAIL | p95 69 ms at N = 1; 4 cores; total CPU 520 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.63x | ≤ 6.00x | FAIL | p95 69 ms at N = 1; 4 cores; total CPU 2160 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.11 | &lt; 1.00 (sublinear) | FAIL | 100 ms at N = 1, 2160 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 15.3 MB | ≤ 10.0 MB | FAIL | artifacts 8154 B in 10 files; toolchain 15.3 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 8.9 MB, shared per user, not counted |
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
| proptest-perf | property cases per second | 34k/s | ≥ 100k/s | FAIL | 6000 extra cases took 175 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 107 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 854 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.7 ms | ≤ 1.0 ms | PASS | 100 tests: 189 ms |
| integration-test-perf | setup per integration test program | 104 ms | ≤ 20 ms | FAIL | 10 programs: 1137 ms for 21 tests, 54 ms per test |
| integration-test-perf | total time growth in programs | 0.994 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 209 ms at 2, 1034 ms at 10 programs |
| integration-test-perf | cost per doc test | 90 ms | ≤ 20 ms | FAIL | 10 doc tests: 1009 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 1.18x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.59x | ≤ 3.00x | PASS | hd 5.0 ms (p10 -0.4, p90 6.8), Node 8.5 ms, Python 89 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 1.47x | ≤ 3.00x | PASS | hd 12 ms (p10 4.9, p90 25), Node 8.2 ms, Python 277 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 1.27x | ≤ 3.00x | PASS | hd 11 ms (p10 6.3, p90 16), Node 8.9 ms, Python 12 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 1.88x | ≤ 3.00x | PASS | hd 15 ms (p10 12, p90 20), Node 8.2 ms, Python 14 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 1.11x | ≤ 3.00x | PASS | hd 22 ms (p10 17, p90 26), Node 19 ms, Python 17 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 4452 B | ≤ 2048 B | FAIL | benchmarks: fib 4190 B, sum 4126 B, string-build 6463 B, map 5931 B, sort 6605 B |
| size-startup-heap | tiny program, start to first output | 40 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.96x | ≤ 2.00x | PASS | hd +13.5 MB, Node +14.1 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.37x | ≤ 2.00x | PASS | hd +7.9 MB, Node +21.7 MB over an empty program |
| host-call-overhead | Console println, per call | 7.5 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 19 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 34 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 14 ns | ≤ 100 ns | PASS | 14 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 9.6M/s | ≥ 1.0M/s | PASS | 208 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 8.4M/s | ≥ 1.0M/s | PASS | 237 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.06x | ≥ 0.50x | FAIL | hd 21 MB/s (45488 B document), Node 366 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.10x | ≥ 0.50x | FAIL | hd 18 MB/s (45488 B document), Node 177 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 1.45x | ≥ 0.50x | PASS | hd 179 MB/s, Node 123 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 2.25x | ≥ 0.50x | PASS | hd 665 MB/s, Node 296 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.11x | ≥ 0.50x | FAIL | hd 9.2 MB/s, Node 86 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 6412 B | ≤ 10240 B | PASS | 13529 B at 1075 lines, 71239 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 765 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 14104000 requests; RSS 72.5 MB after warm-up, 72.5 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 5405 of 5405 unlisted cases passed |
| conformance | listed known failures | 5 | ≤ 0 | FAIL | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

- [2026-10-27T02](daily/2026-10-27T02.json)
- [2026-10-26T18](daily/2026-10-26T18.json)
- [2026-10-26T03](daily/2026-10-26T03.json)
- [2026-10-25](daily/2026-10-25.json)
