# Daily goal metrics

Finished: 2026-11-13T04:12:47.252Z. Commit: 8d928bfcf98c0dbffbdb0db4afa68f0364ba12e2.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 7763 64-Core Processor).
Compiler: target/release/hd.

101 passed, 30 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 40 ms → fail 52 ms
- edit-latency: p95 check after an edit: pass 43 ms → pass 55 ms
- cold-check: median cold check, 10k lines: pass 182 ms → pass 241 ms
- test-latency: median edit-then-test: pass 204 ms → pass 266 ms
- pathological: overlapping-from: time at 4x: pass 56 ms → pass 68 ms
- pathological: deep-blocks: time at 4x: pass 22 ms → pass 27 ms
- pathological: method-chain: time at 4x: pass 25 ms → pass 32 ms
- pathological: wide-list-literal: time at 4x: pass 40 ms → pass 50 ms
- pathological: large-enum-match: time at 4x: pass 32 ms → pass 40 ms
- pathological: deep-generic: time at 4x: pass 24 ms → pass 31 ms
- pathological: question-chain: time at 4x: pass 29 ms → pass 36 ms
- pathological: big-file: time at 4x: pass 161 ms → pass 215 ms
- fmt: hd fmt, 10k lines: pass 26 ms → pass 32 ms
- dev-speed: worst case dev / optimized: pass 2.28x → pass 1.55x
- resources: warm check, 10k lines: CPU time: pass 30 ms → pass 50 ms
- long-session: RSS growth, REPL inputs 100 to 1000: fail 87.3 MB → fail 96.5 MB
- startup: hd help (no version command): wall time: pass 3.1 ms → pass 4.0 ms
- startup: hd check empty.hd: wall time: pass 15 ms → pass 18 ms
- startup: hd check empty.hd: CPU time: pass 0.0 ms → pass 10 ms
- proptest-perf: property cases per second: fail 37k/s → fail 23k/s
- proptest-perf: time to shrink a known failure: pass 98 ms → pass 125 ms
- unit-test-perf: 1,000 unit tests, warm: pass 885 ms → fail 1130 ms
- unit-test-perf: overhead per unit test: pass 0.7 ms → pass 0.9 ms
- integration-test-perf: setup per integration test program: fail 130 ms → fail 161 ms
- integration-test-perf: cost per doc test: fail 119 ms → fail 146 ms
- runtime: geomean hd / Node: pass 0.87x → pass 1.05x
- runtime: string-build: hd / Node: pass 0.75x → pass 1.03x
- runtime: map: hd / Node: pass 2.26x → pass 2.70x
- runtime: sort: hd / Node: pass 0.40x → pass 0.61x
- size-startup-heap: tiny program, start to first output: fail 45 ms → fail 56 ms
- host-call-overhead: Console println, per call: fail 6.7 µs → fail 8.7 µs
- host-call-overhead: Fs read_text!, per call: fail 19 µs → fail 27 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 33 µs → fail 47 µs
- suspension-overhead: await of a ready value, per await: pass 9.7 ns → pass 2.2 ns
- suspension-overhead: race! of two ready tasks, throughput: pass 10.8M/s → pass 7.2M/s
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.26x → fail 0.20x
- text-throughput: split, hd / Node MB/s: pass 2.59x → pass 2.33x
- text-throughput: regex find_all, hd / Node MB/s: fail 0.12x → fail 0.09x
- long-run-memory: RSS growth after warm-up, 1 min: pass 0.4 MB → pass 0.3 MB

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 52 ms | ≤ 50 ms | FAIL |  |
| edit-latency | p95 check after an edit | 55 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 241 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 266 ms | ≤ 300 ms | PASS |  |
| mistakes | programs with exactly one diagnostic | 98.6% | ≥ 95.0% | PASS | 68 of 69 |
| mistakes | one diagnostic with the expected code | 98.6% | ≥ 95.0% | PASS | 68 of 69; first misses: some-on-result |
| mistakes | fix-it makes hd check pass | 33.3% | ≥ 80.0% | FAIL | 23 of 69 resolved; 23 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 92 tokens | ≤ 60 tokens | FAIL | largest: some-on-result; median 43 tokens, 171 B; 66 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 68 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 25.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 52 → 68 ms, start-up 31 ms |
| pathological | deep-blocks: time at 4x | 27 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 20.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 26 → 27 ms, start-up 31 ms |
| pathological | deep-parens: time at 4x | 29 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-parens: peak RSS at 4x | 20.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 26 → 29 ms, start-up 31 ms |
| pathological | method-chain: time at 4x | 32 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 22.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 28 → 32 ms, start-up 31 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 50 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 24.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 39 → 50 ms, start-up 31 ms |
| pathological | large-enum-match: time at 4x | 40 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 24.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 40 → 40 ms, start-up 31 ms |
| pathological | deep-generic: time at 4x | 31 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 21.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 27 → 31 ms, start-up 31 ms |
| pathological | question-chain: time at 4x | 36 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 23.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 36 → 36 ms, start-up 31 ms |
| pathological | big-file: time at 4x | 215 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 53.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.19 | ≤ 1.30 | PASS | 2x→4x; 112 → 215 ms, start-up 31 ms |
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
| check-cost | checked run time / unchecked run time | 1.93x | ≤ 1.30x | FAIL | checked 848 ms, unchecked 439 ms |
| dev-speed | geomean dev / optimized | 1.16x | ≤ 4.00x | PASS | fib 1.13x, sum 1.32x, string-build 0.89x, map 1.04x, sort 1.55x |
| dev-speed | worst case dev / optimized | 1.55x | ≤ 10.00x | PASS | sort |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 50 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 25.5 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.20 CPU-s, 35.8 MB | completes ≤ 30 s | PASS | wall 127 ms |
| resources | check, small, warm | 0.01 CPU-s, 18.3 MB | completes ≤ 30 s | PASS | wall 21 ms |
| resources | test, small, cold | 2.15 CPU-s, 91.6 MB | completes ≤ 30 s | PASS | wall 865 ms |
| resources | test, small, warm | 1.09 CPU-s, 70.2 MB | completes ≤ 30 s | PASS | wall 430 ms |
| resources | build, small, cold | 1.02 CPU-s, 80.6 MB | completes ≤ 30 s | PASS | wall 383 ms |
| resources | build, small, warm | 0.03 CPU-s, 30.8 MB | completes ≤ 30 s | PASS | wall 34 ms |
| resources | check, 10k, cold | 0.54 CPU-s, 57.4 MB | completes ≤ 30 s | PASS | wall 245 ms |
| resources | check, 10k, warm | 0.05 CPU-s, 25.5 MB | completes ≤ 30 s | PASS | wall 47 ms |
| resources | test, 10k, cold | 3.10 CPU-s, 123.0 MB | completes ≤ 30 s | PASS | wall 1330 ms |
| resources | test, 10k, warm | 1.38 CPU-s, 78.1 MB | completes ≤ 30 s | PASS | wall 551 ms |
| resources | build, 10k, cold | 1.42 CPU-s, 104.0 MB | completes ≤ 30 s | PASS | wall 486 ms |
| resources | build, 10k, warm | 0.06 CPU-s, 34.6 MB | completes ≤ 30 s | PASS | wall 58 ms |
| resources | check, 50k, cold | 2.18 CPU-s, 161.6 MB | completes ≤ 150 s | PASS | wall 803 ms |
| resources | check, 50k, warm | 0.16 CPU-s, 56.3 MB | completes ≤ 150 s | PASS | wall 150 ms |
| resources | test, 50k, cold | 8.63 CPU-s, 275.1 MB | completes ≤ 150 s | PASS | wall 4273 ms |
| resources | test, 50k, warm | 3.69 CPU-s, 113.0 MB | completes ≤ 150 s | PASS | wall 1616 ms |
| resources | build, 50k, cold | 3.09 CPU-s, 206.9 MB | completes ≤ 150 s | PASS | wall 1034 ms |
| resources | build, 50k, warm | 0.18 CPU-s, 64.1 MB | completes ≤ 150 s | PASS | wall 158 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 96.5 MB | ≤ 22.0 MB | FAIL | RSS 94.5 MB at input 100, 191.0 MB at input 1000; build per input 89 ms at 50, 982 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 4.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.9 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 18 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 10 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 17.0 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.23x | ≤ 1.50x | FAIL | p95 129 ms at N = 1; 4 cores; total CPU 1050 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.38x | ≤ 6.00x | FAIL | p95 129 ms at N = 1; 4 cores; total CPU 4400 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.11 | &lt; 1.00 (sublinear) | FAIL | 200 ms at N = 1, 4400 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 20.1 MB | ≤ 10.0 MB | FAIL | artifacts 7631 B in 10 files; toolchain 20.1 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 10.8 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.92x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 9 files | the files of one check's cache | FAIL | one check alone: 7 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 4.5 MB | ≤ 5.0 MB | PASS | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 79 | ≤ 1 | FAIL | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 23k/s | ≥ 100k/s | FAIL | 6000 extra cases took 258 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 125 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 1130 ms | ≤ 1000 ms | FAIL | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.9 ms | ≤ 1.0 ms | PASS | 100 tests: 282 ms |
| integration-test-perf | setup per integration test program | 161 ms | ≤ 20 ms | FAIL | 10 programs: 1760 ms for 21 tests, 84 ms per test |
| integration-test-perf | total time growth in programs | 0.964 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 341 ms at 2, 1610 ms at 10 programs |
| integration-test-perf | cost per doc test | 146 ms | ≤ 20 ms | FAIL | 10 doc tests: 1611 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 1.05x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.53x | ≤ 3.00x | PASS | hd 5.3 ms (p10 2.0, p90 14), Node 10 ms, Python 98 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 1.45x | ≤ 3.00x | PASS | hd 14 ms (p10 13, p90 19), Node 9.3 ms, Python 274 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 1.03x | ≤ 3.00x | PASS | hd 11 ms (p10 6.9, p90 14), Node 10 ms, Python 15 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 2.70x | ≤ 3.00x | PASS | hd 29 ms (p10 23, p90 33), Node 11 ms, Python 17 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.61x | ≤ 3.00x | PASS | hd 15 ms (p10 10, p90 18), Node 25 ms, Python 19 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 1589 B | ≤ 2048 B | PASS | benchmarks: fib 4977 B, sum 4753 B, string-build 7558 B, map 6506 B, sort 12405 B |
| size-startup-heap | tiny program, start to first output | 56 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.99x | ≤ 2.00x | PASS | hd +14.2 MB, Node +14.4 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.39x | ≤ 2.00x | PASS | hd +8.2 MB, Node +20.9 MB over an empty program |
| host-call-overhead | Console println, per call | 8.7 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 27 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 47 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 2.2 ns | ≤ 100 ns | PASS | 2.2 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 11.9M/s | ≥ 1.0M/s | PASS | 169 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 7.2M/s | ≥ 1.0M/s | PASS | 278 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.19x | ≥ 0.50x | FAIL | hd 50 MB/s (45488 B document), Node 265 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.20x | ≥ 0.50x | FAIL | hd 29 MB/s (45488 B document), Node 146 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 3.11x | ≥ 0.50x | PASS | hd 314 MB/s, Node 101 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 2.33x | ≥ 0.50x | PASS | hd 556 MB/s, Node 239 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.09x | ≥ 0.50x | FAIL | hd 6.7 MB/s, Node 72 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 6720 B | ≤ 10240 B | PASS | 12541 B at 1075 lines, 73032 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 883 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.3 MB | ≤ 10.0 MB | PASS | 16880000 requests; RSS 70.2 MB after warm-up, 73.0 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 7466 of 7466 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

- [2026-11-13T04](daily/2026-11-13T04.json)
- [2026-11-12T19](daily/2026-11-12T19.json)
- [2026-11-12T04](daily/2026-11-12T04.json)
- [2026-11-11T19](daily/2026-11-11T19.json)
- [2026-11-11T03](daily/2026-11-11T03.json)
- [2026-11-10T19](daily/2026-11-10T19.json)
- [2026-11-10T05](daily/2026-11-10T05.json)
- [2026-11-09T22](daily/2026-11-09T22.json)
- [2026-11-09T03](daily/2026-11-09T03.json)
- [2026-11-08T18](daily/2026-11-08T18.json)
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
