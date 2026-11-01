# Daily goal metrics

Finished: 2026-11-01T18:05:32.815Z. Commit: d86c1f06dddcaaa112b4af194139933f6c5c4a98.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V74 80-Core Processor).
Compiler: target/release/hd.

97 passed, 34 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 44 ms → fail 59 ms
- edit-latency: p95 check after an edit: pass 48 ms → pass 60 ms
- cold-check: median cold check, 10k lines: pass 131 ms → pass 186 ms
- test-latency: median edit-then-test: pass 292 ms → fail 380 ms
- pathological: overlapping-from: time at 4x: pass 35 ms → pass 46 ms
- pathological: deep-blocks: time at 4x: pass 15 ms → pass 19 ms
- pathological: deep-parens: time at 4x: pass 15 ms → pass 18 ms
- pathological: method-chain: time at 4x: fail - → pass 23 ms
- pathological: method-chain: peak RSS at 4x: fail - → pass 20.7 MB
- pathological: large-enum-match: time at 4x: pass 303 ms → pass 30 ms
- pathological: large-enum-match: growth exponent: fail 2.35 → pass noise
- pathological: deep-generic: time at 4x: pass 16 ms → pass 21 ms
- pathological: question-chain: time at 4x: pass 21 ms → pass 26 ms
- pathological: big-file: time at 4x: pass 123 ms → pass 220 ms
- pathological: big-file: growth exponent: pass 1.17 → fail 1.53
- fmt: hd fmt, 10k lines: pass 17 ms → pass 24 ms
- check-cost: checked run time / unchecked run time: fail 3.19x → fail 1.94x
- dev-speed: worst case dev / optimized: pass 1.21x → pass 1.02x
- resources: warm check, 10k lines: CPU time: pass 40 ms → pass 60 ms
- long-session: RSS growth, REPL inputs 100 to 1000: fail 39.9 MB → fail 53.9 MB
- startup: hd help (no version command): wall time: pass 2.8 ms → pass 3.1 ms
- startup: hd check empty.hd: wall time: pass 11 ms → pass 14 ms
- proptest-perf: property cases per second: fail 57k/s → fail 44k/s
- proptest-perf: time to shrink a known failure: pass 60 ms → pass 77 ms
- unit-test-perf: 1,000 unit tests, warm: pass 947 ms → fail 1278 ms
- unit-test-perf: overhead per unit test: pass 0.5 ms → pass 0.7 ms
- integration-test-perf: setup per integration test program: fail 265 ms → fail 343 ms
- integration-test-perf: total time growth in programs: pass 0.988 → fail 1.003
- integration-test-perf: cost per doc test: fail 187 ms → fail 247 ms
- runtime: geomean hd / Node: fail - → pass 1.12x
- runtime: fib: hd / Node: fail - → pass 0.66x
- runtime: sum: hd / Node: pass 0.59x → pass 0.71x
- runtime: string-build: hd / Node: pass 0.97x → pass 0.63x
- runtime: map: hd / Node: fail 3.75x → fail 3.14x
- runtime: sort: hd / Node: pass 0.38x → pass 1.87x
- allocations: counted loop, bytes per iteration: pass 0.01 B/iter → pass 0.00 B/iter
- allocations: list loop, bytes per iteration: pass 0.00 B/iter → pass 0.04 B/iter
- allocations: iterator chain, bytes per iteration: pass 0.00 B/iter → pass 0.02 B/iter
- size-startup-heap: tiny program, start to first output: fail 95 ms → fail 117 ms
- size-startup-heap: peak heap, map: hd / Node: pass 0.87x → pass 0.99x
- size-startup-heap: peak heap, string-build: hd / Node: pass 0.22x → pass 0.26x
- host-call-overhead: Console println, per call: fail 5.2 µs → fail 6.6 µs
- host-call-overhead: Fs read_text!, per call: fail 13 µs → fail 18 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 23 µs → fail 31 µs
- suspension-overhead: await of a ready value, per await: pass 19 ns → pass 17 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 16.0M/s → pass 13.3M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 15.6M/s → pass 9.6M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.06x → fail 0.13x
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.11x → fail 0.24x
- text-throughput: string build, hd / Node MB/s: pass 1.89x → pass 1.70x
- text-throughput: split, hd / Node MB/s: pass 3.05x → pass 2.28x

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 59 ms | ≤ 50 ms | FAIL |  |
| edit-latency | p95 check after an edit | 60 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 186 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 380 ms | ≤ 300 ms | FAIL |  |
| mistakes | programs with exactly one diagnostic | 95.7% | ≥ 95.0% | PASS | 66 of 69 |
| mistakes | one diagnostic with the expected code | 95.7% | ≥ 95.0% | PASS | 66 of 69; first misses: mut-on-binding-type, mut-on-parameter-type, some-on-result |
| mistakes | fix-it makes hd check pass | 34.8% | ≥ 80.0% | FAIL | 24 of 69 resolved; 24 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 93 tokens | ≤ 60 tokens | FAIL | largest: mut-on-parameter-type; median 44 tokens, 175 B; 64 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 46 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 23.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 32 → 46 ms, start-up 19 ms |
| pathological | deep-blocks: time at 4x | 19 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 19.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 19 ms, start-up 19 ms |
| pathological | deep-parens: time at 4x | 18 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 18.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 18 → 18 ms, start-up 19 ms |
| pathological | method-chain: time at 4x | 23 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 20.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 21 → 23 ms, start-up 19 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 46 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | wide-list-literal: peak RSS at 4x | 22.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 33 → 46 ms, start-up 19 ms |
| pathological | large-enum-match: time at 4x | 30 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 21.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 23 → 30 ms, start-up 19 ms |
| pathological | deep-generic: time at 4x | 21 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 19.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 21 ms, start-up 19 ms |
| pathological | question-chain: time at 4x | 26 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 21.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 26 ms, start-up 19 ms |
| pathological | big-file: time at 4x | 220 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 52.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.53 | ≤ 1.30 | FAIL | 2x→4x; 88 → 220 ms, start-up 19 ms |
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
| check-cost | checked run time / unchecked run time | 1.94x | ≤ 1.30x | FAIL | checked 661 ms, unchecked 340 ms |
| dev-speed | geomean dev / optimized | 1.00x | ≤ 4.00x | PASS | fib 1.00x, sum 1.01x, string-build 1.02x, map 1.00x, sort 0.98x |
| dev-speed | worst case dev / optimized | 1.02x | ≤ 10.00x | PASS | string-build |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 60 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 33.2 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.13 CPU-s, 33.1 MB | completes ≤ 30 s | PASS | wall 87 ms |
| resources | check, small, warm | 0.01 CPU-s, 18.1 MB | completes ≤ 30 s | PASS | wall 18 ms |
| resources | test, small, cold | 2.87 CPU-s, 120.0 MB | completes ≤ 30 s | PASS | wall 1108 ms |
| resources | test, small, warm | 2.22 CPU-s, 121.3 MB | completes ≤ 30 s | PASS | wall 844 ms |
| resources | build, small, cold | 0.67 CPU-s, 95.5 MB | completes ≤ 30 s | PASS | wall 288 ms |
| resources | build, small, warm | 0.05 CPU-s, 40.9 MB | completes ≤ 30 s | PASS | wall 61 ms |
| resources | check, 10k, cold | 0.43 CPU-s, 58.8 MB | completes ≤ 30 s | PASS | wall 224 ms |
| resources | check, 10k, warm | 0.06 CPU-s, 33.2 MB | completes ≤ 30 s | PASS | wall 56 ms |
| resources | test, 10k, cold | 3.76 CPU-s, 139.4 MB | completes ≤ 30 s | PASS | wall 1638 ms |
| resources | test, 10k, warm | 2.83 CPU-s, 130.0 MB | completes ≤ 30 s | PASS | wall 1256 ms |
| resources | build, 10k, cold | 0.95 CPU-s, 116.0 MB | completes ≤ 30 s | PASS | wall 377 ms |
| resources | build, 10k, warm | 0.10 CPU-s, 48.0 MB | completes ≤ 30 s | PASS | wall 102 ms |
| resources | check, 50k, cold | 1.54 CPU-s, 178.8 MB | completes ≤ 150 s | PASS | wall 651 ms |
| resources | check, 50k, warm | 0.29 CPU-s, 95.5 MB | completes ≤ 150 s | PASS | wall 227 ms |
| resources | test, 50k, cold | 13.72 CPU-s, 309.1 MB | completes ≤ 150 s | PASS | wall 9684 ms |
| resources | test, 50k, warm | 11.11 CPU-s, 167.5 MB | completes ≤ 150 s | PASS | wall 8370 ms |
| resources | build, 50k, cold | 2.20 CPU-s, 292.1 MB | completes ≤ 150 s | PASS | wall 897 ms |
| resources | build, 50k, warm | 0.32 CPU-s, 116.0 MB | completes ≤ 150 s | PASS | wall 302 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 53.9 MB | ≤ 22.0 MB | FAIL | RSS 174.5 MB at input 100, 228.4 MB at input 1000; build per input 132 ms at 50, 597 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.1 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.8 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 14 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 16.0 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.08x | ≤ 1.50x | FAIL | p95 89 ms at N = 1; 4 cores; total CPU 670 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.55x | ≤ 6.00x | FAIL | p95 89 ms at N = 1; 4 cores; total CPU 2850 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.11 | &lt; 1.00 (sublinear) | FAIL | 130 ms at N = 1, 2850 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 17.1 MB | ≤ 10.0 MB | FAIL | artifacts 6919 B in 10 files; toolchain 17.1 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 10.2 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.77x | ≥ 2.40x | FAIL | by --jobs |
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
| proptest-perf | property cases per second | 44k/s | ≥ 100k/s | FAIL | 6000 extra cases took 137 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 77 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 1278 ms | ≤ 1000 ms | FAIL | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.7 ms | ≤ 1.0 ms | PASS | 100 tests: 624 ms |
| integration-test-perf | setup per integration test program | 343 ms | ≤ 20 ms | FAIL | 10 programs: 3679 ms for 21 tests, 175 ms per test |
| integration-test-perf | total time growth in programs | 1.003 | exponent &lt; 1 (sublinear) | FAIL | beyond the 0-program run: 681 ms at 2, 3422 ms at 10 programs |
| integration-test-perf | cost per doc test | 247 ms | ≤ 20 ms | FAIL | 10 doc tests: 2721 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 1.12x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.66x | ≤ 3.00x | PASS | hd 5.6 ms (p10 -16.4, p90 14), Node 8.5 ms, Python 79 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 0.71x | ≤ 3.00x | PASS | hd 5.8 ms (p10 -6.7, p90 9.2), Node 8.2 ms, Python 238 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.63x | ≤ 3.00x | PASS | hd 5.7 ms (p10 -0.6, p90 13), Node 9.0 ms, Python 12 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 3.14x | ≤ 3.00x | FAIL | hd 29 ms (p10 20, p90 38), Node 9.1 ms, Python 15 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 1.87x | ≤ 3.00x | PASS | hd 37 ms (p10 32, p90 51), Node 19 ms, Python 15 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.04 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.02 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 2337 B | ≤ 2048 B | FAIL | benchmarks: fib 3656 B, sum 3610 B, string-build 5423 B, map 5203 B, sort 6347 B |
| size-startup-heap | tiny program, start to first output | 117 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.99x | ≤ 2.00x | PASS | hd +14.0 MB, Node +14.1 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.26x | ≤ 2.00x | PASS | hd +5.5 MB, Node +20.9 MB over an empty program |
| host-call-overhead | Console println, per call | 6.6 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 18 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 31 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 17 ns | ≤ 100 ns | PASS | 17 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 13.3M/s | ≥ 1.0M/s | PASS | 150 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 9.6M/s | ≥ 1.0M/s | PASS | 209 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.13x | ≥ 0.50x | FAIL | hd 48 MB/s (45488 B document), Node 366 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.24x | ≥ 0.50x | FAIL | hd 45 MB/s (45488 B document), Node 185 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 1.70x | ≥ 0.50x | PASS | hd 196 MB/s, Node 115 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 2.28x | ≥ 0.50x | PASS | hd 644 MB/s, Node 282 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.10x | ≥ 0.50x | FAIL | hd 8.4 MB/s, Node 81 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 5327 B | ≤ 10240 B | PASS | 10200 B at 1075 lines, 58147 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 664 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 15784000 requests; RSS 99.7 MB after warm-up, 99.7 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 6011 of 6011 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
