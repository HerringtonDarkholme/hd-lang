# Daily goal metrics

Finished: 2026-11-05T20:14:24.068Z. Commit: e4b79d14bc3fe26bc2e14c45e42677c2f63e1cf9.
Machine: ubuntu-latest, 4 CPUs (Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz).
Compiler: target/release/hd.

99 passed, 32 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: fail 81 ms → fail 71 ms
- edit-latency: p95 check after an edit: pass 83 ms → pass 72 ms
- pathological: overlapping-from: time at 4x: pass 69 ms → pass 56 ms
- pathological: deep-blocks: time at 4x: pass 27 ms → pass 22 ms
- pathological: deep-parens: time at 4x: pass 26 ms → pass 21 ms
- pathological: method-chain: time at 4x: pass 36 ms → pass 28 ms
- pathological: wide-list-literal: time at 4x: pass 69 ms → pass 59 ms
- pathological: deep-generic: time at 4x: pass 30 ms → pass 26 ms
- pathological: question-chain: time at 4x: pass 37 ms → pass 32 ms
- fmt: hd fmt, 10k lines: pass 32 ms → pass 27 ms
- dev-speed: worst case dev / optimized: pass 2.54x → pass 2.28x
- long-session: RSS growth, REPL inputs 100 to 1000: fail 86.0 MB → fail 41.4 MB
- startup: hd help (no version command): wall time: pass 3.7 ms → pass 3.3 ms
- startup: hd check empty.hd: wall time: pass 19 ms → pass 16 ms
- proptest-perf: property cases per second: fail 27k/s → fail 24k/s
- unit-test-perf: overhead per unit test: pass 0.9 ms → pass 1.0 ms
- runtime: geomean hd / Node: pass 0.66x → pass 0.56x
- runtime: fib: hd / Node: pass 0.15x → pass 0.06x
- runtime: sum: hd / Node: pass 0.97x → pass 0.80x
- runtime: string-build: hd / Node: pass 0.34x → pass 0.59x
- runtime: sort: hd / Node: pass 0.68x → pass 0.50x
- allocations: counted loop, bytes per iteration: pass 0.03 B/iter → pass 0.00 B/iter
- allocations: map loop, bytes per iteration: pass 0.00 B/iter → pass 0.01 B/iter
- size-startup-heap: tiny program, release Wasm size: fail 2732 B → pass 1636 B
- host-call-overhead: Console println, per call: fail 8.3 µs → fail 7.0 µs
- host-call-overhead: Fs read_text!, per call: fail 26 µs → fail 17 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 45 µs → fail 32 µs
- suspension-overhead: await of a ready value, per await: pass 45 ns → pass 27 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 9.9M/s → pass 11.5M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.21x → fail 0.05x
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.25x → fail 0.06x
- text-throughput: split, hd / Node MB/s: pass 2.57x → pass 2.14x
- long-run-memory: RSS growth after warm-up, 1 min: pass 0.2 MB → pass 0.0 MB

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 71 ms | ≤ 50 ms | FAIL |  |
| edit-latency | p95 check after an edit | 72 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 244 ms | ≤ 1000 ms | PASS | Timing change ≤10%: noise. |
| test-latency | median edit-then-test | 509 ms | ≤ 300 ms | FAIL | Timing change ≤10%: noise. |
| mistakes | programs with exactly one diagnostic | 95.7% | ≥ 95.0% | PASS | 66 of 69 |
| mistakes | one diagnostic with the expected code | 95.7% | ≥ 95.0% | PASS | 66 of 69; first misses: mut-on-binding-type, mut-on-parameter-type, some-on-result |
| mistakes | fix-it makes hd check pass | 33.3% | ≥ 80.0% | FAIL | 23 of 69 resolved; 23 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 93 tokens | ≤ 60 tokens | FAIL | largest: mut-on-parameter-type; median 43 tokens, 171 B; 64 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 56 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 24.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 40 → 56 ms, start-up 21 ms |
| pathological | deep-blocks: time at 4x | 22 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 19.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 22 ms, start-up 21 ms |
| pathological | deep-parens: time at 4x | 21 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 19.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 21 → 21 ms, start-up 21 ms |
| pathological | method-chain: time at 4x | 28 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 21.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 24 → 28 ms, start-up 21 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 59 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 23.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 40 → 59 ms, start-up 21 ms |
| pathological | large-enum-match: time at 4x | 36 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | large-enum-match: peak RSS at 4x | 22.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 29 → 36 ms, start-up 21 ms |
| pathological | deep-generic: time at 4x | 26 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 20.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 24 → 26 ms, start-up 21 ms |
| pathological | question-chain: time at 4x | 32 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 23.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 28 → 32 ms, start-up 21 ms |
| pathological | big-file: time at 4x | 231 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | big-file: peak RSS at 4x | 53.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.21 | ≤ 1.30 | PASS | 2x→4x; 112 → 231 ms, start-up 21 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 23 of 23 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 27 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.96x | ≤ 1.30x | FAIL | checked 814 ms, unchecked 415 ms |
| dev-speed | geomean dev / optimized | 1.31x | ≤ 4.00x | PASS | fib 1.11x, sum 1.02x, string-build 1.17x, map 1.27x, sort 2.28x |
| dev-speed | worst case dev / optimized | 2.28x | ≤ 10.00x | PASS | sort |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 80 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 34.4 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.18 CPU-s, 34.7 MB | completes ≤ 30 s | PASS | wall 116 ms |
| resources | check, small, warm | 0.01 CPU-s, 18.3 MB | completes ≤ 30 s | PASS | wall 20 ms |
| resources | test, small, cold | 3.96 CPU-s, 120.8 MB | completes ≤ 30 s | PASS | wall 1490 ms |
| resources | test, small, warm | 2.95 CPU-s, 119.7 MB | completes ≤ 30 s | PASS | wall 1097 ms |
| resources | build, small, cold | 0.99 CPU-s, 100.9 MB | completes ≤ 30 s | PASS | wall 408 ms |
| resources | build, small, warm | 0.07 CPU-s, 42.6 MB | completes ≤ 30 s | PASS | wall 77 ms |
| resources | check, 10k, cold | 0.53 CPU-s, 62.0 MB | completes ≤ 30 s | PASS | wall 244 ms |
| resources | check, 10k, warm | 0.08 CPU-s, 34.4 MB | completes ≤ 30 s | PASS | wall 68 ms |
| resources | test, 10k, cold | 5.14 CPU-s, 140.5 MB | completes ≤ 30 s | PASS | wall 2290 ms |
| resources | test, 10k, warm | 3.53 CPU-s, 128.9 MB | completes ≤ 30 s | PASS | wall 1513 ms |
| resources | build, 10k, cold | 1.38 CPU-s, 124.1 MB | completes ≤ 30 s | PASS | wall 522 ms |
| resources | build, 10k, warm | 0.13 CPU-s, 50.8 MB | completes ≤ 30 s | PASS | wall 122 ms |
| resources | check, 50k, cold | 2.13 CPU-s, 189.6 MB | completes ≤ 150 s | PASS | wall 845 ms |
| resources | check, 50k, warm | 0.35 CPU-s, 100.5 MB | completes ≤ 150 s | PASS | wall 269 ms |
| resources | test, 50k, cold | 17.70 CPU-s, 352.6 MB | completes ≤ 150 s | PASS | wall 12206 ms |
| resources | test, 50k, warm | 13.50 CPU-s, 176.7 MB | completes ≤ 150 s | PASS | wall 9923 ms |
| resources | build, 50k, cold | 3.10 CPU-s, 294.4 MB | completes ≤ 150 s | PASS | wall 1157 ms |
| resources | build, 50k, warm | 0.38 CPU-s, 121.7 MB | completes ≤ 150 s | PASS | wall 365 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 41.4 MB | ≤ 22.0 MB | FAIL | RSS 169.7 MB at input 100, 164.8 MB at input 1000; build per input 136 ms at 50, 950 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.3 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 4.0 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 16 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 16.1 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.16x | ≤ 1.50x | FAIL | p95 121 ms at N = 1; 4 cores; total CPU 980 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.95x | ≤ 6.00x | FAIL | p95 121 ms at N = 1; 4 cores; total CPU 4110 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.11 | &lt; 1.00 (sublinear) | FAIL | 190 ms at N = 1, 4110 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 18.0 MB | ≤ 10.0 MB | FAIL | artifacts 7824 B in 10 files; toolchain 18.0 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 11.1 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.72x | ≥ 2.40x | FAIL | by --jobs |
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
| proptest-perf | property cases per second | 24k/s | ≥ 100k/s | FAIL | 6000 extra cases took 254 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 96 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 Timing change ≤10%: noise. |
| unit-test-perf | 1,000 unit tests, warm | 1667 ms | ≤ 1000 ms | FAIL | the run reports 1000 of 1000 passed Timing change ≤10%: noise. |
| unit-test-perf | overhead per unit test | 1.0 ms | ≤ 1.0 ms | PASS | 100 tests: 780 ms |
| integration-test-perf | setup per integration test program | 448 ms | ≤ 20 ms | FAIL | 10 programs: 4841 ms for 21 tests, 231 ms per test Timing change ≤10%: noise. |
| integration-test-perf | total time growth in programs | 0.992 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 914 ms at 2, 4516 ms at 10 programs |
| integration-test-perf | cost per doc test | 323 ms | ≤ 20 ms | FAIL | 10 doc tests: 3559 ms; the run reports 11 of 11 passed Timing change ≤10%: noise. |
| runtime | geomean hd / Node | 0.56x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.06x | ≤ 3.00x | PASS | hd 0.5 ms (p10 -7.2, p90 2.7), Node 8.9 ms, Python 88 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 0.80x | ≤ 3.00x | PASS | hd 9.3 ms (p10 -13.3, p90 60), Node 12 ms, Python 298 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.59x | ≤ 3.00x | PASS | hd 6.2 ms (p10 -18.5, p90 12), Node 11 ms, Python 14 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 3.98x | ≤ 3.00x | FAIL | hd 43 ms (p10 33, p90 51), Node 11 ms, Python 18 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.50x | ≤ 3.00x | PASS | hd 12 ms (p10 2.5, p90 25), Node 24 ms, Python 18 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.01 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 1636 B | ≤ 2048 B | PASS | benchmarks: fib 5415 B, sum 5227 B, string-build 8612 B, map 6710 B, sort 13650 B |
| size-startup-heap | tiny program, start to first output | 156 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm Timing change ≤10%: noise. |
| size-startup-heap | peak heap, map: hd / Node | 0.88x | ≤ 2.00x | PASS | hd +12.5 MB, Node +14.2 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.25x | ≤ 2.00x | PASS | hd +5.2 MB, Node +20.9 MB over an empty program |
| host-call-overhead | Console println, per call | 7.0 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 17 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 32 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 27 ns | ≤ 100 ns | PASS | 27 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 11.5M/s | ≥ 1.0M/s | PASS | 174 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 8.3M/s | ≥ 1.0M/s | PASS | 240 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.05x | ≥ 0.50x | FAIL | hd 15 MB/s (45488 B document), Node 300 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.06x | ≥ 0.50x | FAIL | hd 8.8 MB/s (45488 B document), Node 151 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 3.16x | ≥ 0.50x | PASS | hd 315 MB/s, Node 100 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 2.14x | ≥ 0.50x | PASS | hd 556 MB/s, Node 260 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.12x | ≥ 0.50x | FAIL | hd 9.1 MB/s, Node 76 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 10753 B | ≤ 10240 B | FAIL | 17217 B at 1075 lines, 114006 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 908 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 16952000 requests; RSS 99.7 MB after warm-up, 99.9 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 6740 of 6740 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
