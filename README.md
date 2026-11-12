# Daily goal metrics

Finished: 2026-11-12T19:59:06.247Z. Commit: e777ff8beaec2f6d5fdb82d7515ee658cedca0b8.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V74 80-Core Processor).
Compiler: target/release/hd.

103 passed, 28 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 47 ms → pass 40 ms
- edit-latency: p95 check after an edit: pass 50 ms → pass 43 ms
- cold-check: median cold check, 10k lines: pass 221 ms → pass 182 ms
- test-latency: median edit-then-test: pass 243 ms → pass 204 ms
- pathological: overlapping-from: time at 4x: pass 64 ms → pass 56 ms
- pathological: method-chain: time at 4x: pass 30 ms → pass 25 ms
- pathological: wide-list-literal: time at 4x: pass 58 ms → pass 40 ms
- pathological: large-enum-match: time at 4x: pass 38 ms → pass 32 ms
- pathological: deep-generic: time at 4x: pass 34 ms → pass 24 ms
- pathological: question-chain: time at 4x: pass 33 ms → pass 29 ms
- pathological: big-file: time at 4x: pass 184 ms → pass 161 ms
- fmt: hd fmt, 10k lines: pass 30 ms → pass 26 ms
- check-cost: checked run time / unchecked run time: fail 1.62x → fail 1.79x
- dev-speed: geomean dev / optimized: pass 1.45x → pass 1.22x
- dev-speed: worst case dev / optimized: pass 4.27x → pass 2.28x
- resources: warm check, 10k lines: CPU time: pass 40 ms → pass 30 ms
- long-session: RSS growth, REPL inputs 100 to 1000: fail 104.5 MB → fail 87.3 MB
- startup: hd check empty.hd: wall time: pass 18 ms → pass 15 ms
- proptest-perf: time to shrink a known failure: pass 117 ms → pass 98 ms
- unit-test-perf: 1,000 unit tests, warm: fail 1088 ms → pass 885 ms
- unit-test-perf: overhead per unit test: pass 0.9 ms → pass 0.7 ms
- integration-test-perf: setup per integration test program: fail 157 ms → fail 130 ms
- integration-test-perf: cost per doc test: fail 140 ms → fail 119 ms
- runtime: geomean hd / Node: pass 1.22x → pass 0.87x
- runtime: fib: hd / Node: pass 0.65x → pass 0.49x
- runtime: sum: hd / Node: pass 2.05x → pass 1.52x
- runtime: string-build: hd / Node: pass 0.90x → pass 0.75x
- runtime: sort: hd / Node: pass 1.06x → pass 0.40x
- size-startup-heap: tiny program, start to first output: fail 54 ms → fail 45 ms
- host-call-overhead: Fs read_text!, per call: fail 15 µs → fail 19 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 28 µs → fail 33 µs
- suspension-overhead: await of a ready value, per await: pass 13 ns → pass 9.7 ns
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.22x → fail 0.26x
- text-throughput: string build, hd / Node MB/s: pass 4.40x → pass 3.18x
- long-run-memory: RSS growth after warm-up, 1 min: pass 0.5 MB → pass 0.4 MB

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 40 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 43 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 182 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 204 ms | ≤ 300 ms | PASS |  |
| mistakes | programs with exactly one diagnostic | 98.6% | ≥ 95.0% | PASS | 68 of 69 |
| mistakes | one diagnostic with the expected code | 98.6% | ≥ 95.0% | PASS | 68 of 69; first misses: some-on-result |
| mistakes | fix-it makes hd check pass | 33.3% | ≥ 80.0% | FAIL | 23 of 69 resolved; 23 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 92 tokens | ≤ 60 tokens | FAIL | largest: some-on-result; median 43 tokens, 171 B; 66 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 56 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 26.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 44 → 56 ms, start-up 27 ms |
| pathological | deep-blocks: time at 4x | 22 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-blocks: peak RSS at 4x | 20.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 22 ms, start-up 27 ms |
| pathological | deep-parens: time at 4x | 27 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-parens: peak RSS at 4x | 21.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 27 ms, start-up 27 ms |
| pathological | method-chain: time at 4x | 25 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 22.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 23 → 25 ms, start-up 27 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 40 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 24.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 30 → 40 ms, start-up 27 ms |
| pathological | large-enum-match: time at 4x | 32 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 23.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 34 → 32 ms, start-up 27 ms |
| pathological | deep-generic: time at 4x | 24 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 20.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 24 ms, start-up 27 ms |
| pathological | question-chain: time at 4x | 29 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 23.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 31 → 29 ms, start-up 27 ms |
| pathological | big-file: time at 4x | 161 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 53.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.16 | ≤ 1.30 | PASS | 2x→4x; 86 → 161 ms, start-up 27 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 23 of 23 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 26 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.79x | ≤ 1.30x | FAIL | checked 651 ms, unchecked 363 ms |
| dev-speed | geomean dev / optimized | 1.22x | ≤ 4.00x | PASS | fib 1.03x, sum 1.42x, string-build 0.70x, map 1.14x, sort 2.28x |
| dev-speed | worst case dev / optimized | 2.28x | ≤ 10.00x | PASS | sort |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 30 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 25.9 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.15 CPU-s, 36.0 MB | completes ≤ 30 s | PASS | wall 98 ms |
| resources | check, small, warm | 0.00 CPU-s, 18.5 MB | completes ≤ 30 s | PASS | wall 17 ms |
| resources | test, small, cold | 1.59 CPU-s, 90.1 MB | completes ≤ 30 s | PASS | wall 848 ms |
| resources | test, small, warm | 0.80 CPU-s, 70.2 MB | completes ≤ 30 s | PASS | wall 410 ms |
| resources | build, small, cold | 0.74 CPU-s, 84.2 MB | completes ≤ 30 s | PASS | wall 301 ms |
| resources | build, small, warm | 0.02 CPU-s, 31.3 MB | completes ≤ 30 s | PASS | wall 29 ms |
| resources | check, 10k, cold | 0.41 CPU-s, 57.8 MB | completes ≤ 30 s | PASS | wall 189 ms |
| resources | check, 10k, warm | 0.03 CPU-s, 25.9 MB | completes ≤ 30 s | PASS | wall 37 ms |
| resources | test, 10k, cold | 2.31 CPU-s, 123.3 MB | completes ≤ 30 s | PASS | wall 1048 ms |
| resources | test, 10k, warm | 1.03 CPU-s, 78.4 MB | completes ≤ 30 s | PASS | wall 423 ms |
| resources | build, 10k, cold | 1.02 CPU-s, 105.9 MB | completes ≤ 30 s | PASS | wall 362 ms |
| resources | build, 10k, warm | 0.05 CPU-s, 34.3 MB | completes ≤ 30 s | PASS | wall 46 ms |
| resources | check, 50k, cold | 1.58 CPU-s, 161.4 MB | completes ≤ 150 s | PASS | wall 608 ms |
| resources | check, 50k, warm | 0.12 CPU-s, 56.5 MB | completes ≤ 150 s | PASS | wall 116 ms |
| resources | test, 50k, cold | 6.36 CPU-s, 284.0 MB | completes ≤ 150 s | PASS | wall 3160 ms |
| resources | test, 50k, warm | 2.65 CPU-s, 105.0 MB | completes ≤ 150 s | PASS | wall 978 ms |
| resources | build, 50k, cold | 2.25 CPU-s, 207.1 MB | completes ≤ 150 s | PASS | wall 812 ms |
| resources | build, 50k, warm | 0.14 CPU-s, 64.4 MB | completes ≤ 150 s | PASS | wall 123 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 87.3 MB | ≤ 22.0 MB | FAIL | RSS 93.7 MB at input 100, 181.1 MB at input 1000; build per input 73 ms at 50, 780 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.1 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 4.2 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 15 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 16.8 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.09x | ≤ 1.50x | FAIL | p95 102 ms at N = 1; 4 cores; total CPU 790 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.66x | ≤ 6.00x | FAIL | p95 102 ms at N = 1; 4 cores; total CPU 3310 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.12 | &lt; 1.00 (sublinear) | FAIL | 150 ms at N = 1, 3310 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 20.0 MB | ≤ 10.0 MB | FAIL | artifacts 7635 B in 10 files; toolchain 20.0 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 10.9 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.79x | ≥ 2.40x | FAIL | by --jobs |
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
| proptest-perf | property cases per second | 37k/s | ≥ 100k/s | FAIL | 6000 extra cases took 163 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 98 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 885 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.7 ms | ≤ 1.0 ms | PASS | 100 tests: 208 ms |
| integration-test-perf | setup per integration test program | 130 ms | ≤ 20 ms | FAIL | 10 programs: 1430 ms for 21 tests, 68 ms per test |
| integration-test-perf | total time growth in programs | 0.983 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 269 ms at 2, 1308 ms at 10 programs |
| integration-test-perf | cost per doc test | 119 ms | ≤ 20 ms | FAIL | 10 doc tests: 1312 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 0.87x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.49x | ≤ 3.00x | PASS | hd 4.1 ms (p10 2.0, p90 12), Node 8.5 ms, Python 76 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 1.52x | ≤ 3.00x | PASS | hd 12 ms (p10 9.8, p90 22), Node 8.2 ms, Python 243 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.75x | ≤ 3.00x | PASS | hd 6.8 ms (p10 1.6, p90 8.5), Node 9.1 ms, Python 11 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 2.26x | ≤ 3.00x | PASS | hd 21 ms (p10 15, p90 28), Node 9.2 ms, Python 15 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.40x | ≤ 3.00x | PASS | hd 7.9 ms (p10 6.4, p90 15), Node 20 ms, Python 15 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 1597 B | ≤ 2048 B | PASS | benchmarks: fib 5081 B, sum 4875 B, string-build 7936 B, map 6616 B, sort 12758 B |
| size-startup-heap | tiny program, start to first output | 45 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 1.00x | ≤ 2.00x | PASS | hd +14.3 MB, Node +14.3 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.39x | ≤ 2.00x | PASS | hd +8.1 MB, Node +20.7 MB over an empty program |
| host-call-overhead | Console println, per call | 6.7 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm Timing change ≤10%: noise. |
| host-call-overhead | Fs read_text!, per call | 19 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 33 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 9.7 ns | ≤ 100 ns | PASS | 9.7 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 12.4M/s | ≥ 1.0M/s | PASS | 161 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 10.8M/s | ≥ 1.0M/s | PASS | 185 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.18x | ≥ 0.50x | FAIL | hd 65 MB/s (45488 B document), Node 364 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.26x | ≥ 0.50x | FAIL | hd 48 MB/s (45488 B document), Node 183 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 3.18x | ≥ 0.50x | PASS | hd 361 MB/s, Node 113 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 2.59x | ≥ 0.50x | PASS | hd 731 MB/s, Node 282 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.12x | ≥ 0.50x | FAIL | hd 9.6 MB/s, Node 82 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 6906 B | ≤ 10240 B | PASS | 12823 B at 1075 lines, 74987 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 897 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.4 MB | ≤ 10.0 MB | PASS | 21898000 requests; RSS 72.8 MB after warm-up, 73.1 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 7459 of 7459 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
- [2026-10-30T03](daily/2026-10-30T03.json)
