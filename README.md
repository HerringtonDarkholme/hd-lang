# Daily goal metrics

Finished: 2026-11-08T03:13:51.825Z. Commit: dddbbb88231b45fa95724973904df328d5c5d1c4.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V45 96-Core Processor).
Compiler: target/release/hd.

97 passed, 34 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 41 ms → pass 35 ms
- edit-latency: p95 check after an edit: pass 43 ms → pass 37 ms
- cold-check: median cold check, 10k lines: pass 174 ms → pass 142 ms
- test-latency: median edit-then-test: pass 297 ms → pass 248 ms
- pathological: overlapping-from: time at 4x: pass 46 ms → pass 37 ms
- pathological: method-chain: time at 4x: pass 23 ms → pass 20 ms
- pathological: wide-list-literal: time at 4x: pass 45 ms → pass 37 ms
- pathological: large-enum-match: time at 4x: pass 29 ms → pass 25 ms
- pathological: question-chain: time at 4x: pass 26 ms → pass 23 ms
- pathological: big-file: time at 4x: pass 164 ms → pass 130 ms
- fmt: hd fmt, 10k lines: pass 23 ms → pass 17 ms
- resources: warm check, 10k lines: CPU time: pass 40 ms → pass 30 ms
- long-session: RSS growth, REPL inputs 100 to 1000: fail 92.4 MB → fail 73.0 MB
- proptest-perf: property cases per second: fail 48k/s → fail 70k/s
- proptest-perf: time to shrink a known failure: pass 81 ms → pass 69 ms
- unit-test-perf: 1,000 unit tests, warm: fail 1204 ms → pass 989 ms
- unit-test-perf: overhead per unit test: pass 0.7 ms → pass 0.5 ms
- integration-test-perf: setup per integration test program: fail 319 ms → fail 271 ms
- integration-test-perf: total time growth in programs: pass 0.994 → fail 1.001
- integration-test-perf: cost per doc test: fail 225 ms → fail 189 ms
- runtime: geomean hd / Node: pass 0.49x → fail -
- runtime: fib: hd / Node: pass 0.03x → fail -
- runtime: string-build: hd / Node: pass 0.63x → pass 0.48x
- runtime: sort: hd / Node: pass 0.41x → pass 0.36x
- allocations: iterator chain, bytes per iteration: pass 0.00 B/iter → pass 0.04 B/iter
- size-startup-heap: tiny program, start to first output: fail 120 ms → fail 102 ms
- size-startup-heap: peak heap, map: hd / Node: pass 0.93x → pass 0.83x
- size-startup-heap: peak heap, string-build: hd / Node: pass 0.28x → pass 0.19x
- host-call-overhead: Console println, per call: fail 6.8 µs → fail 5.7 µs
- host-call-overhead: Fs read_text!, per call: fail 18 µs → fail 14 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 32 µs → fail 25 µs
- suspension-overhead: await of a ready value, per await: pass 20 ns → pass 0.00 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 13.7M/s → pass 18.8M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 10.6M/s → pass 16.7M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.27x → fail 0.17x
- text-throughput: string build, hd / Node MB/s: pass 4.77x → pass 3.70x
- text-throughput: split, hd / Node MB/s: pass 1.83x → pass 3.15x

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 35 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 37 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 142 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 248 ms | ≤ 300 ms | PASS |  |
| mistakes | programs with exactly one diagnostic | 95.7% | ≥ 95.0% | PASS | 66 of 69 |
| mistakes | one diagnostic with the expected code | 95.7% | ≥ 95.0% | PASS | 66 of 69; first misses: mut-on-binding-type, mut-on-parameter-type, some-on-result |
| mistakes | fix-it makes hd check pass | 33.3% | ≥ 80.0% | FAIL | 23 of 69 resolved; 23 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 93 tokens | ≤ 60 tokens | FAIL | largest: mut-on-parameter-type; median 43 tokens, 171 B; 64 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 37 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 24.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 30 → 37 ms, start-up 17 ms |
| pathological | deep-blocks: time at 4x | 19 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 19.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 17 → 19 ms, start-up 17 ms |
| pathological | deep-parens: time at 4x | 18 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 19.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 18 → 18 ms, start-up 17 ms |
| pathological | method-chain: time at 4x | 20 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 21.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 18 → 20 ms, start-up 17 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 37 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 22.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 28 → 37 ms, start-up 17 ms |
| pathological | large-enum-match: time at 4x | 25 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 22.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 25 ms, start-up 17 ms |
| pathological | deep-generic: time at 4x | 19 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-generic: peak RSS at 4x | 19.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 18 → 19 ms, start-up 17 ms |
| pathological | question-chain: time at 4x | 23 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 23.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 23 ms, start-up 17 ms |
| pathological | big-file: time at 4x | 130 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 52.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.23 | ≤ 1.30 | PASS | 2x→4x; 66 → 130 ms, start-up 17 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 23 of 23 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 17 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.89x | ≤ 1.30x | FAIL | checked 445 ms, unchecked 236 ms |
| dev-speed | geomean dev / optimized | - | ≤ 4.00x | FAIL | optimized work is within noise: string-build (32.1 ms) |
| dev-speed | worst case dev / optimized | - | ≤ 10.00x | FAIL | optimized work is within noise: string-build (32.1 ms) |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 30 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 27.2 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.11 CPU-s, 34.7 MB | completes ≤ 30 s | PASS | wall 84 ms |
| resources | check, small, warm | 0.00 CPU-s, 17.5 MB | completes ≤ 30 s | PASS | wall 15 ms |
| resources | test, small, cold | 2.56 CPU-s, 120.8 MB | completes ≤ 30 s | PASS | wall 965 ms |
| resources | test, small, warm | 1.95 CPU-s, 121.0 MB | completes ≤ 30 s | PASS | wall 705 ms |
| resources | build, small, cold | 0.55 CPU-s, 81.6 MB | completes ≤ 30 s | PASS | wall 228 ms |
| resources | build, small, warm | 0.02 CPU-s, 31.3 MB | completes ≤ 30 s | PASS | wall 27 ms |
| resources | check, 10k, cold | 0.30 CPU-s, 57.5 MB | completes ≤ 30 s | PASS | wall 148 ms |
| resources | check, 10k, warm | 0.03 CPU-s, 27.2 MB | completes ≤ 30 s | PASS | wall 33 ms |
| resources | test, 10k, cold | 3.29 CPU-s, 128.8 MB | completes ≤ 30 s | PASS | wall 1418 ms |
| resources | test, 10k, warm | 2.34 CPU-s, 130.4 MB | completes ≤ 30 s | PASS | wall 962 ms |
| resources | build, 10k, cold | 0.81 CPU-s, 106.7 MB | completes ≤ 30 s | PASS | wall 311 ms |
| resources | build, 10k, warm | 0.04 CPU-s, 36.5 MB | completes ≤ 30 s | PASS | wall 47 ms |
| resources | check, 50k, cold | 1.18 CPU-s, 164.3 MB | completes ≤ 150 s | PASS | wall 472 ms |
| resources | check, 50k, warm | 0.16 CPU-s, 68.0 MB | completes ≤ 150 s | PASS | wall 113 ms |
| resources | test, 50k, cold | 10.49 CPU-s, 277.5 MB | completes ≤ 150 s | PASS | wall 7041 ms |
| resources | test, 50k, warm | 7.66 CPU-s, 171.2 MB | completes ≤ 150 s | PASS | wall 5546 ms |
| resources | build, 50k, cold | 1.69 CPU-s, 210.0 MB | completes ≤ 150 s | PASS | wall 610 ms |
| resources | build, 50k, warm | 0.14 CPU-s, 76.6 MB | completes ≤ 150 s | PASS | wall 137 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 73.0 MB | ≤ 22.0 MB | FAIL | RSS 102.9 MB at input 100, 175.8 MB at input 1000; build per input 58 ms at 50, 523 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.2 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 4.0 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 14 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 15.9 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.08x | ≤ 1.50x | FAIL | p95 81 ms at N = 1; 4 cores; total CPU 590 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.41x | ≤ 6.00x | FAIL | p95 81 ms at N = 1; 4 cores; total CPU 2560 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.14 | &lt; 1.00 (sublinear) | FAIL | 110 ms at N = 1, 2560 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 18.3 MB | ≤ 10.0 MB | FAIL | artifacts 7847 B in 10 files; toolchain 18.3 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 11.1 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.93x | ≥ 2.40x | FAIL | by --jobs |
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
| proptest-perf | property cases per second | 70k/s | ≥ 100k/s | FAIL | 6000 extra cases took 86 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 69 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 989 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.5 ms | ≤ 1.0 ms | PASS | 100 tests: 502 ms |
| integration-test-perf | setup per integration test program | 271 ms | ≤ 20 ms | FAIL | 10 programs: 2901 ms for 21 tests, 138 ms per test |
| integration-test-perf | total time growth in programs | 1.001 | exponent &lt; 1 (sublinear) | FAIL | beyond the 0-program run: 540 ms at 2, 2706 ms at 10 programs |
| integration-test-perf | cost per doc test | 189 ms | ≤ 20 ms | FAIL | 10 doc tests: 2076 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | - | ≤ 1.50x | FAIL | 1 of 5 cases not measured |
| runtime | fib: hd / Node | - | ≤ 3.00x | FAIL | no work measured beyond start-up; hd -1.8 ms (p10 -4.6, p90 2.6), Node 5.6 ms, Python 51 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 1.03x | ≤ 3.00x | PASS | hd 6.9 ms (p10 0.1, p90 13), Node 6.8 ms, Python 161 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.48x | ≤ 3.00x | PASS | hd 3.3 ms (p10 -7.4, p90 8.1), Node 7.0 ms, Python 8.6 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 3.97x | ≤ 3.00x | FAIL | hd 29 ms (p10 19, p90 32), Node 7.3 ms, Python 11 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.36x | ≤ 3.00x | PASS | hd 5.5 ms (p10 4.3, p90 13), Node 15 ms, Python 10 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.04 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 1628 B | ≤ 2048 B | PASS | benchmarks: fib 5414 B, sum 5226 B, string-build 8628 B, map 6631 B, sort 13538 B |
| size-startup-heap | tiny program, start to first output | 102 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.83x | ≤ 2.00x | PASS | hd +11.9 MB, Node +14.4 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.19x | ≤ 2.00x | PASS | hd +4.0 MB, Node +20.7 MB over an empty program |
| host-call-overhead | Console println, per call | 5.7 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 14 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 25 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 0.00 ns | ≤ 100 ns | PASS | -0.69 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 18.8M/s | ≥ 1.0M/s | PASS | 106 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 16.7M/s | ≥ 1.0M/s | PASS | 120 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.17x | ≥ 0.50x | FAIL | hd 87 MB/s (45488 B document), Node 515 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.28x | ≥ 0.50x | FAIL | hd 69 MB/s (45488 B document), Node 249 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 3.70x | ≥ 0.50x | PASS | hd 594 MB/s, Node 160 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 3.15x | ≥ 0.50x | PASS | hd 1165 MB/s, Node 370 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.11x | ≥ 0.50x | FAIL | hd 13 MB/s, Node 113 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 10676 B | ≤ 10240 B | FAIL | 17132 B at 1075 lines, 113231 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 908 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 30390000 requests; RSS 97.8 MB after warm-up, 97.9 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 6754 of 6754 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
- [2026-10-27T18](daily/2026-10-27T18.json)
- [2026-10-27T02](daily/2026-10-27T02.json)
- [2026-10-26T18](daily/2026-10-26T18.json)
- [2026-10-26T03](daily/2026-10-26T03.json)
- [2026-10-25](daily/2026-10-25.json)
