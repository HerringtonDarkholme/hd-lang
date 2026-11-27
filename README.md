# Daily goal metrics

Finished: 2026-11-27T21:32:59.843Z. Commit: 28c6e44ec69cedb924817555b4e1b1d6457b8dfc.
Machine: ubuntu-latest, 4 CPUs (INTEL(R) XEON(R) PLATINUM 8573C).
Compiler: target/dist/hd.

292 passed, 21 failed, 5 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 25 ms → pass 34 ms
- edit-latency: p95 check after an edit: pass 31 ms → pass 46 ms
- cold-check: median cold check, 10k lines: pass 294 ms → pass 382 ms
- test-latency: median edit-then-test: pass 98 ms → pass 128 ms
- pathological: deep-parens: time at 4x: pass 25 ms → pass 29 ms
- pathological: method-chain: time at 4x: pass 30 ms → pass 34 ms
- pathological: iterator-chain: time at 4x: pass 28 ms → pass 44 ms
- pathological: wide-list-literal: time at 4x: pass 44 ms → pass 54 ms
- pathological: large-enum-match: time at 4x: pass 30 ms → pass 44 ms
- pathological: deep-generic: time at 4x: pass 22 ms → pass 28 ms
- pathological: question-chain: time at 4x: pass 27 ms → pass 34 ms
- pathological: big-file: time at 4x: pass 116 ms → pass 142 ms
- fmt: hd fmt, 10k lines: pass 26 ms → pass 33 ms
- check-cost: checked run time / unchecked run time: fail 1.89x → pass 0.89x
- dev-speed: geomean dev / optimized: pass 1.49x → pass 1.65x
- dev-speed: worst case dev / optimized: pass 2.76x → pass 2.42x
- startup: hd help (no version command): wall time: pass 5.9 ms → pass 5.3 ms
- concurrency: p95 latency at N = 4 vs N = 1: fail 1.63x → fail 1.81x
- proptest-perf: property cases per second: fail 78k/s → fail 69k/s
- proptest-perf: time to shrink a known failure: pass 16 ms → pass 11 ms
- unit-test-perf: 1,000 unit tests, warm: pass 56 ms → pass 62 ms
- integration-test-perf: setup per integration test program: pass 6.5 ms → pass 9.8 ms
- integration-test-perf: cost per doc test: pass 4.3 ms → pass 5.8 ms
- runtime: sum: hd / Node: pass 0.13x → pass 0.18x
- runtime: string-build: hd / Node: pass 0.55x → pass 0.42x
- runtime: map: hd / Node: pass 0.73x → pass 0.58x
- runtime: sort: hd / Node: pass 0.41x → pass 0.49x
- size-startup-heap: peak heap, map: hd / Node: pass 0.77x → pass 0.53x
- host-call-overhead: Console println, per call: fail 2.3 µs → fail 1.9 µs
- host-call-overhead: Fs read_text!, per call: pass 9.8 µs → pass 5.5 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 16 µs → fail 9.0 µs
- suspension-overhead: await of a ready value, per await: pass 6.5 ns → pass 5.6 ns
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.17x → fail 0.10x
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.19x → fail 0.21x
- text-throughput: string build, hd / Node MB/s: pass 2.75x → pass 3.97x
- text-throughput: split, hd / Node MB/s: pass 1.63x → pass 0.93x
- text-throughput: regex find_all, hd / Node MB/s: pass 0.65x → fail 0.45x

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 34 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 46 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 382 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 128 ms | ≤ 300 ms | PASS |  |
| mistakes | programs with exactly one diagnostic | 100.0% | ≥ 95.0% | PASS | 69 of 69 |
| mistakes | one diagnostic with the expected code | 100.0% | ≥ 95.0% | PASS | 69 of 69 |
| mistakes | fix-it makes hd check pass | 55.1% | ≥ 80.0% | FAIL | 38 of 69 resolved; 38 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 74 tokens | ≤ 60 tokens | FAIL | largest: interpolate-optional; median 35 tokens, 140 B; 63 of 69 within |
| mistakes | largest diagnostic JSON (bytes / 4) | 197 tokens | ≤ 60 tokens | FAIL | largest: interpolate-optional, 787 B; median 76 tokens, 304 B; 69 diagnostics |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 159 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 56 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | overlapping-from: peak RSS at 4x | 46.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 41 → 56 ms, start-up 28 ms |
| pathological | deep-blocks: time at 4x | 29 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-blocks: peak RSS at 4x | 41.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 32 → 29 ms, start-up 28 ms |
| pathological | deep-parens: time at 4x | 29 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 42.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 30 → 29 ms, start-up 28 ms |
| pathological | method-chain: time at 4x | 34 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 41.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 30 → 34 ms, start-up 28 ms |
| pathological | iterator-chain: time at 4x | 44 ms | ≤ 2000 ms | PASS |  |
| pathological | iterator-chain: peak RSS at 4x | 42.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | iterator-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 33 → 44 ms, start-up 28 ms |
| pathological | wide-list-literal: time at 4x | 54 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 48.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 40 → 54 ms, start-up 28 ms |
| pathological | large-enum-match: time at 4x | 44 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 42.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 33 → 44 ms, start-up 28 ms |
| pathological | deep-generic: time at 4x | 28 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 42.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 34 → 28 ms, start-up 28 ms |
| pathological | question-chain: time at 4x | 34 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 44.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 30 → 34 ms, start-up 28 ms |
| pathological | big-file: time at 4x | 142 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 86.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 0.95 | ≤ 1.30 | PASS | 2x→4x; 87 → 142 ms, start-up 28 ms |
| scale | fn-items check | 4.03x | ≤ 5x | PASS | hashes 196701 → 792617 (4.03x, limit 5.00x) |
| scale | fn-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | fn-items build | 4.02x | ≤ 5x | PASS | hashes 565142 → 2272445 (4.02x, limit 5.00x) |
| scale | bang-chain check | 4.03x | ≤ 5x | PASS | hashes 190562 → 767714 (4.03x, limit 5.00x) |
| scale | bang-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | bang-chain build | 4.03x | ≤ 5x | PASS | hashes 556351 → 2239751 (4.03x, limit 5.00x) |
| scale | struct-items check | 4.04x | ≤ 5x | PASS | hashes 198279 → 800719 (4.04x, limit 5.00x) |
| scale | struct-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | struct-items build | 4.01x | ≤ 5x | PASS | hashes 741621 → 2972721 (4.01x, limit 5.00x) |
| scale | enum-items check | 4.00x | ≤ 5x | PASS | hashes 526279 → 2105335 (4.00x, limit 5.00x) |
| scale | enum-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | enum-items build | 4.00x | ≤ 5x | PASS | hashes 1566972 → 6268138 (4.00x, limit 5.00x) |
| scale | trait-items check | 4.01x | ≤ 5x | PASS | allocs 525879 → 2108333 (4.01x, limit 5.00x) |
| scale | trait-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | trait-items build | 4.00x | ≤ 5x | PASS | allocs 1137028 → 4551367 (4.00x, limit 5.00x) |
| scale | impl-methods check | 4.05x | ≤ 5x | PASS | allocs 212243 → 859153 (4.05x, limit 5.00x) |
| scale | impl-methods fmt | 3.97x | ≤ 5x | PASS | allocs 4045 → 16057 (3.97x, limit 5.00x) |
| scale | impl-methods build | 4.01x | ≤ 5x | PASS | hashes 800540 → 3211167 (4.01x, limit 5.00x) |
| scale | traits-of-one-struct check | 4.01x | ≤ 5x | PASS | allocs 401228 → 1607108 (4.01x, limit 5.00x) |
| scale | traits-of-one-struct fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | traits-of-one-struct build | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | impls-of-one-trait check | 4.01x | ≤ 5x | PASS | allocs 374860 → 1504757 (4.01x, limit 5.00x) |
| scale | impls-of-one-trait fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | impls-of-one-trait build | 4.00x | ≤ 5x | PASS | hashes 1592373 → 6375683 (4.00x, limit 5.00x) |
| scale | operator-impls check | 4.01x | ≤ 5x | PASS | allocs 450470 → 1806906 (4.01x, limit 5.00x) |
| scale | operator-impls fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | operator-impls build | 4.00x | ≤ 5x | PASS | scope_probes 25000 → 100000 (4.00x, limit 5.00x) |
| scale | derive-items check | 4.00x | ≤ 5x | PASS | trait_lookup 23498 → 93998 (4.00x, limit 5.00x) |
| scale | derive-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | derive-items build | 4.00x | ≤ 5x | PASS | trait_lookup 23498 → 93998 (4.00x, limit 5.00x) |
| scale | dyn-values check | 4.01x | ≤ 5x | PASS | allocs 457077 → 1831048 (4.01x, limit 5.00x) |
| scale | dyn-values fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | dyn-values build | 4.00x | ≤ 5x | PASS | scope_probes 15500 → 62000 (4.00x, limit 5.00x) |
| scale | consts check | 3.97x | ≤ 5x | PASS | hashes 63624 → 252820 (3.97x, limit 5.00x) |
| scale | consts fmt | 3.89x | ≤ 5x | PASS | allocs 2087 → 8109 (3.89x, limit 5.00x) |
| scale | consts build | 4.00x | ≤ 5x | PASS | loan_queries 4506 → 18006 (4.00x, limit 5.00x) |
| scale | local-items check | 4.00x | ≤ 5x | PASS | scope_probes 21500 → 86000 (4.00x, limit 5.00x) |
| scale | local-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | local-items build | 4.00x | ≤ 5x | PASS | hashes 1466234 → 5866165 (4.00x, limit 5.00x) |
| scale | syntax-items check | 4.00x | ≤ 5x | PASS | item_body_checks 1700 → 6800 (4.00x, limit 5.00x) |
| scale | syntax-items fmt | 4.00x | ≤ 5x | PASS | hashes 17960 → 71864 (4.00x, limit 5.00x) |
| scale | syntax-items build | 4.00x | ≤ 5x | PASS | item_body_checks 1700 → 6800 (4.00x, limit 5.00x) |
| scale | struct-chain check | 4.04x | ≤ 5x | PASS | hashes 150722 → 609374 (4.04x, limit 5.00x) |
| scale | struct-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | struct-chain-values check | 4.01x | ≤ 5x | PASS | hashes 115248 → 462400 (4.01x, limit 5.00x) |
| scale | struct-chain-values fmt | 4.02x | ≤ 5x | PASS | hashes 2082 → 8370 (4.02x, limit 5.00x) |
| scale | struct-chain-values build | 4.00x | ≤ 5x | PASS | scope_probes 4900 → 19600 (4.00x, limit 5.00x) |
| scale | modules check | 4.00x | ≤ 5x | PASS | hashes 1154578 → 4622154 (4.00x, limit 5.00x) |
| scale | modules fmt | 4.00x | ≤ 5x | PASS | hashes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | modules build | 4.00x | ≤ 5x | PASS | hashes 1532858 → 6134515 (4.00x, limit 5.00x) |
| scale | use-names check | 4.03x | ≤ 5x | PASS | hashes 217218 → 874714 (4.03x, limit 5.00x) |
| scale | use-names fmt | 4.01x | ≤ 5x | PASS | hashes 3783 → 15159 (4.01x, limit 5.00x) |
| scale | use-names build | 4.01x | ≤ 5x | PASS | hashes 587068 → 2356175 (4.01x, limit 5.00x) |
| scale | generic-fns check | 4.01x | ≤ 5x | PASS | hashes 304157 → 1219973 (4.01x, limit 5.00x) |
| scale | generic-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-fns build | 4.01x | ≤ 5x | PASS | hashes 1020894 → 4096971 (4.01x, limit 5.00x) |
| scale | generic-params check | 4.03x | ≤ 5x | PASS | hashes 15773 → 63509 (4.03x, limit 5.00x) |
| scale | generic-params fmt | 4.10x | ≤ 10x | PASS | alloc_bytes 350594 → 1438988 (4.10x, limit 10.00x) |
| scale | generic-params build | 4.05x | ≤ 5x | PASS | hashes 21969 → 89043 (4.05x, limit 5.00x) |
| scale | param-bounds check | 4.00x | ≤ 5x | PASS | scope_probes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | param-bounds fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | param-bounds build | 4.01x | ≤ 5x | PASS | allocs 27592 → 110724 (4.01x, limit 5.00x) |
| scale | type-defaults check | 4.01x | ≤ 5x | PASS | allocs 689224 → 2762846 (4.01x, limit 5.00x) |
| scale | type-defaults fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | type-defaults build | 4.00x | ≤ 5x | PASS | allocs 1371918 → 5489249 (4.00x, limit 5.00x) |
| scale | alias-chain check | 4.00x | ≤ 5x | PASS | scope_probes 6600 → 26400 (4.00x, limit 5.00x) |
| scale | alias-chain fmt | 4.03x | ≤ 5x | PASS | hashes 1234 → 4978 (4.03x, limit 5.00x) |
| scale | alias-chain build | 4.00x | ≤ 5x | PASS | scope_probes 6600 → 26400 (4.00x, limit 5.00x) |
| scale | row-chain check | 4.03x | ≤ 5x | PASS | hashes 220816 → 889500 (4.03x, limit 5.00x) |
| scale | row-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | row-chain build | 4.02x | ≤ 5x | PASS | hashes 611341 → 2459917 (4.02x, limit 5.00x) |
| scale | row-traits check | 4.00x | ≤ 5x | PASS | scope_probes 2300 → 9200 (4.00x, limit 5.00x) |
| scale | row-traits fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | row-traits build | 4.00x | ≤ 5x | PASS | scope_probes 2300 → 9200 (4.00x, limit 5.00x) |
| scale | closures check | 4.00x | ≤ 5x | PASS | scope_probes 2000 → 8000 (4.00x, limit 5.00x) |
| scale | closures fmt | 3.94x | ≤ 5x | PASS | allocs 3061 → 12073 (3.94x, limit 5.00x) |
| scale | closures build | 4.00x | ≤ 5x | PASS | scope_probes 2000 → 8000 (4.00x, limit 5.00x) |
| scale | generic-local-fns check | 4.03x | ≤ 5x | PASS | hashes 236130 → 952118 (4.03x, limit 5.00x) |
| scale | generic-local-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-local-fns build | 4.01x | ≤ 5x | PASS | hashes 1187813 → 4763684 (4.01x, limit 5.00x) |
| scale | let-statements check | 4.00x | ≤ 5x | PASS | scope_probes 500 → 2000 (4.00x, limit 5.00x) |
| scale | let-statements fmt | 3.94x | ≤ 5x | PASS | allocs 2554 → 10066 (3.94x, limit 5.00x) |
| scale | let-statements build | 4.08x | ≤ 5x | PASS | hashes 290624 → 1185113 (4.08x, limit 5.00x) |
| scale | matches check | 4.00x | ≤ 5x | PASS | scope_probes 1000 → 4000 (4.00x, limit 5.00x) |
| scale | matches fmt | 3.97x | ≤ 5x | PASS | allocs 6057 → 24069 (3.97x, limit 5.00x) |
| scale | matches build | 4.00x | ≤ 5x | PASS | scope_probes 1000 → 4000 (4.00x, limit 5.00x) |
| scale | enum-arms check | 4.00x | ≤ 5x | PASS | scope_probes 5000 → 20000 (4.00x, limit 5.00x) |
| scale | enum-arms fmt | 3.91x | ≤ 5x | PASS | allocs 1551 → 6065 (3.91x, limit 5.00x) |
| scale | enum-arms build | 4.00x | ≤ 5x | PASS | scope_probes 5000 → 20000 (4.00x, limit 5.00x) |
| scale | tuple-patterns check | 4.00x | ≤ 5x | PASS | scope_probes 1000 → 4000 (4.00x, limit 5.00x) |
| scale | tuple-patterns fmt | 3.95x | ≤ 5x | PASS | allocs 3054 → 12066 (3.95x, limit 5.00x) |
| scale | tuple-patterns build | 4.00x | ≤ 5x | PASS | hashes 469575 → 1878306 (4.00x, limit 5.00x) |
| scale | range-patterns check | 4.02x | ≤ 5x | PASS | hashes 254166 → 1022470 (4.02x, limit 5.00x) |
| scale | range-patterns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | range-patterns build | 4.01x | ≤ 5x | PASS | hashes 976678 → 3918833 (4.01x, limit 5.00x) |
| scale | wide-struct check | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | wide-struct fmt | 3.95x | ≤ 5x | PASS | allocs 2544 → 10056 (3.95x, limit 5.00x) |
| scale | wide-struct build | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | call-args check | 4.00x | ≤ 5x | PASS | scope_probes 1100 → 4400 (4.00x, limit 5.00x) |
| scale | call-args fmt | 4.10x | ≤ 10x | PASS | alloc_bytes 361569 → 1482933 (4.10x, limit 10.00x) |
| scale | call-args build | 4.03x | ≤ 5x | PASS | hashes 24228 → 97536 (4.03x, limit 5.00x) |
| scale | binary-chain check | 4.00x | ≤ 5x | PASS | hashes 2500 → 10000 (4.00x, limit 5.00x) |
| scale | binary-chain fmt | 3.78x | ≤ 5x | PASS | allocs 545 → 2060 (3.78x, limit 5.00x) |
| scale | binary-chain build | 4.04x | ≤ 5x | PASS | hashes 36420 → 147021 (4.04x, limit 5.00x) |
| scale | interpolation check | 4.00x | ≤ 5x | PASS | trait_lookup 2501 → 10001 (4.00x, limit 5.00x) |
| scale | interpolation fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | interpolation build | 4.03x | ≤ 5x | PASS | hashes 4498404 → 18146301 (4.03x, limit 5.00x) |
| scale | list-literal check | 4.00x | ≤ 5x | PASS | hashes 7392 → 29580 (4.00x, limit 5.00x) |
| scale | list-literal fmt | 3.82x | ≤ 5x | PASS | allocs 533 → 2038 (3.82x, limit 5.00x) |
| scale | list-literal build | 4.11x | ≤ 5x | PASS | hashes 60401 → 248397 (4.11x, limit 5.00x) |
| scale | method-chain check | 4.00x | ≤ 5x | PASS | trait_lookup 500 → 2000 (4.00x, limit 5.00x) |
| scale | method-chain fmt | 4.04x | ≤ 10x | PASS | alloc_bytes 838129 → 3386711 (4.04x, limit 10.00x) |
| scale | method-chain build | 4.00x | ≤ 5x | PASS | trait_lookup 500 → 2000 (4.00x, limit 5.00x) |
| scale | pipe-chain check | 4.00x | ≤ 5x | PASS | scope_probes 500 → 2000 (4.00x, limit 5.00x) |
| scale | pipe-chain fmt | 3.77x | ≤ 5x | PASS | allocs 546 → 2056 (3.77x, limit 5.00x) |
| scale | pipe-chain build | 4.00x | ≤ 5x | PASS | scope_probes 500 → 2000 (4.00x, limit 5.00x) |
| scale | question-chain check | 4.00x | ≤ 5x | PASS | scope_probes 500 → 2000 (4.00x, limit 5.00x) |
| scale | question-chain fmt | 3.94x | ≤ 5x | PASS | allocs 1533 → 6044 (3.94x, limit 5.00x) |
| scale | question-chain build | 4.00x | ≤ 5x | PASS | scope_probes 500 → 2000 (4.00x, limit 5.00x) |
| scale | if-nest check | 3.92x | ≤ 15.11x | PASS | hashes 772 → 3028 (3.92x, limit 15.11x) |
| scale | if-nest fmt | - | ≤ 15.11x | PASS | no counter reached the floor |
| scale | if-nest build | 4.08x | ≤ 15.11x | PASS | hashes 6586 → 26885 (4.08x, limit 15.11x) |
| scale | block-nest check | 3.95x | ≤ 15.29x | PASS | hashes 830 → 3278 (3.95x, limit 15.29x) |
| scale | block-nest fmt | - | ≤ 15.29x | PASS | no counter reached the floor |
| scale | block-nest build | 4.05x | ≤ 15.29x | PASS | hashes 7116 → 28786 (4.05x, limit 15.29x) |
| scale | for-nest check | 5.55x | ≤ 14.5x | PASS | hashes 3723 → 20659 (5.55x, limit 14.50x) |
| scale | for-nest fmt | - | ≤ 14.5x | PASS | no counter reached the floor |
| scale | for-nest build | 6.30x | ≤ 14.5x | PASS | hashes 35675 → 224873 (6.30x, limit 14.50x) |
| scale | while-nest check | 3.88x | ≤ 15.11x | PASS | hashes 1049 → 4073 (3.88x, limit 15.11x) |
| scale | while-nest fmt | - | ≤ 15.11x | PASS | no counter reached the floor |
| scale | while-nest build | 4.00x | ≤ 15.11x | PASS | control 400 → 1600 (4.00x, limit 15.11x) |
| scale | match-nest check | 4.86x | ≤ 16.03x | PASS | hashes 1872 → 9092 (4.86x, limit 16.03x) |
| scale | match-nest fmt | - | ≤ 16.03x | PASS | no counter reached the floor |
| scale | match-nest build | 4.26x | ≤ 16.03x | PASS | hashes 8078 → 34403 (4.26x, limit 16.03x) |
| scale | closure-nest check | 3.97x | ≤ 11.45x | PASS | allocs 1510 → 5991 (3.97x, limit 11.45x) |
| scale | closure-nest fmt | - | ≤ 11.45x | PASS | no counter reached the floor |
| scale | closure-nest build | 4.00x | ≤ 11.45x | PASS | use_facts 960 → 3840 (4.00x, limit 11.45x) |
| scale | option-nest check | 4.19x | ≤ 5x | PASS | hashes 1816 → 7600 (4.19x, limit 5.00x) |
| scale | option-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | option-nest build | 4.11x | ≤ 5x | PASS | hashes 13258 → 54443 (4.11x, limit 5.00x) |
| scale | list-type-nest check | 4.29x | ≤ 5x | PASS | hashes 1249 → 5360 (4.29x, limit 5.00x) |
| scale | list-type-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | list-type-nest build | 4.00x | ≤ 5x | PASS | hashes 6032 → 24142 (4.00x, limit 5.00x) |
| scale | generic-nest check | 4.32x | ≤ 5x | PASS | hashes 1604 → 6926 (4.32x, limit 5.00x) |
| scale | generic-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | generic-nest build | 4.51x | ≤ 5x | PASS | hashes 6912 → 31195 (4.51x, limit 5.00x) |
| scale | paren-nest check | - | ≤ 5x | PASS | no counter reached the floor |
| scale | paren-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | paren-nest build | 3.81x | ≤ 5x | PASS | allocs 796 → 3029 (3.81x, limit 5.00x) |
| scale | arith-nest check | 4.00x | ≤ 5x | PASS | hashes 440 → 1760 (4.00x, limit 5.00x) |
| scale | arith-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | arith-nest build | 4.17x | ≤ 5x | PASS | hashes 7570 → 31563 (4.17x, limit 5.00x) |
| scale | bool-nest check | 3.95x | ≤ 5x | PASS | hashes 391 → 1543 (3.95x, limit 5.00x) |
| scale | bool-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | bool-nest build | 4.09x | ≤ 5x | PASS | use_facts 871 → 3559 (4.09x, limit 5.00x) |
| scale | bit-nest check | 3.99x | ≤ 5x | PASS | hashes 818 → 3266 (3.99x, limit 5.00x) |
| scale | bit-nest fmt | 5.84x | ≤ 10x | PASS | alloc_bytes 206985 → 1209058 (5.84x, limit 10.00x) |
| scale | bit-nest build | 4.24x | ≤ 5x | PASS | hashes 11191 → 47455 (4.24x, limit 5.00x) |
| scale | tuple-nest check | 3.96x | ≤ 5x | PASS | hashes 2048 → 8112 (3.96x, limit 5.00x) |
| scale | tuple-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | tuple-nest build | 4.04x | ≤ 5x | PASS | hashes 13024 → 52616 (4.04x, limit 5.00x) |
| scale | tuple-nest-string check | 3.93x | ≤ 5x | PASS | hashes 2069 → 8133 (3.93x, limit 5.00x) |
| scale | tuple-nest-string fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | tuple-nest-string build | 4.31x | ≤ 5x | PASS | control 406 → 1750 (4.31x, limit 5.00x) |
| scale | if-expr-nest check | 3.76x | ≤ 5x | PASS | hashes 322 → 1210 (3.76x, limit 5.00x) |
| scale | if-expr-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | if-expr-nest build | 3.83x | ≤ 5x | PASS | use_facts 644 → 2468 (3.83x, limit 5.00x) |
| scale | call-nest check | 3.87x | ≤ 5x | PASS | hashes 568 → 2200 (3.87x, limit 5.00x) |
| scale | call-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | call-nest build | 2.88x | ≤ 5x | PASS | hashes 5273 → 15176 (2.88x, limit 5.00x) |
| scale | index-nest check | - | ≤ 5x | PASS | no counter reached the floor |
| scale | index-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | index-nest build | 4.06x | ≤ 5x | PASS | hashes 3252 → 13210 (4.06x, limit 5.00x) |
| scale | fn-nest check | 3.95x | ≤ 13.42x | PASS | hashes 2950 → 11642 (3.95x, limit 13.42x) |
| scale | fn-nest fmt | - | ≤ 13.42x | PASS | no counter reached the floor |
| scale | fn-nest build | 4.05x | ≤ 13.42x | PASS | hashes 13102 → 53014 (4.05x, limit 13.42x) |
| scale | module-path-nest check | 5.08x | ≤ 14.36x | PASS | hashes 35284 → 179108 (5.08x, limit 14.36x) |
| scale | module-path-nest fmt | 9.99x | ≤ 28.72x | PASS | alloc_bytes 158233 → 1580610 (9.99x, limit 28.72x) |
| scale | module-path-nest build | 7.55x | ≤ 14.36x | PASS | hashes 1730 → 13066 (7.55x, limit 14.36x) |
| scale | coverage: node kinds | 122/122 | 122/122 | PASS | exempt: Error |
| scale | coverage: node kinds repeated | 113/122 | reported | PASS | in no repeat shape: ReturnStmt, RangeExpr, LogicalOrExpr, LogicalAndExpr, BitwiseOrExpr, BitwiseXorExpr, BitwiseAndExpr, ShiftExpr, WhileExpr; exempt: Error |
| scale | coverage: node kinds nested | 59/122 | reported | PASS | in no nest shape: EnumDecl, TraitDecl, ImplDecl, TypeDecl, TestsBlock, Decorator, DefaultValue, ConstParameter, LifetimeParameter, BoundList, WhereClause, WhereBound, TypeDefault, ConstArgument, AssociatedTypeBinding, NamedArgument, EmbeddedField, EnumVariant, VariantSharedData, AssociatedTypeDecl, Derivation, DiscardStmt, BreakStmt, ContinueStmt, UsingStmt, ElseIfClause, MatchGuard, PipeExpr, RefExpr, DerefExpr, PowerExpr, TryExpr, SuspendExpr, PathExpr, TypeArgsExpr, TrailingCallExpr, PlaceholderExpr, MapExpr, MapEntry, SpreadExpr, ComprehensionExpr, ComprehensionFor, ComprehensionIf, ContextExpr, ContextEntry, Interpolation, LiteralPattern, NamedPattern, DataPattern, DataPatternField, SpreadPattern, RangePattern, DynType, ImplType, RefType, UnionType, ParenType, FunctionType, ProjectionType, ContextType, RestType, InferType, MarkerBound; exempt: Error |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 4 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 38 of 38 safe |
| fmt | hd fmt, 10k lines | 33 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 0.89x | ≤ 1.30x | PASS | checked 384 ms, unchecked 431 ms |
| dev-speed | geomean dev / optimized | 1.65x | ≤ 4.00x | PASS | fib 1.00x, sum 2.42x, string-build 1.94x, map 1.17x, sort 2.23x |
| dev-speed | worst case dev / optimized | 2.42x | ≤ 10.00x | PASS | sum |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 0.0 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 22.1 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.16 CPU-s, 56.9 MB | completes ≤ 30 s | PASS | wall 175 ms |
| resources | check, small, warm | 0.00 CPU-s, 22.0 MB | completes ≤ 30 s | PASS | wall 9.4 ms |
| resources | test, small, cold | 0.81 CPU-s, 118.1 MB | completes ≤ 30 s | PASS | wall 817 ms |
| resources | test, small, warm | 0.02 CPU-s, 42.7 MB | completes ≤ 30 s | PASS | wall 32 ms |
| resources | build, small, cold | 0.64 CPU-s, 102.5 MB | completes ≤ 30 s | PASS | wall 646 ms |
| resources | build, small, warm | 0.00 CPU-s, 22.4 MB | completes ≤ 30 s | PASS | wall 9.3 ms |
| resources | check, 10k, cold | 0.37 CPU-s, 90.9 MB | completes ≤ 30 s | PASS | wall 385 ms |
| resources | check, 10k, warm | 0.00 CPU-s, 22.1 MB | completes ≤ 30 s | PASS | wall 13 ms |
| resources | test, 10k, cold | 1.32 CPU-s, 159.0 MB | completes ≤ 30 s | PASS | wall 1339 ms |
| resources | test, 10k, warm | 0.05 CPU-s, 60.3 MB | completes ≤ 30 s | PASS | wall 72 ms |
| resources | build, 10k, cold | 0.63 CPU-s, 105.0 MB | completes ≤ 30 s | PASS | wall 652 ms |
| resources | build, 10k, warm | 0.00 CPU-s, 22.1 MB | completes ≤ 30 s | PASS | wall 11 ms |
| resources | check, 50k, cold | 1.29 CPU-s, 238.7 MB | completes ≤ 150 s | PASS | wall 1308 ms |
| resources | check, 50k, warm | 0.00 CPU-s, 24.2 MB | completes ≤ 150 s | PASS | wall 15 ms |
| resources | test, 50k, cold | 3.60 CPU-s, 386.4 MB | completes ≤ 150 s | PASS | wall 3604 ms |
| resources | test, 50k, warm | 0.19 CPU-s, 119.6 MB | completes ≤ 150 s | PASS | wall 251 ms |
| resources | build, 50k, cold | 0.68 CPU-s, 110.7 MB | completes ≤ 150 s | PASS | wall 692 ms |
| resources | build, 50k, warm | 0.00 CPU-s, 24.4 MB | completes ≤ 150 s | PASS | wall 15 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 116.7 MB | ≤ 22.0 MB | FAIL | RSS 85.5 MB at input 100, 191.4 MB at input 1000; build per input 118 ms at 50, 1775 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 5.3 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 9.6 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd, just written: wall time | 8.9 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd check empty.hd, just written: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, just written: peak RSS | 21.8 MB | ≤ 10.0 MB | FAIL |  |
| startup | hd check empty.hd, settled: wall time | 8.2 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd check empty.hd, settled: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, settled: peak RSS | 21.5 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 1.81x | ≤ 1.50x | FAIL | p95 137 ms at N = 1; 4 cores; total CPU 400 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 2.60x | ≤ 6.00x | PASS | p95 137 ms at N = 1; 4 cores; total CPU 770 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 0.47 | &lt; 1.00 (sublinear) | PASS | 210 ms at N = 1, 770 ms at N = 16 |
| disk | artifacts per worktree | 0.0 MB | ≤ 10.0 MB | PASS | 10 files; HD_CACHE 12.5 MB, shared per user, not counted |
| disk | toolchain, once per machine | 34.9 MB | recorded, no target | PASS | toolchain files: /home/runner/work/hd-lang/hd-lang/target/dist/hd |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 2.09x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 301 files | the files of one check's cache | FAIL | one check alone: 36 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 13.2 MB | ≤ 5.0 MB | FAIL | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 1 | ≤ 1 | PASS | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 69k/s | ≥ 100k/s | FAIL | 6000 extra cases took 86 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 11 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 62 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.0 ms | ≤ 1.0 ms | PASS | 100 tests: 28 ms |
| integration-test-perf | setup per integration test program | 9.8 ms | ≤ 20 ms | PASS | 10 programs: 119 ms for 21 tests, 5.7 ms per test |
| integration-test-perf | total time growth in programs | 0.902 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 24 ms at 2, 101 ms at 10 programs |
| integration-test-perf | cost per doc test | 5.8 ms | ≤ 20 ms | PASS | 10 doc tests: 80 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 0.44x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.79x | ≤ 3.00x | PASS | hd 6.4 ms (p10 6.3, p90 6.5), Node 8.1 ms, Python 85 ms; release build, run as hd run |
| runtime | sum: hd / Node | 0.18x | ≤ 3.00x | PASS | hd 9.9 ms (p10 8.5, p90 15), Node 54 ms, Python 259 ms; release build, run as hd run |
| runtime | string-build: hd / Node | 0.42x | ≤ 3.00x | PASS | hd 2.7 ms (p10 2.7, p90 2.8), Node 6.3 ms, Python 11 ms; release build, run as hd run |
| runtime | map: hd / Node | 0.58x | ≤ 3.00x | PASS | hd 4.4 ms (p10 4.3, p90 4.4), Node 7.6 ms, Python 15 ms; release build, run as hd run |
| runtime | sort: hd / Node | 0.49x | ≤ 3.00x | PASS | hd 11 ms (p10 11, p90 11), Node 22 ms, Python 16 ms; release build, run as hd run |
| allocations | counted loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | list loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | map loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | iterator chain, bytes per iteration | - | ≤ 1 allocation (≤ 32 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| size-startup-heap | tiny program, release Wasm size | - | ≤ 2048 B | N/A | not supported by this hd: the build wrote no .wasm file (release build, run as hd run) |
| size-startup-heap | tiny program, start to first output | 8.8 ms | ≤ 5.0 ms | FAIL | release build, run as hd run Timing change ≤10%: noise. |
| size-startup-heap | peak heap, map: hd / Node | 0.53x | ≤ 2.00x | PASS | hd +7.3 MB, Node +13.9 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.16x | ≤ 2.00x | PASS | hd +3.1 MB, Node +19.7 MB over an empty program |
| host-call-overhead | Console println, per call | 1.9 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd run |
| host-call-overhead | Fs read_text!, per call | 5.5 µs | ≤ 10 µs | PASS | 10000 calls; release build, run as hd run |
| host-call-overhead | serde boundary (list_dir!), per call | 9.0 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd run |
| suspension-overhead | await of a ready value, per await | 5.6 ns | ≤ 100 ns | PASS | 5.6 ns per iteration; release build, run as hd run |
| suspension-overhead | all! of two ready tasks, throughput | 12.6M/s | ≥ 1.0M/s | PASS | 159 ns per iteration; release build, run as hd run |
| suspension-overhead | race! of two ready tasks, throughput | 8.4M/s | ≥ 1.0M/s | PASS | 239 ns per iteration; release build, run as hd run |
| serde-throughput | JSON encode, hd / Node MB/s | 0.10x | ≥ 0.50x | FAIL | hd 59 MB/s (45488 B document), Node 571 MB/s; release build, run as hd run |
| serde-throughput | JSON decode, hd / Node MB/s | 0.21x | ≥ 0.50x | FAIL | hd 41 MB/s (45488 B document), Node 197 MB/s; release build, run as hd run |
| text-throughput | string build, hd / Node MB/s | 3.97x | ≥ 0.50x | PASS | hd 517 MB/s, Node 130 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd run |
| text-throughput | split, hd / Node MB/s | 0.93x | ≥ 0.50x | PASS | hd 370 MB/s, Node 400 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd run |
| text-throughput | regex find_all, hd / Node MB/s | 0.45x | ≥ 0.50x | FAIL | hd 41 MB/s, Node 90 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd run |
| dead-code | Wasm bytes per 1,000 lines | - | ≤ 10240 B | FAIL | the build wrote no file to measure (release build, run as hd run) |
| dead-code | no unused std in the program | - | none | FAIL | the build wrote no file to measure (release build, run as hd run) |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 15218000 requests; RSS 41.6 MB after warm-up, 41.6 MB at the end; release build, run as hd run |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 9463 of 9463 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

- [2026-11-27T21](daily/2026-11-27T21.json)
- [2026-11-27T06](daily/2026-11-27T06.json)
- [2026-11-26T21](daily/2026-11-26T21.json)
- [2026-11-26T06](daily/2026-11-26T06.json)
- [2026-11-25T21](daily/2026-11-25T21.json)
- [2026-11-25T06](daily/2026-11-25T06.json)
- [2026-11-24T22](daily/2026-11-24T22.json)
- [2026-11-24T07](daily/2026-11-24T07.json)
- [2026-11-24T00](daily/2026-11-24T00.json)
- [2026-11-23T04](daily/2026-11-23T04.json)
- [2026-11-22T19](daily/2026-11-22T19.json)
- [2026-11-22T04](daily/2026-11-22T04.json)
- [2026-11-21T19](daily/2026-11-21T19.json)
- [2026-11-21T05](daily/2026-11-21T05.json)
- [2026-11-20T20](daily/2026-11-20T20.json)
- [2026-11-20T05](daily/2026-11-20T05.json)
- [2026-11-19T20](daily/2026-11-19T20.json)
- [2026-11-19T05](daily/2026-11-19T05.json)
- [2026-11-18T20](daily/2026-11-18T20.json)
- [2026-11-18T05](daily/2026-11-18T05.json)
- [2026-11-17T21](daily/2026-11-17T21.json)
- [2026-11-17T05](daily/2026-11-17T05.json)
- [2026-11-16T22](daily/2026-11-16T22.json)
- [2026-11-16T03](daily/2026-11-16T03.json)
- [2026-11-15T19](daily/2026-11-15T19.json)
- [2026-11-15T03](daily/2026-11-15T03.json)
- [2026-11-14T18](daily/2026-11-14T18.json)
- [2026-11-14T04](daily/2026-11-14T04.json)
