# Daily goal metrics

Finished: 2026-10-26T18:48:57.688Z. Commit: bd03cb5d33af43a96c5b6708fa444e1ea6c33106.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 7763 64-Core Processor).
Compiler: target/release/hd.

98 passed, 33 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 35 ms → pass 45 ms
- edit-latency: p95 check after an edit: pass 35 ms → pass 49 ms
- cold-check: median cold check, 10k lines: pass 132 ms → pass 178 ms
- test-latency: median edit-then-test: pass 160 ms → pass 209 ms
- mistakes: programs with exactly one diagnostic: fail 94.3% → pass 95.7%
- mistakes: one diagnostic with the expected code: fail 94.3% → pass 95.7%
- pathological: overlapping-from: time at 4x: pass 41 ms → pass 53 ms
- pathological: deep-blocks: time at 4x: pass 15 ms → pass 19 ms
- pathological: deep-parens: time at 4x: pass 14 ms → pass 19 ms
- pathological: wide-list-literal: time at 4x: pass 39 ms → pass 53 ms
- pathological: large-enum-match: time at 4x: pass 387 ms → pass 450 ms
- pathological: deep-generic: time at 4x: pass 17 ms → pass 22 ms
- pathological: question-chain: time at 4x: pass 22 ms → pass 27 ms
- pathological: big-file: time at 4x: pass 133 ms → pass 175 ms
- fmt: hd fmt, 10k lines: pass 11 ms → pass 15 ms
- check-cost: checked run time / unchecked run time: fail 2.50x → fail 2.83x
- resources: warm check, 10k lines: CPU time: pass 30 ms → pass 50 ms
- long-session: RSS growth, REPL inputs 100 to 1000: fail 368.8 MB → fail 94.6 MB
- startup: hd help (no version command): wall time: pass 2.9 ms → pass 3.3 ms
- proptest-perf: property cases per second: fail 25k/s → fail 18k/s
- proptest-perf: time to shrink a known failure: pass 109 ms → pass 131 ms
- unit-test-perf: 1,000 unit tests, warm: pass 878 ms → fail 1086 ms
- unit-test-perf: overhead per unit test: pass 0.8 ms → pass 0.9 ms
- integration-test-perf: setup per integration test program: fail 101 ms → fail 127 ms
- integration-test-perf: total time growth in programs: fail 1.006 → pass 0.953
- integration-test-perf: cost per doc test: fail 92 ms → fail 112 ms
- runtime: geomean hd / Node: pass 1.47x → pass 1.26x
- runtime: fib: hd / Node: pass 0.78x → pass 0.58x
- runtime: sum: hd / Node: pass 1.49x → pass 2.18x
- runtime: string-build: hd / Node: pass 2.19x → pass 1.12x
- runtime: sort: hd / Node: pass 1.38x → pass 1.17x
- size-startup-heap: tiny program, start to first output: fail 38 ms → fail 48 ms
- host-call-overhead: Console println, per call: fail 4.4 µs → fail 5.1 µs
- host-call-overhead: Fs read_text!, per call: fail 20 µs → fail 27 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 34 µs → fail 49 µs
- suspension-overhead: await of a ready value, per await: pass 15 ns → pass 12 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 10.7M/s → pass 8.0M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 8.1M/s → pass 5.7M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.04x → fail 0.05x
- text-throughput: string build, hd / Node MB/s: pass 0.74x → pass 1.57x
- text-throughput: split, hd / Node MB/s: pass 2.13x → pass 1.75x
- dead-code: Wasm bytes per 1,000 lines: fail 10780 B → pass 6705 B
- conformance: listed known failures: pass 0 → fail 4

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 45 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 49 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 178 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 209 ms | ≤ 300 ms | PASS |  |
| mistakes | programs with exactly one diagnostic | 95.7% | ≥ 95.0% | PASS | 67 of 70 |
| mistakes | one diagnostic with the expected code | 95.7% | ≥ 95.0% | PASS | 67 of 70; first misses: mut-on-binding-type, mut-on-parameter-type, some-on-result |
| mistakes | fix-it makes hd check pass | 34.3% | ≥ 80.0% | FAIL | 24 of 70 resolved; 24 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 93 tokens | ≤ 60 tokens | FAIL | largest: mut-on-parameter-type; median 43 tokens, 171 B; 65 of 70 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 53 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 21.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 37 → 53 ms, start-up 18 ms |
| pathological | deep-blocks: time at 4x | 19 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 15.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 19 ms, start-up 18 ms |
| pathological | deep-parens: time at 4x | 19 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 15.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 19 ms, start-up 18 ms |
| pathological | method-chain: time at 4x | - | ≤ 2000 ms | FAIL | 4x: hd check failed: thread '&lt;unknown>' (7462) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 4x: hd check failed: thread '&lt;unknown>' (7462) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 1x→2x; 20 → 22 ms, start-up 18 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 53 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 19.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 39 → 53 ms, start-up 18 ms |
| pathological | large-enum-match: time at 4x | 450 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 19.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | 2.30 | ≤ 1.30 | FAIL | 2x→4x; 106 → 450 ms, start-up 18 ms |
| pathological | deep-generic: time at 4x | 22 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 16.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 22 ms, start-up 18 ms |
| pathological | question-chain: time at 4x | 27 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 18.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 27 ms, start-up 18 ms |
| pathological | big-file: time at 4x | 175 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 51.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.06 | ≤ 1.30 | PASS | 2x→4x; 93 → 175 ms, start-up 18 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 69 of 70; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 24 of 24 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 15 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 2.83x | ≤ 1.30x | FAIL | checked 1150 ms, unchecked 407 ms |
| dev-speed | geomean dev / optimized | 1.01x | ≤ 4.00x | PASS | fib 1.01x, sum 1.04x, string-build 0.93x, map 1.09x, sort 1.00x |
| dev-speed | worst case dev / optimized | 1.09x | ≤ 10.00x | PASS | map |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 50 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 23.6 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.13 CPU-s, 26.9 MB | completes ≤ 30 s | PASS | wall 86 ms |
| resources | check, small, warm | 0.01 CPU-s, 14.5 MB | completes ≤ 30 s | PASS | wall 16 ms |
| resources | test, small, cold | 1.34 CPU-s, 79.4 MB | completes ≤ 30 s | PASS | wall 558 ms |
| resources | test, small, warm | 0.76 CPU-s, 73.7 MB | completes ≤ 30 s | PASS | wall 319 ms |
| resources | build, small, cold | 0.57 CPU-s, 77.5 MB | completes ≤ 30 s | PASS | wall 243 ms |
| resources | build, small, warm | 0.02 CPU-s, 24.9 MB | completes ≤ 30 s | PASS | wall 28 ms |
| resources | check, 10k, cold | 0.41 CPU-s, 48.2 MB | completes ≤ 30 s | PASS | wall 180 ms |
| resources | check, 10k, warm | 0.05 CPU-s, 23.6 MB | completes ≤ 30 s | PASS | wall 40 ms |
| resources | test, 10k, cold | 2.40 CPU-s, 103.5 MB | completes ≤ 30 s | PASS | wall 1206 ms |
| resources | test, 10k, warm | 1.41 CPU-s, 82.1 MB | completes ≤ 30 s | PASS | wall 793 ms |
| resources | build, 10k, cold | 0.88 CPU-s, 94.2 MB | completes ≤ 30 s | PASS | wall 321 ms |
| resources | build, 10k, warm | 0.06 CPU-s, 30.6 MB | completes ≤ 30 s | PASS | wall 55 ms |
| resources | check, 50k, cold | 1.71 CPU-s, 143.2 MB | completes ≤ 150 s | PASS | wall 633 ms |
| resources | check, 50k, warm | 0.24 CPU-s, 61.8 MB | completes ≤ 150 s | PASS | wall 144 ms |
| resources | test, 50k, cold | 15.18 CPU-s, 260.5 MB | completes ≤ 150 s | PASS | wall 11804 ms |
| resources | test, 50k, warm | 12.15 CPU-s, 119.1 MB | completes ≤ 150 s | PASS | wall 10174 ms |
| resources | build, 50k, cold | 2.20 CPU-s, 240.1 MB | completes ≤ 150 s | PASS | wall 798 ms |
| resources | build, 50k, warm | 0.20 CPU-s, 67.9 MB | completes ≤ 150 s | PASS | wall 187 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 94.6 MB | ≤ 22.0 MB | FAIL | RSS 107.3 MB at input 100, 199.8 MB at input 1000; build per input 51 ms at 50, 672 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.3 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.4 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 14 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 12.8 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.27x | ≤ 1.50x | FAIL | p95 87 ms at N = 1; 4 cores; total CPU 710 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.99x | ≤ 6.00x | FAIL | p95 87 ms at N = 1; 4 cores; total CPU 2940 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.12 | &lt; 1.00 (sublinear) | FAIL | 130 ms at N = 1, 2940 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 15.2 MB | ≤ 10.0 MB | FAIL | artifacts 8292 B in 10 files; toolchain 15.2 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 8.6 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.88x | ≥ 2.40x | FAIL | by --jobs |
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
| proptest-perf | property cases per second | 18k/s | ≥ 100k/s | FAIL | 6000 extra cases took 336 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 131 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 1086 ms | ≤ 1000 ms | FAIL | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.9 ms | ≤ 1.0 ms | PASS | 100 tests: 242 ms |
| integration-test-perf | setup per integration test program | 127 ms | ≤ 20 ms | FAIL | 10 programs: 1408 ms for 21 tests, 67 ms per test |
| integration-test-perf | total time growth in programs | 0.953 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 277 ms at 2, 1284 ms at 10 programs |
| integration-test-perf | cost per doc test | 112 ms | ≤ 20 ms | FAIL | 10 doc tests: 1258 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 1.26x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.58x | ≤ 3.00x | PASS | hd 5.8 ms (p10 3.5, p90 15), Node 10 ms, Python 119 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 2.18x | ≤ 3.00x | PASS | hd 20 ms (p10 13, p90 34), Node 9.4 ms, Python 340 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 1.12x | ≤ 3.00x | PASS | hd 13 ms (p10 7.5, p90 15), Node 12 ms, Python 15 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 1.96x | ≤ 3.00x | PASS | hd 19 ms (p10 17, p90 26), Node 9.8 ms, Python 17 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 1.17x | ≤ 3.00x | PASS | hd 29 ms (p10 23, p90 32), Node 25 ms, Python 21 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 6518 B | ≤ 2048 B | FAIL | benchmarks: fib 4198 B, sum 4134 B, string-build 6490 B, map 5939 B, sort 6625 B |
| size-startup-heap | tiny program, start to first output | 48 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.94x | ≤ 2.00x | PASS | hd +13.3 MB, Node +14.2 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.36x | ≤ 2.00x | PASS | hd +7.8 MB, Node +21.8 MB over an empty program |
| host-call-overhead | Console println, per call | 5.1 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 27 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 49 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 12 ns | ≤ 100 ns | PASS | 12 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 8.0M/s | ≥ 1.0M/s | PASS | 251 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 5.7M/s | ≥ 1.0M/s | PASS | 349 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.05x | ≥ 0.50x | FAIL | hd 14 MB/s (45488 B document), Node 266 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.06x | ≥ 0.50x | FAIL | hd 9.4 MB/s (45488 B document), Node 147 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 1.57x | ≥ 0.50x | PASS | hd 165 MB/s, Node 105 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 1.75x | ≥ 0.50x | PASS | hd 430 MB/s, Node 245 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.06x | ≥ 0.50x | FAIL | hd 4.5 MB/s, Node 75 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 6705 B | ≤ 10240 B | PASS | 13976 B at 1075 lines, 74325 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 765 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 2.8 MB | ≤ 10.0 MB | PASS | 8204000 requests; RSS 68.8 MB after warm-up, 71.7 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 5377 of 5377 unlisted cases passed |
| conformance | listed known failures | 4 | ≤ 0 | FAIL | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

- [2026-10-26T18](daily/2026-10-26T18.json)
- [2026-10-26T03](daily/2026-10-26T03.json)
- [2026-10-25](daily/2026-10-25.json)
