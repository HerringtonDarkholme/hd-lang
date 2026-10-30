# Daily goal metrics

Finished: 2026-10-30T18:06:55.081Z. Commit: ab551e0dcd715d0f31180c191552c527abd7c26e.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V74 80-Core Processor).
Compiler: target/release/hd.

99 passed, 32 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- pathological: wide-list-literal: time at 4x: pass 39 ms → pass 43 ms
- pathological: large-enum-match: time at 4x: pass 331 ms → pass 477 ms
- pathological: large-enum-match: growth exponent: fail 2.13 → fail 2.52
- pathological: big-file: time at 4x: pass 134 ms → pass 153 ms
- fmt: hd fmt, 10k lines: pass 21 ms → pass 24 ms
- dev-speed: worst case dev / optimized: pass 1.30x → pass 1.07x
- long-session: RSS growth, REPL inputs 100 to 1000: fail 42.4 MB → fail 86.2 MB
- startup: hd help (no version command): wall time: pass 4.1 ms → pass 3.1 ms
- startup: hd check empty.hd: wall time: pass 15 ms → pass 13 ms
- concurrency: p95 latency at N = 4 vs N = 1: fail 2.52x → fail 2.09x
- proptest-perf: property cases per second: fail 50k/s → fail 38k/s
- runtime: geomean hd / Node: pass 1.11x → pass 1.23x
- runtime: sum: hd / Node: pass 1.18x → pass 1.89x
- runtime: string-build: hd / Node: pass 1.35x → pass 1.16x
- runtime: sort: hd / Node: pass 0.97x → pass 1.13x
- host-call-overhead: Console println, per call: fail 5.9 µs → fail 6.8 µs
- host-call-overhead: Fs read_text!, per call: fail 16 µs → fail 18 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 28 µs → fail 32 µs
- suspension-overhead: await of a ready value, per await: pass 13 ns → pass 30 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 14.3M/s → pass 11.1M/s
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.10x → fail 0.12x
- text-throughput: regex find_all, hd / Node MB/s: fail 0.12x → fail 0.10x
- long-run-memory: RSS growth after warm-up, 1 min: pass 0.8 MB → pass 3.2 MB
- conformance: listed known failures: fail 14 → pass 0

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 57 ms | ≤ 50 ms | FAIL | Timing change ≤10%: noise. |
| edit-latency | p95 check after an edit | 58 ms | ≤ 200 ms | PASS | Timing change ≤10%: noise. |
| cold-check | median cold check, 10k lines | 172 ms | ≤ 1000 ms | PASS | Timing change ≤10%: noise. |
| test-latency | median edit-then-test | 252 ms | ≤ 300 ms | PASS | Timing change ≤10%: noise. |
| mistakes | programs with exactly one diagnostic | 95.7% | ≥ 95.0% | PASS | 66 of 69 |
| mistakes | one diagnostic with the expected code | 95.7% | ≥ 95.0% | PASS | 66 of 69; first misses: mut-on-binding-type, mut-on-parameter-type, some-on-result |
| mistakes | fix-it makes hd check pass | 34.8% | ≥ 80.0% | FAIL | 24 of 69 resolved; 24 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 93 tokens | ≤ 60 tokens | FAIL | largest: mut-on-parameter-type; median 44 tokens, 175 B; 64 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 45 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | overlapping-from: peak RSS at 4x | 23.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 30 → 45 ms, start-up 18 ms |
| pathological | deep-blocks: time at 4x | 18 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 17.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 17 → 18 ms, start-up 18 ms |
| pathological | deep-parens: time at 4x | 17 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-parens: peak RSS at 4x | 18.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 17 → 17 ms, start-up 18 ms |
| pathological | method-chain: time at 4x | - | ≤ 2000 ms | FAIL | 4x: hd check failed: thread '&lt;unknown>' (6785) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 4x: hd check failed: thread '&lt;unknown>' (6785) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 1x→2x; 18 → 20 ms, start-up 18 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 43 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 21.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 36 → 43 ms, start-up 18 ms |
| pathological | large-enum-match: time at 4x | 477 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 20.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | 2.52 | ≤ 1.30 | FAIL | 2x→4x; 98 → 477 ms, start-up 18 ms |
| pathological | deep-generic: time at 4x | 19 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-generic: peak RSS at 4x | 17.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 18 → 19 ms, start-up 18 ms |
| pathological | question-chain: time at 4x | 24 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | question-chain: peak RSS at 4x | 20.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 21 → 24 ms, start-up 18 ms |
| pathological | big-file: time at 4x | 153 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 51.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.13 | ≤ 1.30 | PASS | 2x→4x; 80 → 153 ms, start-up 18 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 24 of 24 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 24 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 2.75x | ≤ 1.30x | FAIL | checked 970 ms, unchecked 352 ms |
| dev-speed | geomean dev / optimized | 1.01x | ≤ 4.00x | PASS | fib 1.07x, sum 0.99x, string-build 0.98x, map 1.02x, sort 0.99x |
| dev-speed | worst case dev / optimized | 1.07x | ≤ 10.00x | PASS | fib |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 60 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 31.8 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.12 CPU-s, 31.2 MB | completes ≤ 30 s | PASS | wall 82 ms |
| resources | check, small, warm | 0.01 CPU-s, 17.5 MB | completes ≤ 30 s | PASS | wall 18 ms |
| resources | test, small, cold | 1.14 CPU-s, 108.4 MB | completes ≤ 30 s | PASS | wall 531 ms |
| resources | test, small, warm | 0.61 CPU-s, 69.2 MB | completes ≤ 30 s | PASS | wall 296 ms |
| resources | build, small, cold | 0.54 CPU-s, 91.7 MB | completes ≤ 30 s | PASS | wall 250 ms |
| resources | build, small, warm | 0.05 CPU-s, 39.7 MB | completes ≤ 30 s | PASS | wall 60 ms |
| resources | check, 10k, cold | 0.35 CPU-s, 57.1 MB | completes ≤ 30 s | PASS | wall 173 ms |
| resources | check, 10k, warm | 0.06 CPU-s, 31.8 MB | completes ≤ 30 s | PASS | wall 54 ms |
| resources | test, 10k, cold | 1.92 CPU-s, 131.6 MB | completes ≤ 30 s | PASS | wall 1009 ms |
| resources | test, 10k, warm | 1.13 CPU-s, 76.3 MB | completes ≤ 30 s | PASS | wall 681 ms |
| resources | build, 10k, cold | 0.79 CPU-s, 114.6 MB | completes ≤ 30 s | PASS | wall 327 ms |
| resources | build, 10k, warm | 0.10 CPU-s, 46.8 MB | completes ≤ 30 s | PASS | wall 97 ms |
| resources | check, 50k, cold | 1.41 CPU-s, 175.3 MB | completes ≤ 150 s | PASS | wall 610 ms |
| resources | check, 50k, warm | 0.28 CPU-s, 93.4 MB | completes ≤ 150 s | PASS | wall 224 ms |
| resources | test, 50k, cold | 11.65 CPU-s, 298.7 MB | completes ≤ 150 s | PASS | wall 8971 ms |
| resources | test, 50k, warm | 9.34 CPU-s, 120.3 MB | completes ≤ 150 s | PASS | wall 7810 ms |
| resources | build, 50k, cold | 1.94 CPU-s, 272.2 MB | completes ≤ 150 s | PASS | wall 839 ms |
| resources | build, 50k, warm | 0.31 CPU-s, 113.3 MB | completes ≤ 150 s | PASS | wall 298 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 86.2 MB | ≤ 22.0 MB | FAIL | RSS 132.5 MB at input 100, 207.4 MB at input 1000; build per input 128 ms at 50, 583 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.1 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.7 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 13 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 14.9 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.09x | ≤ 1.50x | FAIL | p95 83 ms at N = 1; 4 cores; total CPU 610 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.60x | ≤ 6.00x | FAIL | p95 83 ms at N = 1; 4 cores; total CPU 2680 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.12 | &lt; 1.00 (sublinear) | FAIL | 120 ms at N = 1, 2680 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 16.4 MB | ≤ 10.0 MB | FAIL | artifacts 6822 B in 10 files; toolchain 16.4 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 9.9 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.72x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 3 files | the files of one check's cache | PASS | one check alone: 3 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 4.3 MB | ≤ 5.0 MB | PASS | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 79 | ≤ 1 | FAIL | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 38k/s | ≥ 100k/s | FAIL | 6000 extra cases took 159 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 98 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 Timing change ≤10%: noise. |
| unit-test-perf | 1,000 unit tests, warm | 862 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed Timing change ≤10%: noise. |
| unit-test-perf | overhead per unit test | 0.7 ms | ≤ 1.0 ms | PASS | 100 tests: 204 ms |
| integration-test-perf | setup per integration test program | 126 ms | ≤ 20 ms | FAIL | 10 programs: 1386 ms for 21 tests, 66 ms per test Timing change ≤10%: noise. |
| integration-test-perf | total time growth in programs | 0.999 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 252 ms at 2, 1257 ms at 10 programs |
| integration-test-perf | cost per doc test | 113 ms | ≤ 20 ms | FAIL | 10 doc tests: 1267 ms; the run reports 11 of 11 passed Timing change ≤10%: noise. |
| runtime | geomean hd / Node | 1.23x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.47x | ≤ 3.00x | PASS | hd 4.0 ms (p10 3.0, p90 7.7), Node 8.5 ms, Python 79 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 1.89x | ≤ 3.00x | PASS | hd 15 ms (p10 12, p90 23), Node 8.2 ms, Python 247 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 1.16x | ≤ 3.00x | PASS | hd 11 ms (p10 9.8, p90 14), Node 9.2 ms, Python 11 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 2.42x | ≤ 3.00x | PASS | hd 22 ms (p10 15, p90 26), Node 9.1 ms, Python 15 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 1.13x | ≤ 3.00x | PASS | hd 22 ms (p10 18, p90 29), Node 19 ms, Python 16 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 2337 B | ≤ 2048 B | FAIL | benchmarks: fib 3656 B, sum 3610 B, string-build 5329 B, map 5203 B, sort 5694 B |
| size-startup-heap | tiny program, start to first output | 38 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm Timing change ≤10%: noise. |
| size-startup-heap | peak heap, map: hd / Node | 0.94x | ≤ 2.00x | PASS | hd +13.4 MB, Node +14.3 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.39x | ≤ 2.00x | PASS | hd +8.1 MB, Node +20.9 MB over an empty program |
| host-call-overhead | Console println, per call | 6.8 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 18 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 32 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 30 ns | ≤ 100 ns | PASS | 30 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 11.1M/s | ≥ 1.0M/s | PASS | 180 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 9.4M/s | ≥ 1.0M/s | PASS | 212 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.05x | ≥ 0.50x | FAIL | hd 20 MB/s (45488 B document), Node 364 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.12x | ≥ 0.50x | FAIL | hd 21 MB/s (45488 B document), Node 186 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 1.72x | ≥ 0.50x | PASS | hd 200 MB/s, Node 116 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 3.13x | ≥ 0.50x | PASS | hd 884 MB/s, Node 282 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.10x | ≥ 0.50x | FAIL | hd 8.6 MB/s, Node 84 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 5343 B | ≤ 10240 B | PASS | 10089 B at 1075 lines, 58182 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 664 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 3.2 MB | ≤ 10.0 MB | PASS | 16076000 requests; RSS 65.8 MB after warm-up, 69.1 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 5749 of 5749 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
