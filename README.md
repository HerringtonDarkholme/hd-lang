# Daily goal metrics

Finished: 2026-11-09T03:36:37.935Z. Commit: 341f29652b48b8cee02607e2bf8c5b05654a65dc.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V45 96-Core Processor).
Compiler: target/release/hd.

95 passed, 36 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 45 ms → pass 35 ms
- edit-latency: p95 check after an edit: pass 48 ms → pass 37 ms
- cold-check: median cold check, 10k lines: pass 207 ms → pass 146 ms
- test-latency: median edit-then-test: fail 350 ms → pass 262 ms
- pathological: deep-blocks: time at 4x: pass 27 ms → pass 24 ms
- pathological: deep-parens: time at 4x: pass 23 ms → pass 19 ms
- pathological: wide-list-literal: time at 4x: pass 52 ms → pass 40 ms
- pathological: large-enum-match: time at 4x: pass 36 ms → pass 28 ms
- pathological: deep-generic: time at 4x: pass 31 ms → pass 21 ms
- pathological: question-chain: time at 4x: pass 35 ms → pass 26 ms
- pathological: big-file: time at 4x: pass 191 ms → pass 141 ms
- pathological: big-file: growth exponent: fail 1.35 → pass 1.23
- fmt: hd fmt, 10k lines: pass 30 ms → pass 23 ms
- check-cost: checked run time / unchecked run time: fail 1.75x → fail 1.94x
- resources: warm check, 10k lines: CPU time: pass 30 ms → pass 20 ms
- long-session: RSS growth, REPL inputs 100 to 1000: fail 85.0 MB → fail 102.4 MB
- startup: hd help (no version command): wall time: pass 2.8 ms → pass 3.2 ms
- startup: hd check empty.hd: wall time: pass 15 ms → pass 13 ms
- proptest-perf: property cases per second: fail 37k/s → fail 75k/s
- proptest-perf: time to shrink a known failure: pass 84 ms → pass 63 ms
- unit-test-perf: 1,000 unit tests, warm: fail 1396 ms → fail 1018 ms
- unit-test-perf: overhead per unit test: pass 0.8 ms → pass 0.6 ms
- integration-test-perf: setup per integration test program: fail 375 ms → fail 281 ms
- integration-test-perf: cost per doc test: fail 257 ms → fail 187 ms
- runtime: geomean hd / Node: pass 0.68x → fail -
- runtime: fib: hd / Node: pass 0.40x → fail -
- runtime: sum: hd / Node: pass 2.72x → pass 1.02x
- runtime: string-build: hd / Node: pass 0.08x → fail -
- runtime: map: hd / Node: fail 3.24x → fail 3.73x
- allocations: iterator chain, bytes per iteration: pass 0.03 B/iter → pass 0.01 B/iter
- size-startup-heap: tiny program, start to first output: fail 142 ms → fail 98 ms
- size-startup-heap: peak heap, string-build: hd / Node: pass 0.22x → pass 0.26x
- suspension-overhead: await of a ready value, per await: pass 40 ns → pass 15 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 16.0M/s → pass 21.4M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 11.0M/s → pass 13.4M/s
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.23x → fail 0.28x
- text-throughput: split, hd / Node MB/s: pass 2.13x → pass 3.86x
- text-throughput: regex find_all, hd / Node MB/s: fail 0.13x → fail 0.11x

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 35 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 37 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 146 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 262 ms | ≤ 300 ms | PASS |  |
| mistakes | programs with exactly one diagnostic | 98.6% | ≥ 95.0% | PASS | 68 of 69 |
| mistakes | one diagnostic with the expected code | 98.6% | ≥ 95.0% | PASS | 68 of 69; first misses: some-on-result |
| mistakes | fix-it makes hd check pass | 33.3% | ≥ 80.0% | FAIL | 23 of 69 resolved; 23 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 92 tokens | ≤ 60 tokens | FAIL | largest: some-on-result; median 43 tokens, 171 B; 66 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 54 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | overlapping-from: peak RSS at 4x | 25.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 35 → 54 ms, start-up 19 ms |
| pathological | deep-blocks: time at 4x | 24 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 19.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 24 ms, start-up 19 ms |
| pathological | deep-parens: time at 4x | 19 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 19.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 23 → 19 ms, start-up 19 ms |
| pathological | method-chain: time at 4x | 25 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | method-chain: peak RSS at 4x | 21.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 25 ms, start-up 19 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 40 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 24.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 30 → 40 ms, start-up 19 ms |
| pathological | large-enum-match: time at 4x | 28 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 23.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 30 → 28 ms, start-up 19 ms |
| pathological | deep-generic: time at 4x | 21 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 20.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 21 ms, start-up 19 ms |
| pathological | question-chain: time at 4x | 26 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 23.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 27 → 26 ms, start-up 19 ms |
| pathological | big-file: time at 4x | 141 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 53.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.23 | ≤ 1.30 | PASS | 2x→4x; 71 → 141 ms, start-up 19 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 23 of 23 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 23 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.94x | ≤ 1.30x | FAIL | checked 487 ms, unchecked 251 ms |
| dev-speed | geomean dev / optimized | - | ≤ 4.00x | FAIL | optimized work is within noise: string-build (30.1 ms) |
| dev-speed | worst case dev / optimized | - | ≤ 10.00x | FAIL | optimized work is within noise: string-build (30.1 ms) |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 20 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 26.1 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.11 CPU-s, 35.0 MB | completes ≤ 30 s | PASS | wall 83 ms |
| resources | check, small, warm | 0.00 CPU-s, 18.4 MB | completes ≤ 30 s | PASS | wall 16 ms |
| resources | test, small, cold | 2.53 CPU-s, 120.1 MB | completes ≤ 30 s | PASS | wall 940 ms |
| resources | test, small, warm | 1.97 CPU-s, 119.9 MB | completes ≤ 30 s | PASS | wall 697 ms |
| resources | build, small, cold | 0.56 CPU-s, 82.3 MB | completes ≤ 30 s | PASS | wall 226 ms |
| resources | build, small, warm | 0.02 CPU-s, 31.1 MB | completes ≤ 30 s | PASS | wall 26 ms |
| resources | check, 10k, cold | 0.32 CPU-s, 57.7 MB | completes ≤ 30 s | PASS | wall 155 ms |
| resources | check, 10k, warm | 0.02 CPU-s, 26.1 MB | completes ≤ 30 s | PASS | wall 32 ms |
| resources | test, 10k, cold | 3.08 CPU-s, 129.1 MB | completes ≤ 30 s | PASS | wall 1199 ms |
| resources | test, 10k, warm | 2.13 CPU-s, 130.0 MB | completes ≤ 30 s | PASS | wall 761 ms |
| resources | build, 10k, cold | 0.77 CPU-s, 104.4 MB | completes ≤ 30 s | PASS | wall 288 ms |
| resources | build, 10k, warm | 0.04 CPU-s, 35.0 MB | completes ≤ 30 s | PASS | wall 40 ms |
| resources | check, 50k, cold | 1.22 CPU-s, 163.4 MB | completes ≤ 150 s | PASS | wall 510 ms |
| resources | check, 50k, warm | 0.10 CPU-s, 57.2 MB | completes ≤ 150 s | PASS | wall 94 ms |
| resources | test, 50k, cold | 6.13 CPU-s, 276.4 MB | completes ≤ 150 s | PASS | wall 2733 ms |
| resources | test, 50k, warm | 3.62 CPU-s, 186.7 MB | completes ≤ 150 s | PASS | wall 1268 ms |
| resources | build, 50k, cold | 1.67 CPU-s, 208.0 MB | completes ≤ 150 s | PASS | wall 602 ms |
| resources | build, 50k, warm | 0.12 CPU-s, 65.1 MB | completes ≤ 150 s | PASS | wall 103 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 102.4 MB | ≤ 22.0 MB | FAIL | RSS 94.6 MB at input 100, 197.0 MB at input 1000; build per input 67 ms at 50, 524 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.2 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 4.0 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 13 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 16.9 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.09x | ≤ 1.50x | FAIL | p95 80 ms at N = 1; 4 cores; total CPU 600 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.71x | ≤ 6.00x | FAIL | p95 80 ms at N = 1; 4 cores; total CPU 2590 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.11 | &lt; 1.00 (sublinear) | FAIL | 120 ms at N = 1, 2590 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 18.7 MB | ≤ 10.0 MB | FAIL | artifacts 7800 B in 10 files; toolchain 18.7 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 11.0 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.91x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 9 files | the files of one check's cache | FAIL | one check alone: 7 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 3.8 MB | ≤ 5.0 MB | PASS | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 79 | ≤ 1 | FAIL | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 75k/s | ≥ 100k/s | FAIL | 6000 extra cases took 80 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 63 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 1018 ms | ≤ 1000 ms | FAIL | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.6 ms | ≤ 1.0 ms | PASS | 100 tests: 517 ms |
| integration-test-perf | setup per integration test program | 281 ms | ≤ 20 ms | FAIL | 10 programs: 3003 ms for 21 tests, 143 ms per test |
| integration-test-perf | total time growth in programs | 0.990 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 570 ms at 2, 2802 ms at 10 programs |
| integration-test-perf | cost per doc test | 187 ms | ≤ 20 ms | FAIL | 10 doc tests: 2072 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | - | ≤ 1.50x | FAIL | 2 of 5 cases not measured |
| runtime | fib: hd / Node | - | ≤ 3.00x | FAIL | no work measured beyond start-up; hd -1.1 ms (p10 -3.1, p90 3.8), Node 5.2 ms, Python 47 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 1.02x | ≤ 3.00x | PASS | hd 6.3 ms (p10 4.6, p90 14), Node 6.1 ms, Python 148 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | - | ≤ 3.00x | FAIL | no work measured beyond start-up; hd -0.9 ms (p10 -7.4, p90 5.3), Node 6.6 ms, Python 7.9 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 3.73x | ≤ 3.00x | FAIL | hd 26 ms (p10 19, p90 28), Node 6.9 ms, Python 10 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.50x | ≤ 3.00x | PASS | hd 7.1 ms (p10 1.2, p90 10), Node 14 ms, Python 9.7 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.01 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 1625 B | ≤ 2048 B | PASS | benchmarks: fib 5414 B, sum 5226 B, string-build 8603 B, map 6631 B, sort 13517 B |
| size-startup-heap | tiny program, start to first output | 98 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.96x | ≤ 2.00x | PASS | hd +13.7 MB, Node +14.3 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.26x | ≤ 2.00x | PASS | hd +5.5 MB, Node +20.7 MB over an empty program |
| host-call-overhead | Console println, per call | 6.1 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm Timing change ≤10%: noise. |
| host-call-overhead | Fs read_text!, per call | 15 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm Timing change ≤10%: noise. |
| host-call-overhead | serde boundary (list_dir!), per call | 26 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm Timing change ≤10%: noise. |
| suspension-overhead | await of a ready value, per await | 15 ns | ≤ 100 ns | PASS | 15 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 21.4M/s | ≥ 1.0M/s | PASS | 94 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 13.4M/s | ≥ 1.0M/s | PASS | 149 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.17x | ≥ 0.50x | FAIL | hd 90 MB/s (45488 B document), Node 513 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.28x | ≥ 0.50x | FAIL | hd 70 MB/s (45488 B document), Node 247 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 3.27x | ≥ 0.50x | PASS | hd 515 MB/s, Node 158 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 3.86x | ≥ 0.50x | PASS | hd 1413 MB/s, Node 366 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.11x | ≥ 0.50x | FAIL | hd 12 MB/s, Node 109 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 11002 B | ≤ 10240 B | FAIL | 17441 B at 1075 lines, 116473 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 908 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 29220000 requests; RSS 98.4 MB after warm-up, 98.4 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 6926 of 6926 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
- [2026-10-26T18](daily/2026-10-26T18.json)
