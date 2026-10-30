# Daily goal metrics

Finished: 2026-10-30T03:28:19.383Z. Commit: 937eca14f3a5078b78fbf2790d5ebb173c595fa1.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V45 96-Core Processor).
Compiler: target/release/hd.

98 passed, 33 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: fail 77 ms → fail 54 ms
- edit-latency: p95 check after an edit: pass 80 ms → pass 63 ms
- cold-check: median cold check, 10k lines: pass 225 ms → pass 159 ms
- test-latency: median edit-then-test: fail 336 ms → pass 242 ms
- pathological: overlapping-from: time at 4x: pass 61 ms → pass 42 ms
- pathological: deep-blocks: time at 4x: pass 25 ms → pass 18 ms
- pathological: deep-parens: time at 4x: pass 24 ms → pass 18 ms
- pathological: wide-list-literal: time at 4x: pass 61 ms → pass 39 ms
- pathological: large-enum-match: time at 4x: pass 483 ms → pass 331 ms
- pathological: deep-generic: time at 4x: pass 28 ms → pass 20 ms
- pathological: question-chain: time at 4x: pass 34 ms → pass 25 ms
- pathological: big-file: time at 4x: pass 207 ms → pass 134 ms
- fmt: hd fmt, 10k lines: pass 35 ms → pass 21 ms
- dev-speed: worst case dev / optimized: pass 1.05x → pass 1.30x
- resources: warm check, 10k lines: CPU time: pass 80 ms → pass 60 ms
- long-session: RSS growth, REPL inputs 100 to 1000: pass 14.8 MB → fail 42.4 MB
- startup: hd help (no version command): wall time: pass 4.8 ms → pass 4.1 ms
- startup: hd check empty.hd: wall time: pass 19 ms → pass 15 ms
- concurrency: p95 latency at N = 4 vs N = 1: fail 2.06x → fail 2.52x
- proptest-perf: property cases per second: fail 27k/s → fail 50k/s
- proptest-perf: time to shrink a known failure: pass 133 ms → pass 94 ms
- unit-test-perf: 1,000 unit tests, warm: fail 1113 ms → pass 852 ms
- unit-test-perf: overhead per unit test: pass 0.9 ms → pass 0.7 ms
- integration-test-perf: setup per integration test program: fail 159 ms → fail 120 ms
- integration-test-perf: cost per doc test: fail 150 ms → fail 110 ms
- runtime: geomean hd / Node: pass 1.30x → pass 1.11x
- runtime: fib: hd / Node: pass 0.81x → pass 0.43x
- runtime: sum: hd / Node: pass 1.39x → pass 1.18x
- runtime: map: hd / Node: pass 2.05x → pass 2.60x
- runtime: sort: hd / Node: pass 1.12x → pass 0.97x
- allocations: map loop, bytes per iteration: pass 0.21 B/iter → pass 0.00 B/iter
- size-startup-heap: tiny program, start to first output: fail 50 ms → fail 37 ms
- host-call-overhead: Console println, per call: fail 8.9 µs → fail 5.9 µs
- host-call-overhead: Fs read_text!, per call: fail 24 µs → fail 16 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 43 µs → fail 28 µs
- suspension-overhead: await of a ready value, per await: pass 7.3 ns → pass 13 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 9.5M/s → pass 14.3M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 7.2M/s → pass 9.4M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.06x → fail 0.05x
- text-throughput: string build, hd / Node MB/s: pass 1.48x → pass 1.63x
- text-throughput: split, hd / Node MB/s: pass 1.63x → pass 3.26x
- long-run-memory: RSS growth after warm-up, 1 min: pass 3.2 MB → pass 0.8 MB
- conformance: listed known failures: pass 0 → fail 14

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 54 ms | ≤ 50 ms | FAIL |  |
| edit-latency | p95 check after an edit | 63 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 159 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 242 ms | ≤ 300 ms | PASS |  |
| mistakes | programs with exactly one diagnostic | 95.7% | ≥ 95.0% | PASS | 66 of 69 |
| mistakes | one diagnostic with the expected code | 95.7% | ≥ 95.0% | PASS | 66 of 69; first misses: mut-on-binding-type, mut-on-parameter-type, some-on-result |
| mistakes | fix-it makes hd check pass | 34.8% | ≥ 80.0% | FAIL | 24 of 69 resolved; 24 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 93 tokens | ≤ 60 tokens | FAIL | largest: mut-on-parameter-type; median 44 tokens, 175 B; 64 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 42 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 22.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 32 → 42 ms, start-up 20 ms |
| pathological | deep-blocks: time at 4x | 18 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 17.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 18 ms, start-up 20 ms |
| pathological | deep-parens: time at 4x | 18 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 17.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 18 → 18 ms, start-up 20 ms |
| pathological | method-chain: time at 4x | - | ≤ 2000 ms | FAIL | 4x: hd check failed: thread '&lt;unknown>' (6664) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 4x: hd check failed: thread '&lt;unknown>' (6664) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 1x→2x; 20 → 21 ms, start-up 20 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 39 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 21.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 36 → 39 ms, start-up 20 ms |
| pathological | large-enum-match: time at 4x | 331 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 20.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | 2.13 | ≤ 1.30 | FAIL | 2x→4x; 91 → 331 ms, start-up 20 ms |
| pathological | deep-generic: time at 4x | 20 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 18.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 20 ms, start-up 20 ms |
| pathological | question-chain: time at 4x | 25 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 21.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 25 ms, start-up 20 ms |
| pathological | big-file: time at 4x | 134 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 52.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.14 | ≤ 1.30 | PASS | 2x→4x; 71 → 134 ms, start-up 20 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 24 of 24 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 21 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 2.88x | ≤ 1.30x | FAIL | checked 772 ms, unchecked 268 ms |
| dev-speed | geomean dev / optimized | 1.08x | ≤ 4.00x | PASS | fib 1.07x, sum 0.96x, string-build 1.30x, map 1.11x, sort 0.97x |
| dev-speed | worst case dev / optimized | 1.30x | ≤ 10.00x | PASS | string-build |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 60 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 32.3 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.10 CPU-s, 31.9 MB | completes ≤ 30 s | PASS | wall 76 ms |
| resources | check, small, warm | 0.01 CPU-s, 17.3 MB | completes ≤ 30 s | PASS | wall 19 ms |
| resources | test, small, cold | 1.06 CPU-s, 107.4 MB | completes ≤ 30 s | PASS | wall 528 ms |
| resources | test, small, warm | 0.57 CPU-s, 69.4 MB | completes ≤ 30 s | PASS | wall 282 ms |
| resources | build, small, cold | 0.47 CPU-s, 91.6 MB | completes ≤ 30 s | PASS | wall 233 ms |
| resources | build, small, warm | 0.05 CPU-s, 39.6 MB | completes ≤ 30 s | PASS | wall 59 ms |
| resources | check, 10k, cold | 0.31 CPU-s, 57.0 MB | completes ≤ 30 s | PASS | wall 163 ms |
| resources | check, 10k, warm | 0.06 CPU-s, 32.3 MB | completes ≤ 30 s | PASS | wall 53 ms |
| resources | test, 10k, cold | 1.74 CPU-s, 127.1 MB | completes ≤ 30 s | PASS | wall 913 ms |
| resources | test, 10k, warm | 1.04 CPU-s, 76.3 MB | completes ≤ 30 s | PASS | wall 620 ms |
| resources | build, 10k, cold | 0.70 CPU-s, 114.0 MB | completes ≤ 30 s | PASS | wall 309 ms |
| resources | build, 10k, warm | 0.09 CPU-s, 46.3 MB | completes ≤ 30 s | PASS | wall 94 ms |
| resources | check, 50k, cold | 1.23 CPU-s, 174.5 MB | completes ≤ 150 s | PASS | wall 567 ms |
| resources | check, 50k, warm | 0.30 CPU-s, 94.5 MB | completes ≤ 150 s | PASS | wall 252 ms |
| resources | test, 50k, cold | 9.47 CPU-s, 288.7 MB | completes ≤ 150 s | PASS | wall 7058 ms |
| resources | test, 50k, warm | 7.51 CPU-s, 124.2 MB | completes ≤ 150 s | PASS | wall 6087 ms |
| resources | build, 50k, cold | 1.68 CPU-s, 271.6 MB | completes ≤ 150 s | PASS | wall 766 ms |
| resources | build, 50k, warm | 0.29 CPU-s, 112.8 MB | completes ≤ 150 s | PASS | wall 281 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 42.4 MB | ≤ 22.0 MB | FAIL | RSS 156.4 MB at input 100, 185.9 MB at input 1000; build per input 107 ms at 50, 490 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 4.1 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.7 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 15 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 14.8 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.52x | ≤ 1.50x | FAIL | p95 79 ms at N = 1; 4 cores; total CPU 710 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.04x | ≤ 6.00x | FAIL | p95 79 ms at N = 1; 4 cores; total CPU 2440 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.12 | &lt; 1.00 (sublinear) | FAIL | 110 ms at N = 1, 2440 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 16.3 MB | ≤ 10.0 MB | FAIL | artifacts 6822 B in 10 files; toolchain 16.3 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 9.9 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.66x | ≥ 2.40x | FAIL | by --jobs |
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
| proptest-perf | property cases per second | 50k/s | ≥ 100k/s | FAIL | 6000 extra cases took 121 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 94 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 852 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.7 ms | ≤ 1.0 ms | PASS | 100 tests: 218 ms |
| integration-test-perf | setup per integration test program | 120 ms | ≤ 20 ms | FAIL | 10 programs: 1364 ms for 21 tests, 65 ms per test |
| integration-test-perf | total time growth in programs | 0.996 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 249 ms at 2, 1236 ms at 10 programs |
| integration-test-perf | cost per doc test | 110 ms | ≤ 20 ms | FAIL | 10 doc tests: 1236 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 1.11x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.43x | ≤ 3.00x | PASS | hd 2.4 ms (p10 1.5, p90 6.0), Node 5.5 ms, Python 62 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 1.18x | ≤ 3.00x | PASS | hd 8.0 ms (p10 -3.1, p90 21), Node 6.8 ms, Python 211 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 1.35x | ≤ 3.00x | PASS | hd 9.7 ms (p10 7.9, p90 11), Node 7.2 ms, Python 10 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 2.60x | ≤ 3.00x | PASS | hd 20 ms (p10 15, p90 32), Node 7.6 ms, Python 13 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.97x | ≤ 3.00x | PASS | hd 15 ms (p10 10, p90 19), Node 16 ms, Python 14 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 2337 B | ≤ 2048 B | FAIL | benchmarks: fib 3656 B, sum 3610 B, string-build 5329 B, map 5203 B, sort 5694 B |
| size-startup-heap | tiny program, start to first output | 37 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.95x | ≤ 2.00x | PASS | hd +13.5 MB, Node +14.2 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.39x | ≤ 2.00x | PASS | hd +8.1 MB, Node +20.5 MB over an empty program |
| host-call-overhead | Console println, per call | 5.9 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 16 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 28 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 13 ns | ≤ 100 ns | PASS | 13 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 14.3M/s | ≥ 1.0M/s | PASS | 140 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 9.4M/s | ≥ 1.0M/s | PASS | 214 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.05x | ≥ 0.50x | FAIL | hd 25 MB/s (45488 B document), Node 508 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.10x | ≥ 0.50x | FAIL | hd 24 MB/s (45488 B document), Node 247 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 1.63x | ≥ 0.50x | PASS | hd 236 MB/s, Node 144 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 3.26x | ≥ 0.50x | PASS | hd 1162 MB/s, Node 357 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.12x | ≥ 0.50x | FAIL | hd 12 MB/s, Node 104 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 5343 B | ≤ 10240 B | PASS | 10089 B at 1075 lines, 58182 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 664 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.8 MB | ≤ 10.0 MB | PASS | 21922000 requests; RSS 68.7 MB after warm-up, 69.5 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 5652 of 5652 unlisted cases passed |
| conformance | listed known failures | 14 | ≤ 0 | FAIL | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
