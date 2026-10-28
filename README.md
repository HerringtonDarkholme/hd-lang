# Daily goal metrics

Finished: 2026-10-28T03:03:26.257Z. Commit: 92f651ed9b78eaa31c3e8c889cfec2e0194c0ca4.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V45 96-Core Processor).
Compiler: target/release/hd.

99 passed, 32 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- cold-check: median cold check, 10k lines: pass 185 ms → pass 131 ms
- test-latency: median edit-then-test: pass 223 ms → pass 195 ms
- pathological: overlapping-from: time at 4x: pass 56 ms → pass 36 ms
- pathological: deep-blocks: time at 4x: pass 21 ms → pass 15 ms
- pathological: deep-parens: time at 4x: pass 20 ms → pass 14 ms
- pathological: wide-list-literal: time at 4x: pass 58 ms → pass 40 ms
- pathological: large-enum-match: time at 4x: pass 458 ms → pass 343 ms
- pathological: deep-generic: time at 4x: pass 22 ms → pass 16 ms
- pathological: question-chain: time at 4x: pass 31 ms → pass 20 ms
- pathological: big-file: time at 4x: pass 180 ms → pass 118 ms
- fmt: hd fmt, 10k lines: pass 33 ms → pass 19 ms
- check-cost: checked run time / unchecked run time: fail 2.88x → fail 3.45x
- resources: warm check, 10k lines: CPU time: pass 60 ms → pass 50 ms
- resources: warm check, 10k lines: peak RSS: pass 24.0 MB → pass 30.7 MB
- startup: hd help (no version command): wall time: pass 5.0 ms → pass 2.8 ms
- startup: hd check empty.hd: wall time: pass 18 ms → pass 11 ms
- proptest-perf: property cases per second: fail 21k/s → fail 55k/s
- proptest-perf: time to shrink a known failure: pass 140 ms → pass 83 ms
- unit-test-perf: 1,000 unit tests, warm: fail 1245 ms → pass 752 ms
- unit-test-perf: overhead per unit test: fail 1.1 ms → pass 0.6 ms
- integration-test-perf: setup per integration test program: fail 145 ms → fail 99 ms
- integration-test-perf: cost per doc test: fail 130 ms → fail 96 ms
- runtime: geomean hd / Node: pass 1.08x → pass 1.21x
- runtime: fib: hd / Node: pass 0.29x → pass 0.60x
- runtime: sum: hd / Node: pass 2.17x → pass 1.47x
- runtime: string-build: hd / Node: pass 0.98x → pass 1.15x
- runtime: map: hd / Node: pass 2.09x → pass 2.44x
- size-startup-heap: tiny program, release Wasm size: fail 4522 B → fail 2321 B
- size-startup-heap: tiny program, start to first output: fail 55 ms → fail 31 ms
- host-call-overhead: Console println, per call: fail 9.6 µs → fail 5.4 µs
- host-call-overhead: Fs read_text!, per call: fail 28 µs → fail 15 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 50 µs → fail 27 µs
- suspension-overhead: await of a ready value, per await: pass 20 ns → pass 10 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 8.1M/s → pass 16.3M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 6.2M/s → pass 13.3M/s
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.09x → fail 0.10x
- text-throughput: string build, hd / Node MB/s: pass 1.55x → pass 1.37x
- text-throughput: split, hd / Node MB/s: pass 1.90x → pass 2.10x
- long-run-memory: RSS growth after warm-up, 1 min: pass 2.6 MB → pass 0.4 MB
- conformance: listed known failures: fail 4 → fail 1

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 47 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 48 ms | ≤ 200 ms | PASS | Timing change ≤10%: noise. |
| cold-check | median cold check, 10k lines | 131 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 195 ms | ≤ 300 ms | PASS |  |
| mistakes | programs with exactly one diagnostic | 95.7% | ≥ 95.0% | PASS | 67 of 70 |
| mistakes | one diagnostic with the expected code | 95.7% | ≥ 95.0% | PASS | 67 of 70; first misses: mut-on-binding-type, mut-on-parameter-type, some-on-result |
| mistakes | fix-it makes hd check pass | 34.3% | ≥ 80.0% | FAIL | 24 of 70 resolved; 24 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 93 tokens | ≤ 60 tokens | FAIL | largest: mut-on-parameter-type; median 43 tokens, 171 B; 65 of 70 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 36 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 21.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 26 → 36 ms, start-up 14 ms |
| pathological | deep-blocks: time at 4x | 15 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 16.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 15 → 15 ms, start-up 14 ms |
| pathological | deep-parens: time at 4x | 14 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 15.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 15 → 14 ms, start-up 14 ms |
| pathological | method-chain: time at 4x | - | ≤ 2000 ms | FAIL | 4x: hd check failed: thread '&lt;unknown>' (7355) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 4x: hd check failed: thread '&lt;unknown>' (7355) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 1x→2x; 15 → 17 ms, start-up 14 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 40 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 19.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 29 → 40 ms, start-up 14 ms |
| pathological | large-enum-match: time at 4x | 343 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 19.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | 2.23 | ≤ 1.30 | FAIL | 2x→4x; 85 → 343 ms, start-up 14 ms |
| pathological | deep-generic: time at 4x | 16 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 16.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 15 → 16 ms, start-up 14 ms |
| pathological | question-chain: time at 4x | 20 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 19.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 17 → 20 ms, start-up 14 ms |
| pathological | big-file: time at 4x | 118 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 51.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.12 | ≤ 1.30 | PASS | 2x→4x; 62 → 118 ms, start-up 14 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 69 of 70; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 24 of 24 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 19 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 3.45x | ≤ 1.30x | FAIL | checked 805 ms, unchecked 233 ms |
| dev-speed | geomean dev / optimized | 1.02x | ≤ 4.00x | PASS | fib 0.90x, sum 1.06x, string-build 1.15x, map 0.99x, sort 1.01x |
| dev-speed | worst case dev / optimized | 1.15x | ≤ 10.00x | PASS | string-build |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 50 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 30.7 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.08 CPU-s, 27.2 MB | completes ≤ 30 s | PASS | wall 59 ms |
| resources | check, small, warm | 0.00 CPU-s, 16.0 MB | completes ≤ 30 s | PASS | wall 14 ms |
| resources | test, small, cold | 0.85 CPU-s, 86.6 MB | completes ≤ 30 s | PASS | wall 396 ms |
| resources | test, small, warm | 0.49 CPU-s, 72.5 MB | completes ≤ 30 s | PASS | wall 236 ms |
| resources | build, small, cold | 0.36 CPU-s, 85.7 MB | completes ≤ 30 s | PASS | wall 177 ms |
| resources | build, small, warm | 0.04 CPU-s, 35.3 MB | completes ≤ 30 s | PASS | wall 45 ms |
| resources | check, 10k, cold | 0.26 CPU-s, 53.9 MB | completes ≤ 30 s | PASS | wall 131 ms |
| resources | check, 10k, warm | 0.05 CPU-s, 30.7 MB | completes ≤ 30 s | PASS | wall 44 ms |
| resources | test, 10k, cold | 1.48 CPU-s, 109.2 MB | completes ≤ 30 s | PASS | wall 787 ms |
| resources | test, 10k, warm | 0.90 CPU-s, 81.7 MB | completes ≤ 30 s | PASS | wall 530 ms |
| resources | build, 10k, cold | 0.56 CPU-s, 108.1 MB | completes ≤ 30 s | PASS | wall 244 ms |
| resources | build, 10k, warm | 0.08 CPU-s, 44.0 MB | completes ≤ 30 s | PASS | wall 77 ms |
| resources | check, 50k, cold | 1.09 CPU-s, 170.2 MB | completes ≤ 150 s | PASS | wall 490 ms |
| resources | check, 50k, warm | 0.24 CPU-s, 92.7 MB | completes ≤ 150 s | PASS | wall 189 ms |
| resources | test, 50k, cold | 8.64 CPU-s, 287.6 MB | completes ≤ 150 s | PASS | wall 6574 ms |
| resources | test, 50k, warm | 7.16 CPU-s, 140.8 MB | completes ≤ 150 s | PASS | wall 5756 ms |
| resources | build, 50k, cold | 1.42 CPU-s, 282.7 MB | completes ≤ 150 s | PASS | wall 635 ms |
| resources | build, 50k, warm | 0.24 CPU-s, 110.3 MB | completes ≤ 150 s | PASS | wall 239 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 106.6 MB | ≤ 22.0 MB | FAIL | RSS 113.4 MB at input 100, 220.0 MB at input 1000; build per input 76 ms at 50, 460 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 2.8 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.4 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 11 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 13.4 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 1.88x | ≤ 1.50x | FAIL | p95 67 ms at N = 1; 4 cores; total CPU 440 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 7.47x | ≤ 6.00x | FAIL | p95 67 ms at N = 1; 4 cores; total CPU 1910 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.10 | &lt; 1.00 (sublinear) | FAIL | 90 ms at N = 1, 1910 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 15.5 MB | ≤ 10.0 MB | FAIL | artifacts 6819 B in 10 files; toolchain 15.5 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 8.8 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.71x | ≥ 2.40x | FAIL | by --jobs |
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
| proptest-perf | property cases per second | 55k/s | ≥ 100k/s | FAIL | 6000 extra cases took 109 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 83 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 752 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.6 ms | ≤ 1.0 ms | PASS | 100 tests: 176 ms |
| integration-test-perf | setup per integration test program | 99 ms | ≤ 20 ms | FAIL | 10 programs: 1101 ms for 21 tests, 52 ms per test |
| integration-test-perf | total time growth in programs | 0.990 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 202 ms at 2, 993 ms at 10 programs |
| integration-test-perf | cost per doc test | 96 ms | ≤ 20 ms | FAIL | 10 doc tests: 1059 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 1.21x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.60x | ≤ 3.00x | PASS | hd 3.3 ms (p10 1.7, p90 6.7), Node 5.4 ms, Python 61 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 1.47x | ≤ 3.00x | PASS | hd 9.5 ms (p10 6.6, p90 13), Node 6.4 ms, Python 196 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 1.15x | ≤ 3.00x | PASS | hd 8.5 ms (p10 6.1, p90 14), Node 7.4 ms, Python 8.9 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 2.44x | ≤ 3.00x | PASS | hd 15 ms (p10 14, p90 18), Node 6.2 ms, Python 10 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 1.05x | ≤ 3.00x | PASS | hd 16 ms (p10 10, p90 20), Node 15 ms, Python 13 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 2321 B | ≤ 2048 B | FAIL | benchmarks: fib 3656 B, sum 3610 B, string-build 5325 B, map 5164 B, sort 5694 B |
| size-startup-heap | tiny program, start to first output | 31 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.93x | ≤ 2.00x | PASS | hd +13.3 MB, Node +14.2 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.39x | ≤ 2.00x | PASS | hd +8.1 MB, Node +20.7 MB over an empty program |
| host-call-overhead | Console println, per call | 5.4 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 15 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 27 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 10 ns | ≤ 100 ns | PASS | 10 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 16.3M/s | ≥ 1.0M/s | PASS | 122 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 13.3M/s | ≥ 1.0M/s | PASS | 150 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.05x | ≥ 0.50x | FAIL | hd 26 MB/s (45488 B document), Node 515 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.10x | ≥ 0.50x | FAIL | hd 24 MB/s (45488 B document), Node 253 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 1.37x | ≥ 0.50x | PASS | hd 231 MB/s, Node 169 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 2.10x | ≥ 0.50x | PASS | hd 819 MB/s, Node 390 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.10x | ≥ 0.50x | FAIL | hd 12 MB/s, Node 121 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 5343 B | ≤ 10240 B | PASS | 10073 B at 1075 lines, 58168 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 664 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.4 MB | ≤ 10.0 MB | PASS | 20512000 requests; RSS 70.2 MB after warm-up, 70.7 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 5411 of 5411 unlisted cases passed |
| conformance | listed known failures | 1 | ≤ 0 | FAIL | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

- [2026-10-28T03](daily/2026-10-28T03.json)
- [2026-10-27T18](daily/2026-10-27T18.json)
- [2026-10-27T02](daily/2026-10-27T02.json)
- [2026-10-26T18](daily/2026-10-26T18.json)
- [2026-10-26T03](daily/2026-10-26T03.json)
- [2026-10-25](daily/2026-10-25.json)
