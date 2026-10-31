# Daily goal metrics

Finished: 2026-10-31T03:23:57.187Z. Commit: 4e2b9ae711ce45f5b21532a284eb13e65985fef3.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 7763 64-Core Processor).
Compiler: target/release/hd.

96 passed, 35 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: fail 57 ms → fail 76 ms
- edit-latency: p95 check after an edit: pass 58 ms → pass 79 ms
- cold-check: median cold check, 10k lines: pass 172 ms → pass 229 ms
- test-latency: median edit-then-test: pass 252 ms → fail 501 ms
- pathological: overlapping-from: time at 4x: pass 45 ms → pass 61 ms
- pathological: deep-blocks: time at 4x: pass 18 ms → pass 24 ms
- pathological: deep-parens: time at 4x: pass 17 ms → pass 22 ms
- pathological: wide-list-literal: time at 4x: pass 43 ms → pass 64 ms
- pathological: large-enum-match: time at 4x: pass 477 ms → pass 593 ms
- pathological: deep-generic: time at 4x: pass 19 ms → pass 26 ms
- pathological: question-chain: time at 4x: pass 24 ms → pass 33 ms
- pathological: big-file: time at 4x: pass 153 ms → pass 215 ms
- fmt: hd fmt, 10k lines: pass 24 ms → pass 33 ms
- check-cost: checked run time / unchecked run time: fail 2.75x → fail 3.08x
- dev-speed: worst case dev / optimized: pass 1.07x → pass 1.20x
- resources: warm check, 10k lines: CPU time: pass 60 ms → pass 80 ms
- startup: hd help (no version command): wall time: pass 3.1 ms → pass 3.8 ms
- startup: hd check empty.hd: wall time: pass 13 ms → pass 17 ms
- proptest-perf: property cases per second: fail 38k/s → fail 26k/s
- unit-test-perf: 1,000 unit tests, warm: pass 862 ms → fail 1663 ms
- unit-test-perf: overhead per unit test: pass 0.7 ms → pass 1.0 ms
- integration-test-perf: setup per integration test program: fail 126 ms → fail 452 ms
- integration-test-perf: cost per doc test: fail 113 ms → fail 319 ms
- runtime: geomean hd / Node: pass 1.23x → pass 0.55x
- runtime: fib: hd / Node: pass 0.47x → pass 0.31x
- runtime: sum: hd / Node: pass 1.89x → pass 0.60x
- runtime: string-build: hd / Node: pass 1.16x → pass 0.17x
- runtime: map: hd / Node: pass 2.42x → fail 3.66x
- runtime: sort: hd / Node: pass 1.13x → pass 0.44x
- allocations: list loop, bytes per iteration: pass 0.00 B/iter → pass 0.01 B/iter
- allocations: map loop, bytes per iteration: pass 0.00 B/iter → pass 0.02 B/iter
- allocations: iterator chain, bytes per iteration: pass 0.00 B/iter → pass 0.01 B/iter
- size-startup-heap: tiny program, start to first output: fail 38 ms → fail 157 ms
- size-startup-heap: peak heap, string-build: hd / Node: pass 0.39x → pass 0.21x
- host-call-overhead: Console println, per call: fail 6.8 µs → fail 8.5 µs
- host-call-overhead: Fs read_text!, per call: fail 18 µs → fail 26 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 32 µs → fail 47 µs
- suspension-overhead: await of a ready value, per await: pass 30 ns → pass 0.00 ns
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.05x → fail 0.07x
- long-run-memory: RSS growth after warm-up, 1 min: pass 3.2 MB → pass 0.0 MB

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 76 ms | ≤ 50 ms | FAIL |  |
| edit-latency | p95 check after an edit | 79 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 229 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 501 ms | ≤ 300 ms | FAIL |  |
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
| pathological | overlapping-from: peak RSS at 4x | 23.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 41 → 61 ms, start-up 23 ms |
| pathological | deep-blocks: time at 4x | 24 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 18.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 26 → 24 ms, start-up 23 ms |
| pathological | deep-parens: time at 4x | 22 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 18.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 22 ms, start-up 23 ms |
| pathological | method-chain: time at 4x | - | ≤ 2000 ms | FAIL | 4x: hd check failed: thread '&lt;unknown>' (7662) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 4x: hd check failed: thread '&lt;unknown>' (7662) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 1x→2x; 26 → 27 ms, start-up 23 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 64 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 21.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 42 → 64 ms, start-up 23 ms |
| pathological | large-enum-match: time at 4x | 593 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 20.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | 2.48 | ≤ 1.30 | FAIL | 2x→4x; 126 → 593 ms, start-up 23 ms |
| pathological | deep-generic: time at 4x | 26 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 18.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 24 → 26 ms, start-up 23 ms |
| pathological | question-chain: time at 4x | 33 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 21.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 28 → 33 ms, start-up 23 ms |
| pathological | big-file: time at 4x | 215 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 51.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.17 | ≤ 1.30 | PASS | 2x→4x; 109 → 215 ms, start-up 23 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 24 of 24 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 33 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 3.08x | ≤ 1.30x | FAIL | checked 1254 ms, unchecked 407 ms |
| dev-speed | geomean dev / optimized | 1.08x | ≤ 4.00x | PASS | fib 1.03x, sum 1.11x, string-build 1.20x, map 1.03x, sort 1.03x |
| dev-speed | worst case dev / optimized | 1.20x | ≤ 10.00x | PASS | string-build |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 80 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 32.3 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.16 CPU-s, 32.0 MB | completes ≤ 30 s | PASS | wall 109 ms |
| resources | check, small, warm | 0.01 CPU-s, 17.5 MB | completes ≤ 30 s | PASS | wall 22 ms |
| resources | test, small, cold | 3.75 CPU-s, 120.0 MB | completes ≤ 30 s | PASS | wall 1416 ms |
| resources | test, small, warm | 2.97 CPU-s, 120.2 MB | completes ≤ 30 s | PASS | wall 1101 ms |
| resources | build, small, cold | 0.76 CPU-s, 92.2 MB | completes ≤ 30 s | PASS | wall 339 ms |
| resources | build, small, warm | 0.07 CPU-s, 39.0 MB | completes ≤ 30 s | PASS | wall 75 ms |
| resources | check, 10k, cold | 0.47 CPU-s, 57.1 MB | completes ≤ 30 s | PASS | wall 231 ms |
| resources | check, 10k, warm | 0.08 CPU-s, 32.3 MB | completes ≤ 30 s | PASS | wall 71 ms |
| resources | test, 10k, cold | 4.78 CPU-s, 134.2 MB | completes ≤ 30 s | PASS | wall 2195 ms |
| resources | test, 10k, warm | 3.61 CPU-s, 129.6 MB | completes ≤ 30 s | PASS | wall 1613 ms |
| resources | build, 10k, cold | 1.12 CPU-s, 115.9 MB | completes ≤ 30 s | PASS | wall 451 ms |
| resources | build, 10k, warm | 0.13 CPU-s, 47.0 MB | completes ≤ 30 s | PASS | wall 123 ms |
| resources | check, 50k, cold | 1.96 CPU-s, 175.2 MB | completes ≤ 150 s | PASS | wall 818 ms |
| resources | check, 50k, warm | 0.38 CPU-s, 94.2 MB | completes ≤ 150 s | PASS | wall 292 ms |
| resources | test, 50k, cold | 17.62 CPU-s, 299.2 MB | completes ≤ 150 s | PASS | wall 12508 ms |
| resources | test, 50k, warm | 14.23 CPU-s, 181.4 MB | completes ≤ 150 s | PASS | wall 10771 ms |
| resources | build, 50k, cold | 2.69 CPU-s, 272.9 MB | completes ≤ 150 s | PASS | wall 1103 ms |
| resources | build, 50k, warm | 0.41 CPU-s, 113.7 MB | completes ≤ 150 s | PASS | wall 391 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 84.1 MB | ≤ 22.0 MB | FAIL | RSS 138.3 MB at input 100, 209.5 MB at input 1000; build per input 110 ms at 50, 734 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.8 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.8 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 17 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 15.1 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.15x | ≤ 1.50x | FAIL | p95 111 ms at N = 1; 4 cores; total CPU 850 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.69x | ≤ 6.00x | FAIL | p95 111 ms at N = 1; 4 cores; total CPU 3660 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.11 | &lt; 1.00 (sublinear) | FAIL | 170 ms at N = 1, 3660 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 16.7 MB | ≤ 10.0 MB | FAIL | artifacts 6822 B in 10 files; toolchain 16.7 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 10.0 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.70x | ≥ 2.40x | FAIL | by --jobs |
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
| proptest-perf | property cases per second | 26k/s | ≥ 100k/s | FAIL | 6000 extra cases took 226 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 107 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 Timing change ≤10%: noise. |
| unit-test-perf | 1,000 unit tests, warm | 1663 ms | ≤ 1000 ms | FAIL | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 1.0 ms | ≤ 1.0 ms | PASS | 100 tests: 783 ms |
| integration-test-perf | setup per integration test program | 452 ms | ≤ 20 ms | FAIL | 10 programs: 4856 ms for 21 tests, 231 ms per test |
| integration-test-perf | total time growth in programs | 0.991 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 916 ms at 2, 4510 ms at 10 programs |
| integration-test-perf | cost per doc test | 319 ms | ≤ 20 ms | FAIL | 10 doc tests: 3560 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 0.55x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.31x | ≤ 3.00x | PASS | hd 3.2 ms (p10 -3.7, p90 11), Node 10 ms, Python 97 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 0.60x | ≤ 3.00x | PASS | hd 5.6 ms (p10 1.7, p90 15), Node 9.4 ms, Python 281 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.17x | ≤ 3.00x | PASS | hd 2.0 ms (p10 -3.5, p90 9.2), Node 11 ms, Python 16 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 3.66x | ≤ 3.00x | FAIL | hd 41 ms (p10 34, p90 44), Node 11 ms, Python 18 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.44x | ≤ 3.00x | PASS | hd 11 ms (p10 2.8, p90 24), Node 25 ms, Python 19 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.01 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.02 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.01 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 2337 B | ≤ 2048 B | FAIL | benchmarks: fib 3656 B, sum 3610 B, string-build 5329 B, map 5203 B, sort 5694 B |
| size-startup-heap | tiny program, start to first output | 157 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.86x | ≤ 2.00x | PASS | hd +12.3 MB, Node +14.4 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.21x | ≤ 2.00x | PASS | hd +4.7 MB, Node +21.8 MB over an empty program |
| host-call-overhead | Console println, per call | 8.5 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 26 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 47 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 0.00 ns | ≤ 100 ns | PASS | -2.2 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 10.4M/s | ≥ 1.0M/s | PASS | 193 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 9.0M/s | ≥ 1.0M/s | PASS | 223 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.07x | ≥ 0.50x | FAIL | hd 20 MB/s (45488 B document), Node 266 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.11x | ≥ 0.50x | FAIL | hd 17 MB/s (45488 B document), Node 145 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 1.78x | ≥ 0.50x | PASS | hd 156 MB/s, Node 88 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 2.90x | ≥ 0.50x | PASS | hd 596 MB/s, Node 206 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.11x | ≥ 0.50x | FAIL | hd 6.8 MB/s, Node 64 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 5343 B | ≤ 10240 B | PASS | 10089 B at 1075 lines, 58182 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 664 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 12248000 requests; RSS 98.7 MB after warm-up, 98.7 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 5785 of 5785 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
