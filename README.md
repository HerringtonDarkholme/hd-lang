# Daily goal metrics

Finished: 2026-11-05T04:23:54.823Z. Commit: d791c9482d0aa73864d3a721dde205bfc5ade821.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 7763 64-Core Processor).
Compiler: target/release/hd.

98 passed, 33 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: fail 64 ms → fail 81 ms
- edit-latency: p95 check after an edit: pass 66 ms → pass 83 ms
- cold-check: median cold check, 10k lines: pass 203 ms → pass 264 ms
- test-latency: median edit-then-test: fail 431 ms → fail 545 ms
- pathological: overlapping-from: time at 4x: pass 51 ms → pass 69 ms
- pathological: deep-blocks: time at 4x: pass 21 ms → pass 27 ms
- pathological: deep-parens: time at 4x: pass 21 ms → pass 26 ms
- pathological: method-chain: time at 4x: pass 26 ms → pass 36 ms
- pathological: wide-list-literal: time at 4x: pass 51 ms → pass 69 ms
- pathological: large-enum-match: time at 4x: pass 33 ms → pass 40 ms
- pathological: deep-generic: time at 4x: pass 24 ms → pass 30 ms
- pathological: question-chain: time at 4x: pass 28 ms → pass 37 ms
- pathological: big-file: time at 4x: pass 182 ms → pass 234 ms
- fmt: hd fmt, 10k lines: pass 24 ms → pass 32 ms
- dev-speed: worst case dev / optimized: pass 2.88x → pass 2.54x
- resources: warm check, 10k lines: CPU time: pass 70 ms → pass 80 ms
- long-session: RSS growth, REPL inputs 100 to 1000: fail 49.2 MB → fail 86.0 MB
- startup: hd help (no version command): wall time: pass 3.3 ms → pass 3.7 ms
- startup: hd check empty.hd: wall time: pass 16 ms → pass 19 ms
- concurrency: p95 latency at N = 16 vs N = 1: fail 7.60x → fail 8.59x
- proptest-perf: property cases per second: fail 42k/s → fail 27k/s
- proptest-perf: time to shrink a known failure: pass 85 ms → pass 100 ms
- unit-test-perf: 1,000 unit tests, warm: fail 1265 ms → fail 1603 ms
- unit-test-perf: overhead per unit test: pass 0.7 ms → pass 0.9 ms
- integration-test-perf: setup per integration test program: fail 351 ms → fail 452 ms
- integration-test-perf: cost per doc test: fail 252 ms → fail 321 ms
- runtime: geomean hd / Node: fail - → pass 0.66x
- runtime: fib: hd / Node: fail - → pass 0.15x
- runtime: sum: hd / Node: pass 0.73x → pass 0.97x
- runtime: string-build: hd / Node: fail - → pass 0.34x
- runtime: map: hd / Node: fail 3.24x → fail 3.81x
- allocations: counted loop, bytes per iteration: pass 0.01 B/iter → pass 0.03 B/iter
- allocations: iterator chain, bytes per iteration: pass 0.04 B/iter → pass 0.00 B/iter
- size-startup-heap: tiny program, start to first output: fail 124 ms → fail 159 ms
- host-call-overhead: Console println, per call: fail 7.1 µs → fail 8.3 µs
- host-call-overhead: Fs read_text!, per call: fail 18 µs → fail 26 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 31 µs → fail 45 µs
- suspension-overhead: await of a ready value, per await: pass 12 ns → pass 45 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 13.5M/s → pass 9.9M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 9.5M/s → pass 7.9M/s
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.28x → fail 0.25x
- text-throughput: string build, hd / Node MB/s: pass 2.22x → pass 3.14x
- text-throughput: split, hd / Node MB/s: pass 2.20x → pass 2.57x
- text-throughput: regex find_all, hd / Node MB/s: fail 0.14x → fail 0.12x
- dead-code: Wasm bytes per 1,000 lines: pass 5865 B → fail 10756 B
- long-run-memory: RSS growth after warm-up, 1 min: pass 0.0 MB → pass 0.2 MB

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 81 ms | ≤ 50 ms | FAIL |  |
| edit-latency | p95 check after an edit | 83 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 264 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 545 ms | ≤ 300 ms | FAIL |  |
| mistakes | programs with exactly one diagnostic | 95.7% | ≥ 95.0% | PASS | 66 of 69 |
| mistakes | one diagnostic with the expected code | 95.7% | ≥ 95.0% | PASS | 66 of 69; first misses: mut-on-binding-type, mut-on-parameter-type, some-on-result |
| mistakes | fix-it makes hd check pass | 33.3% | ≥ 80.0% | FAIL | 23 of 69 resolved; 23 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 93 tokens | ≤ 60 tokens | FAIL | largest: mut-on-parameter-type; median 43 tokens, 171 B; 64 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 69 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 25.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 46 → 69 ms, start-up 27 ms |
| pathological | deep-blocks: time at 4x | 27 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 19.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 26 → 27 ms, start-up 27 ms |
| pathological | deep-parens: time at 4x | 26 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 19.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 26 → 26 ms, start-up 27 ms |
| pathological | method-chain: time at 4x | 36 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 21.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 29 → 36 ms, start-up 27 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 69 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 23.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 47 → 69 ms, start-up 27 ms |
| pathological | large-enum-match: time at 4x | 40 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 22.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 32 → 40 ms, start-up 27 ms |
| pathological | deep-generic: time at 4x | 30 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 19.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 27 → 30 ms, start-up 27 ms |
| pathological | question-chain: time at 4x | 37 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 23.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 31 → 37 ms, start-up 27 ms |
| pathological | big-file: time at 4x | 234 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 53.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.17 | ≤ 1.30 | PASS | 2x→4x; 118 → 234 ms, start-up 27 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 23 of 23 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 32 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.91x | ≤ 1.30x | FAIL | checked 826 ms, unchecked 431 ms |
| dev-speed | geomean dev / optimized | 1.20x | ≤ 4.00x | PASS | fib 1.09x, sum 1.06x, string-build 0.83x, map 1.02x, sort 2.54x |
| dev-speed | worst case dev / optimized | 2.54x | ≤ 10.00x | PASS | sort |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 80 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 33.8 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.19 CPU-s, 35.1 MB | completes ≤ 30 s | PASS | wall 122 ms |
| resources | check, small, warm | 0.02 CPU-s, 19.1 MB | completes ≤ 30 s | PASS | wall 23 ms |
| resources | test, small, cold | 3.97 CPU-s, 120.7 MB | completes ≤ 30 s | PASS | wall 1538 ms |
| resources | test, small, warm | 3.06 CPU-s, 121.2 MB | completes ≤ 30 s | PASS | wall 1102 ms |
| resources | build, small, cold | 1.04 CPU-s, 99.4 MB | completes ≤ 30 s | PASS | wall 440 ms |
| resources | build, small, warm | 0.08 CPU-s, 42.3 MB | completes ≤ 30 s | PASS | wall 83 ms |
| resources | check, 10k, cold | 0.54 CPU-s, 61.2 MB | completes ≤ 30 s | PASS | wall 261 ms |
| resources | check, 10k, warm | 0.08 CPU-s, 33.8 MB | completes ≤ 30 s | PASS | wall 76 ms |
| resources | test, 10k, cold | 5.29 CPU-s, 148.6 MB | completes ≤ 30 s | PASS | wall 2420 ms |
| resources | test, 10k, warm | 3.63 CPU-s, 130.2 MB | completes ≤ 30 s | PASS | wall 1617 ms |
| resources | build, 10k, cold | 1.42 CPU-s, 127.3 MB | completes ≤ 30 s | PASS | wall 542 ms |
| resources | build, 10k, warm | 0.13 CPU-s, 50.6 MB | completes ≤ 30 s | PASS | wall 132 ms |
| resources | check, 50k, cold | 2.19 CPU-s, 186.0 MB | completes ≤ 150 s | PASS | wall 893 ms |
| resources | check, 50k, warm | 0.39 CPU-s, 98.8 MB | completes ≤ 150 s | PASS | wall 299 ms |
| resources | test, 50k, cold | 19.29 CPU-s, 348.7 MB | completes ≤ 150 s | PASS | wall 13710 ms |
| resources | test, 50k, warm | 14.66 CPU-s, 175.2 MB | completes ≤ 150 s | PASS | wall 11092 ms |
| resources | build, 50k, cold | 3.22 CPU-s, 290.6 MB | completes ≤ 150 s | PASS | wall 1254 ms |
| resources | build, 50k, warm | 0.43 CPU-s, 119.7 MB | completes ≤ 150 s | PASS | wall 403 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 86.0 MB | ≤ 22.0 MB | FAIL | RSS 157.3 MB at input 100, 230.5 MB at input 1000; build per input 141 ms at 50, 874 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.7 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 4.0 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 19 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 16.0 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.17x | ≤ 1.50x | FAIL | p95 126 ms at N = 1; 4 cores; total CPU 1020 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.59x | ≤ 6.00x | FAIL | p95 126 ms at N = 1; 4 cores; total CPU 4230 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.12 | &lt; 1.00 (sublinear) | FAIL | 190 ms at N = 1, 4230 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 17.9 MB | ≤ 10.0 MB | FAIL | artifacts 7824 B in 10 files; toolchain 17.9 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 10.9 MB, shared per user, not counted |
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
| proptest-perf | property cases per second | 27k/s | ≥ 100k/s | FAIL | 6000 extra cases took 221 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 100 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 1603 ms | ≤ 1000 ms | FAIL | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.9 ms | ≤ 1.0 ms | PASS | 100 tests: 769 ms |
| integration-test-perf | setup per integration test program | 452 ms | ≤ 20 ms | FAIL | 10 programs: 4873 ms for 21 tests, 232 ms per test |
| integration-test-perf | total time growth in programs | 0.995 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 914 ms at 2, 4531 ms at 10 programs |
| integration-test-perf | cost per doc test | 321 ms | ≤ 20 ms | FAIL | 10 doc tests: 3567 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 0.66x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.15x | ≤ 3.00x | PASS | hd 1.5 ms (p10 -6.6, p90 7.8), Node 10 ms, Python 98 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 0.97x | ≤ 3.00x | PASS | hd 9.1 ms (p10 0.9, p90 41), Node 9.4 ms, Python 302 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.34x | ≤ 3.00x | PASS | hd 3.6 ms (p10 -1.1, p90 11), Node 11 ms, Python 15 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 3.81x | ≤ 3.00x | FAIL | hd 41 ms (p10 37, p90 54), Node 11 ms, Python 18 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.68x | ≤ 3.00x | PASS | hd 17 ms (p10 9.2, p90 28), Node 25 ms, Python 20 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.03 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 2732 B | ≤ 2048 B | FAIL | benchmarks: fib 5415 B, sum 5227 B, string-build 8681 B, map 6710 B, sort 13650 B |
| size-startup-heap | tiny program, start to first output | 159 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.92x | ≤ 2.00x | PASS | hd +13.0 MB, Node +14.1 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.24x | ≤ 2.00x | PASS | hd +5.2 MB, Node +22.0 MB over an empty program |
| host-call-overhead | Console println, per call | 8.3 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 26 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 45 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 45 ns | ≤ 100 ns | PASS | 45 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 9.9M/s | ≥ 1.0M/s | PASS | 202 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 7.9M/s | ≥ 1.0M/s | PASS | 252 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.21x | ≥ 0.50x | FAIL | hd 58 MB/s (45488 B document), Node 268 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.25x | ≥ 0.50x | FAIL | hd 36 MB/s (45488 B document), Node 145 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 3.14x | ≥ 0.50x | PASS | hd 314 MB/s, Node 100 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 2.57x | ≥ 0.50x | PASS | hd 600 MB/s, Node 233 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.12x | ≥ 0.50x | FAIL | hd 8.4 MB/s, Node 71 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 10756 B | ≤ 10240 B | FAIL | 17350 B at 1075 lines, 114167 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 908 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.2 MB | ≤ 10.0 MB | PASS | 14816000 requests; RSS 101.0 MB after warm-up, 101.2 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 6704 of 6704 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
