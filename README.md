# Daily goal metrics

Finished: 2026-11-25T06:15:39.187Z. Commit: ecc9f92e65d2eea4ee125013121b947a8db06713.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V45 96-Core Processor).
Compiler: target/dist/hd.

287 passed, 21 failed, 4 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- cold-check: median cold check, 10k lines: pass 314 ms → pass 252 ms
- test-latency: median edit-then-test: pass 107 ms → pass 79 ms
- pathological: deep-parens: time at 4x: pass 18 ms → pass 20 ms
- pathological: large-enum-match: time at 4x: pass 26 ms → pass 31 ms
- pathological: deep-generic: time at 4x: pass 19 ms → pass 26 ms
- pathological: question-chain: time at 4x: pass 26 ms → pass 23 ms
- pathological: big-file: time at 4x: pass 109 ms → pass 89 ms
- scale: module-path-nest check: pass 3.66x → pass 4.30x
- check-cost: checked run time / unchecked run time: fail 1.94x → fail 2.35x
- dev-speed: geomean dev / optimized: pass 1.79x → pass 1.57x
- dev-speed: worst case dev / optimized: pass 3.65x → pass 3.13x
- startup: hd help (no version command): wall time: pass 4.5 ms → pass 5.2 ms
- startup: hd check empty.hd, just written: wall time: pass 6.1 ms → pass 7.7 ms
- startup: hd check empty.hd, settled: wall time: pass 5.7 ms → pass 6.9 ms
- concurrency: p95 latency at N = 16 vs N = 1: pass 2.31x → pass 3.11x
- concurrency: total CPU growth exponent, N = 1 to 16: pass 0.32 → pass 0.48
- parallel-speedup: speedup, 50k lines, 4 threads vs 1: fail 2.10x → fail 1.82x
- integration-test-perf: setup per integration test program: pass 8.2 ms → pass 6.4 ms
- integration-test-perf: total time growth in programs: pass 0.954 → pass 0.804
- integration-test-perf: cost per doc test: pass 5.3 ms → pass 3.5 ms
- runtime: fib: hd / Node: pass 0.80x → pass 0.67x
- runtime: sort: hd / Node: pass 0.66x → pass 0.56x
- size-startup-heap: tiny program, start to first output: fail 6.5 ms → fail 7.5 ms
- size-startup-heap: peak heap, string-build: hd / Node: pass 0.24x → pass 0.49x
- host-call-overhead: Console println, per call: fail 1.3 µs → fail 2.2 µs
- host-call-overhead: Fs read_text!, per call: pass 3.5 µs → pass 8.1 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 6.7 µs → fail 13 µs
- suspension-overhead: await of a ready value, per await: pass 12 ns → pass 2.4 ns
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.19x → fail 0.14x
- text-throughput: string build, hd / Node MB/s: pass 1.26x → pass 1.79x
- text-throughput: regex find_all, hd / Node MB/s: fail 0.29x → fail 0.25x

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 22 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 25 ms | ≤ 200 ms | PASS | Timing change ≤10%: noise. |
| cold-check | median cold check, 10k lines | 252 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 79 ms | ≤ 300 ms | PASS |  |
| mistakes | programs with exactly one diagnostic | 100.0% | ≥ 95.0% | PASS | 69 of 69 |
| mistakes | one diagnostic with the expected code | 100.0% | ≥ 95.0% | PASS | 69 of 69 |
| mistakes | fix-it makes hd check pass | 53.6% | ≥ 80.0% | FAIL | 37 of 69 resolved; 37 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 74 tokens | ≤ 60 tokens | FAIL | largest: interpolate-optional; median 35 tokens, 139 B; 63 of 69 within |
| mistakes | largest diagnostic JSON (bytes / 4) | 197 tokens | ≤ 60 tokens | FAIL | largest: interpolate-optional, 787 B; median 76 tokens, 303 B; 69 diagnostics |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 159 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 39 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | overlapping-from: peak RSS at 4x | 45.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 28 → 39 ms, start-up 20 ms |
| pathological | deep-blocks: time at 4x | 20 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-blocks: peak RSS at 4x | 41.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 20 ms, start-up 20 ms |
| pathological | deep-parens: time at 4x | 20 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 41.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 20 ms, start-up 20 ms |
| pathological | method-chain: time at 4x | 22 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | method-chain: peak RSS at 4x | 41.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 22 ms, start-up 20 ms |
| pathological | iterator-chain: time at 4x | 24 ms | ≤ 2000 ms | PASS |  |
| pathological | iterator-chain: peak RSS at 4x | 41.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | iterator-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 25 → 24 ms, start-up 20 ms |
| pathological | wide-list-literal: time at 4x | 36 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | wide-list-literal: peak RSS at 4x | 47.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 28 → 36 ms, start-up 20 ms |
| pathological | large-enum-match: time at 4x | 31 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 43.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 21 → 31 ms, start-up 20 ms |
| pathological | deep-generic: time at 4x | 26 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 42.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 23 → 26 ms, start-up 20 ms |
| pathological | question-chain: time at 4x | 23 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 43.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 23 ms, start-up 20 ms |
| pathological | big-file: time at 4x | 89 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 87.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 55 → 89 ms, start-up 20 ms |
| scale | fn-items check | 4.03x | ≤ 5x | PASS | hashes 200029 → 806961 (4.03x, limit 5.00x) |
| scale | fn-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | fn-items build | 4.02x | ≤ 5x | PASS | hashes 511087 → 2052516 (4.02x, limit 5.00x) |
| scale | struct-items check | 4.05x | ≤ 5x | PASS | hashes 203329 → 823905 (4.05x, limit 5.00x) |
| scale | struct-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | struct-items build | 4.01x | ≤ 5x | PASS | hashes 624643 → 2506266 (4.01x, limit 5.00x) |
| scale | enum-items check | 4.00x | ≤ 5x | PASS | scope_probes 23500 → 94000 (4.00x, limit 5.00x) |
| scale | enum-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | enum-items build | 4.00x | ≤ 5x | PASS | hashes 1386371 → 5548607 (4.00x, limit 5.00x) |
| scale | trait-items check | 4.00x | ≤ 5x | PASS | scope_probes 19500 → 78000 (4.00x, limit 5.00x) |
| scale | trait-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | trait-items build | 4.01x | ≤ 5x | PASS | hashes 1780326 → 7143419 (4.01x, limit 5.00x) |
| scale | impl-methods check | 4.00x | ≤ 5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 5.00x) |
| scale | impl-methods fmt | 3.97x | ≤ 5x | PASS | allocs 4045 → 16057 (3.97x, limit 5.00x) |
| scale | impl-methods build | 4.01x | ≤ 5x | PASS | hashes 704019 → 2824092 (4.01x, limit 5.00x) |
| scale | traits-of-one-struct check | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | traits-of-one-struct fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | traits-of-one-struct build | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | impls-of-one-trait check | 4.01x | ≤ 5x | PASS | hashes 605789 → 2428433 (4.01x, limit 5.00x) |
| scale | impls-of-one-trait fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | impls-of-one-trait build | 4.01x | ≤ 5x | PASS | hashes 1446631 → 5794910 (4.01x, limit 5.00x) |
| scale | operator-impls check | 4.00x | ≤ 5x | PASS | hashes 738087 → 2955971 (4.00x, limit 5.00x) |
| scale | operator-impls fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | operator-impls build | 4.00x | ≤ 5x | PASS | scope_probes 25000 → 100000 (4.00x, limit 5.00x) |
| scale | derive-items check | 4.00x | ≤ 5x | PASS | trait_lookup 16998 → 67998 (4.00x, limit 5.00x) |
| scale | derive-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | derive-items build | 4.00x | ≤ 5x | PASS | trait_lookup 16998 → 67998 (4.00x, limit 5.00x) |
| scale | dyn-values check | 4.01x | ≤ 5x | PASS | hashes 786010 → 3151211 (4.01x, limit 5.00x) |
| scale | dyn-values fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | dyn-values build | 4.01x | ≤ 5x | PASS | hashes 1884932 → 7549550 (4.01x, limit 5.00x) |
| scale | consts check | 3.97x | ≤ 5x | PASS | hashes 63462 → 251818 (3.97x, limit 5.00x) |
| scale | consts fmt | 3.89x | ≤ 5x | PASS | allocs 2087 → 8109 (3.89x, limit 5.00x) |
| scale | consts build | 4.00x | ≤ 5x | PASS | loan_queries 4506 → 18006 (4.00x, limit 5.00x) |
| scale | local-items check | 4.00x | ≤ 5x | PASS | scope_probes 19000 → 76000 (4.00x, limit 5.00x) |
| scale | local-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | local-items build | 4.00x | ≤ 5x | PASS | scope_probes 19000 → 76000 (4.00x, limit 5.00x) |
| scale | syntax-items check | 4.00x | ≤ 5x | PASS | item_body_checks 1700 → 6800 (4.00x, limit 5.00x) |
| scale | syntax-items fmt | 4.00x | ≤ 5x | PASS | hashes 17960 → 71864 (4.00x, limit 5.00x) |
| scale | syntax-items build | 4.00x | ≤ 5x | PASS | item_body_checks 1700 → 6800 (4.00x, limit 5.00x) |
| scale | struct-chain check | 4.04x | ≤ 5x | PASS | hashes 151890 → 613946 (4.04x, limit 5.00x) |
| scale | struct-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | struct-chain-values check | 4.03x | ≤ 5x | PASS | hashes 117409 → 472969 (4.03x, limit 5.00x) |
| scale | struct-chain-values fmt | 4.02x | ≤ 5x | PASS | hashes 2082 → 8370 (4.02x, limit 5.00x) |
| scale | struct-chain-values build | 4.00x | ≤ 5x | PASS | scope_probes 4700 → 18800 (4.00x, limit 5.00x) |
| scale | modules check | 4.00x | ≤ 5x | PASS | hashes 5773039 → 23096023 (4.00x, limit 5.00x) |
| scale | modules fmt | 4.00x | ≤ 5x | PASS | hashes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | modules build | 4.00x | ≤ 5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 5.00x) |
| scale | use-names check | 4.04x | ≤ 5x | PASS | hashes 217192 → 876816 (4.04x, limit 5.00x) |
| scale | use-names fmt | 4.01x | ≤ 5x | PASS | hashes 3783 → 15159 (4.01x, limit 5.00x) |
| scale | use-names build | 4.00x | ≤ 5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 5.00x) |
| scale | generic-fns check | 4.02x | ≤ 5x | PASS | hashes 305908 → 1229724 (4.02x, limit 5.00x) |
| scale | generic-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-fns build | 4.01x | ≤ 5x | PASS | hashes 948260 → 3801581 (4.01x, limit 5.00x) |
| scale | generic-params check | 4.03x | ≤ 5x | PASS | hashes 15846 → 63843 (4.03x, limit 5.00x) |
| scale | generic-params fmt | 4.10x | ≤ 10x | PASS | alloc_bytes 350594 → 1438988 (4.10x, limit 10.00x) |
| scale | generic-params build | 4.03x | ≤ 5x | PASS | hashes 20492 → 82505 (4.03x, limit 5.00x) |
| scale | param-bounds check | 4.00x | ≤ 5x | PASS | scope_probes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | param-bounds fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | param-bounds build | 4.00x | ≤ 5x | PASS | scope_probes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | type-defaults check | 4.00x | ≤ 5x | PASS | scope_probes 30500 → 122000 (4.00x, limit 5.00x) |
| scale | type-defaults fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | type-defaults build | 4.01x | ≤ 5x | PASS | control 138017 → 553517 (4.01x, limit 5.00x) |
| scale | alias-chain check | 4.00x | ≤ 5x | PASS | type_nodes 3399 → 13599 (4.00x, limit 5.00x) |
| scale | alias-chain fmt | 4.03x | ≤ 5x | PASS | hashes 1234 → 4978 (4.03x, limit 5.00x) |
| scale | alias-chain build | 4.00x | ≤ 5x | PASS | type_nodes 3399 → 13599 (4.00x, limit 5.00x) |
| scale | row-chain check | 4.03x | ≤ 5x | PASS | hashes 223372 → 899624 (4.03x, limit 5.00x) |
| scale | row-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | row-chain build | 4.01x | ≤ 5x | PASS | hashes 583869 → 2344187 (4.01x, limit 5.00x) |
| scale | row-traits check | 4.00x | ≤ 5x | PASS | scope_probes 2100 → 8400 (4.00x, limit 5.00x) |
| scale | row-traits fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | row-traits build | 4.00x | ≤ 5x | PASS | scope_probes 2100 → 8400 (4.00x, limit 5.00x) |
| scale | closures check | 4.00x | ≤ 5x | PASS | local_body_checks 1000 → 4000 (4.00x, limit 5.00x) |
| scale | closures fmt | 3.94x | ≤ 5x | PASS | allocs 3061 → 12073 (3.94x, limit 5.00x) |
| scale | closures build | 4.00x | ≤ 5x | PASS | local_body_checks 1000 → 4000 (4.00x, limit 5.00x) |
| scale | generic-local-fns check | 4.03x | ≤ 5x | PASS | hashes 237875 → 958683 (4.03x, limit 5.00x) |
| scale | generic-local-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-local-fns build | 4.01x | ≤ 5x | PASS | hashes 945041 → 3790161 (4.01x, limit 5.00x) |
| scale | let-statements check | 3.96x | ≤ 5x | PASS | allocs 25349 → 100450 (3.96x, limit 5.00x) |
| scale | let-statements fmt | 3.94x | ≤ 5x | PASS | allocs 2554 → 10066 (3.94x, limit 5.00x) |
| scale | let-statements build | 4.00x | ≤ 5x | PASS | control 68499 → 273999 (4.00x, limit 5.00x) |
| scale | matches check | 4.00x | ≤ 5x | PASS | decision_rows 6000 → 24000 (4.00x, limit 5.00x) |
| scale | matches fmt | 3.97x | ≤ 5x | PASS | allocs 6057 → 24069 (3.97x, limit 5.00x) |
| scale | matches build | 4.06x | ≤ 5x | PASS | hashes 919016 → 3731575 (4.06x, limit 5.00x) |
| scale | enum-arms check | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | enum-arms fmt | 3.91x | ≤ 5x | PASS | allocs 1551 → 6065 (3.91x, limit 5.00x) |
| scale | enum-arms build | 4.00x | ≤ 5x | PASS | hashes 213698 → 855058 (4.00x, limit 5.00x) |
| scale | tuple-patterns check | 4.00x | ≤ 5x | PASS | hashes 91665 → 367065 (4.00x, limit 5.00x) |
| scale | tuple-patterns fmt | 3.95x | ≤ 5x | PASS | allocs 3054 → 12066 (3.95x, limit 5.00x) |
| scale | tuple-patterns build | 4.01x | ≤ 5x | PASS | hashes 432856 → 1734389 (4.01x, limit 5.00x) |
| scale | range-patterns check | 4.02x | ≤ 5x | PASS | hashes 257718 → 1036982 (4.02x, limit 5.00x) |
| scale | range-patterns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | range-patterns build | 4.01x | ≤ 5x | PASS | hashes 848120 → 3400069 (4.01x, limit 5.00x) |
| scale | wide-struct check | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | wide-struct fmt | 3.95x | ≤ 5x | PASS | allocs 2544 → 10056 (3.95x, limit 5.00x) |
| scale | wide-struct build | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | call-args check | 4.00x | ≤ 5x | PASS | scope_probes 1100 → 4400 (4.00x, limit 5.00x) |
| scale | call-args fmt | 4.10x | ≤ 10x | PASS | alloc_bytes 361569 → 1482933 (4.10x, limit 10.00x) |
| scale | call-args build | 4.00x | ≤ 5x | PASS | control 4999 → 19999 (4.00x, limit 5.00x) |
| scale | binary-chain check | 4.00x | ≤ 5x | PASS | hashes 2500 → 10000 (4.00x, limit 5.00x) |
| scale | binary-chain fmt | 3.78x | ≤ 5x | PASS | allocs 545 → 2060 (3.78x, limit 5.00x) |
| scale | binary-chain build | 4.03x | ≤ 5x | PASS | hashes 24506 → 98822 (4.03x, limit 5.00x) |
| scale | interpolation check | 4.00x | ≤ 5x | PASS | trait_lookup 2501 → 10001 (4.00x, limit 5.00x) |
| scale | interpolation fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | interpolation build | 4.03x | ≤ 5x | PASS | hashes 3404827 → 13735152 (4.03x, limit 5.00x) |
| scale | list-literal check | 4.00x | ≤ 5x | PASS | hashes 7392 → 29580 (4.00x, limit 5.00x) |
| scale | list-literal fmt | 3.82x | ≤ 5x | PASS | allocs 533 → 2038 (3.82x, limit 5.00x) |
| scale | list-literal build | 4.04x | ≤ 5x | PASS | hashes 38074 → 153766 (4.04x, limit 5.00x) |
| scale | method-chain check | 4.00x | ≤ 5x | PASS | trait_lookup 500 → 2000 (4.00x, limit 5.00x) |
| scale | method-chain fmt | 4.04x | ≤ 10x | PASS | alloc_bytes 838129 → 3386711 (4.04x, limit 10.00x) |
| scale | method-chain build | 4.00x | ≤ 5x | PASS | trait_lookup 500 → 2000 (4.00x, limit 5.00x) |
| scale | pipe-chain check | 4.00x | ≤ 5x | PASS | hashes 16017 → 64017 (4.00x, limit 5.00x) |
| scale | pipe-chain fmt | 3.77x | ≤ 5x | PASS | allocs 546 → 2056 (3.77x, limit 5.00x) |
| scale | pipe-chain build | 3.98x | ≤ 5x | PASS | hashes 103697 → 412577 (3.98x, limit 5.00x) |
| scale | question-chain check | 4.00x | ≤ 5x | PASS | hashes 42028 → 168028 (4.00x, limit 5.00x) |
| scale | question-chain fmt | 3.94x | ≤ 5x | PASS | allocs 1533 → 6044 (3.94x, limit 5.00x) |
| scale | question-chain build | 3.99x | ≤ 5x | PASS | hashes 222127 → 886891 (3.99x, limit 5.00x) |
| scale | if-nest check | 3.92x | ≤ 15.11x | PASS | hashes 756 → 2964 (3.92x, limit 15.11x) |
| scale | if-nest fmt | - | ≤ 15.11x | PASS | no counter reached the floor |
| scale | if-nest build | 4.03x | ≤ 15.11x | PASS | hashes 5373 → 21645 (4.03x, limit 15.11x) |
| scale | block-nest check | 3.95x | ≤ 15.29x | PASS | hashes 814 → 3214 (3.95x, limit 15.29x) |
| scale | block-nest fmt | - | ≤ 15.29x | PASS | no counter reached the floor |
| scale | block-nest build | 4.00x | ≤ 15.29x | PASS | hashes 5873 → 23465 (4.00x, limit 15.29x) |
| scale | for-nest check | 5.14x | ≤ 14.5x | PASS | hashes 3627 → 18627 (5.14x, limit 14.50x) |
| scale | for-nest fmt | - | ≤ 14.5x | PASS | no counter reached the floor |
| scale | for-nest build | 4.64x | ≤ 14.5x | PASS | hashes 26105 → 121001 (4.64x, limit 14.50x) |
| scale | while-nest check | 3.88x | ≤ 15.11x | PASS | hashes 1033 → 4009 (3.88x, limit 15.11x) |
| scale | while-nest fmt | - | ≤ 15.11x | PASS | no counter reached the floor |
| scale | while-nest build | 4.00x | ≤ 15.11x | PASS | use_facts 1232 → 4928 (4.00x, limit 15.11x) |
| scale | match-nest check | 4.72x | ≤ 16.03x | PASS | hashes 1984 → 9372 (4.72x, limit 16.03x) |
| scale | match-nest fmt | - | ≤ 16.03x | PASS | no counter reached the floor |
| scale | match-nest build | 4.25x | ≤ 16.03x | PASS | hashes 7448 → 31649 (4.25x, limit 16.03x) |
| scale | closure-nest check | 3.95x | ≤ 11.45x | PASS | hashes 2039 → 8049 (3.95x, limit 11.45x) |
| scale | closure-nest fmt | - | ≤ 11.45x | PASS | no counter reached the floor |
| scale | closure-nest build | 4.00x | ≤ 11.45x | PASS | use_facts 560 → 2240 (4.00x, limit 11.45x) |
| scale | option-nest check | 4.18x | ≤ 5x | PASS | hashes 1832 → 7664 (4.18x, limit 5.00x) |
| scale | option-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | option-nest build | 4.14x | ≤ 5x | PASS | hashes 10764 → 44535 (4.14x, limit 5.00x) |
| scale | list-type-nest check | 4.26x | ≤ 5x | PASS | hashes 1257 → 5360 (4.26x, limit 5.00x) |
| scale | list-type-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | list-type-nest build | - | ≤ 5x | FAIL | control 0 → 2040 (∞x, limit 5.00x); allocs 1306 → 7740 (5.93x, limit 5.00x) |
| scale | generic-nest check | 4.32x | ≤ 5x | PASS | hashes 1590 → 6874 (4.32x, limit 5.00x) |
| scale | generic-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | generic-nest build | 4.21x | ≤ 5x | PASS | hashes 6433 → 27104 (4.21x, limit 5.00x) |
| scale | paren-nest check | - | ≤ 5x | PASS | no counter reached the floor |
| scale | paren-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | paren-nest build | 4.08x | ≤ 5x | PASS | control 795 → 3243 (4.08x, limit 5.00x) |
| scale | arith-nest check | 4.00x | ≤ 5x | PASS | hashes 440 → 1760 (4.00x, limit 5.00x) |
| scale | arith-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | arith-nest build | 4.04x | ≤ 5x | PASS | hashes 5617 → 22688 (4.04x, limit 5.00x) |
| scale | bool-nest check | 3.95x | ≤ 5x | PASS | hashes 391 → 1543 (3.95x, limit 5.00x) |
| scale | bool-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | bool-nest build | 4.09x | ≤ 5x | PASS | use_facts 490 → 2002 (4.09x, limit 5.00x) |
| scale | bit-nest check | 3.99x | ≤ 5x | PASS | hashes 818 → 3266 (3.99x, limit 5.00x) |
| scale | bit-nest fmt | 5.84x | ≤ 10x | PASS | alloc_bytes 206985 → 1209058 (5.84x, limit 10.00x) |
| scale | bit-nest build | 4.12x | ≤ 5x | PASS | hashes 7529 → 31055 (4.12x, limit 5.00x) |
| scale | tuple-nest check | 4.01x | ≤ 5x | PASS | hashes 2070 → 8294 (4.01x, limit 5.00x) |
| scale | tuple-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | tuple-nest build | 4.01x | ≤ 5x | PASS | hashes 10938 → 43874 (4.01x, limit 5.00x) |
| scale | if-expr-nest check | 3.76x | ≤ 5x | PASS | hashes 322 → 1210 (3.76x, limit 5.00x) |
| scale | if-expr-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | if-expr-nest build | 3.82x | ≤ 5x | PASS | use_facts 357 → 1365 (3.82x, limit 5.00x) |
| scale | call-nest check | 3.90x | ≤ 5x | PASS | hashes 529 → 2065 (3.90x, limit 5.00x) |
| scale | call-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | call-nest build | 3.28x | ≤ 5x | PASS | hashes 4328 → 14192 (3.28x, limit 5.00x) |
| scale | index-nest check | - | ≤ 5x | PASS | no counter reached the floor |
| scale | index-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | index-nest build | 4.68x | ≤ 5x | PASS | hashes 1690 → 7915 (4.68x, limit 5.00x) |
| scale | fn-nest check | 3.98x | ≤ 13.42x | PASS | hashes 2946 → 11726 (3.98x, limit 13.42x) |
| scale | fn-nest fmt | - | ≤ 13.42x | PASS | no counter reached the floor |
| scale | fn-nest build | 4.00x | ≤ 13.42x | PASS | use_facts 896 → 3584 (4.00x, limit 13.42x) |
| scale | module-path-nest check | 4.30x | ≤ 14.36x | PASS | allocs 60449 → 260130 (4.30x, limit 14.36x) |
| scale | module-path-nest fmt | 9.99x | ≤ 28.72x | PASS | alloc_bytes 158233 → 1580610 (9.99x, limit 28.72x) |
| scale | module-path-nest build | 5.02x | ≤ 14.36x | PASS | hashes 19715 → 98981 (5.02x, limit 14.36x) |
| scale | coverage: node kinds | 122/122 | 122/122 | PASS | exempt: Error |
| scale | coverage: node kinds repeated | 113/122 | reported | PASS | in no repeat shape: ReturnStmt, RangeExpr, LogicalOrExpr, LogicalAndExpr, BitwiseOrExpr, BitwiseXorExpr, BitwiseAndExpr, ShiftExpr, WhileExpr; exempt: Error |
| scale | coverage: node kinds nested | 59/122 | reported | PASS | in no nest shape: EnumDecl, TraitDecl, ImplDecl, TypeDecl, TestsBlock, Decorator, DefaultValue, ConstParameter, LifetimeParameter, BoundList, WhereClause, WhereBound, TypeDefault, ConstArgument, AssociatedTypeBinding, NamedArgument, EmbeddedField, EnumVariant, VariantSharedData, AssociatedTypeDecl, Derivation, DiscardStmt, BreakStmt, ContinueStmt, UsingStmt, ElseIfClause, MatchGuard, PipeExpr, RefExpr, DerefExpr, PowerExpr, TryExpr, SuspendExpr, PathExpr, TypeArgsExpr, TrailingCallExpr, PlaceholderExpr, MapExpr, MapEntry, SpreadExpr, ComprehensionExpr, ComprehensionFor, ComprehensionIf, ContextExpr, ContextEntry, Interpolation, LiteralPattern, NamedPattern, DataPattern, DataPatternField, SpreadPattern, RangePattern, DynType, ImplType, RefType, UnionType, ParenType, FunctionType, ProjectionType, ContextType, RestType, InferType, MarkerBound; exempt: Error |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 4 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 37 of 37 safe |
| fmt | hd fmt, 10k lines | 21 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 2.35x | ≤ 1.30x | FAIL | checked 274 ms, unchecked 117 ms |
| dev-speed | geomean dev / optimized | 1.57x | ≤ 4.00x | PASS | fib 0.77x, sum 2.12x, string-build 1.62x, map 1.13x, sort 3.13x |
| dev-speed | worst case dev / optimized | 3.13x | ≤ 10.00x | PASS | sort |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 0.0 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 21.7 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.09 CPU-s, 61.9 MB | completes ≤ 30 s | PASS | wall 106 ms |
| resources | check, small, warm | 0.00 CPU-s, 21.5 MB | completes ≤ 30 s | PASS | wall 7.6 ms |
| resources | test, small, cold | 0.55 CPU-s, 126.7 MB | completes ≤ 30 s | PASS | wall 563 ms |
| resources | test, small, warm | 0.01 CPU-s, 42.2 MB | completes ≤ 30 s | PASS | wall 23 ms |
| resources | build, small, cold | 0.48 CPU-s, 107.3 MB | completes ≤ 30 s | PASS | wall 488 ms |
| resources | build, small, warm | 0.00 CPU-s, 21.7 MB | completes ≤ 30 s | PASS | wall 6.8 ms |
| resources | check, 10k, cold | 0.23 CPU-s, 108.0 MB | completes ≤ 30 s | PASS | wall 246 ms |
| resources | check, 10k, warm | 0.00 CPU-s, 21.7 MB | completes ≤ 30 s | PASS | wall 8.9 ms |
| resources | test, 10k, cold | 0.87 CPU-s, 175.6 MB | completes ≤ 30 s | PASS | wall 878 ms |
| resources | test, 10k, warm | 0.03 CPU-s, 57.3 MB | completes ≤ 30 s | PASS | wall 44 ms |
| resources | build, 10k, cold | 0.45 CPU-s, 113.5 MB | completes ≤ 30 s | PASS | wall 461 ms |
| resources | build, 10k, warm | 0.00 CPU-s, 21.7 MB | completes ≤ 30 s | PASS | wall 8.0 ms |
| resources | check, 50k, cold | 0.91 CPU-s, 330.1 MB | completes ≤ 150 s | PASS | wall 928 ms |
| resources | check, 50k, warm | 0.00 CPU-s, 31.0 MB | completes ≤ 150 s | PASS | wall 13 ms |
| resources | test, 50k, cold | 2.40 CPU-s, 459.7 MB | completes ≤ 150 s | PASS | wall 2407 ms |
| resources | test, 50k, warm | 0.13 CPU-s, 116.8 MB | completes ≤ 150 s | PASS | wall 145 ms |
| resources | build, 50k, cold | 0.59 CPU-s, 137.4 MB | completes ≤ 150 s | PASS | wall 603 ms |
| resources | build, 50k, warm | 0.00 CPU-s, 31.2 MB | completes ≤ 150 s | PASS | wall 13 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 103.3 MB | ≤ 22.0 MB | FAIL | RSS 91.2 MB at input 100, 188.7 MB at input 1000; build per input 88 ms at 50, 1264 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 5.2 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 9.8 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd, just written: wall time | 7.7 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, just written: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, just written: peak RSS | 21.1 MB | ≤ 10.0 MB | FAIL |  |
| startup | hd check empty.hd, settled: wall time | 6.9 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, settled: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, settled: peak RSS | 21.0 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 1.69x | ≤ 1.50x | FAIL | p95 92 ms at N = 1; 4 cores; total CPU 210 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 3.11x | ≤ 6.00x | PASS | p95 92 ms at N = 1; 4 cores; total CPU 450 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 0.48 | &lt; 1.00 (sublinear) | PASS | 120 ms at N = 1, 450 ms at N = 16 |
| disk | artifacts per worktree | 0.0 MB | ≤ 10.0 MB | PASS | 10 files; HD_CACHE 13.0 MB, shared per user, not counted |
| disk | toolchain, once per machine | 33.0 MB | recorded, no target | PASS | toolchain files: /home/runner/work/hd-lang/hd-lang/target/dist/hd |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.82x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 314 files | the files of one check's cache | FAIL | one check alone: 36 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 13.4 MB | ≤ 5.0 MB | FAIL | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 1 | ≤ 1 | PASS | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 126k/s | ≥ 100k/s | PASS | 6000 extra cases took 48 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 12 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 Timing change ≤10%: noise. |
| unit-test-perf | 1,000 unit tests, warm | 45 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed Timing change ≤10%: noise. |
| unit-test-perf | overhead per unit test | 0.0 ms | ≤ 1.0 ms | PASS | 100 tests: 19 ms |
| integration-test-perf | setup per integration test program | 6.4 ms | ≤ 20 ms | PASS | 10 programs: 80 ms for 21 tests, 3.8 ms per test |
| integration-test-perf | total time growth in programs | 0.804 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 18 ms at 2, 67 ms at 10 programs |
| integration-test-perf | cost per doc test | 3.5 ms | ≤ 20 ms | PASS | 10 doc tests: 51 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 0.50x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.67x | ≤ 3.00x | PASS | hd 3.5 ms (p10 3.4, p90 3.6), Node 5.2 ms, Python 49 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 0.13x | ≤ 3.00x | PASS | hd 4.7 ms (p10 4.6, p90 4.7), Node 36 ms, Python 155 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.86x | ≤ 3.00x | PASS | hd 2.9 ms (p10 2.8, p90 2.9), Node 3.4 ms, Python 7.9 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 0.77x | ≤ 3.00x | PASS | hd 3.8 ms (p10 3.7, p90 3.9), Node 5.0 ms, Python 10 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.56x | ≤ 3.00x | PASS | hd 7.8 ms (p10 7.5, p90 7.9), Node 14 ms, Python 9.9 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | list loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | map loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | iterator chain, bytes per iteration | - | ≤ 1 allocation (≤ 32 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| size-startup-heap | tiny program, release Wasm size | 5017 B | ≤ 2048 B | FAIL | benchmarks: fib 5141 B, sum 4917 B, string-build 8090 B, map 7095 B, sort 13206 B |
| size-startup-heap | tiny program, start to first output | 7.5 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.94x | ≤ 2.00x | PASS | hd +13.6 MB, Node +14.4 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.49x | ≤ 2.00x | PASS | hd +9.9 MB, Node +20.1 MB over an empty program |
| host-call-overhead | Console println, per call | 2.2 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 8.1 µs | ≤ 10 µs | PASS | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 13 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 2.4 ns | ≤ 100 ns | PASS | 2.4 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 12.3M/s | ≥ 1.0M/s | PASS | 163 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 9.5M/s | ≥ 1.0M/s | PASS | 210 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.13x | ≥ 0.50x | FAIL | hd 115 MB/s (45488 B document), Node 884 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.14x | ≥ 0.50x | FAIL | hd 46 MB/s (45488 B document), Node 337 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 1.79x | ≥ 0.50x | PASS | hd 344 MB/s, Node 193 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 1.19x | ≥ 0.50x | PASS | hd 431 MB/s, Node 361 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.25x | ≥ 0.50x | FAIL | hd 45 MB/s, Node 179 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 6516 B | ≤ 10240 B | PASS | 12607 B at 1075 lines, 71255 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 915 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 19090000 requests; RSS 90.2 MB after warm-up, 90.2 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 9339 of 9339 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
- [2026-11-13T19](daily/2026-11-13T19.json)
- [2026-11-13T04](daily/2026-11-13T04.json)
- [2026-11-12T19](daily/2026-11-12T19.json)
- [2026-11-12T04](daily/2026-11-12T04.json)
- [2026-11-11T19](daily/2026-11-11T19.json)
