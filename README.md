# Daily goal metrics

Finished: 2026-11-08T18:10:22.192Z. Commit: 456bf2c6a7e1aeac73284dc8accbf95263557a2c.
Machine: ubuntu-latest, 4 CPUs (INTEL(R) XEON(R) PLATINUM 8573C).
Compiler: target/release/hd.

96 passed, 35 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 35 ms → pass 45 ms
- edit-latency: p95 check after an edit: pass 37 ms → pass 48 ms
- cold-check: median cold check, 10k lines: pass 142 ms → pass 207 ms
- test-latency: median edit-then-test: pass 248 ms → fail 350 ms
- pathological: overlapping-from: time at 4x: pass 37 ms → pass 57 ms
- pathological: deep-blocks: time at 4x: pass 19 ms → pass 27 ms
- pathological: deep-parens: time at 4x: pass 18 ms → pass 23 ms
- pathological: method-chain: time at 4x: pass 20 ms → pass 27 ms
- pathological: wide-list-literal: time at 4x: pass 37 ms → pass 52 ms
- pathological: large-enum-match: time at 4x: pass 25 ms → pass 36 ms
- pathological: deep-generic: time at 4x: pass 19 ms → pass 31 ms
- pathological: question-chain: time at 4x: pass 23 ms → pass 35 ms
- pathological: big-file: time at 4x: pass 130 ms → pass 191 ms
- pathological: big-file: growth exponent: pass 1.23 → fail 1.35
- fmt: hd fmt, 10k lines: pass 17 ms → pass 30 ms
- long-session: RSS growth, REPL inputs 100 to 1000: fail 73.0 MB → fail 85.0 MB
- startup: hd help (no version command): wall time: pass 3.2 ms → pass 2.8 ms
- cache-contention: compilation cache, 8 concurrent checks: written once: pass 3 files → fail 9 files
- cache-growth: cache size after 100 edits, cap 5 MB: pass 4.5 MB → pass 3.8 MB
- proptest-perf: property cases per second: fail 70k/s → fail 37k/s
- proptest-perf: time to shrink a known failure: pass 69 ms → pass 84 ms
- unit-test-perf: 1,000 unit tests, warm: pass 989 ms → fail 1396 ms
- unit-test-perf: overhead per unit test: pass 0.5 ms → pass 0.8 ms
- integration-test-perf: setup per integration test program: fail 271 ms → fail 375 ms
- integration-test-perf: total time growth in programs: fail 1.001 → pass 0.997
- integration-test-perf: cost per doc test: fail 189 ms → fail 257 ms
- runtime: geomean hd / Node: fail - → pass 0.68x
- runtime: fib: hd / Node: fail - → pass 0.40x
- runtime: sum: hd / Node: pass 1.03x → pass 2.72x
- runtime: string-build: hd / Node: pass 0.48x → pass 0.08x
- runtime: map: hd / Node: fail 3.97x → fail 3.24x
- runtime: sort: hd / Node: pass 0.36x → pass 0.52x
- allocations: iterator chain, bytes per iteration: pass 0.04 B/iter → pass 0.03 B/iter
- size-startup-heap: tiny program, start to first output: fail 102 ms → fail 142 ms
- size-startup-heap: peak heap, map: hd / Node: pass 0.83x → pass 0.92x
- size-startup-heap: peak heap, string-build: hd / Node: pass 0.19x → pass 0.22x
- host-call-overhead: Console println, per call: fail 5.7 µs → fail 6.6 µs
- suspension-overhead: await of a ready value, per await: pass 0.00 ns → pass 40 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 18.8M/s → pass 16.0M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 16.7M/s → pass 11.0M/s
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.28x → fail 0.23x
- text-throughput: string build, hd / Node MB/s: pass 3.70x → pass 3.24x
- text-throughput: split, hd / Node MB/s: pass 3.15x → pass 2.13x
- text-throughput: regex find_all, hd / Node MB/s: fail 0.11x → fail 0.13x

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 45 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 48 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 207 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 350 ms | ≤ 300 ms | FAIL |  |
| mistakes | programs with exactly one diagnostic | 98.6% | ≥ 95.0% | PASS | 68 of 69 |
| mistakes | one diagnostic with the expected code | 98.6% | ≥ 95.0% | PASS | 68 of 69; first misses: some-on-result |
| mistakes | fix-it makes hd check pass | 33.3% | ≥ 80.0% | FAIL | 23 of 69 resolved; 23 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 92 tokens | ≤ 60 tokens | FAIL | largest: some-on-result; median 43 tokens, 171 B; 66 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 57 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 25.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 41 → 57 ms, start-up 24 ms |
| pathological | deep-blocks: time at 4x | 27 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 20.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 23 → 27 ms, start-up 24 ms |
| pathological | deep-parens: time at 4x | 23 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 20.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 28 → 23 ms, start-up 24 ms |
| pathological | method-chain: time at 4x | 27 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 21.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 25 → 27 ms, start-up 24 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 52 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 24.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 37 → 52 ms, start-up 24 ms |
| pathological | large-enum-match: time at 4x | 36 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 23.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 38 → 36 ms, start-up 24 ms |
| pathological | deep-generic: time at 4x | 31 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 20.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 27 → 31 ms, start-up 24 ms |
| pathological | question-chain: time at 4x | 35 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 23.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 26 → 35 ms, start-up 24 ms |
| pathological | big-file: time at 4x | 191 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 52.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.35 | ≤ 1.30 | FAIL | 2x→4x; 90 → 191 ms, start-up 24 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 23 of 23 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 30 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.75x | ≤ 1.30x | FAIL | checked 695 ms, unchecked 398 ms |
| dev-speed | geomean dev / optimized | - | ≤ 4.00x | FAIL | optimized work is within noise: string-build (47.5 ms) |
| dev-speed | worst case dev / optimized | - | ≤ 10.00x | FAIL | optimized work is within noise: string-build (47.5 ms) |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 30 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 25.7 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.17 CPU-s, 35.7 MB | completes ≤ 30 s | PASS | wall 112 ms |
| resources | check, small, warm | 0.01 CPU-s, 18.3 MB | completes ≤ 30 s | PASS | wall 18 ms |
| resources | test, small, cold | 3.52 CPU-s, 120.4 MB | completes ≤ 30 s | PASS | wall 1296 ms |
| resources | test, small, warm | 2.59 CPU-s, 120.6 MB | completes ≤ 30 s | PASS | wall 898 ms |
| resources | build, small, cold | 0.82 CPU-s, 82.0 MB | completes ≤ 30 s | PASS | wall 317 ms |
| resources | build, small, warm | 0.02 CPU-s, 31.4 MB | completes ≤ 30 s | PASS | wall 31 ms |
| resources | check, 10k, cold | 0.49 CPU-s, 58.3 MB | completes ≤ 30 s | PASS | wall 215 ms |
| resources | check, 10k, warm | 0.03 CPU-s, 25.7 MB | completes ≤ 30 s | PASS | wall 39 ms |
| resources | test, 10k, cold | 4.23 CPU-s, 130.4 MB | completes ≤ 30 s | PASS | wall 1630 ms |
| resources | test, 10k, warm | 2.83 CPU-s, 131.0 MB | completes ≤ 30 s | PASS | wall 967 ms |
| resources | build, 10k, cold | 1.17 CPU-s, 107.0 MB | completes ≤ 30 s | PASS | wall 427 ms |
| resources | build, 10k, warm | 0.06 CPU-s, 35.9 MB | completes ≤ 30 s | PASS | wall 52 ms |
| resources | check, 50k, cold | 1.89 CPU-s, 165.1 MB | completes ≤ 150 s | PASS | wall 691 ms |
| resources | check, 50k, warm | 0.14 CPU-s, 57.8 MB | completes ≤ 150 s | PASS | wall 133 ms |
| resources | test, 50k, cold | 8.88 CPU-s, 283.5 MB | completes ≤ 150 s | PASS | wall 3918 ms |
| resources | test, 50k, warm | 4.76 CPU-s, 175.5 MB | completes ≤ 150 s | PASS | wall 1686 ms |
| resources | build, 50k, cold | 2.66 CPU-s, 210.9 MB | completes ≤ 150 s | PASS | wall 878 ms |
| resources | build, 50k, warm | 0.16 CPU-s, 66.6 MB | completes ≤ 150 s | PASS | wall 146 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 85.0 MB | ≤ 22.0 MB | FAIL | RSS 107.5 MB at input 100, 192.6 MB at input 1000; build per input 96 ms at 50, 874 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 2.8 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 4.1 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 15 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 16.5 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.13x | ≤ 1.50x | FAIL | p95 115 ms at N = 1; 4 cores; total CPU 890 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.62x | ≤ 6.00x | FAIL | p95 115 ms at N = 1; 4 cores; total CPU 3720 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.09 | &lt; 1.00 (sublinear) | FAIL | 180 ms at N = 1, 3720 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 18.6 MB | ≤ 10.0 MB | FAIL | artifacts 7847 B in 10 files; toolchain 18.6 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 11.1 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.92x | ≥ 2.40x | FAIL | by --jobs |
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
| proptest-perf | property cases per second | 37k/s | ≥ 100k/s | FAIL | 6000 extra cases took 160 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 84 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 1396 ms | ≤ 1000 ms | FAIL | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.8 ms | ≤ 1.0 ms | PASS | 100 tests: 672 ms |
| integration-test-perf | setup per integration test program | 375 ms | ≤ 20 ms | FAIL | 10 programs: 4009 ms for 21 tests, 191 ms per test |
| integration-test-perf | total time growth in programs | 0.997 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 752 ms at 2, 3745 ms at 10 programs |
| integration-test-perf | cost per doc test | 257 ms | ≤ 20 ms | FAIL | 10 doc tests: 2840 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 0.68x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.40x | ≤ 3.00x | PASS | hd 3.2 ms (p10 -9.6, p90 10), Node 7.9 ms, Python 77 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 2.72x | ≤ 3.00x | PASS | hd 18 ms (p10 9.4, p90 25), Node 6.5 ms, Python 257 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.08x | ≤ 3.00x | PASS | hd 0.8 ms (p10 -9.6, p90 3.8), Node 9.6 ms, Python 12 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 3.24x | ≤ 3.00x | FAIL | hd 34 ms (p10 23, p90 37), Node 10 ms, Python 15 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.52x | ≤ 3.00x | PASS | hd 11 ms (p10 -0.1, p90 22), Node 21 ms, Python 16 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.03 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 1628 B | ≤ 2048 B | PASS | benchmarks: fib 5414 B, sum 5226 B, string-build 8628 B, map 6631 B, sort 13538 B |
| size-startup-heap | tiny program, start to first output | 142 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.92x | ≤ 2.00x | PASS | hd +13.0 MB, Node +14.1 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.22x | ≤ 2.00x | PASS | hd +4.5 MB, Node +20.9 MB over an empty program |
| host-call-overhead | Console println, per call | 6.6 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 14 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 25 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 40 ns | ≤ 100 ns | PASS | 40 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 16.0M/s | ≥ 1.0M/s | PASS | 125 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 11.0M/s | ≥ 1.0M/s | PASS | 182 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.16x | ≥ 0.50x | FAIL | hd 55 MB/s (45488 B document), Node 348 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.23x | ≥ 0.50x | FAIL | hd 42 MB/s (45488 B document), Node 180 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 3.24x | ≥ 0.50x | PASS | hd 376 MB/s, Node 116 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 2.13x | ≥ 0.50x | PASS | hd 609 MB/s, Node 285 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.13x | ≥ 0.50x | FAIL | hd 9.5 MB/s, Node 73 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 10676 B | ≤ 10240 B | FAIL | 17132 B at 1075 lines, 113231 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 908 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 19684000 requests; RSS 99.4 MB after warm-up, 99.4 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 6804 of 6804 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
- [2026-10-26T03](daily/2026-10-26T03.json)
