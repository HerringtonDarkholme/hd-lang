# Daily goal metrics

Finished: 2026-11-02T03:22:39.686Z. Commit: f5cc84306ffd8619d377bd133ffb06d466916ef4.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 7763 64-Core Processor).
Compiler: target/release/hd.

98 passed, 33 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: fail 59 ms → fail 72 ms
- edit-latency: p95 check after an edit: pass 60 ms → pass 75 ms
- cold-check: median cold check, 10k lines: pass 186 ms → pass 237 ms
- test-latency: median edit-then-test: fail 380 ms → fail 479 ms
- pathological: overlapping-from: time at 4x: pass 46 ms → pass 61 ms
- pathological: deep-blocks: time at 4x: pass 19 ms → pass 22 ms
- pathological: deep-parens: time at 4x: pass 18 ms → pass 21 ms
- pathological: method-chain: time at 4x: pass 23 ms → pass 30 ms
- pathological: wide-list-literal: time at 4x: pass 46 ms → pass 63 ms
- pathological: large-enum-match: time at 4x: pass 30 ms → pass 36 ms
- pathological: deep-generic: time at 4x: pass 21 ms → pass 26 ms
- pathological: question-chain: time at 4x: pass 26 ms → pass 32 ms
- pathological: big-file: growth exponent: fail 1.53 → pass 1.10
- fmt: hd fmt, 10k lines: pass 24 ms → pass 31 ms
- check-cost: checked run time / unchecked run time: fail 1.94x → fail 1.73x
- dev-speed: geomean dev / optimized: pass 1.00x → pass 0.85x
- resources: warm check, 10k lines: CPU time: pass 60 ms → pass 80 ms
- long-session: RSS growth, REPL inputs 100 to 1000: fail 53.9 MB → fail 105.7 MB
- startup: hd help (no version command): wall time: pass 3.1 ms → pass 3.7 ms
- startup: hd check empty.hd: wall time: pass 14 ms → pass 17 ms
- proptest-perf: property cases per second: fail 44k/s → fail 24k/s
- proptest-perf: time to shrink a known failure: pass 77 ms → pass 107 ms
- unit-test-perf: 1,000 unit tests, warm: fail 1278 ms → fail 1584 ms
- unit-test-perf: overhead per unit test: pass 0.7 ms → pass 0.9 ms
- integration-test-perf: setup per integration test program: fail 343 ms → fail 433 ms
- integration-test-perf: total time growth in programs: fail 1.003 → pass 0.989
- integration-test-perf: cost per doc test: fail 247 ms → fail 312 ms
- runtime: geomean hd / Node: pass 1.12x → pass 0.67x
- runtime: fib: hd / Node: pass 0.66x → pass 0.03x
- runtime: sum: hd / Node: pass 0.71x → pass 1.07x
- runtime: string-build: hd / Node: pass 0.63x → pass 0.34x
- runtime: map: hd / Node: fail 3.14x → fail 4.05x
- runtime: sort: hd / Node: pass 1.87x → fail 3.10x
- allocations: counted loop, bytes per iteration: pass 0.00 B/iter → pass 0.04 B/iter
- allocations: list loop, bytes per iteration: pass 0.04 B/iter → pass 0.00 B/iter
- allocations: iterator chain, bytes per iteration: pass 0.02 B/iter → pass 0.00 B/iter
- size-startup-heap: tiny program, start to first output: fail 117 ms → fail 159 ms
- size-startup-heap: peak heap, map: hd / Node: pass 0.99x → pass 0.84x
- host-call-overhead: Console println, per call: fail 6.6 µs → fail 8.3 µs
- host-call-overhead: Fs read_text!, per call: fail 18 µs → fail 26 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 31 µs → fail 46 µs
- suspension-overhead: await of a ready value, per await: pass 17 ns → pass 60 ns
- suspension-overhead: race! of two ready tasks, throughput: pass 9.6M/s → pass 7.8M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.13x → fail 0.18x
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.24x → fail 0.20x
- text-throughput: split, hd / Node MB/s: pass 2.28x → pass 4.31x
- long-run-memory: RSS growth after warm-up, 1 min: pass 0.0 MB → pass 0.2 MB

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 72 ms | ≤ 50 ms | FAIL |  |
| edit-latency | p95 check after an edit | 75 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 237 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 479 ms | ≤ 300 ms | FAIL |  |
| mistakes | programs with exactly one diagnostic | 95.7% | ≥ 95.0% | PASS | 66 of 69 |
| mistakes | one diagnostic with the expected code | 95.7% | ≥ 95.0% | PASS | 66 of 69; first misses: mut-on-binding-type, mut-on-parameter-type, some-on-result |
| mistakes | fix-it makes hd check pass | 34.8% | ≥ 80.0% | FAIL | 24 of 69 resolved; 24 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 93 tokens | ≤ 60 tokens | FAIL | largest: mut-on-parameter-type; median 44 tokens, 175 B; 64 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 61 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 24.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 42 → 61 ms, start-up 22 ms |
| pathological | deep-blocks: time at 4x | 22 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 18.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 22 ms, start-up 22 ms |
| pathological | deep-parens: time at 4x | 21 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 18.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 21 → 21 ms, start-up 22 ms |
| pathological | method-chain: time at 4x | 30 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 20.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 25 → 30 ms, start-up 22 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: trait-resolution-depth: `map` needs a proof that its receiver implements Iterator, and that proof needs more than 64 nested bounds &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 63 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 22.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 42 → 63 ms, start-up 22 ms |
| pathological | large-enum-match: time at 4x | 36 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 21.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 27 → 36 ms, start-up 22 ms |
| pathological | deep-generic: time at 4x | 26 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 18.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 23 → 26 ms, start-up 22 ms |
| pathological | question-chain: time at 4x | 32 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 21.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 26 → 32 ms, start-up 22 ms |
| pathological | big-file: time at 4x | 225 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | big-file: peak RSS at 4x | 52.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.10 | ≤ 1.30 | PASS | 2x→4x; 117 → 225 ms, start-up 22 ms |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 24 of 24 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 31 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.73x | ≤ 1.30x | FAIL | checked 748 ms, unchecked 431 ms |
| dev-speed | geomean dev / optimized | 0.85x | ≤ 4.00x | PASS | fib 0.78x, sum 0.82x, string-build 0.86x, map 0.79x, sort 0.99x |
| dev-speed | worst case dev / optimized | 0.99x | ≤ 10.00x | PASS | sort |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 80 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 33.0 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.16 CPU-s, 33.1 MB | completes ≤ 30 s | PASS | wall 111 ms |
| resources | check, small, warm | 0.01 CPU-s, 18.4 MB | completes ≤ 30 s | PASS | wall 22 ms |
| resources | test, small, cold | 3.73 CPU-s, 120.1 MB | completes ≤ 30 s | PASS | wall 1415 ms |
| resources | test, small, warm | 2.85 CPU-s, 120.3 MB | completes ≤ 30 s | PASS | wall 1042 ms |
| resources | build, small, cold | 0.91 CPU-s, 97.0 MB | completes ≤ 30 s | PASS | wall 373 ms |
| resources | build, small, warm | 0.07 CPU-s, 40.8 MB | completes ≤ 30 s | PASS | wall 72 ms |
| resources | check, 10k, cold | 0.50 CPU-s, 58.8 MB | completes ≤ 30 s | PASS | wall 235 ms |
| resources | check, 10k, warm | 0.08 CPU-s, 33.0 MB | completes ≤ 30 s | PASS | wall 68 ms |
| resources | test, 10k, cold | 4.85 CPU-s, 143.6 MB | completes ≤ 30 s | PASS | wall 2077 ms |
| resources | test, 10k, warm | 3.50 CPU-s, 130.5 MB | completes ≤ 30 s | PASS | wall 1565 ms |
| resources | build, 10k, cold | 1.28 CPU-s, 120.8 MB | completes ≤ 30 s | PASS | wall 487 ms |
| resources | build, 10k, warm | 0.12 CPU-s, 48.1 MB | completes ≤ 30 s | PASS | wall 122 ms |
| resources | check, 50k, cold | 2.09 CPU-s, 178.6 MB | completes ≤ 150 s | PASS | wall 842 ms |
| resources | check, 50k, warm | 0.37 CPU-s, 95.6 MB | completes ≤ 150 s | PASS | wall 274 ms |
| resources | test, 50k, cold | 17.71 CPU-s, 308.8 MB | completes ≤ 150 s | PASS | wall 12485 ms |
| resources | test, 50k, warm | 14.15 CPU-s, 183.2 MB | completes ≤ 150 s | PASS | wall 11038 ms |
| resources | build, 50k, cold | 2.97 CPU-s, 292.0 MB | completes ≤ 150 s | PASS | wall 1145 ms |
| resources | build, 50k, warm | 0.40 CPU-s, 115.9 MB | completes ≤ 150 s | PASS | wall 371 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 105.7 MB | ≤ 22.0 MB | FAIL | RSS 140.9 MB at input 100, 246.5 MB at input 1000; build per input 105 ms at 50, 729 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.7 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.8 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 17 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 15.6 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.16x | ≤ 1.50x | FAIL | p95 116 ms at N = 1; 4 cores; total CPU 930 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.24x | ≤ 6.00x | FAIL | p95 116 ms at N = 1; 4 cores; total CPU 3860 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.13 | &lt; 1.00 (sublinear) | FAIL | 170 ms at N = 1, 3860 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 17.3 MB | ≤ 10.0 MB | FAIL | artifacts 6925 B in 10 files; toolchain 17.3 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 10.2 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.81x | ≥ 2.40x | FAIL | by --jobs |
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
| proptest-perf | property cases per second | 24k/s | ≥ 100k/s | FAIL | 6000 extra cases took 251 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 107 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 1584 ms | ≤ 1000 ms | FAIL | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.9 ms | ≤ 1.0 ms | PASS | 100 tests: 774 ms |
| integration-test-perf | setup per integration test program | 433 ms | ≤ 20 ms | FAIL | 10 programs: 4683 ms for 21 tests, 223 ms per test |
| integration-test-perf | total time growth in programs | 0.989 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 885 ms at 2, 4349 ms at 10 programs |
| integration-test-perf | cost per doc test | 312 ms | ≤ 20 ms | FAIL | 10 doc tests: 3449 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 0.67x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.03x | ≤ 3.00x | PASS | hd 0.3 ms (p10 -9.2, p90 4.7), Node 10 ms, Python 103 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 1.07x | ≤ 3.00x | PASS | hd 10 ms (p10 -7.2, p90 14), Node 9.4 ms, Python 292 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.34x | ≤ 3.00x | PASS | hd 3.5 ms (p10 -8.6, p90 7.3), Node 11 ms, Python 16 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 4.05x | ≤ 3.00x | FAIL | hd 44 ms (p10 36, p90 58), Node 11 ms, Python 18 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 3.10x | ≤ 3.00x | FAIL | hd 78 ms (p10 67, p90 104), Node 25 ms, Python 19 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.04 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 2337 B | ≤ 2048 B | FAIL | benchmarks: fib 3656 B, sum 3610 B, string-build 5423 B, map 5203 B, sort 6169 B |
| size-startup-heap | tiny program, start to first output | 159 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.84x | ≤ 2.00x | PASS | hd +11.9 MB, Node +14.3 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.24x | ≤ 2.00x | PASS | hd +4.9 MB, Node +20.8 MB over an empty program |
| host-call-overhead | Console println, per call | 8.3 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 26 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 46 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 60 ns | ≤ 100 ns | PASS | 60 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 12.8M/s | ≥ 1.0M/s | PASS | 156 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 7.8M/s | ≥ 1.0M/s | PASS | 256 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.18x | ≥ 0.50x | FAIL | hd 46 MB/s (45488 B document), Node 257 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.20x | ≥ 0.50x | FAIL | hd 28 MB/s (45488 B document), Node 141 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 1.67x | ≥ 0.50x | PASS | hd 158 MB/s, Node 95 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 4.31x | ≥ 0.50x | PASS | hd 998 MB/s, Node 232 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.10x | ≥ 0.50x | FAIL | hd 6.8 MB/s, Node 71 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 5327 B | ≤ 10240 B | PASS | 10204 B at 1075 lines, 58151 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 664 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.2 MB | ≤ 10.0 MB | PASS | 12206000 requests; RSS 99.6 MB after warm-up, 99.8 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 6061 of 6061 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
