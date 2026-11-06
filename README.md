# Daily goal metrics

Finished: 2026-11-06T04:15:50.454Z. Commit: 40b96c98503ad3d3968f631f49f835b26d676324.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V74 80-Core Processor).
Compiler: target/release/hd.

97 passed, 34 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: fail 71 ms → fail 63 ms
- cold-check: median cold check, 10k lines: pass 244 ms → pass 199 ms
- test-latency: median edit-then-test: fail 509 ms → fail 409 ms
- pathological: overlapping-from: time at 4x: pass 56 ms → pass 48 ms
- pathological: deep-blocks: time at 4x: pass 22 ms → pass 19 ms
- pathological: method-chain: time at 4x: pass 28 ms → pass 25 ms
- pathological: wide-list-literal: time at 4x: pass 59 ms → pass 49 ms
- pathological: large-enum-match: time at 4x: pass 36 ms → pass 31 ms
- pathological: deep-generic: time at 4x: pass 26 ms → pass 23 ms
- pathological: question-chain: time at 4x: pass 32 ms → pass 27 ms
- pathological: big-file: time at 4x: pass 231 ms → pass 173 ms
- dev-speed: geomean dev / optimized: pass 1.31x → fail -
- dev-speed: worst case dev / optimized: pass 2.28x → fail -
- resources: warm check, 10k lines: CPU time: pass 80 ms → pass 70 ms
- long-session: RSS growth, REPL inputs 100 to 1000: fail 41.4 MB → fail 77.1 MB
- proptest-perf: property cases per second: fail 24k/s → fail 33k/s
- unit-test-perf: 1,000 unit tests, warm: fail 1667 ms → fail 1227 ms
- unit-test-perf: overhead per unit test: pass 1.0 ms → pass 0.7 ms
- integration-test-perf: setup per integration test program: fail 448 ms → fail 348 ms
- integration-test-perf: cost per doc test: fail 323 ms → fail 250 ms
- runtime: geomean hd / Node: pass 0.56x → pass 0.74x
- runtime: fib: hd / Node: pass 0.06x → pass 0.30x
- runtime: sum: hd / Node: pass 0.80x → pass 1.31x
- runtime: string-build: hd / Node: pass 0.59x → pass 0.91x
- runtime: map: hd / Node: fail 3.98x → fail 3.58x
- runtime: sort: hd / Node: pass 0.50x → pass 0.17x
- allocations: counted loop, bytes per iteration: pass 0.00 B/iter → pass 0.03 B/iter
- allocations: list loop, bytes per iteration: pass 0.00 B/iter → pass 0.01 B/iter
- allocations: map loop, bytes per iteration: pass 0.01 B/iter → pass 0.00 B/iter
- allocations: iterator chain, bytes per iteration: pass 0.00 B/iter → pass 0.01 B/iter
- size-startup-heap: tiny program, start to first output: fail 156 ms → fail 118 ms
- size-startup-heap: peak heap, string-build: hd / Node: pass 0.25x → pass 0.22x
- suspension-overhead: await of a ready value, per await: pass 27 ns → pass 0.00 ns
- suspension-overhead: race! of two ready tasks, throughput: pass 8.3M/s → pass 9.3M/s
- text-throughput: string build, hd / Node MB/s: pass 3.16x → pass 3.91x
- text-throughput: split, hd / Node MB/s: pass 2.14x → pass 2.95x

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 63 ms | ≤ 50 ms | FAIL |  |
| edit-latency | p95 check after an edit | 66 ms | ≤ 200 ms | PASS | Timing change ≤10%: noise. |
| cold-check | median cold check, 10k lines | 199 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 409 ms | ≤ 300 ms | FAIL |  |
| mistakes | programs with exactly one diagnostic | 95.7% | ≥ 95.0% | PASS | 66 of 69 |
| mistakes | one diagnostic with the expected code | 95.7% | ≥ 95.0% | PASS | 66 of 69; first misses: mut-on-binding-type, mut-on-parameter-type, some-on-result |
| mistakes | fix-it makes hd check pass | 33.3% | ≥ 80.0% | FAIL | 23 of 69 resolved; 23 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 93 tokens | ≤ 60 tokens | FAIL | largest: mut-on-parameter-type; median 43 tokens, 171 B; 64 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 48 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 24.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 35 → 48 ms, start-up 19 ms |
| pathological | deep-blocks: time at 4x | 19 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 19.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 19 ms, start-up 19 ms |
| pathological | deep-parens: time at 4x | 20 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-parens: peak RSS at 4x | 20.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 20 ms, start-up 19 ms |
| pathological | method-chain: time at 4x | 25 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 21.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 25 ms, start-up 19 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 49 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 23.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 34 → 49 ms, start-up 19 ms |
| pathological | large-enum-match: time at 4x | 31 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 22.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 25 → 31 ms, start-up 19 ms |
| pathological | deep-generic: time at 4x | 23 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 20.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 21 → 23 ms, start-up 19 ms |
| pathological | question-chain: time at 4x | 27 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 22.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 23 → 27 ms, start-up 19 ms |
| pathological | big-file: time at 4x | 173 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 53.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.16 | ≤ 1.30 | PASS | 2x→4x; 88 → 173 ms, start-up 19 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 23 of 23 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 25 ms | ≤ 200 ms | PASS | Timing change ≤10%: noise. |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.97x | ≤ 1.30x | FAIL | checked 633 ms, unchecked 321 ms |
| dev-speed | geomean dev / optimized | - | ≤ 4.00x | FAIL | optimized work is within noise: string-build (33.1 ms) |
| dev-speed | worst case dev / optimized | - | ≤ 10.00x | FAIL | optimized work is within noise: string-build (33.1 ms) |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 70 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 34.6 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.14 CPU-s, 35.0 MB | completes ≤ 30 s | PASS | wall 97 ms |
| resources | check, small, warm | 0.01 CPU-s, 18.4 MB | completes ≤ 30 s | PASS | wall 18 ms |
| resources | test, small, cold | 3.04 CPU-s, 120.8 MB | completes ≤ 30 s | PASS | wall 1196 ms |
| resources | test, small, warm | 2.23 CPU-s, 121.5 MB | completes ≤ 30 s | PASS | wall 821 ms |
| resources | build, small, cold | 0.75 CPU-s, 99.0 MB | completes ≤ 30 s | PASS | wall 327 ms |
| resources | build, small, warm | 0.06 CPU-s, 42.9 MB | completes ≤ 30 s | PASS | wall 66 ms |
| resources | check, 10k, cold | 0.40 CPU-s, 62.6 MB | completes ≤ 30 s | PASS | wall 199 ms |
| resources | check, 10k, warm | 0.07 CPU-s, 34.6 MB | completes ≤ 30 s | PASS | wall 58 ms |
| resources | test, 10k, cold | 3.96 CPU-s, 154.8 MB | completes ≤ 30 s | PASS | wall 1921 ms |
| resources | test, 10k, warm | 2.81 CPU-s, 131.0 MB | completes ≤ 30 s | PASS | wall 1280 ms |
| resources | build, 10k, cold | 1.06 CPU-s, 128.2 MB | completes ≤ 30 s | PASS | wall 418 ms |
| resources | build, 10k, warm | 0.11 CPU-s, 50.7 MB | completes ≤ 30 s | PASS | wall 107 ms |
| resources | check, 50k, cold | 1.65 CPU-s, 190.3 MB | completes ≤ 150 s | PASS | wall 704 ms |
| resources | check, 50k, warm | 0.29 CPU-s, 100.6 MB | completes ≤ 150 s | PASS | wall 231 ms |
| resources | test, 50k, cold | 14.38 CPU-s, 352.1 MB | completes ≤ 150 s | PASS | wall 10239 ms |
| resources | test, 50k, warm | 11.04 CPU-s, 183.2 MB | completes ≤ 150 s | PASS | wall 8542 ms |
| resources | build, 50k, cold | 2.35 CPU-s, 306.4 MB | completes ≤ 150 s | PASS | wall 945 ms |
| resources | build, 50k, warm | 0.33 CPU-s, 121.5 MB | completes ≤ 150 s | PASS | wall 317 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 77.1 MB | ≤ 22.0 MB | FAIL | RSS 168.8 MB at input 100, 245.9 MB at input 1000; build per input 193 ms at 50, 705 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.2 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.9 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 16 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 16.2 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.10x | ≤ 1.50x | FAIL | p95 102 ms at N = 1; 4 cores; total CPU 750 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.44x | ≤ 6.00x | FAIL | p95 102 ms at N = 1; 4 cores; total CPU 3270 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.11 | &lt; 1.00 (sublinear) | FAIL | 150 ms at N = 1, 3270 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 18.1 MB | ≤ 10.0 MB | FAIL | artifacts 7824 B in 10 files; toolchain 18.1 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 11.1 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.79x | ≥ 2.40x | FAIL | by --jobs |
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
| proptest-perf | property cases per second | 33k/s | ≥ 100k/s | FAIL | 6000 extra cases took 182 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 87 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 Timing change ≤10%: noise. |
| unit-test-perf | 1,000 unit tests, warm | 1227 ms | ≤ 1000 ms | FAIL | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.7 ms | ≤ 1.0 ms | PASS | 100 tests: 615 ms |
| integration-test-perf | setup per integration test program | 348 ms | ≤ 20 ms | FAIL | 10 programs: 3748 ms for 21 tests, 178 ms per test |
| integration-test-perf | total time growth in programs | 1.000 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 697 ms at 2, 3483 ms at 10 programs |
| integration-test-perf | cost per doc test | 250 ms | ≤ 20 ms | FAIL | 10 doc tests: 2760 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 0.74x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.30x | ≤ 3.00x | PASS | hd 2.6 ms (p10 -7.9, p90 7.3), Node 8.5 ms, Python 78 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 1.31x | ≤ 3.00x | PASS | hd 11 ms (p10 2.1, p90 20), Node 8.3 ms, Python 238 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.91x | ≤ 3.00x | PASS | hd 8.3 ms (p10 0.6, p90 15), Node 9.1 ms, Python 11 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 3.58x | ≤ 3.00x | FAIL | hd 33 ms (p10 29, p90 39), Node 9.2 ms, Python 15 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.17x | ≤ 3.00x | PASS | hd 3.2 ms (p10 -2.2, p90 21), Node 19 ms, Python 15 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.03 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.01 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.01 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 1636 B | ≤ 2048 B | PASS | benchmarks: fib 5415 B, sum 5227 B, string-build 8612 B, map 6627 B, sort 13433 B |
| size-startup-heap | tiny program, start to first output | 118 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.88x | ≤ 2.00x | PASS | hd +12.6 MB, Node +14.3 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.22x | ≤ 2.00x | PASS | hd +4.6 MB, Node +20.7 MB over an empty program |
| host-call-overhead | Console println, per call | 7.1 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm Timing change ≤10%: noise. |
| host-call-overhead | Fs read_text!, per call | 18 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm Timing change ≤10%: noise. |
| host-call-overhead | serde boundary (list_dir!), per call | 32 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 0.00 ns | ≤ 100 ns | PASS | -15 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 11.9M/s | ≥ 1.0M/s | PASS | 168 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 9.3M/s | ≥ 1.0M/s | PASS | 216 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.05x | ≥ 0.50x | FAIL | hd 18 MB/s (45488 B document), Node 367 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.06x | ≥ 0.50x | FAIL | hd 10 MB/s (45488 B document), Node 184 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 3.91x | ≥ 0.50x | PASS | hd 455 MB/s, Node 116 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 2.95x | ≥ 0.50x | PASS | hd 836 MB/s, Node 284 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.13x | ≥ 0.50x | FAIL | hd 11 MB/s, Node 82 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 10753 B | ≤ 10240 B | FAIL | 17217 B at 1075 lines, 114006 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 908 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 19950000 requests; RSS 100.8 MB after warm-up, 100.8 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 6743 of 6743 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
