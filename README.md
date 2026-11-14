# Daily goal metrics

Finished: 2026-11-14T18:04:11.757Z. Commit: 13f94845fffef2f41aa4e377b954c15a8cfc9791.
Machine: ubuntu-latest, 4 CPUs (Intel(R) Xeon(R) 6975P-C).
Compiler: target/release/hd.

104 passed, 27 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 47 ms → pass 34 ms
- edit-latency: p95 check after an edit: pass 49 ms → pass 36 ms
- cold-check: median cold check, 10k lines: pass 227 ms → pass 168 ms
- test-latency: median edit-then-test: pass 252 ms → pass 143 ms
- mistakes: largest diagnostic text (bytes / 4): fail 92 tokens → fail 65 tokens
- pathological: overlapping-from: time at 4x: pass 54 ms → pass 37 ms
- pathological: deep-blocks: time at 4x: pass 23 ms → pass 17 ms
- pathological: deep-parens: time at 4x: pass 22 ms → pass 17 ms
- pathological: method-chain: time at 4x: pass 34 ms → pass 22 ms
- pathological: iterator-chain: time at 4x: fail - → pass 24 ms
- pathological: iterator-chain: peak RSS at 4x: fail - → pass 22.3 MB
- pathological: iterator-chain: growth exponent: fail - → pass noise
- pathological: wide-list-literal: time at 4x: pass 50 ms → pass 35 ms
- pathological: large-enum-match: time at 4x: pass 36 ms → pass 24 ms
- pathological: deep-generic: time at 4x: pass 28 ms → pass 19 ms
- pathological: question-chain: time at 4x: pass 32 ms → pass 23 ms
- pathological: big-file: time at 4x: pass 201 ms → pass 139 ms
- fmt: hd fmt, 10k lines: pass 26 ms → pass 19 ms
- check-cost: checked run time / unchecked run time: fail 2.07x → fail 1.68x
- dev-speed: geomean dev / optimized: pass 1.58x → fail -
- dev-speed: worst case dev / optimized: pass 4.40x → fail -
- resources: warm check, 10k lines: CPU time: pass 40 ms → pass 20 ms
- startup: hd check empty.hd: wall time: pass 16 ms → pass 13 ms
- concurrency: p95 latency at N = 4 vs N = 1: fail 2.31x → fail 2.05x
- concurrency: p95 latency at N = 16 vs N = 1: fail 9.14x → fail 8.02x
- proptest-perf: property cases per second: fail 30k/s → fail 59k/s
- proptest-perf: time to shrink a known failure: pass 128 ms → pass 77 ms
- unit-test-perf: 1,000 unit tests, warm: fail 1160 ms → pass 671 ms
- unit-test-perf: overhead per unit test: pass 1.0 ms → pass 0.6 ms
- integration-test-perf: setup per integration test program: fail 157 ms → fail 71 ms
- integration-test-perf: cost per doc test: fail 140 ms → fail 73 ms
- runtime: geomean hd / Node: pass 1.18x → pass 0.85x
- runtime: fib: hd / Node: pass 0.64x → pass 0.90x
- runtime: sum: hd / Node: pass 2.49x → pass 0.31x
- runtime: string-build: hd / Node: pass 1.02x → pass 0.88x
- runtime: sort: hd / Node: pass 0.56x → pass 0.69x
- allocations: counted loop, bytes per iteration: pass 0.00 B/iter → pass 0.03 B/iter
- allocations: map loop, bytes per iteration: pass 0.00 B/iter → pass 0.02 B/iter
- size-startup-heap: tiny program, release Wasm size: pass 1594 B → fail 5028 B
- size-startup-heap: tiny program, start to first output: fail 55 ms → fail 37 ms
- size-startup-heap: peak heap, string-build: hd / Node: pass 0.39x → pass 0.43x
- host-call-overhead: Console println, per call: fail 6.9 µs → fail 2.6 µs
- host-call-overhead: Fs read_text!, per call: fail 19 µs → fail 10 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 34 µs → fail 21 µs
- suspension-overhead: await of a ready value, per await: pass 5.8 ns → pass 4.1 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 9.0M/s → pass 18.6M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 9.6M/s → pass 16.2M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.17x → fail 0.12x
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.24x → fail 0.21x
- text-throughput: string build, hd / Node MB/s: pass 2.25x → pass 3.19x
- text-throughput: split, hd / Node MB/s: pass 2.30x → pass 2.05x
- long-run-memory: RSS growth after warm-up, 1 min: pass 0.6 MB → pass 2.5 MB

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 34 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 36 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 168 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 143 ms | ≤ 300 ms | PASS |  |
| mistakes | programs with exactly one diagnostic | 100.0% | ≥ 95.0% | PASS | 69 of 69 |
| mistakes | one diagnostic with the expected code | 100.0% | ≥ 95.0% | PASS | 69 of 69 |
| mistakes | fix-it makes hd check pass | 33.3% | ≥ 80.0% | FAIL | 23 of 69 resolved; 23 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 65 tokens | ≤ 60 tokens | FAIL | largest: mut-method-on-readonly-binding; median 43 tokens, 171 B; 67 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 37 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 27.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 28 → 37 ms, start-up 18 ms |
| pathological | deep-blocks: time at 4x | 17 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 21.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 18 → 17 ms, start-up 18 ms |
| pathological | deep-parens: time at 4x | 17 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 21.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 17 → 17 ms, start-up 18 ms |
| pathological | method-chain: time at 4x | 22 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 24.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 21 → 22 ms, start-up 18 ms |
| pathological | iterator-chain: time at 4x | 24 ms | ≤ 2000 ms | PASS |  |
| pathological | iterator-chain: peak RSS at 4x | 22.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | iterator-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 24 ms, start-up 18 ms |
| pathological | wide-list-literal: time at 4x | 35 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 25.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 24 → 35 ms, start-up 18 ms |
| pathological | large-enum-match: time at 4x | 24 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 24.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 21 → 24 ms, start-up 18 ms |
| pathological | deep-generic: time at 4x | 19 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 21.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 19 ms, start-up 18 ms |
| pathological | question-chain: time at 4x | 23 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 24.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 21 → 23 ms, start-up 18 ms |
| pathological | big-file: time at 4x | 139 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 54.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.19 | ≤ 1.30 | PASS | 2x→4x; 71 → 139 ms, start-up 18 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 23 of 23 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 19 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.68x | ≤ 1.30x | FAIL | checked 532 ms, unchecked 316 ms |
| dev-speed | geomean dev / optimized | - | ≤ 4.00x | FAIL | optimized work is within noise: string-build (39.9 ms) |
| dev-speed | worst case dev / optimized | - | ≤ 10.00x | FAIL | optimized work is within noise: string-build (39.9 ms) |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 20 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 25.6 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.14 CPU-s, 36.8 MB | completes ≤ 30 s | PASS | wall 91 ms |
| resources | check, small, warm | 0.00 CPU-s, 18.8 MB | completes ≤ 30 s | PASS | wall 15 ms |
| resources | test, small, cold | 1.15 CPU-s, 89.0 MB | completes ≤ 30 s | PASS | wall 492 ms |
| resources | test, small, warm | 0.44 CPU-s, 66.8 MB | completes ≤ 30 s | PASS | wall 193 ms |
| resources | build, small, cold | 0.68 CPU-s, 85.3 MB | completes ≤ 30 s | PASS | wall 265 ms |
| resources | build, small, warm | 0.02 CPU-s, 31.3 MB | completes ≤ 30 s | PASS | wall 28 ms |
| resources | check, 10k, cold | 0.38 CPU-s, 58.3 MB | completes ≤ 30 s | PASS | wall 170 ms |
| resources | check, 10k, warm | 0.02 CPU-s, 25.6 MB | completes ≤ 30 s | PASS | wall 33 ms |
| resources | test, 10k, cold | 1.82 CPU-s, 124.9 MB | completes ≤ 30 s | PASS | wall 828 ms |
| resources | test, 10k, warm | 0.65 CPU-s, 75.0 MB | completes ≤ 30 s | PASS | wall 274 ms |
| resources | build, 10k, cold | 0.94 CPU-s, 105.7 MB | completes ≤ 30 s | PASS | wall 351 ms |
| resources | build, 10k, warm | 0.04 CPU-s, 35.2 MB | completes ≤ 30 s | PASS | wall 43 ms |
| resources | check, 50k, cold | 1.50 CPU-s, 163.4 MB | completes ≤ 150 s | PASS | wall 554 ms |
| resources | check, 50k, warm | 0.11 CPU-s, 57.3 MB | completes ≤ 150 s | PASS | wall 105 ms |
| resources | test, 50k, cold | 5.37 CPU-s, 282.4 MB | completes ≤ 150 s | PASS | wall 2749 ms |
| resources | test, 50k, warm | 1.93 CPU-s, 99.3 MB | completes ≤ 150 s | PASS | wall 731 ms |
| resources | build, 50k, cold | 2.10 CPU-s, 209.6 MB | completes ≤ 150 s | PASS | wall 697 ms |
| resources | build, 50k, warm | 0.13 CPU-s, 64.6 MB | completes ≤ 150 s | PASS | wall 116 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 97.0 MB | ≤ 22.0 MB | FAIL | RSS 95.8 MB at input 100, 186.9 MB at input 1000; build per input 61 ms at 50, 684 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.0 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 4.3 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 13 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 16.9 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.05x | ≤ 1.50x | FAIL | p95 95 ms at N = 1; 4 cores; total CPU 680 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.02x | ≤ 6.00x | FAIL | p95 95 ms at N = 1; 4 cores; total CPU 2870 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.06 | &lt; 1.00 (sublinear) | FAIL | 150 ms at N = 1, 2870 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 21.1 MB | ≤ 10.0 MB | FAIL | artifacts 8052 B in 10 files; toolchain 21.1 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 10.9 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.83x | ≥ 2.40x | FAIL | by --jobs |
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
| proptest-perf | property cases per second | 59k/s | ≥ 100k/s | FAIL | 6000 extra cases took 102 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 77 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 671 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.6 ms | ≤ 1.0 ms | PASS | 100 tests: 141 ms |
| integration-test-perf | setup per integration test program | 71 ms | ≤ 20 ms | FAIL | 10 programs: 795 ms for 21 tests, 38 ms per test |
| integration-test-perf | total time growth in programs | 0.898 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 171 ms at 2, 724 ms at 10 programs |
| integration-test-perf | cost per doc test | 73 ms | ≤ 20 ms | FAIL | 10 doc tests: 820 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 0.85x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.90x | ≤ 3.00x | PASS | hd 5.6 ms (p10 4.0, p90 7.2), Node 6.2 ms, Python 59 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 0.31x | ≤ 3.00x | PASS | hd 13 ms (p10 9.0, p90 23), Node 42 ms, Python 200 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.88x | ≤ 3.00x | PASS | hd 6.0 ms (p10 1.3, p90 11), Node 6.8 ms, Python 8.4 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 2.66x | ≤ 3.00x | PASS | hd 18 ms (p10 13, p90 22), Node 6.9 ms, Python 12 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.69x | ≤ 3.00x | PASS | hd 11 ms (p10 7.1, p90 12), Node 16 ms, Python 13 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.03 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.02 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 5028 B | ≤ 2048 B | FAIL | benchmarks: fib 5157 B, sum 4933 B, string-build 7813 B, map 6658 B, sort 12731 B |
| size-startup-heap | tiny program, start to first output | 37 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 1.06x | ≤ 2.00x | PASS | hd +15.1 MB, Node +14.3 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.43x | ≤ 2.00x | PASS | hd +8.6 MB, Node +20.1 MB over an empty program |
| host-call-overhead | Console println, per call | 2.6 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 10 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 21 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 4.1 ns | ≤ 100 ns | PASS | 4.1 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 18.6M/s | ≥ 1.0M/s | PASS | 108 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 16.2M/s | ≥ 1.0M/s | PASS | 124 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.12x | ≥ 0.50x | FAIL | hd 85 MB/s (45488 B document), Node 722 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.21x | ≥ 0.50x | FAIL | hd 58 MB/s (45488 B document), Node 270 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 3.19x | ≥ 0.50x | PASS | hd 496 MB/s, Node 155 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 2.05x | ≥ 0.50x | PASS | hd 1108 MB/s, Node 539 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.09x | ≥ 0.50x | FAIL | hd 12 MB/s, Node 126 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 6493 B | ≤ 10240 B | PASS | 12586 B at 1075 lines, 71027 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 915 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 2.5 MB | ≤ 10.0 MB | PASS | 33286000 requests; RSS 67.7 MB after warm-up, 70.3 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 7685 of 7685 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

- [2026-11-14T18](daily/2026-11-14T18.json)
- [2026-11-14T04](daily/2026-11-14T04.json)
- [2026-11-13T19](daily/2026-11-13T19.json)
- [2026-11-13T04](daily/2026-11-13T04.json)
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
