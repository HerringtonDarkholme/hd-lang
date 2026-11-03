# Daily goal metrics

Finished: 2026-11-03T19:43:00.637Z. Commit: 13a9ad8c67804dd504392335dbd219fee357c26f.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 7763 64-Core Processor).
Compiler: target/release/hd.

96 passed, 35 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: fail 51 ms → fail 81 ms
- edit-latency: p95 check after an edit: pass 52 ms → pass 85 ms
- cold-check: median cold check, 10k lines: pass 151 ms → pass 259 ms
- test-latency: median edit-then-test: fail 320 ms → fail 543 ms
- pathological: overlapping-from: time at 4x: pass 41 ms → pass 66 ms
- pathological: deep-blocks: time at 4x: pass 17 ms → pass 27 ms
- pathological: deep-parens: time at 4x: pass 17 ms → pass 25 ms
- pathological: method-chain: time at 4x: pass 22 ms → pass 32 ms
- pathological: wide-list-literal: time at 4x: pass 39 ms → pass 69 ms
- pathological: large-enum-match: time at 4x: pass 25 ms → pass 40 ms
- pathological: deep-generic: time at 4x: pass 18 ms → pass 30 ms
- pathological: question-chain: time at 4x: pass 22 ms → pass 36 ms
- pathological: big-file: time at 4x: pass 140 ms → pass 246 ms
- pathological: big-file: growth exponent: fail 1.33 → pass 1.22
- fmt: hd fmt, 10k lines: pass 18 ms → pass 33 ms
- dev-speed: worst case dev / optimized: pass 1.28x → pass 0.99x
- resources: warm check, 10k lines: CPU time: pass 40 ms → pass 90 ms
- long-session: RSS growth, REPL inputs 100 to 1000: fail 74.1 MB → fail 52.0 MB
- startup: hd help (no version command): wall time: pass 2.9 ms → pass 3.8 ms
- startup: hd check empty.hd: wall time: pass 12 ms → pass 18 ms
- startup: hd check empty.hd: CPU time: pass 0.0 ms → pass 10 ms
- concurrency: p95 latency at N = 4 vs N = 1: fail 2.01x → fail 2.22x
- proptest-perf: property cases per second: fail 71k/s → fail 24k/s
- proptest-perf: time to shrink a known failure: pass 54 ms → pass 101 ms
- unit-test-perf: 1,000 unit tests, warm: pass 983 ms → fail 1766 ms
- unit-test-perf: overhead per unit test: pass 0.6 ms → fail 1.0 ms
- integration-test-perf: setup per integration test program: fail 280 ms → fail 505 ms
- integration-test-perf: total time growth in programs: fail 1.000 → pass 0.984
- integration-test-perf: cost per doc test: fail 194 ms → fail 378 ms
- runtime: geomean hd / Node: fail - → pass 1.37x
- runtime: fib: hd / Node: pass 0.67x → pass 0.56x
- runtime: sum: hd / Node: fail - → pass 1.26x
- runtime: string-build: hd / Node: pass 0.02x → pass 0.50x
- runtime: map: hd / Node: fail 3.52x → fail 4.02x
- allocations: list loop, bytes per iteration: pass 0.03 B/iter → pass 0.01 B/iter
- allocations: map loop, bytes per iteration: pass 0.01 B/iter → pass 0.00 B/iter
- allocations: iterator chain, bytes per iteration: pass 0.00 B/iter → pass 0.01 B/iter
- size-startup-heap: tiny program, start to first output: fail 97 ms → fail 199 ms
- size-startup-heap: peak heap, map: hd / Node: pass 0.83x → pass 0.95x
- host-call-overhead: Console println, per call: fail 5.6 µs → fail 9.6 µs
- host-call-overhead: Fs read_text!, per call: fail 13 µs → fail 27 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 24 µs → fail 49 µs
- suspension-overhead: await of a ready value, per await: pass 14 ns → pass 48 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 16.8M/s → pass 12.5M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 14.7M/s → pass 8.2M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.16x → fail 0.14x
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.20x → fail 0.23x
- text-throughput: split, hd / Node MB/s: pass 4.86x → pass 2.07x
- text-throughput: regex find_all, hd / Node MB/s: fail 0.11x → fail 0.09x
- long-run-memory: RSS growth after warm-up, 1 min: pass 0.0 MB → pass 0.2 MB
- conformance: listed known failures: pass 0 → fail 9

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 81 ms | ≤ 50 ms | FAIL |  |
| edit-latency | p95 check after an edit | 85 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 259 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 543 ms | ≤ 300 ms | FAIL |  |
| mistakes | programs with exactly one diagnostic | 95.7% | ≥ 95.0% | PASS | 66 of 69 |
| mistakes | one diagnostic with the expected code | 95.7% | ≥ 95.0% | PASS | 66 of 69; first misses: mut-on-binding-type, mut-on-parameter-type, some-on-result |
| mistakes | fix-it makes hd check pass | 34.8% | ≥ 80.0% | FAIL | 24 of 69 resolved; 24 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 93 tokens | ≤ 60 tokens | FAIL | largest: mut-on-parameter-type; median 44 tokens, 175 B; 64 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 66 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 24.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 46 → 66 ms, start-up 25 ms |
| pathological | deep-blocks: time at 4x | 27 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 19.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 25 → 27 ms, start-up 25 ms |
| pathological | deep-parens: time at 4x | 25 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 18.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 27 → 25 ms, start-up 25 ms |
| pathological | method-chain: time at 4x | 32 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 20.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 29 → 32 ms, start-up 25 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 69 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 22.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 47 → 69 ms, start-up 25 ms |
| pathological | large-enum-match: time at 4x | 40 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 22.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 32 → 40 ms, start-up 25 ms |
| pathological | deep-generic: time at 4x | 30 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 19.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 26 → 30 ms, start-up 25 ms |
| pathological | question-chain: time at 4x | 36 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 21.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 30 → 36 ms, start-up 25 ms |
| pathological | big-file: time at 4x | 246 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 53.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.22 | ≤ 1.30 | PASS | 2x→4x; 120 → 246 ms, start-up 25 ms |
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
| check-cost | checked run time / unchecked run time | 1.97x | ≤ 1.30x | FAIL | checked 791 ms, unchecked 401 ms |
| dev-speed | geomean dev / optimized | 0.97x | ≤ 4.00x | PASS | fib 0.94x, sum 0.96x, string-build 0.98x, map 0.98x, sort 0.99x |
| dev-speed | worst case dev / optimized | 0.99x | ≤ 10.00x | PASS | sort |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 90 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 33.0 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.19 CPU-s, 34.2 MB | completes ≤ 30 s | PASS | wall 126 ms |
| resources | check, small, warm | 0.02 CPU-s, 18.8 MB | completes ≤ 30 s | PASS | wall 24 ms |
| resources | test, small, cold | 4.15 CPU-s, 120.6 MB | completes ≤ 30 s | PASS | wall 1587 ms |
| resources | test, small, warm | 3.11 CPU-s, 121.9 MB | completes ≤ 30 s | PASS | wall 1172 ms |
| resources | build, small, cold | 1.00 CPU-s, 98.7 MB | completes ≤ 30 s | PASS | wall 420 ms |
| resources | build, small, warm | 0.08 CPU-s, 41.9 MB | completes ≤ 30 s | PASS | wall 80 ms |
| resources | check, 10k, cold | 0.55 CPU-s, 59.8 MB | completes ≤ 30 s | PASS | wall 268 ms |
| resources | check, 10k, warm | 0.09 CPU-s, 33.0 MB | completes ≤ 30 s | PASS | wall 75 ms |
| resources | test, 10k, cold | 5.25 CPU-s, 144.5 MB | completes ≤ 30 s | PASS | wall 2255 ms |
| resources | test, 10k, warm | 3.77 CPU-s, 131.4 MB | completes ≤ 30 s | PASS | wall 1641 ms |
| resources | build, 10k, cold | 1.40 CPU-s, 117.9 MB | completes ≤ 30 s | PASS | wall 545 ms |
| resources | build, 10k, warm | 0.13 CPU-s, 49.4 MB | completes ≤ 30 s | PASS | wall 133 ms |
| resources | check, 50k, cold | 2.21 CPU-s, 179.3 MB | completes ≤ 150 s | PASS | wall 911 ms |
| resources | check, 50k, warm | 0.40 CPU-s, 95.9 MB | completes ≤ 150 s | PASS | wall 302 ms |
| resources | test, 50k, cold | 18.95 CPU-s, 302.2 MB | completes ≤ 150 s | PASS | wall 13218 ms |
| resources | test, 50k, warm | 15.14 CPU-s, 181.7 MB | completes ≤ 150 s | PASS | wall 11310 ms |
| resources | build, 50k, cold | 3.20 CPU-s, 281.9 MB | completes ≤ 150 s | PASS | wall 1254 ms |
| resources | build, 50k, warm | 0.42 CPU-s, 116.5 MB | completes ≤ 150 s | PASS | wall 405 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 52.0 MB | ≤ 22.0 MB | FAIL | RSS 166.3 MB at input 100, 216.6 MB at input 1000; build per input 153 ms at 50, 779 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.8 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 4.0 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 18 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 10 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 16.1 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.22x | ≤ 1.50x | FAIL | p95 124 ms at N = 1; 4 cores; total CPU 990 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.71x | ≤ 6.00x | FAIL | p95 124 ms at N = 1; 4 cores; total CPU 4100 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.11 | &lt; 1.00 (sublinear) | FAIL | 190 ms at N = 1, 4100 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 17.4 MB | ≤ 10.0 MB | FAIL | artifacts 7033 B in 10 files; toolchain 17.4 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 10.5 MB, shared per user, not counted |
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
| proptest-perf | property cases per second | 24k/s | ≥ 100k/s | FAIL | 6000 extra cases took 250 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 101 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 1766 ms | ≤ 1000 ms | FAIL | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 1.0 ms | ≤ 1.0 ms | FAIL | 100 tests: 873 ms |
| integration-test-perf | setup per integration test program | 505 ms | ≤ 20 ms | FAIL | 10 programs: 5563 ms for 21 tests, 265 ms per test |
| integration-test-perf | total time growth in programs | 0.984 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 1065 ms at 2, 5191 ms at 10 programs |
| integration-test-perf | cost per doc test | 378 ms | ≤ 20 ms | FAIL | 10 doc tests: 4028 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 1.37x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.56x | ≤ 3.00x | PASS | hd 5.7 ms (p10 -6.9, p90 20), Node 10 ms, Python 99 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 1.26x | ≤ 3.00x | PASS | hd 12 ms (p10 -14.8, p90 24), Node 9.4 ms, Python 290 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.50x | ≤ 3.00x | PASS | hd 5.5 ms (p10 -6.3, p90 19), Node 11 ms, Python 16 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 4.02x | ≤ 3.00x | FAIL | hd 48 ms (p10 30, p90 67), Node 12 ms, Python 18 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 3.47x | ≤ 3.00x | FAIL | hd 88 ms (p10 67, p90 97), Node 25 ms, Python 21 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.01 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.01 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 2334 B | ≤ 2048 B | FAIL | benchmarks: fib 3876 B, sum 3830 B, string-build 5796 B, map 5423 B, sort 6390 B |
| size-startup-heap | tiny program, start to first output | 199 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.95x | ≤ 2.00x | PASS | hd +13.6 MB, Node +14.4 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.20x | ≤ 2.00x | PASS | hd +4.1 MB, Node +20.8 MB over an empty program |
| host-call-overhead | Console println, per call | 9.6 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 27 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 49 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 48 ns | ≤ 100 ns | PASS | 48 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 12.5M/s | ≥ 1.0M/s | PASS | 160 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 8.2M/s | ≥ 1.0M/s | PASS | 244 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.14x | ≥ 0.50x | FAIL | hd 31 MB/s (45488 B document), Node 231 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.23x | ≥ 0.50x | FAIL | hd 32 MB/s (45488 B document), Node 142 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 1.83x | ≥ 0.50x | PASS | hd 173 MB/s, Node 95 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 2.07x | ≥ 0.50x | PASS | hd 472 MB/s, Node 228 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.09x | ≥ 0.50x | FAIL | hd 6.4 MB/s, Node 68 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 5325 B | ≤ 10240 B | PASS | 10425 B at 1075 lines, 58352 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 664 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.2 MB | ≤ 10.0 MB | PASS | 12774000 requests; RSS 99.6 MB after warm-up, 99.8 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 6169 of 6169 unlisted cases passed |
| conformance | listed known failures | 9 | ≤ 0 | FAIL | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
