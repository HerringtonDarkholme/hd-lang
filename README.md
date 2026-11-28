# Daily goal metrics

Finished: 2026-11-28T05:50:45.425Z. Commit: b8c20d227ef21bb9879893fce1a8b2749cc2c2e3.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 7763 64-Core Processor).
Compiler: target/dist/hd.

290 passed, 23 failed, 5 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 34 ms → pass 30 ms
- edit-latency: p95 check after an edit: pass 46 ms → pass 38 ms
- pathological: overlapping-from: time at 4x: pass 56 ms → pass 62 ms
- pathological: deep-parens: time at 4x: pass 29 ms → pass 25 ms
- pathological: large-enum-match: time at 4x: pass 44 ms → pass 37 ms
- pathological: deep-generic: time at 4x: pass 28 ms → pass 31 ms
- pathological: big-file: time at 4x: pass 142 ms → pass 165 ms
- scale: for-nest build: pass 6.30x → pass 4.13x
- check-cost: checked run time / unchecked run time: pass 0.89x → fail 2.20x
- dev-speed: worst case dev / optimized: pass 2.42x → pass 2.68x
- long-session: RSS growth, REPL inputs 100 to 1000: fail 116.7 MB → fail 684.3 MB
- startup: hd check empty.hd, settled: wall time: pass 8.2 ms → pass 9.3 ms
- proptest-perf: time to shrink a known failure: pass 11 ms → pass 21 ms
- integration-test-perf: setup per integration test program: pass 9.8 ms → pass 8.3 ms
- runtime: sum: hd / Node: pass 0.18x → pass 0.12x
- runtime: string-build: hd / Node: pass 0.42x → pass 0.51x
- runtime: map: hd / Node: pass 0.58x → pass 0.95x
- size-startup-heap: tiny program, start to first output: fail 8.8 ms → fail 11 ms
- size-startup-heap: peak heap, string-build: hd / Node: pass 0.16x → pass 0.20x
- host-call-overhead: Console println, per call: fail 1.9 µs → fail 2.9 µs
- host-call-overhead: Fs read_text!, per call: pass 5.5 µs → fail 10 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 9.0 µs → fail 18 µs
- suspension-overhead: await of a ready value, per await: pass 5.6 ns → pass 0.00 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 12.6M/s → pass 9.5M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.10x → fail 0.12x
- text-throughput: string build, hd / Node MB/s: pass 3.97x → pass 2.94x
- text-throughput: split, hd / Node MB/s: pass 0.93x → pass 1.05x

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 30 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 38 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 383 ms | ≤ 1000 ms | PASS | Timing change ≤10%: noise. |
| test-latency | median edit-then-test | 130 ms | ≤ 300 ms | PASS | Timing change ≤10%: noise. |
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
| pathological | overlapping-from: time at 4x | 62 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 49.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 42 → 62 ms, start-up 26 ms |
| pathological | deep-blocks: time at 4x | 30 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-blocks: peak RSS at 4x | 42.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 25 → 30 ms, start-up 26 ms |
| pathological | deep-parens: time at 4x | 25 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 42.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 27 → 25 ms, start-up 26 ms |
| pathological | method-chain: time at 4x | 34 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 42.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 29 → 34 ms, start-up 26 ms |
| pathological | iterator-chain: time at 4x | 41 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | iterator-chain: peak RSS at 4x | 42.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | iterator-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 31 → 41 ms, start-up 26 ms |
| pathological | wide-list-literal: time at 4x | 56 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | wide-list-literal: peak RSS at 4x | 48.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 45 → 56 ms, start-up 26 ms |
| pathological | large-enum-match: time at 4x | 37 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 43.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 31 → 37 ms, start-up 26 ms |
| pathological | deep-generic: time at 4x | 31 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 42.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 32 → 31 ms, start-up 26 ms |
| pathological | question-chain: time at 4x | 34 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 44.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 31 → 34 ms, start-up 26 ms |
| pathological | big-file: time at 4x | 165 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 88.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.04 | ≤ 1.30 | PASS | 2x→4x; 93 → 165 ms, start-up 26 ms |
| scale | fn-items check | 4.03x | ≤ 5x | PASS | hashes 196701 → 792617 (4.03x, limit 5.00x) |
| scale | fn-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | fn-items build | 4.02x | ≤ 5x | PASS | hashes 584914 → 2351593 (4.02x, limit 5.00x) |
| scale | bang-chain check | 4.03x | ≤ 5x | PASS | hashes 190562 → 767714 (4.03x, limit 5.00x) |
| scale | bang-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | bang-chain build | 4.03x | ≤ 5x | PASS | hashes 532743 → 2145331 (4.03x, limit 5.00x) |
| scale | struct-items check | 4.04x | ≤ 5x | PASS | hashes 198279 → 800719 (4.04x, limit 5.00x) |
| scale | struct-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | struct-items build | 4.01x | ≤ 5x | PASS | hashes 756894 → 3033870 (4.01x, limit 5.00x) |
| scale | enum-items check | 4.00x | ≤ 5x | PASS | hashes 526279 → 2105335 (4.00x, limit 5.00x) |
| scale | enum-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | enum-items build | 4.00x | ≤ 5x | PASS | hashes 1627463 → 6510069 (4.00x, limit 5.00x) |
| scale | trait-items check | 4.01x | ≤ 5x | PASS | allocs 527391 → 2114347 (4.01x, limit 5.00x) |
| scale | trait-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | trait-items build | 4.00x | ≤ 5x | PASS | allocs 1246676 → 4989537 (4.00x, limit 5.00x) |
| scale | impl-methods check | 4.05x | ≤ 5x | PASS | allocs 213752 → 865164 (4.05x, limit 5.00x) |
| scale | impl-methods fmt | 3.97x | ≤ 5x | PASS | allocs 4045 → 16057 (3.97x, limit 5.00x) |
| scale | impl-methods build | 4.01x | ≤ 5x | PASS | hashes 834585 → 3347464 (4.01x, limit 5.00x) |
| scale | traits-of-one-struct check | 4.01x | ≤ 5x | PASS | allocs 402737 → 1613119 (4.01x, limit 5.00x) |
| scale | traits-of-one-struct fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | traits-of-one-struct build | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | impls-of-one-trait check | 4.01x | ≤ 5x | PASS | allocs 376373 → 1510772 (4.01x, limit 5.00x) |
| scale | impls-of-one-trait fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | impls-of-one-trait build | 4.00x | ≤ 5x | PASS | hashes 1678196 → 6719134 (4.00x, limit 5.00x) |
| scale | operator-impls check | 4.01x | ≤ 5x | PASS | allocs 452485 → 1814923 (4.01x, limit 5.00x) |
| scale | operator-impls fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | operator-impls build | 4.00x | ≤ 5x | PASS | scope_probes 25000 → 100000 (4.00x, limit 5.00x) |
| scale | derive-items check | 4.00x | ≤ 5x | PASS | trait_lookup 23498 → 93998 (4.00x, limit 5.00x) |
| scale | derive-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | derive-items build | 4.00x | ≤ 5x | PASS | trait_lookup 23498 → 93998 (4.00x, limit 5.00x) |
| scale | dyn-values check | 4.01x | ≤ 5x | PASS | allocs 458586 → 1837059 (4.01x, limit 5.00x) |
| scale | dyn-values fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | dyn-values build | 4.00x | ≤ 5x | PASS | scope_probes 15500 → 62000 (4.00x, limit 5.00x) |
| scale | consts check | 3.97x | ≤ 5x | PASS | hashes 63624 → 252820 (3.97x, limit 5.00x) |
| scale | consts fmt | 3.89x | ≤ 5x | PASS | allocs 2087 → 8109 (3.89x, limit 5.00x) |
| scale | consts build | 4.00x | ≤ 5x | PASS | loan_queries 4506 → 18006 (4.00x, limit 5.00x) |
| scale | local-items check | 4.00x | ≤ 5x | PASS | scope_probes 21500 → 86000 (4.00x, limit 5.00x) |
| scale | local-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | local-items build | 4.00x | ≤ 5x | PASS | hashes 1512402 → 6050897 (4.00x, limit 5.00x) |
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
| scale | modules build | 4.00x | ≤ 5x | PASS | hashes 1555026 → 6223247 (4.00x, limit 5.00x) |
| scale | use-names check | 4.03x | ≤ 5x | PASS | hashes 217218 → 874714 (4.03x, limit 5.00x) |
| scale | use-names fmt | 4.01x | ≤ 5x | PASS | hashes 3783 → 15159 (4.01x, limit 5.00x) |
| scale | use-names build | 4.01x | ≤ 5x | PASS | hashes 606843 → 2435326 (4.01x, limit 5.00x) |
| scale | generic-fns check | 4.01x | ≤ 5x | PASS | hashes 304157 → 1219973 (4.01x, limit 5.00x) |
| scale | generic-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-fns build | 4.01x | ≤ 5x | PASS | hashes 1064568 → 4271709 (4.01x, limit 5.00x) |
| scale | generic-params check | 4.03x | ≤ 5x | PASS | hashes 15773 → 63509 (4.03x, limit 5.00x) |
| scale | generic-params fmt | 4.10x | ≤ 10x | PASS | alloc_bytes 350594 → 1438988 (4.10x, limit 10.00x) |
| scale | generic-params build | 4.05x | ≤ 5x | PASS | hashes 25102 → 101620 (4.05x, limit 5.00x) |
| scale | param-bounds check | 4.00x | ≤ 5x | PASS | scope_probes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | param-bounds fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | param-bounds build | 4.00x | ≤ 5x | PASS | allocs 37780 → 151244 (4.00x, limit 5.00x) |
| scale | type-defaults check | 4.01x | ≤ 5x | PASS | allocs 690737 → 2768861 (4.01x, limit 5.00x) |
| scale | type-defaults fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | type-defaults build | 4.00x | ≤ 5x | PASS | allocs 1496086 → 5985439 (4.00x, limit 5.00x) |
| scale | alias-chain check | 4.00x | ≤ 5x | PASS | scope_probes 6600 → 26400 (4.00x, limit 5.00x) |
| scale | alias-chain fmt | 4.03x | ≤ 5x | PASS | hashes 1234 → 4978 (4.03x, limit 5.00x) |
| scale | alias-chain build | 4.00x | ≤ 5x | PASS | scope_probes 6600 → 26400 (4.00x, limit 5.00x) |
| scale | row-chain check | 4.03x | ≤ 5x | PASS | hashes 220816 → 889500 (4.03x, limit 5.00x) |
| scale | row-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | row-chain build | 4.02x | ≤ 5x | PASS | hashes 643733 → 2589497 (4.02x, limit 5.00x) |
| scale | row-traits check | 4.00x | ≤ 5x | PASS | scope_probes 2300 → 9200 (4.00x, limit 5.00x) |
| scale | row-traits fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | row-traits build | 4.00x | ≤ 5x | PASS | scope_probes 2300 → 9200 (4.00x, limit 5.00x) |
| scale | closures check | 4.00x | ≤ 5x | PASS | scope_probes 2000 → 8000 (4.00x, limit 5.00x) |
| scale | closures fmt | 3.94x | ≤ 5x | PASS | allocs 3061 → 12073 (3.94x, limit 5.00x) |
| scale | closures build | 4.00x | ≤ 5x | PASS | scope_probes 2000 → 8000 (4.00x, limit 5.00x) |
| scale | generic-local-fns check | 4.03x | ≤ 5x | PASS | hashes 236130 → 952118 (4.03x, limit 5.00x) |
| scale | generic-local-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-local-fns build | 4.01x | ≤ 5x | PASS | hashes 1208078 → 4844825 (4.01x, limit 5.00x) |
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
| scale | range-patterns build | 4.01x | ≤ 5x | PASS | hashes 996450 → 3997981 (4.01x, limit 5.00x) |
| scale | wide-struct check | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | wide-struct fmt | 3.95x | ≤ 5x | PASS | allocs 2544 → 10056 (3.95x, limit 5.00x) |
| scale | wide-struct build | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | call-args check | 4.00x | ≤ 5x | PASS | scope_probes 1100 → 4400 (4.00x, limit 5.00x) |
| scale | call-args fmt | 4.10x | ≤ 10x | PASS | alloc_bytes 361569 → 1482933 (4.10x, limit 10.00x) |
| scale | call-args build | 4.03x | ≤ 5x | PASS | hashes 24636 → 99180 (4.03x, limit 5.00x) |
| scale | binary-chain check | 4.00x | ≤ 5x | PASS | hashes 2500 → 10000 (4.00x, limit 5.00x) |
| scale | binary-chain fmt | 3.78x | ≤ 5x | PASS | allocs 545 → 2060 (3.78x, limit 5.00x) |
| scale | binary-chain build | 4.04x | ≤ 5x | PASS | hashes 36420 → 147021 (4.04x, limit 5.00x) |
| scale | interpolation check | 4.00x | ≤ 5x | PASS | trait_lookup 2501 → 10001 (4.00x, limit 5.00x) |
| scale | interpolation fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | interpolation build | 4.03x | ≤ 5x | PASS | hashes 4515942 → 18216339 (4.03x, limit 5.00x) |
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
| scale | for-nest build | 4.13x | ≤ 14.5x | PASS | hashes 32251 → 133116 (4.13x, limit 14.50x) |
| scale | while-nest check | 3.88x | ≤ 15.11x | PASS | hashes 1049 → 4073 (3.88x, limit 15.11x) |
| scale | while-nest fmt | - | ≤ 15.11x | PASS | no counter reached the floor |
| scale | while-nest build | 4.00x | ≤ 15.11x | PASS | control 400 → 1600 (4.00x, limit 15.11x) |
| scale | match-nest check | 4.86x | ≤ 16.03x | PASS | hashes 1872 → 9092 (4.86x, limit 16.03x) |
| scale | match-nest fmt | - | ≤ 16.03x | PASS | no counter reached the floor |
| scale | match-nest build | 4.26x | ≤ 16.03x | PASS | hashes 8078 → 34403 (4.26x, limit 16.03x) |
| scale | closure-nest check | 3.95x | ≤ 11.45x | PASS | allocs 1519 → 6003 (3.95x, limit 11.45x) |
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
| scale | generic-nest build | 4.62x | ≤ 5x | PASS | hashes 7140 → 33012 (4.62x, limit 5.00x) |
| scale | paren-nest check | - | ≤ 5x | PASS | no counter reached the floor |
| scale | paren-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | paren-nest build | 3.79x | ≤ 5x | PASS | allocs 819 → 3106 (3.79x, limit 5.00x) |
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
| scale | call-nest build | 2.88x | ≤ 5x | PASS | hashes 5327 → 15326 (2.88x, limit 5.00x) |
| scale | index-nest check | - | ≤ 5x | PASS | no counter reached the floor |
| scale | index-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | index-nest build | 4.06x | ≤ 5x | PASS | hashes 3252 → 13210 (4.06x, limit 5.00x) |
| scale | fn-nest check | 3.95x | ≤ 13.42x | PASS | hashes 2950 → 11642 (3.95x, limit 13.42x) |
| scale | fn-nest fmt | - | ≤ 13.42x | PASS | no counter reached the floor |
| scale | fn-nest build | 4.05x | ≤ 13.42x | PASS | hashes 13294 → 53782 (4.05x, limit 13.42x) |
| scale | module-path-nest check | 5.08x | ≤ 14.36x | PASS | hashes 35284 → 179108 (5.08x, limit 14.36x) |
| scale | module-path-nest fmt | 9.99x | ≤ 28.72x | PASS | alloc_bytes 158233 → 1580610 (9.99x, limit 28.72x) |
| scale | module-path-nest build | 7.19x | ≤ 14.36x | PASS | hashes 1931 → 13875 (7.19x, limit 14.36x) |
| scale | coverage: node kinds | 122/122 | 122/122 | PASS | exempt: Error |
| scale | coverage: node kinds repeated | 113/122 | reported | PASS | in no repeat shape: ReturnStmt, RangeExpr, LogicalOrExpr, LogicalAndExpr, BitwiseOrExpr, BitwiseXorExpr, BitwiseAndExpr, ShiftExpr, WhileExpr; exempt: Error |
| scale | coverage: node kinds nested | 59/122 | reported | PASS | in no nest shape: EnumDecl, TraitDecl, ImplDecl, TypeDecl, TestsBlock, Decorator, DefaultValue, ConstParameter, LifetimeParameter, BoundList, WhereClause, WhereBound, TypeDefault, ConstArgument, AssociatedTypeBinding, NamedArgument, EmbeddedField, EnumVariant, VariantSharedData, AssociatedTypeDecl, Derivation, DiscardStmt, BreakStmt, ContinueStmt, UsingStmt, ElseIfClause, MatchGuard, PipeExpr, RefExpr, DerefExpr, PowerExpr, TryExpr, SuspendExpr, PathExpr, TypeArgsExpr, TrailingCallExpr, PlaceholderExpr, MapExpr, MapEntry, SpreadExpr, ComprehensionExpr, ComprehensionFor, ComprehensionIf, ContextExpr, ContextEntry, Interpolation, LiteralPattern, NamedPattern, DataPattern, DataPatternField, SpreadPattern, RangePattern, DynType, ImplType, RefType, UnionType, ParenType, FunctionType, ProjectionType, ContextType, RestType, InferType, MarkerBound; exempt: Error |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 4 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 38 of 38 safe |
| fmt | hd fmt, 10k lines | 34 ms | ≤ 200 ms | PASS | Timing change ≤10%: noise. |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 2.20x | ≤ 1.30x | FAIL | checked 382 ms, unchecked 173 ms |
| dev-speed | geomean dev / optimized | 1.54x | ≤ 4.00x | PASS | fib 1.02x, sum 1.34x, string-build 1.89x, map 1.24x, sort 2.68x |
| dev-speed | worst case dev / optimized | 2.68x | ≤ 10.00x | PASS | sort |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 0.0 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 22.3 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.15 CPU-s, 59.4 MB | completes ≤ 30 s | PASS | wall 167 ms |
| resources | check, small, warm | 0.00 CPU-s, 22.1 MB | completes ≤ 30 s | PASS | wall 10 ms |
| resources | test, small, cold | 0.85 CPU-s, 124.5 MB | completes ≤ 30 s | PASS | wall 865 ms |
| resources | test, small, warm | 0.02 CPU-s, 40.6 MB | completes ≤ 30 s | PASS | wall 41 ms |
| resources | build, small, cold | 0.67 CPU-s, 106.6 MB | completes ≤ 30 s | PASS | wall 682 ms |
| resources | build, small, warm | 0.00 CPU-s, 22.4 MB | completes ≤ 30 s | PASS | wall 10 ms |
| resources | check, 10k, cold | 0.37 CPU-s, 91.0 MB | completes ≤ 30 s | PASS | wall 384 ms |
| resources | check, 10k, warm | 0.00 CPU-s, 22.3 MB | completes ≤ 30 s | PASS | wall 11 ms |
| resources | test, 10k, cold | 1.39 CPU-s, 168.9 MB | completes ≤ 30 s | PASS | wall 1398 ms |
| resources | test, 10k, warm | 0.07 CPU-s, 60.6 MB | completes ≤ 30 s | PASS | wall 80 ms |
| resources | build, 10k, cold | 0.68 CPU-s, 107.1 MB | completes ≤ 30 s | PASS | wall 698 ms |
| resources | build, 10k, warm | 0.00 CPU-s, 22.5 MB | completes ≤ 30 s | PASS | wall 12 ms |
| resources | check, 50k, cold | 1.33 CPU-s, 240.9 MB | completes ≤ 150 s | PASS | wall 1338 ms |
| resources | check, 50k, warm | 0.00 CPU-s, 30.1 MB | completes ≤ 150 s | PASS | wall 16 ms |
| resources | test, 50k, cold | 3.76 CPU-s, 393.0 MB | completes ≤ 150 s | PASS | wall 3767 ms |
| resources | test, 50k, warm | 0.26 CPU-s, 121.7 MB | completes ≤ 150 s | PASS | wall 259 ms |
| resources | build, 50k, cold | 0.72 CPU-s, 117.2 MB | completes ≤ 150 s | PASS | wall 741 ms |
| resources | build, 50k, warm | 0.00 CPU-s, 31.6 MB | completes ≤ 150 s | PASS | wall 18 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 684.3 MB | ≤ 22.0 MB | FAIL | RSS 90.0 MB at input 100, 774.3 MB at input 1000; build per input 123 ms at 50, 1343 ms at 1000; 4 KiB pages |
| startup | hd --version: wall time | 6.9 ms | ≤ 20 ms | PASS |  |
| startup | hd --version: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd --version: peak RSS | 9.5 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd, just written: wall time | 9.1 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd check empty.hd, just written: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, just written: peak RSS | 21.7 MB | ≤ 10.0 MB | FAIL |  |
| startup | hd check empty.hd, settled: wall time | 9.3 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, settled: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, settled: peak RSS | 21.4 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 1.73x | ≤ 1.50x | FAIL | p95 134 ms at N = 1; 4 cores; total CPU 360 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 2.85x | ≤ 6.00x | PASS | p95 134 ms at N = 1; 4 cores; total CPU 870 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 0.51 | &lt; 1.00 (sublinear) | PASS | 210 ms at N = 1, 870 ms at N = 16 |
| disk | artifacts per worktree | 0.0 MB | ≤ 10.0 MB | PASS | 10 files; HD_CACHE 13.4 MB, shared per user, not counted |
| disk | toolchain, once per machine | 35.5 MB | recorded, no target | PASS | toolchain files: /home/runner/work/hd-lang/hd-lang/target/dist/hd |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 2.12x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 318 files | the files of one check's cache | FAIL | one check alone: 36 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 13.8 MB | ≤ 5.0 MB | FAIL | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 1 | ≤ 1 | PASS | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 71k/s | ≥ 100k/s | FAIL | 6000 extra cases took 84 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 21 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 66 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed Timing change ≤10%: noise. |
| unit-test-perf | overhead per unit test | 0.0 ms | ≤ 1.0 ms | PASS | 100 tests: 24 ms |
| integration-test-perf | setup per integration test program | 8.3 ms | ≤ 20 ms | PASS | 10 programs: 102 ms for 21 tests, 4.9 ms per test |
| integration-test-perf | total time growth in programs | 0.899 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 20 ms at 2, 85 ms at 10 programs |
| integration-test-perf | cost per doc test | 6.0 ms | ≤ 20 ms | PASS | 10 doc tests: 77 ms; the run reports 11 of 11 passed Timing change ≤10%: noise. |
| runtime | geomean hd / Node | 0.46x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.81x | ≤ 3.00x | PASS | hd 7.5 ms (p10 7.4, p90 8.4), Node 9.2 ms, Python 101 ms; release build, run as hd run |
| runtime | sum: hd / Node | 0.12x | ≤ 3.00x | PASS | hd 6.3 ms (p10 6.1, p90 6.3), Node 53 ms, Python 284 ms; release build, run as hd run |
| runtime | string-build: hd / Node | 0.51x | ≤ 3.00x | PASS | hd 3.4 ms (p10 3.4, p90 3.5), Node 6.7 ms, Python 15 ms; release build, run as hd run |
| runtime | map: hd / Node | 0.95x | ≤ 3.00x | PASS | hd 7.1 ms (p10 7.1, p90 7.4), Node 7.5 ms, Python 18 ms; release build, run as hd run |
| runtime | sort: hd / Node | 0.46x | ≤ 3.00x | PASS | hd 11 ms (p10 11, p90 11), Node 24 ms, Python 19 ms; release build, run as hd run |
| allocations | counted loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | list loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | map loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | iterator chain, bytes per iteration | - | ≤ 1 allocation (≤ 32 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| size-startup-heap | tiny program, release Wasm size | - | ≤ 2048 B | N/A | not supported by this hd: the build wrote no .wasm file (release build, run as hd run) |
| size-startup-heap | tiny program, start to first output | 11 ms | ≤ 5.0 ms | FAIL | release build, run as hd run |
| size-startup-heap | peak heap, map: hd / Node | 0.53x | ≤ 2.00x | PASS | hd +7.3 MB, Node +13.8 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.20x | ≤ 2.00x | PASS | hd +3.9 MB, Node +19.8 MB over an empty program |
| host-call-overhead | Console println, per call | 2.9 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd run |
| host-call-overhead | Fs read_text!, per call | 10 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd run |
| host-call-overhead | serde boundary (list_dir!), per call | 18 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd run |
| suspension-overhead | await of a ready value, per await | 0.00 ns | ≤ 100 ns | PASS | -2.7 ns per iteration; release build, run as hd run |
| suspension-overhead | all! of two ready tasks, throughput | 9.5M/s | ≥ 1.0M/s | PASS | 211 ns per iteration; release build, run as hd run |
| suspension-overhead | race! of two ready tasks, throughput | 7.7M/s | ≥ 1.0M/s | PASS | 261 ns per iteration; release build, run as hd run |
| serde-throughput | JSON encode, hd / Node MB/s | 0.12x | ≥ 0.50x | FAIL | hd 58 MB/s (45488 B document), Node 466 MB/s; release build, run as hd run |
| serde-throughput | JSON decode, hd / Node MB/s | 0.20x | ≥ 0.50x | FAIL | hd 34 MB/s (45488 B document), Node 168 MB/s; release build, run as hd run |
| text-throughput | string build, hd / Node MB/s | 2.94x | ≥ 0.50x | PASS | hd 338 MB/s, Node 115 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd run |
| text-throughput | split, hd / Node MB/s | 1.05x | ≥ 0.50x | PASS | hd 401 MB/s, Node 381 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd run |
| text-throughput | regex find_all, hd / Node MB/s | 0.47x | ≥ 0.50x | FAIL | hd 37 MB/s, Node 78 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd run |
| dead-code | Wasm bytes per 1,000 lines | - | ≤ 10240 B | FAIL | the build wrote no file to measure (release build, run as hd run) |
| dead-code | no unused std in the program | - | none | FAIL | the build wrote no file to measure (release build, run as hd run) |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 13448000 requests; RSS 41.6 MB after warm-up, 41.6 MB at the end; release build, run as hd run |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 9467 of 9467 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

- [2026-11-28T05](daily/2026-11-28T05.json)
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
