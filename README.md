# Daily goal metrics

Finished: 2026-11-06T19:28:12.346Z. Commit: a5e29b1ac39d41572e47ea5d6e9a80178ad323bd.
Machine: ubuntu-latest, 4 CPUs (INTEL(R) XEON(R) PLATINUM 8573C).
Compiler: target/release/hd.

95 passed, 36 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- fmt: hd fmt, 10k lines: pass 25 ms → pass 22 ms
- check-cost: checked run time / unchecked run time: fail 1.97x → fail 1.38x
- long-session: RSS growth, REPL inputs 100 to 1000: fail 77.1 MB → fail 52.7 MB
- startup: hd help (no version command): wall time: pass 3.2 ms → pass 2.7 ms
- startup: hd check empty.hd: wall time: pass 16 ms → pass 14 ms
- proptest-perf: time to shrink a known failure: pass 87 ms → pass 76 ms
- unit-test-perf: overhead per unit test: pass 0.7 ms → pass 0.8 ms
- runtime: geomean hd / Node: pass 0.74x → fail -
- runtime: fib: hd / Node: pass 0.30x → pass 0.61x
- runtime: sum: hd / Node: pass 1.31x → pass 1.45x
- runtime: string-build: hd / Node: pass 0.91x → fail -
- runtime: sort: hd / Node: pass 0.17x → pass 0.54x
- allocations: counted loop, bytes per iteration: pass 0.03 B/iter → pass 0.01 B/iter
- allocations: map loop, bytes per iteration: pass 0.00 B/iter → pass 0.01 B/iter
- allocations: iterator chain, bytes per iteration: pass 0.01 B/iter → pass 0.00 B/iter
- host-call-overhead: Console println, per call: fail 7.1 µs → fail 5.3 µs
- host-call-overhead: Fs read_text!, per call: fail 18 µs → fail 12 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 32 µs → fail 23 µs
- suspension-overhead: await of a ready value, per await: pass 0.00 ns → pass 0.36 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 11.9M/s → pass 17.4M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 9.3M/s → pass 12.6M/s
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.06x → fail 0.05x
- text-throughput: string build, hd / Node MB/s: pass 3.91x → pass 8.07x
- text-throughput: split, hd / Node MB/s: pass 2.95x → pass 3.29x

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 61 ms | ≤ 50 ms | FAIL | Timing change ≤10%: noise. |
| edit-latency | p95 check after an edit | 62 ms | ≤ 200 ms | PASS | Timing change ≤10%: noise. |
| cold-check | median cold check, 10k lines | 205 ms | ≤ 1000 ms | PASS | Timing change ≤10%: noise. |
| test-latency | median edit-then-test | 421 ms | ≤ 300 ms | FAIL | Timing change ≤10%: noise. |
| mistakes | programs with exactly one diagnostic | 95.7% | ≥ 95.0% | PASS | 66 of 69 |
| mistakes | one diagnostic with the expected code | 95.7% | ≥ 95.0% | PASS | 66 of 69; first misses: mut-on-binding-type, mut-on-parameter-type, some-on-result |
| mistakes | fix-it makes hd check pass | 33.3% | ≥ 80.0% | FAIL | 23 of 69 resolved; 23 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 93 tokens | ≤ 60 tokens | FAIL | largest: mut-on-parameter-type; median 43 tokens, 171 B; 64 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 46 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | overlapping-from: peak RSS at 4x | 24.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 32 → 46 ms, start-up 19 ms |
| pathological | deep-blocks: time at 4x | 20 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-blocks: peak RSS at 4x | 20.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 20 ms, start-up 19 ms |
| pathological | deep-parens: time at 4x | 18 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-parens: peak RSS at 4x | 19.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 18 → 18 ms, start-up 19 ms |
| pathological | method-chain: time at 4x | 24 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | method-chain: peak RSS at 4x | 21.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 24 ms, start-up 19 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 49 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 23.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 33 → 49 ms, start-up 19 ms |
| pathological | large-enum-match: time at 4x | 31 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 23.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 24 → 31 ms, start-up 19 ms |
| pathological | deep-generic: time at 4x | 22 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-generic: peak RSS at 4x | 19.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 22 ms, start-up 19 ms |
| pathological | question-chain: time at 4x | 26 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | question-chain: peak RSS at 4x | 23.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 23 → 26 ms, start-up 19 ms |
| pathological | big-file: time at 4x | 184 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | big-file: peak RSS at 4x | 53.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.23 | ≤ 1.30 | PASS | 2x→4x; 90 → 184 ms, start-up 19 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 23 of 23 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 22 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.38x | ≤ 1.30x | FAIL | checked 626 ms, unchecked 454 ms |
| dev-speed | geomean dev / optimized | - | ≤ 4.00x | FAIL | optimized work is within noise: string-build (34.7 ms) |
| dev-speed | worst case dev / optimized | - | ≤ 10.00x | FAIL | optimized work is within noise: string-build (34.7 ms) |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 70 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 34.7 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.15 CPU-s, 35.6 MB | completes ≤ 30 s | PASS | wall 102 ms |
| resources | check, small, warm | 0.01 CPU-s, 19.0 MB | completes ≤ 30 s | PASS | wall 19 ms |
| resources | test, small, cold | 3.23 CPU-s, 122.7 MB | completes ≤ 30 s | PASS | wall 1240 ms |
| resources | test, small, warm | 2.40 CPU-s, 120.7 MB | completes ≤ 30 s | PASS | wall 906 ms |
| resources | build, small, cold | 0.81 CPU-s, 101.3 MB | completes ≤ 30 s | PASS | wall 343 ms |
| resources | build, small, warm | 0.06 CPU-s, 43.1 MB | completes ≤ 30 s | PASS | wall 66 ms |
| resources | check, 10k, cold | 0.45 CPU-s, 62.4 MB | completes ≤ 30 s | PASS | wall 211 ms |
| resources | check, 10k, warm | 0.07 CPU-s, 34.7 MB | completes ≤ 30 s | PASS | wall 58 ms |
| resources | test, 10k, cold | 4.20 CPU-s, 149.7 MB | completes ≤ 30 s | PASS | wall 1912 ms |
| resources | test, 10k, warm | 2.85 CPU-s, 129.2 MB | completes ≤ 30 s | PASS | wall 1247 ms |
| resources | build, 10k, cold | 1.13 CPU-s, 122.9 MB | completes ≤ 30 s | PASS | wall 432 ms |
| resources | build, 10k, warm | 0.11 CPU-s, 50.6 MB | completes ≤ 30 s | PASS | wall 108 ms |
| resources | check, 50k, cold | 1.79 CPU-s, 189.9 MB | completes ≤ 150 s | PASS | wall 716 ms |
| resources | check, 50k, warm | 0.30 CPU-s, 100.5 MB | completes ≤ 150 s | PASS | wall 233 ms |
| resources | test, 50k, cold | 14.96 CPU-s, 334.8 MB | completes ≤ 150 s | PASS | wall 10523 ms |
| resources | test, 50k, warm | 11.26 CPU-s, 188.6 MB | completes ≤ 150 s | PASS | wall 8478 ms |
| resources | build, 50k, cold | 2.55 CPU-s, 294.3 MB | completes ≤ 150 s | PASS | wall 966 ms |
| resources | build, 50k, warm | 0.33 CPU-s, 121.9 MB | completes ≤ 150 s | PASS | wall 313 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 52.7 MB | ≤ 22.0 MB | FAIL | RSS 178.5 MB at input 100, 204.8 MB at input 1000; build per input 162 ms at 50, 770 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 2.7 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 4.2 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 14 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 16.1 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.08x | ≤ 1.50x | FAIL | p95 103 ms at N = 1; 4 cores; total CPU 760 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.42x | ≤ 6.00x | FAIL | p95 103 ms at N = 1; 4 cores; total CPU 3260 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.11 | &lt; 1.00 (sublinear) | FAIL | 150 ms at N = 1, 3260 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 18.3 MB | ≤ 10.0 MB | FAIL | artifacts 7824 B in 10 files; toolchain 18.3 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 11.1 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.81x | ≥ 2.40x | FAIL | by --jobs |
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
| proptest-perf | property cases per second | 36k/s | ≥ 100k/s | FAIL | 6000 extra cases took 165 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 76 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 1291 ms | ≤ 1000 ms | FAIL | the run reports 1000 of 1000 passed Timing change ≤10%: noise. |
| unit-test-perf | overhead per unit test | 0.8 ms | ≤ 1.0 ms | PASS | 100 tests: 616 ms |
| integration-test-perf | setup per integration test program | 361 ms | ≤ 20 ms | FAIL | 10 programs: 3897 ms for 21 tests, 186 ms per test Timing change ≤10%: noise. |
| integration-test-perf | total time growth in programs | 0.987 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 739 ms at 2, 3620 ms at 10 programs |
| integration-test-perf | cost per doc test | 261 ms | ≤ 20 ms | FAIL | 10 doc tests: 2880 ms; the run reports 11 of 11 passed Timing change ≤10%: noise. |
| runtime | geomean hd / Node | - | ≤ 1.50x | FAIL | 1 of 5 cases not measured |
| runtime | fib: hd / Node | 0.61x | ≤ 3.00x | PASS | hd 4.2 ms (p10 -5.9, p90 10.0), Node 6.9 ms, Python 65 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 1.45x | ≤ 3.00x | PASS | hd 8.2 ms (p10 -2.0, p90 18), Node 5.6 ms, Python 217 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | - | ≤ 3.00x | FAIL | no work measured beyond start-up; hd -4.7 ms (p10 -13.1, p90 11), Node 8.1 ms, Python 9.7 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 3.23x | ≤ 3.00x | FAIL | hd 30 ms (p10 24, p90 37), Node 9.2 ms, Python 13 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.54x | ≤ 3.00x | PASS | hd 10.0 ms (p10 3.8, p90 20), Node 19 ms, Python 14 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.01 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.01 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.01 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 1636 B | ≤ 2048 B | PASS | benchmarks: fib 5415 B, sum 5227 B, string-build 8608 B, map 6627 B, sort 13424 B |
| size-startup-heap | tiny program, start to first output | 122 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm Timing change ≤10%: noise. |
| size-startup-heap | peak heap, map: hd / Node | 0.92x | ≤ 2.00x | PASS | hd +13.1 MB, Node +14.2 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.21x | ≤ 2.00x | PASS | hd +4.3 MB, Node +20.7 MB over an empty program |
| host-call-overhead | Console println, per call | 5.3 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 12 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 23 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 0.36 ns | ≤ 100 ns | PASS | 0.36 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 17.4M/s | ≥ 1.0M/s | PASS | 115 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 12.6M/s | ≥ 1.0M/s | PASS | 158 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.05x | ≥ 0.50x | FAIL | hd 18 MB/s (45488 B document), Node 394 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.05x | ≥ 0.50x | FAIL | hd 11 MB/s (45488 B document), Node 201 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 8.07x | ≥ 0.50x | PASS | hd 1019 MB/s, Node 126 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 3.29x | ≥ 0.50x | PASS | hd 1056 MB/s, Node 321 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.13x | ≥ 0.50x | FAIL | hd 11 MB/s, Node 85 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 10607 B | ≤ 10240 B | FAIL | 17065 B at 1075 lines, 112543 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 908 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 25406000 requests; RSS 100.1 MB after warm-up, 100.1 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 6760 of 6760 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
- [2026-10-26T03](daily/2026-10-26T03.json)
- [2026-10-25](daily/2026-10-25.json)
