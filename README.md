# Daily goal metrics

Finished: 2026-11-03T04:36:36.364Z. Commit: 18ac25ef2932de5911dc6935dff6f18c91b6ba65.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V45 96-Core Processor).
Compiler: target/release/hd.

95 passed, 36 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: fail 77 ms → fail 51 ms
- edit-latency: p95 check after an edit: pass 80 ms → pass 52 ms
- cold-check: median cold check, 10k lines: pass 245 ms → pass 151 ms
- test-latency: median edit-then-test: fail 538 ms → fail 320 ms
- pathological: overlapping-from: time at 4x: pass 64 ms → pass 41 ms
- pathological: deep-blocks: time at 4x: pass 27 ms → pass 17 ms
- pathological: deep-parens: time at 4x: pass 26 ms → pass 17 ms
- pathological: method-chain: time at 4x: pass 33 ms → pass 22 ms
- pathological: wide-list-literal: time at 4x: pass 63 ms → pass 39 ms
- pathological: large-enum-match: time at 4x: pass 63 ms → pass 25 ms
- pathological: deep-generic: time at 4x: pass 34 ms → pass 18 ms
- pathological: question-chain: time at 4x: pass 35 ms → pass 22 ms
- pathological: big-file: time at 4x: pass 228 ms → pass 140 ms
- pathological: big-file: growth exponent: pass 1.22 → fail 1.33
- fmt: hd fmt, 10k lines: pass 32 ms → pass 18 ms
- resources: warm check, 10k lines: CPU time: pass 80 ms → pass 40 ms
- long-session: RSS growth, REPL inputs 100 to 1000: fail 42.8 MB → fail 74.1 MB
- startup: hd help (no version command): wall time: pass 4.0 ms → pass 2.9 ms
- startup: hd check empty.hd: wall time: pass 18 ms → pass 12 ms
- startup: hd check empty.hd: CPU time: pass 10 ms → pass 0.0 ms
- proptest-perf: property cases per second: fail 28k/s → fail 71k/s
- proptest-perf: time to shrink a known failure: pass 111 ms → pass 54 ms
- unit-test-perf: 1,000 unit tests, warm: fail 1575 ms → pass 983 ms
- unit-test-perf: overhead per unit test: pass 0.9 ms → pass 0.6 ms
- integration-test-perf: setup per integration test program: fail 447 ms → fail 280 ms
- integration-test-perf: total time growth in programs: pass 0.969 → fail 1.000
- integration-test-perf: cost per doc test: fail 326 ms → fail 194 ms
- runtime: fib: hd / Node: pass 0.13x → pass 0.67x
- runtime: sum: hd / Node: pass 0.87x → fail -
- runtime: string-build: hd / Node: fail - → pass 0.02x
- runtime: map: hd / Node: fail 3.99x → fail 3.52x
- allocations: list loop, bytes per iteration: pass 0.00 B/iter → pass 0.03 B/iter
- allocations: map loop, bytes per iteration: pass 0.00 B/iter → pass 0.01 B/iter
- size-startup-heap: tiny program, start to first output: fail 159 ms → fail 97 ms
- host-call-overhead: Console println, per call: fail 8.8 µs → fail 5.6 µs
- host-call-overhead: Fs read_text!, per call: fail 23 µs → fail 13 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 40 µs → fail 24 µs
- suspension-overhead: await of a ready value, per await: pass 22 ns → pass 14 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 10.3M/s → pass 16.8M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 7.3M/s → pass 14.7M/s
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.23x → fail 0.20x
- text-throughput: string build, hd / Node MB/s: pass 1.72x → pass 1.95x
- text-throughput: split, hd / Node MB/s: pass 1.92x → pass 4.86x
- long-run-memory: RSS growth after warm-up, 1 min: pass 0.2 MB → pass 0.0 MB

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 51 ms | ≤ 50 ms | FAIL |  |
| edit-latency | p95 check after an edit | 52 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 151 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 320 ms | ≤ 300 ms | FAIL |  |
| mistakes | programs with exactly one diagnostic | 95.7% | ≥ 95.0% | PASS | 66 of 69 |
| mistakes | one diagnostic with the expected code | 95.7% | ≥ 95.0% | PASS | 66 of 69; first misses: mut-on-binding-type, mut-on-parameter-type, some-on-result |
| mistakes | fix-it makes hd check pass | 34.8% | ≥ 80.0% | FAIL | 24 of 69 resolved; 24 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 93 tokens | ≤ 60 tokens | FAIL | largest: mut-on-parameter-type; median 44 tokens, 175 B; 64 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 41 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 23.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 30 → 41 ms, start-up 18 ms |
| pathological | deep-blocks: time at 4x | 17 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 19.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 17 → 17 ms, start-up 18 ms |
| pathological | deep-parens: time at 4x | 17 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 18.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 16 → 17 ms, start-up 18 ms |
| pathological | method-chain: time at 4x | 22 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 20.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 22 ms, start-up 18 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 39 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 23.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 30 → 39 ms, start-up 18 ms |
| pathological | large-enum-match: time at 4x | 25 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 22.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 25 ms, start-up 18 ms |
| pathological | deep-generic: time at 4x | 18 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 19.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 16 → 18 ms, start-up 18 ms |
| pathological | question-chain: time at 4x | 22 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 22.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 22 ms, start-up 18 ms |
| pathological | big-file: time at 4x | 140 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 53.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.33 | ≤ 1.30 | FAIL | 2x→4x; 66 → 140 ms, start-up 18 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 24 of 24 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 18 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.91x | ≤ 1.30x | FAIL | checked 459 ms, unchecked 240 ms |
| dev-speed | geomean dev / optimized | 1.03x | ≤ 4.00x | PASS | fib 0.98x, sum 1.28x, string-build 1.00x, map 0.95x, sort 0.99x |
| dev-speed | worst case dev / optimized | 1.28x | ≤ 10.00x | PASS | sum |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 40 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 32.9 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.11 CPU-s, 33.8 MB | completes ≤ 30 s | PASS | wall 72 ms |
| resources | check, small, warm | 0.00 CPU-s, 18.5 MB | completes ≤ 30 s | PASS | wall 15 ms |
| resources | test, small, cold | 2.37 CPU-s, 120.9 MB | completes ≤ 30 s | PASS | wall 921 ms |
| resources | test, small, warm | 1.87 CPU-s, 120.2 MB | completes ≤ 30 s | PASS | wall 686 ms |
| resources | build, small, cold | 0.53 CPU-s, 98.6 MB | completes ≤ 30 s | PASS | wall 235 ms |
| resources | build, small, warm | 0.05 CPU-s, 41.6 MB | completes ≤ 30 s | PASS | wall 50 ms |
| resources | check, 10k, cold | 0.29 CPU-s, 59.8 MB | completes ≤ 30 s | PASS | wall 148 ms |
| resources | check, 10k, warm | 0.04 CPU-s, 32.9 MB | completes ≤ 30 s | PASS | wall 44 ms |
| resources | test, 10k, cold | 2.97 CPU-s, 144.8 MB | completes ≤ 30 s | PASS | wall 1279 ms |
| resources | test, 10k, warm | 2.22 CPU-s, 130.2 MB | completes ≤ 30 s | PASS | wall 955 ms |
| resources | build, 10k, cold | 0.75 CPU-s, 117.9 MB | completes ≤ 30 s | PASS | wall 303 ms |
| resources | build, 10k, warm | 0.07 CPU-s, 49.5 MB | completes ≤ 30 s | PASS | wall 79 ms |
| resources | check, 50k, cold | 1.17 CPU-s, 179.3 MB | completes ≤ 150 s | PASS | wall 509 ms |
| resources | check, 50k, warm | 0.22 CPU-s, 96.2 MB | completes ≤ 150 s | PASS | wall 179 ms |
| resources | test, 50k, cold | 9.87 CPU-s, 308.7 MB | completes ≤ 150 s | PASS | wall 6714 ms |
| resources | test, 50k, warm | 8.04 CPU-s, 172.7 MB | completes ≤ 150 s | PASS | wall 5884 ms |
| resources | build, 50k, cold | 1.72 CPU-s, 281.6 MB | completes ≤ 150 s | PASS | wall 720 ms |
| resources | build, 50k, warm | 0.25 CPU-s, 115.8 MB | completes ≤ 150 s | PASS | wall 245 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 74.1 MB | ≤ 22.0 MB | FAIL | RSS 135.4 MB at input 100, 198.9 MB at input 1000; build per input 129 ms at 50, 446 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 2.9 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.8 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 12 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 15.8 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.01x | ≤ 1.50x | FAIL | p95 72 ms at N = 1; 4 cores; total CPU 520 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.66x | ≤ 6.00x | FAIL | p95 72 ms at N = 1; 4 cores; total CPU 2260 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.12 | &lt; 1.00 (sublinear) | FAIL | 100 ms at N = 1, 2260 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 17.3 MB | ≤ 10.0 MB | FAIL | artifacts 7033 B in 10 files; toolchain 17.3 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 10.4 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.74x | ≥ 2.40x | FAIL | by --jobs |
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
| proptest-perf | property cases per second | 71k/s | ≥ 100k/s | FAIL | 6000 extra cases took 85 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 54 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 983 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.6 ms | ≤ 1.0 ms | PASS | 100 tests: 478 ms |
| integration-test-perf | setup per integration test program | 280 ms | ≤ 20 ms | FAIL | 10 programs: 2993 ms for 21 tests, 143 ms per test |
| integration-test-perf | total time growth in programs | 1.000 | exponent &lt; 1 (sublinear) | FAIL | beyond the 0-program run: 557 ms at 2, 2786 ms at 10 programs |
| integration-test-perf | cost per doc test | 194 ms | ≤ 20 ms | FAIL | 10 doc tests: 2179 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | - | ≤ 1.50x | FAIL | 1 of 5 cases not measured |
| runtime | fib: hd / Node | 0.67x | ≤ 3.00x | PASS | hd 3.4 ms (p10 -6.9, p90 8.7), Node 5.1 ms, Python 47 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | - | ≤ 3.00x | FAIL | no work measured beyond start-up; hd -1.5 ms (p10 -11.0, p90 3.7), Node 6.1 ms, Python 148 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.02x | ≤ 3.00x | PASS | hd 0.1 ms (p10 -4.8, p90 5.8), Node 6.4 ms, Python 7.9 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 3.52x | ≤ 3.00x | FAIL | hd 24 ms (p10 15, p90 34), Node 6.8 ms, Python 11 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 3.38x | ≤ 3.00x | FAIL | hd 48 ms (p10 41, p90 56), Node 14 ms, Python 9.5 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.03 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.01 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 2334 B | ≤ 2048 B | FAIL | benchmarks: fib 3876 B, sum 3830 B, string-build 5796 B, map 5423 B, sort 6390 B |
| size-startup-heap | tiny program, start to first output | 97 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.83x | ≤ 2.00x | PASS | hd +11.8 MB, Node +14.2 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.21x | ≤ 2.00x | PASS | hd +4.4 MB, Node +20.7 MB over an empty program |
| host-call-overhead | Console println, per call | 5.6 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 13 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 24 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 14 ns | ≤ 100 ns | PASS | 14 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 16.8M/s | ≥ 1.0M/s | PASS | 119 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 14.7M/s | ≥ 1.0M/s | PASS | 136 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.16x | ≥ 0.50x | FAIL | hd 84 MB/s (45488 B document), Node 529 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.20x | ≥ 0.50x | FAIL | hd 52 MB/s (45488 B document), Node 257 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 1.95x | ≥ 0.50x | PASS | hd 300 MB/s, Node 154 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 4.86x | ≥ 0.50x | PASS | hd 1803 MB/s, Node 371 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.11x | ≥ 0.50x | FAIL | hd 12 MB/s, Node 114 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 5325 B | ≤ 10240 B | PASS | 10425 B at 1075 lines, 58352 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 664 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 22608000 requests; RSS 101.1 MB after warm-up, 101.2 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 6082 of 6082 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
