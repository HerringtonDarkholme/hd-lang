# Daily goal metrics

Finished: 2026-10-26T03:35:11.536Z. Commit: c485beba36f75d10ce558859851649c5158319e2.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V74 80-Core Processor).
Compiler: target/release/hd.

96 passed, 35 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: fail - → pass 35 ms
- edit-latency: p95 check after an edit: fail - → pass 35 ms
- cold-check: median cold check, 10k lines: fail - → pass 132 ms
- test-latency: median edit-then-test: fail - → pass 160 ms
- mistakes: one diagnostic with the expected code: fail 66.7% → fail 94.3%
- mistakes: fix-it makes hd check pass: fail 26.4% → fail 34.3%
- answer-size: hd doc of a data type: fail - → pass 153 B
- answer-size: one failing hd test, text: pass 75 B → pass 318 B
- answer-size: one failing test, JSON object: fail - → pass 139 B
- pathological: overlapping-from: time at 4x: fail - → pass 41 ms
- pathological: overlapping-from: peak RSS at 4x: fail - → pass 21.1 MB
- pathological: overlapping-from: growth exponent: fail - → pass noise
- pathological: deep-blocks: time at 4x: fail - → pass 15 ms
- pathological: deep-blocks: peak RSS at 4x: fail - → pass 15.5 MB
- pathological: deep-parens: time at 4x: fail - → pass 14 ms
- pathological: deep-parens: peak RSS at 4x: fail - → pass 15.9 MB
- pathological: deep-parens: growth exponent: fail - → pass noise
- pathological: wide-list-literal: time at 4x: pass 45 ms → pass 39 ms
- pathological: deep-generic: time at 4x: fail - → pass 17 ms
- pathological: deep-generic: peak RSS at 4x: fail - → pass 16.3 MB
- pathological: deep-generic: growth exponent: fail - → pass noise
- recheck-precision: modules rechecked, private body edit: fail - → pass 1
- diag-location: diagnostics on the mistake's line: fail 76.4% → pass 98.6%
- fmt: hd fmt, 10k lines: fail - → pass 11 ms
- fmt: second hd fmt changes nothing: fail - → pass no change
- dev-speed: geomean dev / optimized: fail - → pass 1.03x
- dev-speed: worst case dev / optimized: fail - → pass 1.08x
- resources: warm check, 10k lines: CPU time: fail - → pass 30 ms
- resources: warm check, 10k lines: peak RSS: fail - → pass 23.8 MB
- resources: check, small, cold: fail - → pass 0.09 CPU-s, 27.0 MB
- resources: check, small, warm: fail - → pass 0.00 CPU-s, 14.4 MB
- resources: test, small, cold: fail - → pass 0.94 CPU-s, 73.7 MB
- resources: test, small, warm: fail - → pass 0.56 CPU-s, 73.6 MB
- resources: build, small, cold: fail - → pass 0.37 CPU-s, 61.7 MB
- resources: build, small, warm: fail - → pass 0.02 CPU-s, 22.6 MB
- resources: check, 10k, cold: fail - → pass 0.29 CPU-s, 48.5 MB
- resources: check, 10k, warm: fail - → pass 0.03 CPU-s, 23.8 MB
- resources: test, 10k, cold: fail - → pass 1.73 CPU-s, 94.6 MB
- resources: test, 10k, warm: fail - → pass 1.10 CPU-s, 82.2 MB
- resources: build, 10k, cold: fail - → pass 0.59 CPU-s, 88.9 MB
- resources: build, 10k, warm: fail - → pass 0.04 CPU-s, 29.9 MB
- resources: check, 50k, cold: fail - → pass 1.23 CPU-s, 143.0 MB
- resources: check, 50k, warm: fail - → pass 0.18 CPU-s, 61.9 MB
- resources: test, 50k, cold: fail - → pass 11.53 CPU-s, 251.7 MB
- resources: test, 50k, warm: fail - → pass 9.57 CPU-s, 116.2 MB
- resources: build, 50k, cold: fail - → pass 1.55 CPU-s, 236.0 MB
- resources: build, 50k, warm: fail - → pass 0.16 CPU-s, 67.1 MB
- startup: hd check empty.hd: wall time: pass 10 ms → pass 13 ms
- cache-contention: compilation cache, 8 concurrent checks: no corruption: fail - → pass intact
- cache-contention: compilation cache, 8 concurrent checks: written once: fail - → pass 3 files
- cache-growth: cache size after 100 edits, cap 5 MB: fail - → pass 4.5 MB
- proptest-perf: time to shrink a known failure: pass 0.0 ms → pass 109 ms
- unit-test-perf: overhead per unit test: pass 0.7 ms → pass 0.8 ms
- runtime: geomean hd / Node: fail 2.46x → pass 1.47x
- runtime: fib: hd / Node: pass 2.23x → pass 0.78x
- runtime: sum: hd / Node: pass 2.93x → pass 1.49x
- runtime: string-build: hd / Node: fail 3.06x → pass 2.19x
- runtime: map: hd / Node: fail 3.00x → pass 1.94x
- allocations: counted loop, bytes per iteration: fail - → pass 0.00 B/iter
- allocations: list loop, bytes per iteration: fail - → pass 0.00 B/iter
- allocations: map loop, bytes per iteration: fail - → pass 0.00 B/iter
- allocations: iterator chain, bytes per iteration: fail - → pass 0.00 B/iter
- suspension-overhead: await of a ready value, per await: fail - → pass 15 ns
- suspension-overhead: all! of two ready tasks, throughput: fail - → pass 10.7M/s
- suspension-overhead: race! of two ready tasks, throughput: fail - → pass 8.1M/s
- text-throughput: string build, hd / Node MB/s: fail - → pass 0.74x
- text-throughput: split, hd / Node MB/s: fail - → pass 2.13x
- long-run-memory: RSS growth after warm-up, 1 min: fail - → pass 2.9 MB
- incremental-soundness: incremental equals clean, after each edit: fail - → pass 100.0%

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 35 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 35 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 132 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 160 ms | ≤ 300 ms | PASS |  |
| mistakes | programs with exactly one diagnostic | 94.3% | ≥ 95.0% | FAIL | 66 of 70 |
| mistakes | one diagnostic with the expected code | 94.3% | ≥ 95.0% | FAIL | 66 of 70; first misses: loop-keyword, mut-on-binding-type, mut-on-parameter-type, some-on-result |
| mistakes | fix-it makes hd check pass | 34.3% | ≥ 80.0% | FAIL | 24 of 70 resolved; 24 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 93 tokens | ≤ 60 tokens | FAIL | largest: mut-on-parameter-type; median 43 tokens, 171 B; 64 of 70 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 41 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 21.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 28 → 41 ms, start-up 14 ms |
| pathological | deep-blocks: time at 4x | 15 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 15.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 14 → 15 ms, start-up 14 ms |
| pathological | deep-parens: time at 4x | 14 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 15.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 14 → 14 ms, start-up 14 ms |
| pathological | method-chain: time at 4x | - | ≤ 2000 ms | FAIL | 4x: hd check failed: thread '&lt;unknown>' (6532) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 4x: hd check failed: thread '&lt;unknown>' (6532) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 1x→2x; 16 → 17 ms, start-up 14 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 39 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 19.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 27 → 39 ms, start-up 14 ms |
| pathological | large-enum-match: time at 4x | 387 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | large-enum-match: peak RSS at 4x | 18.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | 2.28 | ≤ 1.30 | FAIL | 2x→4x; 91 → 387 ms, start-up 14 ms |
| pathological | deep-generic: time at 4x | 17 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 16.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 15 → 17 ms, start-up 14 ms |
| pathological | question-chain: time at 4x | 22 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | question-chain: peak RSS at 4x | 19.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 18 → 22 ms, start-up 14 ms |
| pathological | big-file: time at 4x | 133 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | big-file: peak RSS at 4x | 51.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.09 | ≤ 1.30 | PASS | 2x→4x; 70 → 133 ms, start-up 14 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 69 of 70; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 24 of 24 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 11 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 2.50x | ≤ 1.30x | FAIL | checked 1148 ms, unchecked 459 ms |
| dev-speed | geomean dev / optimized | 1.03x | ≤ 4.00x | PASS | fib 1.05x, sum 1.02x, string-build 1.08x, map 1.05x, sort 0.98x |
| dev-speed | worst case dev / optimized | 1.08x | ≤ 10.00x | PASS | string-build |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 30 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 23.8 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.09 CPU-s, 27.0 MB | completes ≤ 30 s | PASS | wall 65 ms |
| resources | check, small, warm | 0.00 CPU-s, 14.4 MB | completes ≤ 30 s | PASS | wall 13 ms |
| resources | test, small, cold | 0.94 CPU-s, 73.7 MB | completes ≤ 30 s | PASS | wall 427 ms |
| resources | test, small, warm | 0.56 CPU-s, 73.6 MB | completes ≤ 30 s | PASS | wall 242 ms |
| resources | build, small, cold | 0.37 CPU-s, 61.7 MB | completes ≤ 30 s | PASS | wall 158 ms |
| resources | build, small, warm | 0.02 CPU-s, 22.6 MB | completes ≤ 30 s | PASS | wall 21 ms |
| resources | check, 10k, cold | 0.29 CPU-s, 48.5 MB | completes ≤ 30 s | PASS | wall 135 ms |
| resources | check, 10k, warm | 0.03 CPU-s, 23.8 MB | completes ≤ 30 s | PASS | wall 33 ms |
| resources | test, 10k, cold | 1.73 CPU-s, 94.6 MB | completes ≤ 30 s | PASS | wall 878 ms |
| resources | test, 10k, warm | 1.10 CPU-s, 82.2 MB | completes ≤ 30 s | PASS | wall 618 ms |
| resources | build, 10k, cold | 0.59 CPU-s, 88.9 MB | completes ≤ 30 s | PASS | wall 225 ms |
| resources | build, 10k, warm | 0.04 CPU-s, 29.9 MB | completes ≤ 30 s | PASS | wall 49 ms |
| resources | check, 50k, cold | 1.23 CPU-s, 143.0 MB | completes ≤ 150 s | PASS | wall 472 ms |
| resources | check, 50k, warm | 0.18 CPU-s, 61.9 MB | completes ≤ 150 s | PASS | wall 113 ms |
| resources | test, 50k, cold | 11.53 CPU-s, 251.7 MB | completes ≤ 150 s | PASS | wall 8993 ms |
| resources | test, 50k, warm | 9.57 CPU-s, 116.2 MB | completes ≤ 150 s | PASS | wall 8031 ms |
| resources | build, 50k, cold | 1.55 CPU-s, 236.0 MB | completes ≤ 150 s | PASS | wall 595 ms |
| resources | build, 50k, warm | 0.16 CPU-s, 67.1 MB | completes ≤ 150 s | PASS | wall 143 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 368.8 MB | ≤ 22.0 MB | FAIL | RSS 137.8 MB at input 100, 506.6 MB at input 1000; build per input 72 ms at 50, 619 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 2.9 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.7 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 13 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 12.9 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.11x | ≤ 1.50x | FAIL | p95 70 ms at N = 1; 4 cores; total CPU 500 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.44x | ≤ 6.00x | FAIL | p95 70 ms at N = 1; 4 cores; total CPU 2120 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.10 | &lt; 1.00 (sublinear) | FAIL | 100 ms at N = 1, 2120 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 15.5 MB | ≤ 10.0 MB | FAIL | artifacts 9064 B in 10 files; toolchain 15.5 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 7.5 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.90x | ≥ 2.40x | FAIL | by --jobs |
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
| proptest-perf | property cases per second | 25k/s | ≥ 100k/s | FAIL | 6000 extra cases took 239 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 109 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 878 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed Timing change ≤10%: noise. |
| unit-test-perf | overhead per unit test | 0.8 ms | ≤ 1.0 ms | PASS | 100 tests: 190 ms |
| integration-test-perf | setup per integration test program | 101 ms | ≤ 20 ms | FAIL | 10 programs: 1124 ms for 21 tests, 54 ms per test |
| integration-test-perf | total time growth in programs | 1.006 | exponent &lt; 1 (sublinear) | FAIL | beyond the 0-program run: 202 ms at 2, 1020 ms at 10 programs |
| integration-test-perf | cost per doc test | 92 ms | ≤ 20 ms | FAIL | 10 doc tests: 1026 ms; the run reports 11 of 11 passed Timing change ≤10%: noise. |
| runtime | geomean hd / Node | 1.47x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.78x | ≤ 3.00x | PASS | hd 6.6 ms (p10 2.8, p90 8.4), Node 8.5 ms, Python 89 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 1.49x | ≤ 3.00x | PASS | hd 12 ms (p10 7.6, p90 20), Node 8.2 ms, Python 276 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 2.19x | ≤ 3.00x | PASS | hd 20 ms (p10 17, p90 24), Node 9.2 ms, Python 12 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 1.94x | ≤ 3.00x | PASS | hd 16 ms (p10 12, p90 26), Node 8.4 ms, Python 14 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 1.38x | ≤ 3.00x | PASS | hd 27 ms (p10 19, p90 35), Node 20 ms, Python 17 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 7052 B | ≤ 2048 B | FAIL | benchmarks: fib 4341 B, sum 4279 B, string-build 7502 B, map 6273 B, sort 7186 B |
| size-startup-heap | tiny program, start to first output | 38 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.95x | ≤ 2.00x | PASS | hd +13.3 MB, Node +14.0 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.39x | ≤ 2.00x | PASS | hd +8.5 MB, Node +21.7 MB over an empty program |
| host-call-overhead | Console println, per call | 4.4 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 20 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 34 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 15 ns | ≤ 100 ns | PASS | 15 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 10.7M/s | ≥ 1.0M/s | PASS | 187 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 8.1M/s | ≥ 1.0M/s | PASS | 246 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.04x | ≥ 0.50x | FAIL | hd 15 MB/s (45488 B document), Node 365 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.06x | ≥ 0.50x | FAIL | hd 11 MB/s (45488 B document), Node 184 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 0.74x | ≥ 0.50x | PASS | hd 91 MB/s, Node 122 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 2.13x | ≥ 0.50x | PASS | hd 627 MB/s, Node 294 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.06x | ≥ 0.50x | FAIL | hd 4.6 MB/s, Node 84 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 10780 B | ≤ 10240 B | FAIL | 18310 B at 1075 lines, 115345 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 743 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 2.9 MB | ≤ 10.0 MB | PASS | 8950000 requests; RSS 69.9 MB after warm-up, 72.9 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 5341 of 5341 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

- [2026-10-26T03](daily/2026-10-26T03.json)
- [2026-10-25](daily/2026-10-25.json)
