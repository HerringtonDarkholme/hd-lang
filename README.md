# Daily goal metrics

Finished: 2026-11-21T19:26:46.797Z. Commit: a462ef7a0d91ce4cd8c5f953884dccd1e341d832.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 7763 64-Core Processor).
Compiler: target/dist/hd.

284 passed, 23 failed, 4 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 36 ms → pass 45 ms
- edit-latency: p95 check after an edit: pass 38 ms → pass 47 ms
- cold-check: median cold check, 10k lines: pass 186 ms → pass 243 ms
- test-latency: median edit-then-test: pass 129 ms → pass 157 ms
- pathological: overlapping-from: time at 4x: pass 43 ms → pass 58 ms
- pathological: deep-blocks: time at 4x: pass 22 ms → pass 28 ms
- pathological: deep-parens: time at 4x: pass 21 ms → pass 27 ms
- pathological: method-chain: time at 4x: pass 26 ms → pass 32 ms
- pathological: iterator-chain: time at 4x: pass 33 ms → pass 38 ms
- pathological: large-enum-match: time at 4x: pass 31 ms → pass 40 ms
- pathological: deep-generic: time at 4x: pass 25 ms → pass 32 ms
- pathological: question-chain: time at 4x: pass 29 ms → pass 37 ms
- pathological: big-file: time at 4x: pass 140 ms → pass 192 ms
- fmt: hd fmt, 10k lines: pass 23 ms → pass 31 ms
- check-cost: checked run time / unchecked run time: fail 1.41x → fail 1.90x
- dev-speed: worst case dev / optimized: pass 3.44x → pass 2.88x
- resources: warm check, 10k lines: CPU time: pass 30 ms → pass 40 ms
- startup: hd help (no version command): wall time: pass 3.6 ms → pass 5.3 ms
- startup: hd check empty.hd, just written: wall time: pass 15 ms → fail 21 ms
- startup: hd check empty.hd, just written: CPU time: pass 0.0 ms → pass 10 ms
- startup: hd check empty.hd, settled: wall time: pass 5.8 ms → pass 8.8 ms
- proptest-perf: property cases per second: pass 113k/s → fail 85k/s
- proptest-perf: time to shrink a known failure: pass 17 ms → pass 32 ms
- unit-test-perf: 1,000 unit tests, warm: pass 107 ms → pass 124 ms
- integration-test-perf: setup per integration test program: fail 21 ms → fail 27 ms
- integration-test-perf: cost per doc test: pass 16 ms → pass 20 ms
- runtime: map: hd / Node: pass 1.01x → pass 1.16x
- size-startup-heap: tiny program, start to first output: fail 7.5 ms → fail 9.3 ms
- size-startup-heap: peak heap, map: hd / Node: pass 0.81x → pass 0.96x
- size-startup-heap: peak heap, string-build: hd / Node: pass 0.40x → pass 0.50x
- host-call-overhead: serde boundary (list_dir!), per call: fail 17 µs → fail 20 µs
- suspension-overhead: await of a ready value, per await: pass 9.7 ns → pass 0.00 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 8.2M/s → pass 6.8M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 6.7M/s → pass 5.7M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.13x → fail 0.10x
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.15x → fail 0.17x

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 45 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 47 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 243 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 157 ms | ≤ 300 ms | PASS |  |
| mistakes | programs with exactly one diagnostic | 100.0% | ≥ 95.0% | PASS | 70 of 70 |
| mistakes | one diagnostic with the expected code | 100.0% | ≥ 95.0% | PASS | 70 of 70 |
| mistakes | fix-it makes hd check pass | 52.9% | ≥ 80.0% | FAIL | 37 of 70 resolved; 37 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 85 tokens | ≤ 60 tokens | FAIL | largest: interpolate-optional; median 45 tokens, 178 B; 56 of 70 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 58 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 29.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 44 → 58 ms, start-up 28 ms |
| pathological | deep-blocks: time at 4x | 28 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 23.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 27 → 28 ms, start-up 28 ms |
| pathological | deep-parens: time at 4x | 27 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 23.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 29 → 27 ms, start-up 28 ms |
| pathological | method-chain: time at 4x | 32 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 25.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 31 → 32 ms, start-up 28 ms |
| pathological | iterator-chain: time at 4x | 38 ms | ≤ 2000 ms | PASS |  |
| pathological | iterator-chain: peak RSS at 4x | 25.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | iterator-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 32 → 38 ms, start-up 28 ms |
| pathological | wide-list-literal: time at 4x | 59 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | wide-list-literal: peak RSS at 4x | 29.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 43 → 59 ms, start-up 28 ms |
| pathological | large-enum-match: time at 4x | 40 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 26.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 31 → 40 ms, start-up 28 ms |
| pathological | deep-generic: time at 4x | 32 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 24.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 29 → 32 ms, start-up 28 ms |
| pathological | question-chain: time at 4x | 37 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 27.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 31 → 37 ms, start-up 28 ms |
| pathological | big-file: time at 4x | 192 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 57.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.15 | ≤ 1.30 | PASS | 2x→4x; 102 → 192 ms, start-up 28 ms |
| scale | fn-items check | 4.04x | ≤ 5x | PASS | hashes 192979 → 779035 (4.04x, limit 5.00x) |
| scale | fn-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | fn-items build | 4.01x | ≤ 5x | PASS | hashes 462028 → 1854234 (4.01x, limit 5.00x) |
| scale | struct-items check | 4.03x | ≤ 5x | PASS | hashes 207232 → 834468 (4.03x, limit 5.00x) |
| scale | struct-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | struct-items build | 4.01x | ≤ 5x | PASS | hashes 602573 → 2418897 (4.01x, limit 5.00x) |
| scale | enum-items check | 4.00x | ≤ 5x | PASS | scope_probes 23500 → 94000 (4.00x, limit 5.00x) |
| scale | enum-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | enum-items build | 4.00x | ≤ 5x | PASS | scope_probes 23500 → 94000 (4.00x, limit 5.00x) |
| scale | trait-items check | 4.00x | ≤ 5x | PASS | scope_probes 19500 → 78000 (4.00x, limit 5.00x) |
| scale | trait-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | trait-items build | 4.02x | ≤ 5x | PASS | control 147302 → 592802 (4.02x, limit 5.00x) |
| scale | impl-methods check | 4.00x | ≤ 5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 5.00x) |
| scale | impl-methods fmt | 3.97x | ≤ 5x | PASS | allocs 4045 → 16057 (3.97x, limit 5.00x) |
| scale | impl-methods build | 4.00x | ≤ 5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 5.00x) |
| scale | traits-of-one-struct check | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | traits-of-one-struct fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | traits-of-one-struct build | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | impls-of-one-trait check | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | impls-of-one-trait fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | impls-of-one-trait build | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | operator-impls check | 4.00x | ≤ 5x | PASS | hashes 709959 → 2840051 (4.00x, limit 5.00x) |
| scale | operator-impls fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | operator-impls build | 4.00x | ≤ 5x | PASS | scope_probes 25000 → 100000 (4.00x, limit 5.00x) |
| scale | derive-items check | 4.00x | ≤ 5x | PASS | trait_lookup 16998 → 67998 (4.00x, limit 5.00x) |
| scale | derive-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | derive-items build | 4.00x | ≤ 5x | PASS | trait_lookup 16998 → 67998 (4.00x, limit 5.00x) |
| scale | dyn-values check | 4.00x | ≤ 5x | PASS | hashes 757593 → 3030527 (4.00x, limit 5.00x) |
| scale | dyn-values fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | dyn-values build | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | consts check | 3.97x | ≤ 5x | PASS | hashes 61711 → 245019 (3.97x, limit 5.00x) |
| scale | consts fmt | 3.89x | ≤ 5x | PASS | allocs 2087 → 8109 (3.89x, limit 5.00x) |
| scale | consts build | 4.00x | ≤ 5x | PASS | loan_queries 4006 → 16006 (4.00x, limit 5.00x) |
| scale | local-items check | 4.00x | ≤ 5x | PASS | scope_probes 19000 → 76000 (4.00x, limit 5.00x) |
| scale | local-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | local-items build | 4.00x | ≤ 5x | PASS | scope_probes 19000 → 76000 (4.00x, limit 5.00x) |
| scale | syntax-items check | 4.00x | ≤ 5x | PASS | trait_lookup 9898 → 39598 (4.00x, limit 5.00x) |
| scale | syntax-items fmt | 4.00x | ≤ 5x | PASS | hashes 17160 → 68664 (4.00x, limit 5.00x) |
| scale | syntax-items build | 4.00x | ≤ 5x | PASS | trait_lookup 9898 → 39598 (4.00x, limit 5.00x) |
| scale | struct-chain check | 4.04x | ≤ 5x | PASS | hashes 155339 → 627795 (4.04x, limit 5.00x) |
| scale | struct-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | struct-chain-values check | 4.04x | ≤ 5x | PASS | hashes 114318 → 461386 (4.04x, limit 5.00x) |
| scale | struct-chain-values fmt | 4.02x | ≤ 5x | PASS | hashes 2082 → 8370 (4.02x, limit 5.00x) |
| scale | struct-chain-values build | 4.00x | ≤ 5x | PASS | scope_probes 4700 → 18800 (4.00x, limit 5.00x) |
| scale | modules check | 4.00x | ≤ 5x | PASS | hashes 1170957 → 4686933 (4.00x, limit 5.00x) |
| scale | modules fmt | 4.00x | ≤ 5x | PASS | hashes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | modules build | 4.00x | ≤ 5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 5.00x) |
| scale | use-names check | 4.03x | ≤ 5x | PASS | hashes 223034 → 899622 (4.03x, limit 5.00x) |
| scale | use-names fmt | 4.01x | ≤ 5x | PASS | hashes 3783 → 15159 (4.01x, limit 5.00x) |
| scale | use-names build | 4.02x | ≤ 5x | PASS | hashes 491879 → 1974989 (4.02x, limit 5.00x) |
| scale | generic-fns check | 4.03x | ≤ 5x | PASS | hashes 296464 → 1194288 (4.03x, limit 5.00x) |
| scale | generic-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-fns build | 4.02x | ≤ 5x | PASS | hashes 844024 → 3388998 (4.02x, limit 5.00x) |
| scale | generic-params check | 4.08x | ≤ 5x | PASS | hashes 15526 → 63390 (4.08x, limit 5.00x) |
| scale | generic-params fmt | 4.10x | ≤ 10x | PASS | alloc_bytes 344142 → 1412604 (4.10x, limit 10.00x) |
| scale | generic-params build | 4.00x | ≤ 5x | PASS | scope_probes 800 → 3200 (4.00x, limit 5.00x) |
| scale | param-bounds check | 4.01x | ≤ 5x | PASS | hashes 57379 → 230019 (4.01x, limit 5.00x) |
| scale | param-bounds fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | param-bounds build | 4.00x | ≤ 5x | PASS | scope_probes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | type-defaults check | 4.00x | ≤ 5x | PASS | scope_probes 30500 → 122000 (4.00x, limit 5.00x) |
| scale | type-defaults fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | type-defaults build | 4.03x | ≤ 5x | PASS | control 136418 → 550418 (4.03x, limit 5.00x) |
| scale | alias-chain check | 4.01x | ≤ 5x | PASS | hashes 121908 → 488693 (4.01x, limit 5.00x) |
| scale | alias-chain fmt | 4.03x | ≤ 5x | PASS | hashes 1234 → 4978 (4.03x, limit 5.00x) |
| scale | alias-chain build | 4.00x | ≤ 5x | PASS | type_nodes 3399 → 13599 (4.00x, limit 5.00x) |
| scale | row-chain check | 4.02x | ≤ 5x | PASS | hashes 220048 → 883640 (4.02x, limit 5.00x) |
| scale | row-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | row-chain build | 4.01x | ≤ 5x | PASS | hashes 532018 → 2135053 (4.01x, limit 5.00x) |
| scale | row-traits check | 4.00x | ≤ 5x | PASS | scope_probes 2100 → 8400 (4.00x, limit 5.00x) |
| scale | row-traits fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | row-traits build | 4.00x | ≤ 5x | PASS | scope_probes 2100 → 8400 (4.00x, limit 5.00x) |
| scale | closures check | 4.00x | ≤ 5x | PASS | local_body_checks 1000 → 4000 (4.00x, limit 5.00x) |
| scale | closures fmt | 3.94x | ≤ 5x | PASS | allocs 3061 → 12073 (3.94x, limit 5.00x) |
| scale | closures build | 4.00x | ≤ 5x | PASS | local_body_checks 1000 → 4000 (4.00x, limit 5.00x) |
| scale | generic-local-fns check | 4.03x | ≤ 5x | PASS | hashes 238627 → 960807 (4.03x, limit 5.00x) |
| scale | generic-local-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-local-fns build | 4.01x | ≤ 5x | PASS | hashes 903517 → 3624834 (4.01x, limit 5.00x) |
| scale | let-statements check | 3.96x | ≤ 5x | PASS | allocs 25335 → 100429 (3.96x, limit 5.00x) |
| scale | let-statements fmt | 3.94x | ≤ 5x | PASS | allocs 2554 → 10066 (3.94x, limit 5.00x) |
| scale | let-statements build | 4.00x | ≤ 5x | PASS | control 68499 → 273999 (4.00x, limit 5.00x) |
| scale | matches check | 4.00x | ≤ 5x | PASS | decision_rows 6000 → 24000 (4.00x, limit 5.00x) |
| scale | matches fmt | 3.97x | ≤ 5x | PASS | allocs 6057 → 24069 (3.97x, limit 5.00x) |
| scale | matches build | 4.07x | ≤ 5x | PASS | hashes 865529 → 3524459 (4.07x, limit 5.00x) |
| scale | enum-arms check | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | enum-arms fmt | 3.91x | ≤ 5x | PASS | allocs 1551 → 6065 (3.91x, limit 5.00x) |
| scale | enum-arms build | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | tuple-patterns check | 4.00x | ≤ 5x | PASS | decision_rows 1000 → 4000 (4.00x, limit 5.00x) |
| scale | tuple-patterns fmt | 3.95x | ≤ 5x | PASS | allocs 3054 → 12066 (3.95x, limit 5.00x) |
| scale | tuple-patterns build | 4.00x | ≤ 5x | PASS | control 92471 → 369971 (4.00x, limit 5.00x) |
| scale | range-patterns check | 4.02x | ≤ 5x | PASS | hashes 248724 → 1001056 (4.02x, limit 5.00x) |
| scale | range-patterns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | range-patterns build | 4.01x | ≤ 5x | PASS | hashes 790888 → 3170628 (4.01x, limit 5.00x) |
| scale | wide-struct check | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | wide-struct fmt | 3.95x | ≤ 5x | PASS | allocs 2544 → 10056 (3.95x, limit 5.00x) |
| scale | wide-struct build | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | call-args check | 4.37x | ≤ 5x | PASS | hashes 4324 → 18900 (4.37x, limit 5.00x) |
| scale | call-args fmt | 4.10x | ≤ 10x | PASS | alloc_bytes 361569 → 1482933 (4.10x, limit 10.00x) |
| scale | call-args build | 4.04x | ≤ 5x | PASS | hashes 18745 → 75699 (4.04x, limit 5.00x) |
| scale | binary-chain check | 4.00x | ≤ 5x | PASS | hashes 2500 → 10000 (4.00x, limit 5.00x) |
| scale | binary-chain fmt | 3.78x | ≤ 5x | PASS | allocs 545 → 2060 (3.78x, limit 5.00x) |
| scale | binary-chain build | 4.03x | ≤ 5x | PASS | hashes 22624 → 91252 (4.03x, limit 5.00x) |
| scale | interpolation check | 4.00x | ≤ 5x | PASS | trait_lookup 2501 → 10001 (4.00x, limit 5.00x) |
| scale | interpolation fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | interpolation build | 4.04x | ≤ 5x | PASS | hashes 3184179 → 12852752 (4.04x, limit 5.00x) |
| scale | list-literal check | 4.00x | ≤ 5x | PASS | hashes 7392 → 29580 (4.00x, limit 5.00x) |
| scale | list-literal fmt | 3.82x | ≤ 5x | PASS | allocs 533 → 2038 (3.82x, limit 5.00x) |
| scale | list-literal build | 4.04x | ≤ 5x | PASS | hashes 36198 → 146202 (4.04x, limit 5.00x) |
| scale | method-chain check | 4.00x | ≤ 5x | PASS | trait_lookup 500 → 2000 (4.00x, limit 5.00x) |
| scale | method-chain fmt | 4.04x | ≤ 10x | PASS | alloc_bytes 838129 → 3386711 (4.04x, limit 10.00x) |
| scale | method-chain build | 4.00x | ≤ 5x | PASS | trait_lookup 500 → 2000 (4.00x, limit 5.00x) |
| scale | pipe-chain check | 4.00x | ≤ 5x | PASS | hashes 15517 → 62017 (4.00x, limit 5.00x) |
| scale | pipe-chain fmt | 3.77x | ≤ 5x | PASS | allocs 546 → 2056 (3.77x, limit 5.00x) |
| scale | pipe-chain build | 3.97x | ≤ 5x | PASS | hashes 83263 → 330955 (3.97x, limit 5.00x) |
| scale | question-chain check | 4.00x | ≤ 5x | PASS | hashes 41028 → 164028 (4.00x, limit 5.00x) |
| scale | question-chain fmt | 3.94x | ≤ 5x | PASS | allocs 1533 → 6044 (3.94x, limit 5.00x) |
| scale | question-chain build | 3.99x | ≤ 5x | PASS | loan_queries 4010 → 16010 (3.99x, limit 5.00x) |
| scale | if-nest check | 3.92x | ≤ 15.11x | PASS | hashes 756 → 2964 (3.92x, limit 15.11x) |
| scale | if-nest fmt | - | ≤ 15.11x | PASS | no counter reached the floor |
| scale | if-nest build | 4.02x | ≤ 15.11x | PASS | hashes 5047 → 20311 (4.02x, limit 15.11x) |
| scale | block-nest check | 3.95x | ≤ 15.29x | PASS | hashes 814 → 3214 (3.95x, limit 15.29x) |
| scale | block-nest fmt | - | ≤ 15.29x | PASS | no counter reached the floor |
| scale | block-nest build | 3.99x | ≤ 15.29x | PASS | hashes 5529 → 22065 (3.99x, limit 15.29x) |
| scale | for-nest check | 5.23x | ≤ 14.5x | PASS | hashes 3499 → 18283 (5.23x, limit 14.50x) |
| scale | for-nest fmt | - | ≤ 14.5x | PASS | no counter reached the floor |
| scale | for-nest build | 4.65x | ≤ 14.5x | PASS | hashes 25501 → 118589 (4.65x, limit 14.50x) |
| scale | while-nest check | 3.88x | ≤ 15.11x | PASS | hashes 1033 → 4009 (3.88x, limit 15.11x) |
| scale | while-nest fmt | - | ≤ 15.11x | PASS | no counter reached the floor |
| scale | while-nest build | 4.00x | ≤ 15.11x | PASS | use_facts 1232 → 4928 (4.00x, limit 15.11x) |
| scale | match-nest check | 4.87x | ≤ 16.03x | PASS | hashes 1776 → 8652 (4.87x, limit 16.03x) |
| scale | match-nest fmt | - | ≤ 16.03x | PASS | no counter reached the floor |
| scale | match-nest build | 4.31x | ≤ 16.03x | PASS | hashes 7368 → 31777 (4.31x, limit 16.03x) |
| scale | closure-nest check | 3.95x | ≤ 11.45x | PASS | hashes 1807 → 7141 (3.95x, limit 11.45x) |
| scale | closure-nest fmt | - | ≤ 11.45x | PASS | no counter reached the floor |
| scale | closure-nest build | 4.00x | ≤ 11.45x | PASS | use_facts 490 → 1960 (4.00x, limit 11.45x) |
| scale | option-nest check | 4.18x | ≤ 5x | PASS | hashes 1775 → 7421 (4.18x, limit 5.00x) |
| scale | option-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | option-nest build | 4.13x | ≤ 5x | PASS | hashes 11248 → 46463 (4.13x, limit 5.00x) |
| scale | list-type-nest check | 4.26x | ≤ 5x | PASS | hashes 1260 → 5364 (4.26x, limit 5.00x) |
| scale | list-type-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | list-type-nest build | 4.26x | ≤ 5x | PASS | hashes 4638 → 19762 (4.26x, limit 5.00x) |
| scale | generic-nest check | 4.33x | ≤ 5x | PASS | hashes 1582 → 6844 (4.33x, limit 5.00x) |
| scale | generic-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | generic-nest build | 4.06x | ≤ 5x | PASS | allocs 6363 → 25802 (4.06x, limit 5.00x) |
| scale | paren-nest check | - | ≤ 5x | PASS | no counter reached the floor |
| scale | paren-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | paren-nest build | 4.08x | ≤ 5x | PASS | control 795 → 3243 (4.08x, limit 5.00x) |
| scale | arith-nest check | 4.00x | ≤ 5x | PASS | hashes 440 → 1760 (4.00x, limit 5.00x) |
| scale | arith-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | arith-nest build | 4.03x | ≤ 5x | PASS | hashes 5359 → 21614 (4.03x, limit 5.00x) |
| scale | bool-nest check | 3.95x | ≤ 5x | PASS | hashes 391 → 1543 (3.95x, limit 5.00x) |
| scale | bool-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | bool-nest build | 4.09x | ≤ 5x | PASS | use_facts 490 → 2002 (4.09x, limit 5.00x) |
| scale | bit-nest check | 3.99x | ≤ 5x | PASS | hashes 818 → 3266 (3.99x, limit 5.00x) |
| scale | bit-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | bit-nest build | 4.13x | ≤ 5x | PASS | hashes 7031 → 29021 (4.13x, limit 5.00x) |
| scale | tuple-nest check | 4.06x | ≤ 5x | PASS | hashes 1868 → 7586 (4.06x, limit 5.00x) |
| scale | tuple-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | tuple-nest build | 4.10x | ≤ 5x | PASS | hashes 10425 → 42717 (4.10x, limit 5.00x) |
| scale | if-expr-nest check | 3.76x | ≤ 5x | PASS | hashes 322 → 1210 (3.76x, limit 5.00x) |
| scale | if-expr-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | if-expr-nest build | 3.82x | ≤ 5x | PASS | use_facts 357 → 1365 (3.82x, limit 5.00x) |
| scale | call-nest check | 3.90x | ≤ 5x | PASS | hashes 513 → 2001 (3.90x, limit 5.00x) |
| scale | call-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | call-nest build | 3.17x | ≤ 5x | PASS | hashes 3639 → 11547 (3.17x, limit 5.00x) |
| scale | index-nest check | - | ≤ 5x | PASS | no counter reached the floor |
| scale | index-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | index-nest build | 4.74x | ≤ 5x | PASS | hashes 1629 → 7719 (4.74x, limit 5.00x) |
| scale | fn-nest check | 4.03x | ≤ 13.42x | PASS | hashes 2674 → 10782 (4.03x, limit 13.42x) |
| scale | fn-nest fmt | - | ≤ 13.42x | PASS | no counter reached the floor |
| scale | fn-nest build | 4.00x | ≤ 13.42x | PASS | use_facts 784 → 3136 (4.00x, limit 13.42x) |
| scale | module-path-nest check | 1.58x | ≤ 5x | PASS | allocs 2503 → 3965 (1.58x, limit 5.00x) |
| scale | module-path-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | module-path-nest build | 1.59x | ≤ 5x | PASS | allocs 2512 → 3986 (1.59x, limit 5.00x) |
| scale | coverage: node kinds | 119/119 | 119/119 | PASS | exempt: Error |
| scale | coverage: node kinds repeated | 110/119 | reported | PASS | in no repeat shape: ReturnStmt, RangeExpr, LogicalOrExpr, LogicalAndExpr, BitwiseOrExpr, BitwiseXorExpr, BitwiseAndExpr, ShiftExpr, WhileExpr; exempt: Error |
| scale | coverage: node kinds nested | 57/119 | reported | PASS | in no nest shape: UseGroup, UseItem, EnumDecl, TraitDecl, ImplDecl, TypeDecl, TestsBlock, Decorator, DefaultValue, ConstParameter, LifetimeParameter, BoundList, TypeDefault, ConstArgument, AssociatedTypeBinding, NamedArgument, EmbeddedField, EnumVariant, VariantSharedData, AssociatedTypeDecl, Derivation, DiscardStmt, BreakStmt, ContinueStmt, UsingStmt, ElseIfClause, MatchGuard, PipeExpr, RefExpr, DerefExpr, PowerExpr, TryExpr, SuspendExpr, PathExpr, TypeArgsExpr, TrailingCallExpr, PlaceholderExpr, MapExpr, MapEntry, SpreadExpr, ComprehensionExpr, ComprehensionFor, ComprehensionIf, ContextExpr, ContextEntry, Interpolation, LiteralPattern, NamedPattern, DataPattern, DataPatternField, SpreadPattern, RangePattern, DynType, ImplType, RefType, UnionType, ParenType, FunctionType, ProjectionType, ContextType, RestType, InferType; exempt: Error |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 69 of 70; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 37 of 37 safe |
| fmt | hd fmt, 10k lines | 31 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.90x | ≤ 1.30x | FAIL | checked 380 ms, unchecked 200 ms |
| dev-speed | geomean dev / optimized | 1.60x | ≤ 4.00x | PASS | fib 0.95x, sum 1.87x, string-build 1.64x, map 1.24x, sort 2.88x |
| dev-speed | worst case dev / optimized | 2.88x | ≤ 10.00x | PASS | sort |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 40 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 27.8 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.21 CPU-s, 39.2 MB | completes ≤ 30 s | PASS | wall 132 ms |
| resources | check, small, warm | 0.02 CPU-s, 20.2 MB | completes ≤ 30 s | PASS | wall 23 ms |
| resources | test, small, cold | 1.26 CPU-s, 119.7 MB | completes ≤ 30 s | PASS | wall 590 ms |
| resources | test, small, warm | 0.09 CPU-s, 53.7 MB | completes ≤ 30 s | PASS | wall 80 ms |
| resources | build, small, cold | 1.08 CPU-s, 89.6 MB | completes ≤ 30 s | PASS | wall 408 ms |
| resources | build, small, warm | 0.04 CPU-s, 31.8 MB | completes ≤ 30 s | PASS | wall 40 ms |
| resources | check, 10k, cold | 0.54 CPU-s, 61.7 MB | completes ≤ 30 s | PASS | wall 246 ms |
| resources | check, 10k, warm | 0.04 CPU-s, 27.8 MB | completes ≤ 30 s | PASS | wall 47 ms |
| resources | test, 10k, cold | 2.06 CPU-s, 165.9 MB | completes ≤ 30 s | PASS | wall 1062 ms |
| resources | test, 10k, warm | 0.19 CPU-s, 75.5 MB | completes ≤ 30 s | PASS | wall 171 ms |
| resources | build, 10k, cold | 1.45 CPU-s, 108.3 MB | completes ≤ 30 s | PASS | wall 499 ms |
| resources | build, 10k, warm | 0.06 CPU-s, 36.7 MB | completes ≤ 30 s | PASS | wall 62 ms |
| resources | check, 50k, cold | 2.03 CPU-s, 166.7 MB | completes ≤ 150 s | PASS | wall 765 ms |
| resources | check, 50k, warm | 0.17 CPU-s, 58.8 MB | completes ≤ 150 s | PASS | wall 159 ms |
| resources | test, 50k, cold | 5.36 CPU-s, 371.4 MB | completes ≤ 150 s | PASS | wall 3184 ms |
| resources | test, 50k, warm | 0.59 CPU-s, 152.1 MB | completes ≤ 150 s | PASS | wall 489 ms |
| resources | build, 50k, cold | 3.02 CPU-s, 215.5 MB | completes ≤ 150 s | PASS | wall 1007 ms |
| resources | build, 50k, warm | 0.18 CPU-s, 67.1 MB | completes ≤ 150 s | PASS | wall 166 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 71.3 MB | ≤ 22.0 MB | FAIL | RSS 106.2 MB at input 100, 177.5 MB at input 1000; build per input 101 ms at 50, 1568 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 5.3 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 5.4 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd, just written: wall time | 21 ms | ≤ 20 ms | FAIL |  |
| startup | hd check empty.hd, just written: CPU time | 10 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, just written: peak RSS | 18.8 MB | ≤ 10.0 MB | FAIL |  |
| startup | hd check empty.hd, settled: wall time | 8.8 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, settled: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, settled: peak RSS | 10.1 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 1.49x | ≤ 1.50x | PASS | p95 143 ms at N = 1; 4 cores; total CPU 300 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 1.95x | ≤ 6.00x | PASS | p95 143 ms at N = 1; 4 cores; total CPU 580 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 0.35 | &lt; 1.00 (sublinear) | PASS | 220 ms at N = 1, 580 ms at N = 16 |
| disk | artifacts per worktree | 0.0 MB | ≤ 10.0 MB | PASS | 10 files; HD_CACHE 11.5 MB, shared per user, not counted |
| disk | toolchain, once per machine | 31.6 MB | recorded, no target | PASS | toolchain files: /home/runner/work/hd-lang/hd-lang/target/dist/hd |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.83x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 206 files | the files of one check's cache | FAIL | one check alone: 36 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 14.2 MB | ≤ 5.0 MB | FAIL | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 1 | ≤ 1 | PASS | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 85k/s | ≥ 100k/s | FAIL | 6000 extra cases took 71 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 32 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 124 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.1 ms | ≤ 1.0 ms | PASS | 100 tests: 53 ms |
| integration-test-perf | setup per integration test program | 27 ms | ≤ 20 ms | FAIL | 10 programs: 328 ms for 21 tests, 16 ms per test |
| integration-test-perf | total time growth in programs | 0.899 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 67 ms at 2, 284 ms at 10 programs |
| integration-test-perf | cost per doc test | 20 ms | ≤ 20 ms | PASS | 10 doc tests: 252 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 0.60x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.74x | ≤ 3.00x | PASS | hd 6.9 ms (p10 6.8, p90 8.3), Node 9.2 ms, Python 133 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 0.18x | ≤ 3.00x | PASS | hd 9.4 ms (p10 9.3, p90 13), Node 53 ms, Python 278 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.84x | ≤ 3.00x | PASS | hd 5.3 ms (p10 5.2, p90 5.3), Node 6.3 ms, Python 16 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 1.16x | ≤ 3.00x | PASS | hd 8.6 ms (p10 8.6, p90 8.8), Node 7.4 ms, Python 19 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.61x | ≤ 3.00x | PASS | hd 15 ms (p10 15, p90 15), Node 24 ms, Python 20 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | list loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | map loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | iterator chain, bytes per iteration | - | ≤ 1 allocation (≤ 32 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| size-startup-heap | tiny program, release Wasm size | 5017 B | ≤ 2048 B | FAIL | benchmarks: fib 5141 B, sum 4917 B, string-build 7891 B, map 7095 B, sort 13097 B |
| size-startup-heap | tiny program, start to first output | 9.3 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.96x | ≤ 2.00x | PASS | hd +13.7 MB, Node +14.2 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.50x | ≤ 2.00x | PASS | hd +10.0 MB, Node +19.9 MB over an empty program |
| host-call-overhead | Console println, per call | 2.9 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm Timing change ≤10%: noise. |
| host-call-overhead | Fs read_text!, per call | 12 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm Timing change ≤10%: noise. |
| host-call-overhead | serde boundary (list_dir!), per call | 20 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 0.00 ns | ≤ 100 ns | PASS | -7.7 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 6.8M/s | ≥ 1.0M/s | PASS | 295 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 5.7M/s | ≥ 1.0M/s | PASS | 352 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.10x | ≥ 0.50x | FAIL | hd 47 MB/s (45488 B document), Node 466 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.17x | ≥ 0.50x | FAIL | hd 28 MB/s (45488 B document), Node 165 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 1.61x | ≥ 0.50x | PASS | hd 189 MB/s, Node 117 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 0.70x | ≥ 0.50x | PASS | hd 261 MB/s, Node 375 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.05x | ≥ 0.50x | FAIL | hd 4.9 MB/s, Node 90 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 6516 B | ≤ 10240 B | PASS | 12611 B at 1075 lines, 71260 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 915 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 10662000 requests; RSS 65.3 MB after warm-up, 65.3 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 9061 of 9061 unlisted cases passed |
| conformance | listed known failures | 3 | ≤ 0 | FAIL | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
- [2026-11-09T03](daily/2026-11-09T03.json)
- [2026-11-08T18](daily/2026-11-08T18.json)
- [2026-11-08T03](daily/2026-11-08T03.json)
