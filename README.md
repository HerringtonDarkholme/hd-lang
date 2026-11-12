# Daily goal metrics

Finished: 2026-11-12T04:58:35.757Z. Commit: 0091fc1d93ab9449177fb944d3e661c77800b725.
Machine: ubuntu-latest, 4 CPUs (INTEL(R) XEON(R) PLATINUM 8573C).
Compiler: target/release/hd.

102 passed, 29 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- pathological: overlapping-from: time at 4x: pass 75 ms → pass 64 ms
- pathological: deep-blocks: time at 4x: pass 30 ms → pass 24 ms
- pathological: deep-parens: time at 4x: pass 28 ms → pass 25 ms
- pathological: wide-list-literal: time at 4x: pass 52 ms → pass 58 ms
- pathological: big-file: time at 4x: pass 205 ms → pass 184 ms
- check-cost: checked run time / unchecked run time: fail 1.83x → fail 1.62x
- dev-speed: geomean dev / optimized: pass 1.07x → pass 1.45x
- dev-speed: worst case dev / optimized: pass 1.25x → pass 4.27x
- long-session: RSS growth, REPL inputs 100 to 1000: fail 81.8 MB → fail 104.5 MB
- startup: hd help (no version command): wall time: pass 3.7 ms → pass 3.2 ms
- startup: hd check empty.hd: wall time: pass 16 ms → pass 18 ms
- proptest-perf: property cases per second: fail 26k/s → fail 36k/s
- integration-test-perf: total time growth in programs: fail 1.005 → pass 0.989
- runtime: fib: hd / Node: pass 0.81x → pass 0.65x
- runtime: sum: hd / Node: pass 1.73x → pass 2.05x
- runtime: sort: hd / Node: pass 1.34x → pass 1.06x
- host-call-overhead: Console println, per call: fail 8.9 µs → fail 7.2 µs
- host-call-overhead: Fs read_text!, per call: fail 27 µs → fail 15 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 47 µs → fail 28 µs
- suspension-overhead: await of a ready value, per await: pass 10 ns → pass 13 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 9.1M/s → pass 12.8M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 7.3M/s → pass 11.1M/s
- text-throughput: string build, hd / Node MB/s: pass 2.94x → pass 4.40x
- text-throughput: split, hd / Node MB/s: pass 1.76x → pass 2.36x
- text-throughput: regex find_all, hd / Node MB/s: fail 0.09x → fail 0.13x
- long-run-memory: RSS growth after warm-up, 1 min: pass -0.2 MB → pass 0.5 MB

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 47 ms | ≤ 50 ms | PASS | Timing change ≤10%: noise. |
| edit-latency | p95 check after an edit | 50 ms | ≤ 200 ms | PASS | Timing change ≤10%: noise. |
| cold-check | median cold check, 10k lines | 221 ms | ≤ 1000 ms | PASS | Timing change ≤10%: noise. |
| test-latency | median edit-then-test | 243 ms | ≤ 300 ms | PASS | Timing change ≤10%: noise. |
| mistakes | programs with exactly one diagnostic | 98.6% | ≥ 95.0% | PASS | 68 of 69 |
| mistakes | one diagnostic with the expected code | 98.6% | ≥ 95.0% | PASS | 68 of 69; first misses: some-on-result |
| mistakes | fix-it makes hd check pass | 33.3% | ≥ 80.0% | FAIL | 23 of 69 resolved; 23 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 92 tokens | ≤ 60 tokens | FAIL | largest: some-on-result; median 43 tokens, 171 B; 66 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 64 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 25.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 50 → 64 ms, start-up 29 ms |
| pathological | deep-blocks: time at 4x | 24 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 20.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 23 → 24 ms, start-up 29 ms |
| pathological | deep-parens: time at 4x | 25 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 20.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 31 → 25 ms, start-up 29 ms |
| pathological | method-chain: time at 4x | 30 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | method-chain: peak RSS at 4x | 22.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 27 → 30 ms, start-up 29 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 58 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 24.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 35 → 58 ms, start-up 29 ms |
| pathological | large-enum-match: time at 4x | 38 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 23.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 31 → 38 ms, start-up 29 ms |
| pathological | deep-generic: time at 4x | 34 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-generic: peak RSS at 4x | 21.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 25 → 34 ms, start-up 29 ms |
| pathological | question-chain: time at 4x | 33 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | question-chain: peak RSS at 4x | 23.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 38 → 33 ms, start-up 29 ms |
| pathological | big-file: time at 4x | 184 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 54.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.17 | ≤ 1.30 | PASS | 2x→4x; 98 → 184 ms, start-up 29 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 23 of 23 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 30 ms | ≤ 200 ms | PASS | Timing change ≤10%: noise. |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.62x | ≤ 1.30x | FAIL | checked 722 ms, unchecked 446 ms |
| dev-speed | geomean dev / optimized | 1.45x | ≤ 4.00x | PASS | fib 1.03x, sum 4.27x, string-build 1.13x, map 1.08x, sort 1.18x |
| dev-speed | worst case dev / optimized | 4.27x | ≤ 10.00x | PASS | sum |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 40 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 25.3 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.18 CPU-s, 34.8 MB | completes ≤ 30 s | PASS | wall 117 ms |
| resources | check, small, warm | 0.00 CPU-s, 18.3 MB | completes ≤ 30 s | PASS | wall 19 ms |
| resources | test, small, cold | 1.93 CPU-s, 90.6 MB | completes ≤ 30 s | PASS | wall 866 ms |
| resources | test, small, warm | 0.97 CPU-s, 70.4 MB | completes ≤ 30 s | PASS | wall 464 ms |
| resources | build, small, cold | 0.90 CPU-s, 81.1 MB | completes ≤ 30 s | PASS | wall 350 ms |
| resources | build, small, warm | 0.03 CPU-s, 30.9 MB | completes ≤ 30 s | PASS | wall 32 ms |
| resources | check, 10k, cold | 0.51 CPU-s, 57.5 MB | completes ≤ 30 s | PASS | wall 224 ms |
| resources | check, 10k, warm | 0.04 CPU-s, 25.3 MB | completes ≤ 30 s | PASS | wall 42 ms |
| resources | test, 10k, cold | 2.79 CPU-s, 126.2 MB | completes ≤ 30 s | PASS | wall 1193 ms |
| resources | test, 10k, warm | 1.24 CPU-s, 77.9 MB | completes ≤ 30 s | PASS | wall 503 ms |
| resources | build, 10k, cold | 1.27 CPU-s, 102.4 MB | completes ≤ 30 s | PASS | wall 433 ms |
| resources | build, 10k, warm | 0.05 CPU-s, 34.3 MB | completes ≤ 30 s | PASS | wall 52 ms |
| resources | check, 50k, cold | 2.03 CPU-s, 160.6 MB | completes ≤ 150 s | PASS | wall 731 ms |
| resources | check, 50k, warm | 0.15 CPU-s, 55.7 MB | completes ≤ 150 s | PASS | wall 134 ms |
| resources | test, 50k, cold | 7.64 CPU-s, 278.1 MB | completes ≤ 150 s | PASS | wall 3691 ms |
| resources | test, 50k, warm | 3.34 CPU-s, 122.6 MB | completes ≤ 150 s | PASS | wall 1227 ms |
| resources | build, 50k, cold | 2.84 CPU-s, 205.9 MB | completes ≤ 150 s | PASS | wall 921 ms |
| resources | build, 50k, warm | 0.16 CPU-s, 64.5 MB | completes ≤ 150 s | PASS | wall 144 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 104.5 MB | ≤ 22.0 MB | FAIL | RSS 90.4 MB at input 100, 195.0 MB at input 1000; build per input 91 ms at 50, 902 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.2 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.9 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 18 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 16.7 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.18x | ≤ 1.50x | FAIL | p95 125 ms at N = 1; 4 cores; total CPU 980 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.58x | ≤ 6.00x | FAIL | p95 125 ms at N = 1; 4 cores; total CPU 4030 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.10 | &lt; 1.00 (sublinear) | FAIL | 190 ms at N = 1, 4030 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 19.9 MB | ≤ 10.0 MB | FAIL | artifacts 7635 B in 10 files; toolchain 19.9 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 10.8 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.95x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 9 files | the files of one check's cache | FAIL | one check alone: 7 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 4.4 MB | ≤ 5.0 MB | PASS | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 79 | ≤ 1 | FAIL | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 36k/s | ≥ 100k/s | FAIL | 6000 extra cases took 166 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 117 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 Timing change ≤10%: noise. |
| unit-test-perf | 1,000 unit tests, warm | 1088 ms | ≤ 1000 ms | FAIL | the run reports 1000 of 1000 passed Timing change ≤10%: noise. |
| unit-test-perf | overhead per unit test | 0.9 ms | ≤ 1.0 ms | PASS | 100 tests: 258 ms |
| integration-test-perf | setup per integration test program | 157 ms | ≤ 20 ms | FAIL | 10 programs: 1722 ms for 21 tests, 82 ms per test Timing change ≤10%: noise. |
| integration-test-perf | total time growth in programs | 0.989 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 321 ms at 2, 1578 ms at 10 programs |
| integration-test-perf | cost per doc test | 140 ms | ≤ 20 ms | FAIL | 10 doc tests: 1540 ms; the run reports 11 of 11 passed Timing change ≤10%: noise. |
| runtime | geomean hd / Node | 1.22x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.65x | ≤ 3.00x | PASS | hd 5.3 ms (p10 4.1, p90 10), Node 8.0 ms, Python 76 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 2.05x | ≤ 3.00x | PASS | hd 13 ms (p10 10, p90 25), Node 6.5 ms, Python 253 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.90x | ≤ 3.00x | PASS | hd 8.7 ms (p10 8.0, p90 9.6), Node 9.7 ms, Python 12 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 2.14x | ≤ 3.00x | PASS | hd 23 ms (p10 16, p90 27), Node 11 ms, Python 16 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 1.06x | ≤ 3.00x | PASS | hd 24 ms (p10 15, p90 26), Node 22 ms, Python 16 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 1597 B | ≤ 2048 B | PASS | benchmarks: fib 5081 B, sum 4875 B, string-build 7952 B, map 6323 B, sort 12822 B |
| size-startup-heap | tiny program, start to first output | 54 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm Timing change ≤10%: noise. |
| size-startup-heap | peak heap, map: hd / Node | 0.95x | ≤ 2.00x | PASS | hd +13.6 MB, Node +14.3 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.39x | ≤ 2.00x | PASS | hd +8.1 MB, Node +20.9 MB over an empty program |
| host-call-overhead | Console println, per call | 7.2 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 15 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 28 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 13 ns | ≤ 100 ns | PASS | 13 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 12.8M/s | ≥ 1.0M/s | PASS | 156 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 11.1M/s | ≥ 1.0M/s | PASS | 181 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.20x | ≥ 0.50x | FAIL | hd 67 MB/s (45488 B document), Node 334 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.22x | ≥ 0.50x | FAIL | hd 38 MB/s (45488 B document), Node 170 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 4.40x | ≥ 0.50x | PASS | hd 463 MB/s, Node 105 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 2.36x | ≥ 0.50x | PASS | hd 599 MB/s, Node 254 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.13x | ≥ 0.50x | FAIL | hd 9.8 MB/s, Node 74 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 6906 B | ≤ 10240 B | PASS | 12823 B at 1075 lines, 74987 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 925 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.5 MB | ≤ 10.0 MB | PASS | 20644000 requests; RSS 71.7 MB after warm-up, 72.2 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 7405 of 7405 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
- [2026-10-29T18](daily/2026-10-29T18.json)
