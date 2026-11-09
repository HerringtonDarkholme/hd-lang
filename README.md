# Daily goal metrics

Finished: 2026-11-09T22:26:46.516Z. Commit: 219b361ade0579aca70ff602b5181711e460c99e.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V74 80-Core Processor).
Compiler: target/release/hd.

95 passed, 36 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 35 ms → pass 40 ms
- edit-latency: p95 check after an edit: pass 37 ms → pass 56 ms
- cold-check: median cold check, 10k lines: pass 146 ms → pass 174 ms
- test-latency: median edit-then-test: pass 262 ms → pass 300 ms
- pathological: overlapping-from: time at 4x: pass 54 ms → pass 46 ms
- pathological: deep-parens: time at 4x: pass 19 ms → pass 23 ms
- pathological: wide-list-literal: time at 4x: pass 40 ms → pass 65 ms
- pathological: deep-generic: time at 4x: pass 21 ms → pass 122 ms
- pathological: deep-generic: growth exponent: pass noise → fail 6.12
- pathological: question-chain: time at 4x: pass 26 ms → pass 29 ms
- fmt: hd fmt, 10k lines: pass 23 ms → pass 28 ms
- resources: warm check, 10k lines: CPU time: pass 20 ms → pass 30 ms
- long-session: RSS growth, REPL inputs 100 to 1000: fail 102.4 MB → fail 77.0 MB
- startup: hd check empty.hd: wall time: pass 13 ms → pass 15 ms
- cache-growth: cache size after 100 edits, cap 5 MB: pass 3.8 MB → fail 5.5 MB
- proptest-perf: property cases per second: fail 75k/s → fail 40k/s
- proptest-perf: time to shrink a known failure: pass 63 ms → pass 81 ms
- unit-test-perf: 1,000 unit tests, warm: fail 1018 ms → fail 1213 ms
- unit-test-perf: overhead per unit test: pass 0.6 ms → pass 0.7 ms
- integration-test-perf: setup per integration test program: fail 281 ms → fail 317 ms
- integration-test-perf: cost per doc test: fail 187 ms → fail 222 ms
- runtime: sum: hd / Node: pass 1.02x → pass 1.35x
- runtime: string-build: hd / Node: fail - → pass 0.30x
- runtime: map: hd / Node: fail 3.73x → pass 2.91x
- runtime: sort: hd / Node: pass 0.50x → pass 0.43x
- allocations: counted loop, bytes per iteration: pass 0.00 B/iter → pass 0.03 B/iter
- allocations: iterator chain, bytes per iteration: pass 0.01 B/iter → pass 0.00 B/iter
- size-startup-heap: tiny program, start to first output: fail 98 ms → fail 118 ms
- size-startup-heap: peak heap, map: hd / Node: pass 0.96x → pass 0.82x
- size-startup-heap: peak heap, string-build: hd / Node: pass 0.26x → pass 0.17x
- host-call-overhead: Fs read_text!, per call: fail 15 µs → fail 18 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 26 µs → fail 32 µs
- suspension-overhead: await of a ready value, per await: pass 15 ns → pass 3.7 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 21.4M/s → pass 16.9M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 13.4M/s → pass 9.7M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.17x → fail 0.36x
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.28x → fail 0.41x
- text-throughput: string build, hd / Node MB/s: pass 3.27x → pass 6.19x
- text-throughput: split, hd / Node MB/s: pass 3.86x → pass 2.89x
- text-throughput: regex find_all, hd / Node MB/s: fail 0.11x → fail 0.09x

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 40 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 56 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 174 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 300 ms | ≤ 300 ms | PASS |  |
| mistakes | programs with exactly one diagnostic | 98.6% | ≥ 95.0% | PASS | 68 of 69 |
| mistakes | one diagnostic with the expected code | 98.6% | ≥ 95.0% | PASS | 68 of 69; first misses: some-on-result |
| mistakes | fix-it makes hd check pass | 33.3% | ≥ 80.0% | FAIL | 23 of 69 resolved; 23 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 92 tokens | ≤ 60 tokens | FAIL | largest: some-on-result; median 43 tokens, 171 B; 66 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 46 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 25.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 40 → 46 ms, start-up 19 ms |
| pathological | deep-blocks: time at 4x | 25 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-blocks: peak RSS at 4x | 19.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 25 ms, start-up 19 ms |
| pathological | deep-parens: time at 4x | 23 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 19.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 23 ms, start-up 19 ms |
| pathological | method-chain: time at 4x | 25 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 22.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 25 ms, start-up 19 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 65 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 23.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 32 → 65 ms, start-up 19 ms |
| pathological | large-enum-match: time at 4x | 30 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | large-enum-match: peak RSS at 4x | 22.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 33 → 30 ms, start-up 19 ms |
| pathological | deep-generic: time at 4x | 122 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 20.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | 6.12 | ≤ 1.30 | FAIL | 2x→4x; 21 → 122 ms, start-up 19 ms |
| pathological | question-chain: time at 4x | 29 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 23.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 28 → 29 ms, start-up 19 ms |
| pathological | big-file: time at 4x | 154 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | big-file: peak RSS at 4x | 52.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.12 | ≤ 1.30 | PASS | 2x→4x; 81 → 154 ms, start-up 19 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 23 of 23 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 28 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.92x | ≤ 1.30x | FAIL | checked 638 ms, unchecked 332 ms |
| dev-speed | geomean dev / optimized | - | ≤ 4.00x | FAIL | optimized work is within noise: string-build (45.5 ms) |
| dev-speed | worst case dev / optimized | - | ≤ 10.00x | FAIL | optimized work is within noise: string-build (45.5 ms) |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 30 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 25.2 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.14 CPU-s, 34.5 MB | completes ≤ 30 s | PASS | wall 95 ms |
| resources | check, small, warm | 0.00 CPU-s, 18.3 MB | completes ≤ 30 s | PASS | wall 17 ms |
| resources | test, small, cold | 2.92 CPU-s, 119.9 MB | completes ≤ 30 s | PASS | wall 1072 ms |
| resources | test, small, warm | 2.19 CPU-s, 120.8 MB | completes ≤ 30 s | PASS | wall 782 ms |
| resources | build, small, cold | 0.69 CPU-s, 81.6 MB | completes ≤ 30 s | PASS | wall 273 ms |
| resources | build, small, warm | 0.02 CPU-s, 30.6 MB | completes ≤ 30 s | PASS | wall 28 ms |
| resources | check, 10k, cold | 0.37 CPU-s, 56.7 MB | completes ≤ 30 s | PASS | wall 177 ms |
| resources | check, 10k, warm | 0.03 CPU-s, 25.2 MB | completes ≤ 30 s | PASS | wall 35 ms |
| resources | test, 10k, cold | 3.52 CPU-s, 130.0 MB | completes ≤ 30 s | PASS | wall 1388 ms |
| resources | test, 10k, warm | 2.39 CPU-s, 130.3 MB | completes ≤ 30 s | PASS | wall 879 ms |
| resources | build, 10k, cold | 0.94 CPU-s, 103.2 MB | completes ≤ 30 s | PASS | wall 337 ms |
| resources | build, 10k, warm | 0.04 CPU-s, 34.0 MB | completes ≤ 30 s | PASS | wall 46 ms |
| resources | check, 50k, cold | 1.48 CPU-s, 162.0 MB | completes ≤ 150 s | PASS | wall 584 ms |
| resources | check, 50k, warm | 0.13 CPU-s, 57.0 MB | completes ≤ 150 s | PASS | wall 117 ms |
| resources | test, 50k, cold | 7.37 CPU-s, 275.5 MB | completes ≤ 150 s | PASS | wall 3329 ms |
| resources | test, 50k, warm | 4.10 CPU-s, 182.8 MB | completes ≤ 150 s | PASS | wall 1411 ms |
| resources | build, 50k, cold | 2.09 CPU-s, 207.3 MB | completes ≤ 150 s | PASS | wall 731 ms |
| resources | build, 50k, warm | 0.14 CPU-s, 65.5 MB | completes ≤ 150 s | PASS | wall 126 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 77.0 MB | ≤ 22.0 MB | FAIL | RSS 98.5 MB at input 100, 174.0 MB at input 1000; build per input 124 ms at 50, 720 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.2 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.8 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 15 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 16.4 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.08x | ≤ 1.50x | FAIL | p95 100 ms at N = 1; 4 cores; total CPU 770 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.76x | ≤ 6.00x | FAIL | p95 100 ms at N = 1; 4 cores; total CPU 3190 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.10 | &lt; 1.00 (sublinear) | FAIL | 150 ms at N = 1, 3190 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 19.0 MB | ≤ 10.0 MB | FAIL | artifacts 7802 B in 10 files; toolchain 19.0 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 10.9 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.91x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 9 files | the files of one check's cache | FAIL | one check alone: 7 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 5.5 MB | ≤ 5.0 MB | FAIL | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 79 | ≤ 1 | FAIL | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 40k/s | ≥ 100k/s | FAIL | 6000 extra cases took 150 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 81 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 1213 ms | ≤ 1000 ms | FAIL | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.7 ms | ≤ 1.0 ms | PASS | 100 tests: 596 ms |
| integration-test-perf | setup per integration test program | 317 ms | ≤ 20 ms | FAIL | 10 programs: 3410 ms for 21 tests, 162 ms per test |
| integration-test-perf | total time growth in programs | 1.000 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 635 ms at 2, 3174 ms at 10 programs |
| integration-test-perf | cost per doc test | 222 ms | ≤ 20 ms | FAIL | 10 doc tests: 2438 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | - | ≤ 1.50x | FAIL | 1 of 5 cases not measured |
| runtime | fib: hd / Node | - | ≤ 3.00x | FAIL | no work measured beyond start-up; hd -2.6 ms (p10 -9.0, p90 6.6), Node 8.5 ms, Python 78 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 1.35x | ≤ 3.00x | PASS | hd 11 ms (p10 0.3, p90 19), Node 8.2 ms, Python 238 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.30x | ≤ 3.00x | PASS | hd 2.7 ms (p10 -1.5, p90 11), Node 9.1 ms, Python 12 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 2.91x | ≤ 3.00x | PASS | hd 28 ms (p10 25, p90 39), Node 9.5 ms, Python 15 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.43x | ≤ 3.00x | PASS | hd 8.3 ms (p10 1.7, p90 18), Node 19 ms, Python 15 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.03 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 1627 B | ≤ 2048 B | PASS | benchmarks: fib 5414 B, sum 5226 B, string-build 8603 B, map 6631 B, sort 13928 B |
| size-startup-heap | tiny program, start to first output | 118 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.82x | ≤ 2.00x | PASS | hd +11.7 MB, Node +14.1 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.17x | ≤ 2.00x | PASS | hd +3.8 MB, Node +22.0 MB over an empty program |
| host-call-overhead | Console println, per call | 6.6 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm Timing change ≤10%: noise. |
| host-call-overhead | Fs read_text!, per call | 18 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 32 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 3.7 ns | ≤ 100 ns | PASS | 3.7 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 16.9M/s | ≥ 1.0M/s | PASS | 119 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 9.7M/s | ≥ 1.0M/s | PASS | 207 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.36x | ≥ 0.50x | FAIL | hd 132 MB/s (45488 B document), Node 362 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.41x | ≥ 0.50x | FAIL | hd 75 MB/s (45488 B document), Node 184 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 6.19x | ≥ 0.50x | PASS | hd 722 MB/s, Node 117 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 2.89x | ≥ 0.50x | PASS | hd 813 MB/s, Node 281 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.09x | ≥ 0.50x | FAIL | hd 7.4 MB/s, Node 81 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 11002 B | ≤ 10240 B | FAIL | 17443 B at 1075 lines, 116475 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 908 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 21156000 requests; RSS 99.4 MB after warm-up, 99.4 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 6996 of 6996 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
- [2026-10-30T03](daily/2026-10-30T03.json)
- [2026-10-29T18](daily/2026-10-29T18.json)
- [2026-10-29T03](daily/2026-10-29T03.json)
- [2026-10-28T18](daily/2026-10-28T18.json)
- [2026-10-28T03](daily/2026-10-28T03.json)
- [2026-10-27T18](daily/2026-10-27T18.json)
- [2026-10-27T02](daily/2026-10-27T02.json)
