# Daily goal metrics

Finished: 2026-11-02T21:57:37.029Z. Commit: 9a7a93beef26ebcfec9f1623ba22de0b0b10f60f.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V74 80-Core Processor).
Compiler: target/release/hd.

96 passed, 35 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- test-latency: median edit-then-test: fail 479 ms → fail 538 ms
- pathological: deep-blocks: time at 4x: pass 22 ms → pass 27 ms
- pathological: deep-parens: time at 4x: pass 21 ms → pass 26 ms
- pathological: large-enum-match: time at 4x: pass 36 ms → pass 63 ms
- pathological: deep-generic: time at 4x: pass 26 ms → pass 34 ms
- pathological: big-file: growth exponent: pass 1.10 → pass 1.22
- check-cost: checked run time / unchecked run time: fail 1.73x → fail 1.94x
- dev-speed: geomean dev / optimized: pass 0.85x → pass 1.04x
- dev-speed: worst case dev / optimized: pass 0.99x → pass 1.18x
- long-session: RSS growth, REPL inputs 100 to 1000: fail 105.7 MB → fail 42.8 MB
- startup: hd check empty.hd: CPU time: pass 0.0 ms → pass 10 ms
- proptest-perf: property cases per second: fail 24k/s → fail 28k/s
- runtime: geomean hd / Node: pass 0.67x → fail -
- runtime: fib: hd / Node: pass 0.03x → pass 0.13x
- runtime: sum: hd / Node: pass 1.07x → pass 0.87x
- runtime: string-build: hd / Node: pass 0.34x → fail -
- allocations: counted loop, bytes per iteration: pass 0.04 B/iter → pass 0.00 B/iter
- size-startup-heap: peak heap, string-build: hd / Node: pass 0.24x → pass 0.20x
- host-call-overhead: Fs read_text!, per call: fail 26 µs → fail 23 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 46 µs → fail 40 µs
- suspension-overhead: await of a ready value, per await: pass 60 ns → pass 22 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 12.8M/s → pass 10.3M/s
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.20x → fail 0.23x
- text-throughput: split, hd / Node MB/s: pass 4.31x → pass 1.92x

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 77 ms | ≤ 50 ms | FAIL | Timing change ≤10%: noise. |
| edit-latency | p95 check after an edit | 80 ms | ≤ 200 ms | PASS | Timing change ≤10%: noise. |
| cold-check | median cold check, 10k lines | 245 ms | ≤ 1000 ms | PASS | Timing change ≤10%: noise. |
| test-latency | median edit-then-test | 538 ms | ≤ 300 ms | FAIL |  |
| mistakes | programs with exactly one diagnostic | 95.7% | ≥ 95.0% | PASS | 66 of 69 |
| mistakes | one diagnostic with the expected code | 95.7% | ≥ 95.0% | PASS | 66 of 69; first misses: mut-on-binding-type, mut-on-parameter-type, some-on-result |
| mistakes | fix-it makes hd check pass | 34.8% | ≥ 80.0% | FAIL | 24 of 69 resolved; 24 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 93 tokens | ≤ 60 tokens | FAIL | largest: mut-on-parameter-type; median 44 tokens, 175 B; 64 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 64 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | overlapping-from: peak RSS at 4x | 24.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 44 → 64 ms, start-up 25 ms |
| pathological | deep-blocks: time at 4x | 27 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 19.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 25 → 27 ms, start-up 25 ms |
| pathological | deep-parens: time at 4x | 26 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 18.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 26 → 26 ms, start-up 25 ms |
| pathological | method-chain: time at 4x | 33 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | method-chain: peak RSS at 4x | 21.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 28 → 33 ms, start-up 25 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 63 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 22.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 43 → 63 ms, start-up 25 ms |
| pathological | large-enum-match: time at 4x | 63 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 21.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 51 → 63 ms, start-up 25 ms |
| pathological | deep-generic: time at 4x | 34 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 19.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 29 → 34 ms, start-up 25 ms |
| pathological | question-chain: time at 4x | 35 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | question-chain: peak RSS at 4x | 22.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 32 → 35 ms, start-up 25 ms |
| pathological | big-file: time at 4x | 228 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | big-file: peak RSS at 4x | 53.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.22 | ≤ 1.30 | PASS | 2x→4x; 113 → 228 ms, start-up 25 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 24 of 24 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 32 ms | ≤ 200 ms | PASS | Timing change ≤10%: noise. |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.94x | ≤ 1.30x | FAIL | checked 848 ms, unchecked 438 ms |
| dev-speed | geomean dev / optimized | 1.04x | ≤ 4.00x | PASS | fib 1.03x, sum 0.99x, string-build 1.18x, map 1.01x, sort 1.02x |
| dev-speed | worst case dev / optimized | 1.18x | ≤ 10.00x | PASS | string-build |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 80 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 33.4 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.18 CPU-s, 33.2 MB | completes ≤ 30 s | PASS | wall 123 ms |
| resources | check, small, warm | 0.02 CPU-s, 18.7 MB | completes ≤ 30 s | PASS | wall 24 ms |
| resources | test, small, cold | 3.89 CPU-s, 119.7 MB | completes ≤ 30 s | PASS | wall 1564 ms |
| resources | test, small, warm | 3.01 CPU-s, 120.5 MB | completes ≤ 30 s | PASS | wall 1131 ms |
| resources | build, small, cold | 0.92 CPU-s, 96.4 MB | completes ≤ 30 s | PASS | wall 404 ms |
| resources | build, small, warm | 0.08 CPU-s, 42.1 MB | completes ≤ 30 s | PASS | wall 82 ms |
| resources | check, 10k, cold | 0.50 CPU-s, 59.4 MB | completes ≤ 30 s | PASS | wall 251 ms |
| resources | check, 10k, warm | 0.08 CPU-s, 33.4 MB | completes ≤ 30 s | PASS | wall 74 ms |
| resources | test, 10k, cold | 4.99 CPU-s, 140.5 MB | completes ≤ 30 s | PASS | wall 2159 ms |
| resources | test, 10k, warm | 3.67 CPU-s, 130.5 MB | completes ≤ 30 s | PASS | wall 1633 ms |
| resources | build, 10k, cold | 1.29 CPU-s, 119.6 MB | completes ≤ 30 s | PASS | wall 509 ms |
| resources | build, 10k, warm | 0.13 CPU-s, 49.3 MB | completes ≤ 30 s | PASS | wall 131 ms |
| resources | check, 50k, cold | 2.03 CPU-s, 179.1 MB | completes ≤ 150 s | PASS | wall 856 ms |
| resources | check, 50k, warm | 0.39 CPU-s, 96.2 MB | completes ≤ 150 s | PASS | wall 303 ms |
| resources | test, 50k, cold | 17.75 CPU-s, 289.3 MB | completes ≤ 150 s | PASS | wall 12471 ms |
| resources | test, 50k, warm | 14.33 CPU-s, 164.7 MB | completes ≤ 150 s | PASS | wall 10900 ms |
| resources | build, 50k, cold | 2.91 CPU-s, 282.0 MB | completes ≤ 150 s | PASS | wall 1173 ms |
| resources | build, 50k, warm | 0.47 CPU-s, 116.5 MB | completes ≤ 150 s | PASS | wall 454 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 42.8 MB | ≤ 22.0 MB | FAIL | RSS 169.0 MB at input 100, 194.5 MB at input 1000; build per input 121 ms at 50, 819 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 4.0 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.8 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 18 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd check empty.hd: CPU time | 10 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 16.1 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.06x | ≤ 1.50x | FAIL | p95 121 ms at N = 1; 4 cores; total CPU 910 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.13x | ≤ 6.00x | FAIL | p95 121 ms at N = 1; 4 cores; total CPU 3800 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.10 | &lt; 1.00 (sublinear) | FAIL | 180 ms at N = 1, 3800 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 17.3 MB | ≤ 10.0 MB | FAIL | artifacts 6925 B in 10 files; toolchain 17.3 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 10.4 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.79x | ≥ 2.40x | FAIL | by --jobs |
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
| proptest-perf | property cases per second | 28k/s | ≥ 100k/s | FAIL | 6000 extra cases took 214 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 111 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 Timing change ≤10%: noise. |
| unit-test-perf | 1,000 unit tests, warm | 1575 ms | ≤ 1000 ms | FAIL | the run reports 1000 of 1000 passed Timing change ≤10%: noise. |
| unit-test-perf | overhead per unit test | 0.9 ms | ≤ 1.0 ms | PASS | 100 tests: 796 ms |
| integration-test-perf | setup per integration test program | 447 ms | ≤ 20 ms | FAIL | 10 programs: 4793 ms for 21 tests, 228 ms per test Timing change ≤10%: noise. |
| integration-test-perf | total time growth in programs | 0.969 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 934 ms at 2, 4445 ms at 10 programs |
| integration-test-perf | cost per doc test | 326 ms | ≤ 20 ms | FAIL | 10 doc tests: 3599 ms; the run reports 11 of 11 passed Timing change ≤10%: noise. |
| runtime | geomean hd / Node | - | ≤ 1.50x | FAIL | 1 of 5 cases not measured |
| runtime | fib: hd / Node | 0.13x | ≤ 3.00x | PASS | hd 1.5 ms (p10 -12.5, p90 23), Node 11 ms, Python 99 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 0.87x | ≤ 3.00x | PASS | hd 9.4 ms (p10 7.1, p90 16), Node 11 ms, Python 307 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | - | ≤ 3.00x | FAIL | no work measured beyond start-up; hd -0.6 ms (p10 -8.3, p90 19), Node 11 ms, Python 14 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 3.99x | ≤ 3.00x | FAIL | hd 47 ms (p10 33, p90 52), Node 12 ms, Python 19 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 3.16x | ≤ 3.00x | FAIL | hd 80 ms (p10 67, p90 100), Node 25 ms, Python 19 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 2337 B | ≤ 2048 B | FAIL | benchmarks: fib 3656 B, sum 3610 B, string-build 5423 B, map 5203 B, sort 6169 B |
| size-startup-heap | tiny program, start to first output | 159 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.89x | ≤ 2.00x | PASS | hd +12.7 MB, Node +14.3 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.20x | ≤ 2.00x | PASS | hd +4.1 MB, Node +20.7 MB over an empty program |
| host-call-overhead | Console println, per call | 8.8 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm Timing change ≤10%: noise. |
| host-call-overhead | Fs read_text!, per call | 23 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 40 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 22 ns | ≤ 100 ns | PASS | 22 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 10.3M/s | ≥ 1.0M/s | PASS | 195 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 7.3M/s | ≥ 1.0M/s | PASS | 273 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.17x | ≥ 0.50x | FAIL | hd 48 MB/s (45488 B document), Node 284 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.23x | ≥ 0.50x | FAIL | hd 33 MB/s (45488 B document), Node 144 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 1.72x | ≥ 0.50x | PASS | hd 154 MB/s, Node 90 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 1.92x | ≥ 0.50x | PASS | hd 431 MB/s, Node 224 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.10x | ≥ 0.50x | FAIL | hd 6.5 MB/s, Node 64 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 5327 B | ≤ 10240 B | PASS | 10204 B at 1075 lines, 58151 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 664 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.2 MB | ≤ 10.0 MB | PASS | 12384000 requests; RSS 99.4 MB after warm-up, 99.6 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 6074 of 6074 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
