# Daily goal metrics

Finished: 2026-11-23T04:57:07.072Z. Commit: a53c740c08bfbd8a0f48b492ffbd6c8123ec6827.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 7763 64-Core Processor).
Compiler: target/dist/hd.

286 passed, 21 failed, 4 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 20 ms → pass 24 ms
- cold-check: median cold check, 10k lines: pass 273 ms → pass 357 ms
- test-latency: median edit-then-test: pass 91 ms → pass 109 ms
- pathological: overlapping-from: time at 4x: pass 40 ms → pass 55 ms
- pathological: deep-blocks: time at 4x: pass 19 ms → pass 23 ms
- pathological: deep-parens: time at 4x: pass 19 ms → pass 21 ms
- pathological: deep-parens: peak RSS at 4x: pass 39.1 MB → pass 34.4 MB
- pathological: method-chain: time at 4x: pass 22 ms → pass 27 ms
- pathological: iterator-chain: time at 4x: pass 25 ms → pass 29 ms
- pathological: wide-list-literal: time at 4x: pass 38 ms → pass 54 ms
- pathological: large-enum-match: time at 4x: pass 27 ms → pass 41 ms
- pathological: deep-generic: time at 4x: pass 18 ms → pass 23 ms
- pathological: deep-generic: peak RSS at 4x: pass 34.2 MB → pass 39.2 MB
- pathological: question-chain: time at 4x: pass 23 ms → pass 31 ms
- pathological: big-file: time at 4x: pass 106 ms → pass 150 ms
- scale: coverage: node kinds: fail 121/122 → pass 122/122
- fmt: hd fmt, 10k lines: pass 23 ms → pass 38 ms
- dev-speed: worst case dev / optimized: pass 3.66x → pass 2.75x
- startup: hd help (no version command): wall time: pass 4.9 ms → pass 5.4 ms
- startup: hd check empty.hd, just written: wall time: pass 6.9 ms → pass 9.2 ms
- startup: hd check empty.hd, settled: wall time: pass 6.0 ms → pass 8.0 ms
- concurrency: p95 latency at N = 4 vs N = 1: pass 1.44x → fail 1.60x
- concurrency: p95 latency at N = 16 vs N = 1: pass 2.65x → pass 2.28x
- concurrency: total CPU growth exponent, N = 1 to 16: pass 0.44 → pass 0.38
- proptest-perf: property cases per second: pass 131k/s → fail 88k/s
- proptest-perf: time to shrink a known failure: pass 6.8 ms → pass 21 ms
- unit-test-perf: 1,000 unit tests, warm: pass 48 ms → pass 68 ms
- integration-test-perf: setup per integration test program: pass 8.7 ms → pass 11 ms
- integration-test-perf: cost per doc test: pass 4.6 ms → pass 6.0 ms
- runtime: geomean hd / Node: pass 0.52x → pass 0.60x
- runtime: sum: hd / Node: pass 0.14x → pass 0.18x
- runtime: string-build: hd / Node: pass 0.77x → pass 0.85x
- runtime: map: hd / Node: pass 0.70x → pass 1.18x
- size-startup-heap: tiny program, start to first output: fail 7.4 ms → fail 8.7 ms
- size-startup-heap: peak heap, string-build: hd / Node: pass 0.36x → pass 0.52x
- host-call-overhead: Console println, per call: fail 1.4 µs → fail 2.9 µs
- host-call-overhead: Fs read_text!, per call: pass 3.8 µs → fail 11 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 6.8 µs → fail 19 µs
- suspension-overhead: all! of two ready tasks, throughput: pass 14.0M/s → pass 8.4M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 9.0M/s → pass 5.5M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.11x → fail 0.16x
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.17x → fail 0.20x
- text-throughput: string build, hd / Node MB/s: pass 1.29x → pass 1.49x
- text-throughput: split, hd / Node MB/s: pass 0.81x → pass 0.72x
- text-throughput: regex find_all, hd / Node MB/s: fail 0.31x → fail 0.25x
- conformance: listed known failures: fail 23 → pass 0

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 24 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 24 ms | ≤ 200 ms | PASS | Timing change ≤10%: noise. |
| cold-check | median cold check, 10k lines | 357 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 109 ms | ≤ 300 ms | PASS |  |
| mistakes | programs with exactly one diagnostic | 100.0% | ≥ 95.0% | PASS | 69 of 69 |
| mistakes | one diagnostic with the expected code | 100.0% | ≥ 95.0% | PASS | 69 of 69 |
| mistakes | fix-it makes hd check pass | 52.2% | ≥ 80.0% | FAIL | 36 of 69 resolved; 36 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 85 tokens | ≤ 60 tokens | FAIL | largest: interpolate-optional; median 45 tokens, 178 B; 56 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 55 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 45.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 38 → 55 ms, start-up 21 ms |
| pathological | deep-blocks: time at 4x | 23 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 39.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 23 ms, start-up 21 ms |
| pathological | deep-parens: time at 4x | 21 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 34.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 26 → 21 ms, start-up 21 ms |
| pathological | method-chain: time at 4x | 27 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 39.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 26 → 27 ms, start-up 21 ms |
| pathological | iterator-chain: time at 4x | 29 ms | ≤ 2000 ms | PASS |  |
| pathological | iterator-chain: peak RSS at 4x | 38.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | iterator-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 25 → 29 ms, start-up 21 ms |
| pathological | wide-list-literal: time at 4x | 54 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 45.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 37 → 54 ms, start-up 21 ms |
| pathological | large-enum-match: time at 4x | 41 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 40.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 28 → 41 ms, start-up 21 ms |
| pathological | deep-generic: time at 4x | 23 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 39.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 21 → 23 ms, start-up 21 ms |
| pathological | question-chain: time at 4x | 31 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 40.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 25 → 31 ms, start-up 21 ms |
| pathological | big-file: time at 4x | 150 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 85.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 0.98 | ≤ 1.30 | PASS | 2x→4x; 86 → 150 ms, start-up 21 ms |
| scale | fn-items check | 4.03x | ≤ 5x | PASS | hashes 194894 → 786334 (4.03x, limit 5.00x) |
| scale | fn-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | fn-items build | 4.02x | ≤ 5x | PASS | hashes 506180 → 2033509 (4.02x, limit 5.00x) |
| scale | struct-items check | 4.03x | ≤ 5x | PASS | hashes 205865 → 830493 (4.03x, limit 5.00x) |
| scale | struct-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | struct-items build | 4.02x | ≤ 5x | PASS | hashes 621293 → 2495392 (4.02x, limit 5.00x) |
| scale | enum-items check | 4.00x | ≤ 5x | PASS | scope_probes 23500 → 94000 (4.00x, limit 5.00x) |
| scale | enum-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | enum-items build | 4.00x | ≤ 5x | PASS | scope_probes 23500 → 94000 (4.00x, limit 5.00x) |
| scale | trait-items check | 4.00x | ≤ 5x | PASS | scope_probes 19500 → 78000 (4.00x, limit 5.00x) |
| scale | trait-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | trait-items build | 4.02x | ≤ 5x | PASS | control 147802 → 594802 (4.02x, limit 5.00x) |
| scale | impl-methods check | 4.00x | ≤ 5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 5.00x) |
| scale | impl-methods fmt | 3.97x | ≤ 5x | PASS | allocs 4045 → 16057 (3.97x, limit 5.00x) |
| scale | impl-methods build | 4.00x | ≤ 5x | PASS | hashes 699493 → 2800530 (4.00x, limit 5.00x) |
| scale | traits-of-one-struct check | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | traits-of-one-struct fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | traits-of-one-struct build | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | impls-of-one-trait check | 4.01x | ≤ 5x | PASS | hashes 582040 → 2331708 (4.01x, limit 5.00x) |
| scale | impls-of-one-trait fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | impls-of-one-trait build | 4.00x | ≤ 5x | PASS | hashes 1423231 → 5698358 (4.00x, limit 5.00x) |
| scale | operator-impls check | 4.00x | ≤ 5x | PASS | scope_probes 25000 → 100000 (4.00x, limit 5.00x) |
| scale | operator-impls fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | operator-impls build | 4.00x | ≤ 5x | PASS | scope_probes 25000 → 100000 (4.00x, limit 5.00x) |
| scale | derive-items check | 4.00x | ≤ 5x | PASS | trait_lookup 16998 → 67998 (4.00x, limit 5.00x) |
| scale | derive-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | derive-items build | 4.00x | ≤ 5x | PASS | trait_lookup 16998 → 67998 (4.00x, limit 5.00x) |
| scale | dyn-values check | 4.01x | ≤ 5x | PASS | hashes 762404 → 3054645 (4.01x, limit 5.00x) |
| scale | dyn-values fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | dyn-values build | 4.00x | ≤ 5x | PASS | hashes 1859342 → 7441188 (4.00x, limit 5.00x) |
| scale | consts check | 4.00x | ≤ 5x | PASS | type_nodes 1000 → 4000 (4.00x, limit 5.00x) |
| scale | consts fmt | 3.89x | ≤ 5x | PASS | allocs 2087 → 8109 (3.89x, limit 5.00x) |
| scale | consts build | 4.00x | ≤ 5x | PASS | type_nodes 1000 → 4000 (4.00x, limit 5.00x) |
| scale | local-items check | 4.00x | ≤ 5x | PASS | hashes 556242 → 2225842 (4.00x, limit 5.00x) |
| scale | local-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | local-items build | 4.00x | ≤ 5x | PASS | scope_probes 19000 → 76000 (4.00x, limit 5.00x) |
| scale | syntax-items check | 4.00x | ≤ 5x | PASS | item_body_checks 1700 → 6800 (4.00x, limit 5.00x) |
| scale | syntax-items fmt | 4.00x | ≤ 5x | PASS | hashes 17960 → 71864 (4.00x, limit 5.00x) |
| scale | syntax-items build | 4.00x | ≤ 5x | PASS | item_body_checks 1700 → 6800 (4.00x, limit 5.00x) |
| scale | struct-chain check | 4.04x | ≤ 5x | PASS | hashes 153698 → 621262 (4.04x, limit 5.00x) |
| scale | struct-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | struct-chain-values check | 4.02x | ≤ 5x | PASS | hashes 115649 → 464725 (4.02x, limit 5.00x) |
| scale | struct-chain-values fmt | 4.02x | ≤ 5x | PASS | hashes 2082 → 8370 (4.02x, limit 5.00x) |
| scale | struct-chain-values build | 4.00x | ≤ 5x | PASS | scope_probes 4700 → 18800 (4.00x, limit 5.00x) |
| scale | modules check | 4.00x | ≤ 5x | PASS | hashes 1171715 → 4690503 (4.00x, limit 5.00x) |
| scale | modules fmt | 4.00x | ≤ 5x | PASS | hashes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | modules build | 4.00x | ≤ 5x | PASS | hashes 1488637 → 5958937 (4.00x, limit 5.00x) |
| scale | use-names check | 4.04x | ≤ 5x | PASS | hashes 223272 → 900912 (4.04x, limit 5.00x) |
| scale | use-names fmt | 4.01x | ≤ 5x | PASS | hashes 3783 → 15159 (4.01x, limit 5.00x) |
| scale | use-names build | 4.01x | ≤ 5x | PASS | hashes 535271 → 2148984 (4.01x, limit 5.00x) |
| scale | generic-fns check | 4.03x | ≤ 5x | PASS | hashes 299488 → 1207756 (4.03x, limit 5.00x) |
| scale | generic-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-fns build | 4.01x | ≤ 5x | PASS | hashes 941962 → 3777679 (4.01x, limit 5.00x) |
| scale | generic-params check | 4.02x | ≤ 5x | PASS | hashes 15620 → 62722 (4.02x, limit 5.00x) |
| scale | generic-params fmt | 4.10x | ≤ 10x | PASS | alloc_bytes 350594 → 1438988 (4.10x, limit 10.00x) |
| scale | generic-params build | 4.00x | ≤ 5x | PASS | scope_probes 800 → 3200 (4.00x, limit 5.00x) |
| scale | param-bounds check | 4.00x | ≤ 5x | PASS | scope_probes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | param-bounds fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | param-bounds build | 4.00x | ≤ 5x | PASS | scope_probes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | type-defaults check | 4.00x | ≤ 5x | PASS | scope_probes 30500 → 122000 (4.00x, limit 5.00x) |
| scale | type-defaults fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | type-defaults build | 4.03x | ≤ 5x | PASS | control 136918 → 552418 (4.03x, limit 5.00x) |
| scale | alias-chain check | 4.00x | ≤ 5x | PASS | type_nodes 3399 → 13599 (4.00x, limit 5.00x) |
| scale | alias-chain fmt | 4.03x | ≤ 5x | PASS | hashes 1234 → 4978 (4.03x, limit 5.00x) |
| scale | alias-chain build | 4.00x | ≤ 5x | PASS | type_nodes 3399 → 13599 (4.00x, limit 5.00x) |
| scale | row-chain check | 4.03x | ≤ 5x | PASS | hashes 221512 → 892940 (4.03x, limit 5.00x) |
| scale | row-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | row-chain build | 4.02x | ≤ 5x | PASS | hashes 582260 → 2339352 (4.02x, limit 5.00x) |
| scale | row-traits check | 4.00x | ≤ 5x | PASS | scope_probes 2100 → 8400 (4.00x, limit 5.00x) |
| scale | row-traits fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | row-traits build | 4.00x | ≤ 5x | PASS | scope_probes 2100 → 8400 (4.00x, limit 5.00x) |
| scale | closures check | 4.00x | ≤ 5x | PASS | local_body_checks 1000 → 4000 (4.00x, limit 5.00x) |
| scale | closures fmt | 3.94x | ≤ 5x | PASS | allocs 3061 → 12073 (3.94x, limit 5.00x) |
| scale | closures build | 4.00x | ≤ 5x | PASS | local_body_checks 1000 → 4000 (4.00x, limit 5.00x) |
| scale | generic-local-fns check | 4.03x | ≤ 5x | PASS | hashes 239736 → 966164 (4.03x, limit 5.00x) |
| scale | generic-local-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-local-fns build | 4.01x | ≤ 5x | PASS | hashes 944897 → 3791473 (4.01x, limit 5.00x) |
| scale | let-statements check | 3.96x | ≤ 5x | PASS | allocs 25356 → 100467 (3.96x, limit 5.00x) |
| scale | let-statements fmt | 3.94x | ≤ 5x | PASS | allocs 2554 → 10066 (3.94x, limit 5.00x) |
| scale | let-statements build | 4.00x | ≤ 5x | PASS | control 68499 → 273999 (4.00x, limit 5.00x) |
| scale | matches check | 4.00x | ≤ 5x | PASS | decision_rows 6000 → 24000 (4.00x, limit 5.00x) |
| scale | matches fmt | 3.97x | ≤ 5x | PASS | allocs 6057 → 24069 (3.97x, limit 5.00x) |
| scale | matches build | 4.06x | ≤ 5x | PASS | hashes 918002 → 3727561 (4.06x, limit 5.00x) |
| scale | enum-arms check | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | enum-arms fmt | 3.91x | ≤ 5x | PASS | allocs 1551 → 6065 (3.91x, limit 5.00x) |
| scale | enum-arms build | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | tuple-patterns check | 4.00x | ≤ 5x | PASS | decision_rows 1000 → 4000 (4.00x, limit 5.00x) |
| scale | tuple-patterns fmt | 3.95x | ≤ 5x | PASS | allocs 3054 → 12066 (3.95x, limit 5.00x) |
| scale | tuple-patterns build | 4.00x | ≤ 5x | PASS | hashes 425973 → 1704670 (4.00x, limit 5.00x) |
| scale | range-patterns check | 4.02x | ≤ 5x | PASS | hashes 251639 → 1012355 (4.02x, limit 5.00x) |
| scale | range-patterns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | range-patterns build | 4.01x | ≤ 5x | PASS | hashes 841830 → 3377015 (4.01x, limit 5.00x) |
| scale | wide-struct check | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | wide-struct fmt | 3.95x | ≤ 5x | PASS | allocs 2544 → 10056 (3.95x, limit 5.00x) |
| scale | wide-struct build | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | call-args check | 4.00x | ≤ 5x | PASS | scope_probes 1100 → 4400 (4.00x, limit 5.00x) |
| scale | call-args fmt | 4.10x | ≤ 10x | PASS | alloc_bytes 361569 → 1482933 (4.10x, limit 10.00x) |
| scale | call-args build | 4.01x | ≤ 5x | PASS | hashes 18965 → 76075 (4.01x, limit 5.00x) |
| scale | binary-chain check | 4.00x | ≤ 5x | PASS | hashes 2500 → 10000 (4.00x, limit 5.00x) |
| scale | binary-chain fmt | 3.78x | ≤ 5x | PASS | allocs 545 → 2060 (3.78x, limit 5.00x) |
| scale | binary-chain build | 4.03x | ≤ 5x | PASS | hashes 24506 → 98822 (4.03x, limit 5.00x) |
| scale | interpolation check | 4.00x | ≤ 5x | PASS | trait_lookup 2501 → 10001 (4.00x, limit 5.00x) |
| scale | interpolation fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | interpolation build | 4.03x | ≤ 5x | PASS | hashes 3403827 → 13731152 (4.03x, limit 5.00x) |
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
| scale | question-chain build | 3.99x | ≤ 5x | PASS | loan_queries 4010 → 16010 (3.99x, limit 5.00x) |
| scale | if-nest check | 3.92x | ≤ 15.11x | PASS | hashes 756 → 2964 (3.92x, limit 15.11x) |
| scale | if-nest fmt | - | ≤ 15.11x | PASS | no counter reached the floor |
| scale | if-nest build | 4.03x | ≤ 15.11x | PASS | hashes 5373 → 21645 (4.03x, limit 15.11x) |
| scale | block-nest check | 3.95x | ≤ 15.29x | PASS | hashes 814 → 3214 (3.95x, limit 15.29x) |
| scale | block-nest fmt | - | ≤ 15.29x | PASS | no counter reached the floor |
| scale | block-nest build | 4.00x | ≤ 15.29x | PASS | hashes 5873 → 23465 (4.00x, limit 15.29x) |
| scale | for-nest check | 5.20x | ≤ 14.5x | PASS | hashes 3571 → 18571 (5.20x, limit 14.50x) |
| scale | for-nest fmt | - | ≤ 14.5x | PASS | no counter reached the floor |
| scale | for-nest build | 4.66x | ≤ 14.5x | PASS | hashes 25896 → 120792 (4.66x, limit 14.50x) |
| scale | while-nest check | 3.88x | ≤ 15.11x | PASS | hashes 1033 → 4009 (3.88x, limit 15.11x) |
| scale | while-nest fmt | - | ≤ 15.11x | PASS | no counter reached the floor |
| scale | while-nest build | 4.00x | ≤ 15.11x | PASS | use_facts 1232 → 4928 (4.00x, limit 15.11x) |
| scale | match-nest check | 4.76x | ≤ 16.03x | PASS | hashes 1896 → 9020 (4.76x, limit 16.03x) |
| scale | match-nest fmt | - | ≤ 16.03x | PASS | no counter reached the floor |
| scale | match-nest build | 4.26x | ≤ 16.03x | PASS | hashes 7304 → 31129 (4.26x, limit 16.03x) |
| scale | closure-nest check | 3.97x | ≤ 11.45x | PASS | hashes 1932 → 7668 (3.97x, limit 11.45x) |
| scale | closure-nest fmt | - | ≤ 11.45x | PASS | no counter reached the floor |
| scale | closure-nest build | 4.00x | ≤ 11.45x | PASS | use_facts 560 → 2240 (4.00x, limit 11.45x) |
| scale | option-nest check | 4.18x | ≤ 5x | PASS | hashes 1832 → 7664 (4.18x, limit 5.00x) |
| scale | option-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | option-nest build | 4.15x | ≤ 5x | PASS | hashes 10741 → 44537 (4.15x, limit 5.00x) |
| scale | list-type-nest check | 4.27x | ≤ 5x | PASS | hashes 1255 → 5354 (4.27x, limit 5.00x) |
| scale | list-type-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | list-type-nest build | 4.19x | ≤ 5x | PASS | hashes 4879 → 20420 (4.19x, limit 5.00x) |
| scale | generic-nest check | 4.33x | ≤ 5x | PASS | hashes 1587 → 6870 (4.33x, limit 5.00x) |
| scale | generic-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | generic-nest build | 4.05x | ≤ 5x | PASS | allocs 6642 → 26899 (4.05x, limit 5.00x) |
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
| scale | bit-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | bit-nest build | 4.12x | ≤ 5x | PASS | hashes 7529 → 31055 (4.12x, limit 5.00x) |
| scale | tuple-nest check | 4.07x | ≤ 5x | PASS | hashes 2012 → 8180 (4.07x, limit 5.00x) |
| scale | tuple-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | tuple-nest build | 4.04x | ≤ 5x | PASS | hashes 10793 → 43645 (4.04x, limit 5.00x) |
| scale | if-expr-nest check | 3.76x | ≤ 5x | PASS | hashes 322 → 1210 (3.76x, limit 5.00x) |
| scale | if-expr-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | if-expr-nest build | 3.82x | ≤ 5x | PASS | use_facts 357 → 1365 (3.82x, limit 5.00x) |
| scale | call-nest check | 3.90x | ≤ 5x | PASS | hashes 529 → 2065 (3.90x, limit 5.00x) |
| scale | call-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | call-nest build | 3.28x | ≤ 5x | PASS | hashes 4328 → 14192 (3.28x, limit 5.00x) |
| scale | index-nest check | - | ≤ 5x | PASS | no counter reached the floor |
| scale | index-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | index-nest build | 4.68x | ≤ 5x | PASS | hashes 1690 → 7915 (4.68x, limit 5.00x) |
| scale | fn-nest check | 3.81x | ≤ 13.42x | PASS | hashes 2970 → 11306 (3.81x, limit 13.42x) |
| scale | fn-nest fmt | - | ≤ 13.42x | PASS | no counter reached the floor |
| scale | fn-nest build | 4.00x | ≤ 13.42x | PASS | use_facts 896 → 3584 (4.00x, limit 13.42x) |
| scale | module-path-nest check | 1.50x | ≤ 5x | PASS | allocs 2484 → 3722 (1.50x, limit 5.00x) |
| scale | module-path-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | module-path-nest build | 1.53x | ≤ 5x | PASS | allocs 2570 → 3925 (1.53x, limit 5.00x) |
| scale | coverage: node kinds | 122/122 | 122/122 | PASS | exempt: Error |
| scale | coverage: node kinds repeated | 113/122 | reported | PASS | in no repeat shape: ReturnStmt, RangeExpr, LogicalOrExpr, LogicalAndExpr, BitwiseOrExpr, BitwiseXorExpr, BitwiseAndExpr, ShiftExpr, WhileExpr; exempt: Error |
| scale | coverage: node kinds nested | 57/122 | reported | PASS | in no nest shape: UseGroup, UseItem, EnumDecl, TraitDecl, ImplDecl, TypeDecl, TestsBlock, Decorator, DefaultValue, ConstParameter, LifetimeParameter, BoundList, WhereClause, WhereBound, TypeDefault, ConstArgument, AssociatedTypeBinding, NamedArgument, EmbeddedField, EnumVariant, VariantSharedData, AssociatedTypeDecl, Derivation, DiscardStmt, BreakStmt, ContinueStmt, UsingStmt, ElseIfClause, MatchGuard, PipeExpr, RefExpr, DerefExpr, PowerExpr, TryExpr, SuspendExpr, PathExpr, TypeArgsExpr, TrailingCallExpr, PlaceholderExpr, MapExpr, MapEntry, SpreadExpr, ComprehensionExpr, ComprehensionFor, ComprehensionIf, ContextExpr, ContextEntry, Interpolation, LiteralPattern, NamedPattern, DataPattern, DataPatternField, SpreadPattern, RangePattern, DynType, ImplType, RefType, UnionType, ParenType, FunctionType, ProjectionType, ContextType, RestType, InferType, MarkerBound; exempt: Error |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 36 of 36 safe |
| fmt | hd fmt, 10k lines | 38 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.83x | ≤ 1.30x | FAIL | checked 378 ms, unchecked 207 ms |
| dev-speed | geomean dev / optimized | 1.68x | ≤ 4.00x | PASS | fib 1.16x, sum 2.00x, string-build 1.73x, map 1.22x, sort 2.75x |
| dev-speed | worst case dev / optimized | 2.75x | ≤ 10.00x | PASS | sort |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 0.0 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 21.5 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.15 CPU-s, 55.8 MB | completes ≤ 30 s | PASS | wall 157 ms |
| resources | check, small, warm | 0.00 CPU-s, 21.2 MB | completes ≤ 30 s | PASS | wall 10 ms |
| resources | test, small, cold | 0.87 CPU-s, 116.1 MB | completes ≤ 30 s | PASS | wall 883 ms |
| resources | test, small, warm | 0.02 CPU-s, 39.1 MB | completes ≤ 30 s | PASS | wall 34 ms |
| resources | build, small, cold | 0.69 CPU-s, 101.3 MB | completes ≤ 30 s | PASS | wall 710 ms |
| resources | build, small, warm | 0.00 CPU-s, 21.5 MB | completes ≤ 30 s | PASS | wall 8.7 ms |
| resources | check, 10k, cold | 0.35 CPU-s, 85.8 MB | completes ≤ 30 s | PASS | wall 363 ms |
| resources | check, 10k, warm | 0.00 CPU-s, 21.5 MB | completes ≤ 30 s | PASS | wall 12 ms |
| resources | test, 10k, cold | 1.42 CPU-s, 164.0 MB | completes ≤ 30 s | PASS | wall 1432 ms |
| resources | test, 10k, warm | 0.06 CPU-s, 56.9 MB | completes ≤ 30 s | PASS | wall 71 ms |
| resources | build, 10k, cold | 0.74 CPU-s, 107.3 MB | completes ≤ 30 s | PASS | wall 754 ms |
| resources | build, 10k, warm | 0.00 CPU-s, 21.6 MB | completes ≤ 30 s | PASS | wall 11 ms |
| resources | check, 50k, cold | 1.27 CPU-s, 227.3 MB | completes ≤ 150 s | PASS | wall 1283 ms |
| resources | check, 50k, warm | 0.00 CPU-s, 23.3 MB | completes ≤ 150 s | PASS | wall 17 ms |
| resources | test, 50k, cold | 3.88 CPU-s, 362.7 MB | completes ≤ 150 s | PASS | wall 3880 ms |
| resources | test, 50k, warm | 0.23 CPU-s, 113.9 MB | completes ≤ 150 s | PASS | wall 236 ms |
| resources | build, 50k, cold | 0.88 CPU-s, 131.4 MB | completes ≤ 150 s | PASS | wall 898 ms |
| resources | build, 50k, warm | 0.00 CPU-s, 23.6 MB | completes ≤ 150 s | PASS | wall 16 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 99.9 MB | ≤ 22.0 MB | FAIL | RSS 84.7 MB at input 100, 184.7 MB at input 1000; build per input 116 ms at 50, 1547 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 5.4 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 9.6 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd, just written: wall time | 9.2 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, just written: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, just written: peak RSS | 20.8 MB | ≤ 10.0 MB | FAIL |  |
| startup | hd check empty.hd, settled: wall time | 8.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, settled: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, settled: peak RSS | 20.7 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 1.60x | ≤ 1.50x | FAIL | p95 135 ms at N = 1; 4 cores; total CPU 270 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 2.28x | ≤ 6.00x | PASS | p95 135 ms at N = 1; 4 cores; total CPU 540 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 0.38 | &lt; 1.00 (sublinear) | PASS | 190 ms at N = 1, 540 ms at N = 16 |
| disk | artifacts per worktree | 0.0 MB | ≤ 10.0 MB | PASS | 10 files; HD_CACHE 12.4 MB, shared per user, not counted |
| disk | toolchain, once per machine | 32.5 MB | recorded, no target | PASS | toolchain files: /home/runner/work/hd-lang/hd-lang/target/dist/hd |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 2.05x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 216 files | the files of one check's cache | FAIL | one check alone: 36 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 15.6 MB | ≤ 5.0 MB | FAIL | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 1 | ≤ 1 | PASS | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 88k/s | ≥ 100k/s | FAIL | 6000 extra cases took 69 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 21 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 68 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.0 ms | ≤ 1.0 ms | PASS | 100 tests: 25 ms |
| integration-test-perf | setup per integration test program | 11 ms | ≤ 20 ms | PASS | 10 programs: 132 ms for 21 tests, 6.3 ms per test |
| integration-test-perf | total time growth in programs | 0.900 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 27 ms at 2, 115 ms at 10 programs |
| integration-test-perf | cost per doc test | 6.0 ms | ≤ 20 ms | PASS | 10 doc tests: 77 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 0.60x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.74x | ≤ 3.00x | PASS | hd 6.9 ms (p10 6.8, p90 7.0), Node 9.2 ms, Python 98 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 0.18x | ≤ 3.00x | PASS | hd 9.4 ms (p10 9.3, p90 10), Node 53 ms, Python 275 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.85x | ≤ 3.00x | PASS | hd 5.3 ms (p10 5.2, p90 5.3), Node 6.2 ms, Python 16 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 1.18x | ≤ 3.00x | PASS | hd 8.6 ms (p10 8.5, p90 11), Node 7.3 ms, Python 17 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.61x | ≤ 3.00x | PASS | hd 15 ms (p10 15, p90 15), Node 24 ms, Python 19 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | list loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | map loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | iterator chain, bytes per iteration | - | ≤ 1 allocation (≤ 32 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| size-startup-heap | tiny program, release Wasm size | 5017 B | ≤ 2048 B | FAIL | benchmarks: fib 5141 B, sum 4917 B, string-build 7836 B, map 7095 B, sort 13047 B |
| size-startup-heap | tiny program, start to first output | 8.7 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 1.00x | ≤ 2.00x | PASS | hd +14.2 MB, Node +14.2 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.52x | ≤ 2.00x | PASS | hd +10.3 MB, Node +20.0 MB over an empty program |
| host-call-overhead | Console println, per call | 2.9 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 11 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 19 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 17 ns | ≤ 100 ns | PASS | 17 ns per iteration; release build, run as hd FILE.wasm Timing change ≤10%: noise. |
| suspension-overhead | all! of two ready tasks, throughput | 8.4M/s | ≥ 1.0M/s | PASS | 239 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 5.5M/s | ≥ 1.0M/s | PASS | 361 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.16x | ≥ 0.50x | FAIL | hd 75 MB/s (45488 B document), Node 456 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.20x | ≥ 0.50x | FAIL | hd 30 MB/s (45488 B document), Node 151 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 1.49x | ≥ 0.50x | PASS | hd 175 MB/s, Node 117 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 0.72x | ≥ 0.50x | PASS | hd 272 MB/s, Node 378 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.25x | ≥ 0.50x | FAIL | hd 24 MB/s, Node 96 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 6516 B | ≤ 10240 B | PASS | 12607 B at 1075 lines, 71255 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 915 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 11116000 requests; RSS 89.6 MB after warm-up, 89.6 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 9227 of 9227 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
- [2026-11-11T03](daily/2026-11-11T03.json)
- [2026-11-10T19](daily/2026-11-10T19.json)
- [2026-11-10T05](daily/2026-11-10T05.json)
- [2026-11-09T22](daily/2026-11-09T22.json)
