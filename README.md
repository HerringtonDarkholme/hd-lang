# Daily goal metrics

Finished: 2026-11-21T05:00:35.110Z. Commit: 67eb69a3dcd0853639644442da76c4a0baf87b51.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V74 80-Core Processor).
Compiler: target/dist/hd.

286 passed, 21 failed, 4 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 41 ms → pass 36 ms
- edit-latency: p95 check after an edit: pass 44 ms → pass 38 ms
- cold-check: median cold check, 10k lines: pass 235 ms → pass 186 ms
- test-latency: median edit-then-test: pass 145 ms → pass 129 ms
- pathological: overlapping-from: time at 4x: pass 51 ms → pass 43 ms
- pathological: deep-parens: time at 4x: pass 24 ms → pass 21 ms
- pathological: method-chain: time at 4x: pass 29 ms → pass 26 ms
- pathological: big-file: time at 4x: pass 175 ms → pass 140 ms
- fmt: hd fmt, 10k lines: pass 26 ms → pass 23 ms
- check-cost: checked run time / unchecked run time: fail 1.76x → fail 1.41x
- resources: warm check, 10k lines: CPU time: pass 40 ms → pass 30 ms
- startup: hd check empty.hd, just written: wall time: pass 17 ms → pass 15 ms
- startup: hd check empty.hd, settled: peak RSS: pass 10.0 MB → fail 10.2 MB
- concurrency: p95 latency at N = 4 vs N = 1: fail 1.75x → pass 1.43x
- concurrency: total CPU growth exponent, N = 1 to 16: pass 0.31 → pass 0.36
- proptest-perf: property cases per second: fail 75k/s → pass 113k/s
- proptest-perf: time to shrink a known failure: pass 14 ms → pass 17 ms
- integration-test-perf: setup per integration test program: fail 32 ms → fail 21 ms
- integration-test-perf: total time growth in programs: pass 0.960 → pass 0.846
- integration-test-perf: cost per doc test: fail 23 ms → pass 16 ms
- runtime: sum: hd / Node: pass 0.14x → pass 0.17x
- size-startup-heap: peak heap, map: hd / Node: pass 0.95x → pass 0.81x
- size-startup-heap: peak heap, string-build: hd / Node: pass 0.50x → pass 0.40x
- host-call-overhead: Console println, per call: fail 2.0 µs → fail 3.0 µs
- host-call-overhead: Fs read_text!, per call: pass 6.8 µs → fail 11 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 10 µs → fail 17 µs
- suspension-overhead: await of a ready value, per await: pass 0.00 ns → pass 9.7 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 7.0M/s → pass 8.2M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 4.8M/s → pass 6.7M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.09x → fail 0.13x
- text-throughput: string build, hd / Node MB/s: pass 1.32x → pass 1.53x
- text-throughput: split, hd / Node MB/s: pass 0.88x → pass 0.70x
- text-throughput: regex find_all, hd / Node MB/s: fail 0.07x → fail 0.05x

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 36 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 38 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 186 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 129 ms | ≤ 300 ms | PASS |  |
| mistakes | programs with exactly one diagnostic | 100.0% | ≥ 95.0% | PASS | 70 of 70 |
| mistakes | one diagnostic with the expected code | 100.0% | ≥ 95.0% | PASS | 70 of 70 |
| mistakes | fix-it makes hd check pass | 52.9% | ≥ 80.0% | FAIL | 37 of 70 resolved; 37 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 85 tokens | ≤ 60 tokens | FAIL | largest: interpolate-optional; median 45 tokens, 178 B; 56 of 70 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 43 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 29.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 33 → 43 ms, start-up 23 ms |
| pathological | deep-blocks: time at 4x | 22 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-blocks: peak RSS at 4x | 24.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 22 ms, start-up 23 ms |
| pathological | deep-parens: time at 4x | 21 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 23.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 21 → 21 ms, start-up 23 ms |
| pathological | method-chain: time at 4x | 26 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 25.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 24 → 26 ms, start-up 23 ms |
| pathological | iterator-chain: time at 4x | 33 ms | ≤ 2000 ms | PASS |  |
| pathological | iterator-chain: peak RSS at 4x | 24.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | iterator-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 26 → 33 ms, start-up 23 ms |
| pathological | wide-list-literal: time at 4x | 54 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | wide-list-literal: peak RSS at 4x | 28.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 43 → 54 ms, start-up 23 ms |
| pathological | large-enum-match: time at 4x | 31 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | large-enum-match: peak RSS at 4x | 26.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 26 → 31 ms, start-up 23 ms |
| pathological | deep-generic: time at 4x | 25 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 24.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 25 ms, start-up 23 ms |
| pathological | question-chain: time at 4x | 29 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | question-chain: peak RSS at 4x | 27.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 25 → 29 ms, start-up 23 ms |
| pathological | big-file: time at 4x | 140 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 57.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.10 | ≤ 1.30 | PASS | 2x→4x; 77 → 140 ms, start-up 23 ms |
| scale | fn-items check | 4.04x | ≤ 5x | PASS | hashes 192755 → 779035 (4.04x, limit 5.00x) |
| scale | fn-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | fn-items build | 4.01x | ≤ 5x | PASS | hashes 464020 → 1862342 (4.01x, limit 5.00x) |
| scale | struct-items check | 4.03x | ≤ 5x | PASS | hashes 207232 → 834468 (4.03x, limit 5.00x) |
| scale | struct-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | struct-items build | 4.01x | ≤ 5x | PASS | hashes 611756 → 2455736 (4.01x, limit 5.00x) |
| scale | enum-items check | 4.00x | ≤ 5x | PASS | scope_probes 23500 → 94000 (4.00x, limit 5.00x) |
| scale | enum-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | enum-items build | 4.00x | ≤ 5x | PASS | scope_probes 23500 → 94000 (4.00x, limit 5.00x) |
| scale | trait-items check | 4.00x | ≤ 5x | PASS | scope_probes 19500 → 78000 (4.00x, limit 5.00x) |
| scale | trait-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | trait-items build | 4.02x | ≤ 5x | PASS | control 147295 → 592795 (4.02x, limit 5.00x) |
| scale | impl-methods check | 4.00x | ≤ 5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 5.00x) |
| scale | impl-methods fmt | 3.97x | ≤ 5x | PASS | allocs 4045 → 16057 (3.97x, limit 5.00x) |
| scale | impl-methods build | 4.00x | ≤ 5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 5.00x) |
| scale | traits-of-one-struct check | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | traits-of-one-struct fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | traits-of-one-struct build | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | impls-of-one-trait check | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | impls-of-one-trait fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | impls-of-one-trait build | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | operator-impls check | 4.00x | ≤ 5x | PASS | hashes 712983 → 2851935 (4.00x, limit 5.00x) |
| scale | operator-impls fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | operator-impls build | 4.00x | ≤ 5x | PASS | scope_probes 25000 → 100000 (4.00x, limit 5.00x) |
| scale | derive-items check | 4.00x | ≤ 5x | PASS | trait_lookup 16998 → 67998 (4.00x, limit 5.00x) |
| scale | derive-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | derive-items build | 4.00x | ≤ 5x | PASS | trait_lookup 16998 → 67998 (4.00x, limit 5.00x) |
| scale | dyn-values check | 4.00x | ≤ 5x | PASS | hashes 760590 → 3042505 (4.00x, limit 5.00x) |
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
| scale | use-names check | 4.04x | ≤ 5x | PASS | hashes 222586 → 899622 (4.04x, limit 5.00x) |
| scale | use-names fmt | 4.01x | ≤ 5x | PASS | hashes 3783 → 15159 (4.01x, limit 5.00x) |
| scale | use-names build | 4.02x | ≤ 5x | PASS | hashes 493868 → 1982946 (4.02x, limit 5.00x) |
| scale | generic-fns check | 4.03x | ≤ 5x | PASS | hashes 296492 → 1194288 (4.03x, limit 5.00x) |
| scale | generic-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-fns build | 4.01x | ≤ 5x | PASS | hashes 848896 → 3408226 (4.01x, limit 5.00x) |
| scale | generic-params check | 4.08x | ≤ 5x | PASS | hashes 15526 → 63390 (4.08x, limit 5.00x) |
| scale | generic-params fmt | 4.10x | ≤ 10x | PASS | alloc_bytes 344142 → 1412604 (4.10x, limit 10.00x) |
| scale | generic-params build | 4.00x | ≤ 5x | PASS | scope_probes 800 → 3200 (4.00x, limit 5.00x) |
| scale | param-bounds check | 4.01x | ≤ 5x | PASS | hashes 57379 → 230019 (4.01x, limit 5.00x) |
| scale | param-bounds fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | param-bounds build | 4.00x | ≤ 5x | PASS | scope_probes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | type-defaults check | 4.00x | ≤ 5x | PASS | scope_probes 30500 → 122000 (4.00x, limit 5.00x) |
| scale | type-defaults fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | type-defaults build | 4.03x | ≤ 5x | PASS | control 136411 → 550411 (4.03x, limit 5.00x) |
| scale | alias-chain check | 4.01x | ≤ 5x | PASS | hashes 121908 → 488689 (4.01x, limit 5.00x) |
| scale | alias-chain fmt | 4.03x | ≤ 5x | PASS | hashes 1234 → 4978 (4.03x, limit 5.00x) |
| scale | alias-chain build | 4.00x | ≤ 5x | PASS | type_nodes 3399 → 13599 (4.00x, limit 5.00x) |
| scale | row-chain check | 4.02x | ≤ 5x | PASS | hashes 220048 → 883640 (4.02x, limit 5.00x) |
| scale | row-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | row-chain build | 4.01x | ≤ 5x | PASS | hashes 531440 → 2132818 (4.01x, limit 5.00x) |
| scale | row-traits check | 4.00x | ≤ 5x | PASS | scope_probes 2100 → 8400 (4.00x, limit 5.00x) |
| scale | row-traits fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | row-traits build | 4.00x | ≤ 5x | PASS | scope_probes 2100 → 8400 (4.00x, limit 5.00x) |
| scale | closures check | 4.00x | ≤ 5x | PASS | local_body_checks 1000 → 4000 (4.00x, limit 5.00x) |
| scale | closures fmt | 3.94x | ≤ 5x | PASS | allocs 3061 → 12073 (3.94x, limit 5.00x) |
| scale | closures build | 4.00x | ≤ 5x | PASS | local_body_checks 1000 → 4000 (4.00x, limit 5.00x) |
| scale | generic-local-fns check | 4.03x | ≤ 5x | PASS | hashes 238629 → 960809 (4.03x, limit 5.00x) |
| scale | generic-local-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-local-fns build | 4.01x | ≤ 5x | PASS | hashes 926569 → 3713562 (4.01x, limit 5.00x) |
| scale | let-statements check | 3.96x | ≤ 5x | PASS | allocs 25335 → 100429 (3.96x, limit 5.00x) |
| scale | let-statements fmt | 3.94x | ≤ 5x | PASS | allocs 2554 → 10066 (3.94x, limit 5.00x) |
| scale | let-statements build | 4.00x | ≤ 5x | PASS | control 68499 → 273999 (4.00x, limit 5.00x) |
| scale | matches check | 4.00x | ≤ 5x | PASS | decision_rows 6000 → 24000 (4.00x, limit 5.00x) |
| scale | matches fmt | 3.97x | ≤ 5x | PASS | allocs 6057 → 24069 (3.97x, limit 5.00x) |
| scale | matches build | 4.07x | ≤ 5x | PASS | hashes 870989 → 3546419 (4.07x, limit 5.00x) |
| scale | enum-arms check | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | enum-arms fmt | 3.91x | ≤ 5x | PASS | allocs 1551 → 6065 (3.91x, limit 5.00x) |
| scale | enum-arms build | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | tuple-patterns check | 4.00x | ≤ 5x | PASS | decision_rows 1000 → 4000 (4.00x, limit 5.00x) |
| scale | tuple-patterns fmt | 3.95x | ≤ 5x | PASS | allocs 3054 → 12066 (3.95x, limit 5.00x) |
| scale | tuple-patterns build | 4.00x | ≤ 5x | PASS | control 93471 → 373971 (4.00x, limit 5.00x) |
| scale | range-patterns check | 4.02x | ≤ 5x | PASS | hashes 248724 → 1001056 (4.02x, limit 5.00x) |
| scale | range-patterns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | range-patterns build | 4.01x | ≤ 5x | PASS | hashes 804659 → 3226750 (4.01x, limit 5.00x) |
| scale | wide-struct check | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | wide-struct fmt | 3.95x | ≤ 5x | PASS | allocs 2544 → 10056 (3.95x, limit 5.00x) |
| scale | wide-struct build | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | call-args check | 4.37x | ≤ 5x | PASS | hashes 4324 → 18900 (4.37x, limit 5.00x) |
| scale | call-args fmt | 4.10x | ≤ 10x | PASS | alloc_bytes 361569 → 1482933 (4.10x, limit 10.00x) |
| scale | call-args build | 4.00x | ≤ 5x | PASS | control 4999 → 19999 (4.00x, limit 5.00x) |
| scale | binary-chain check | 4.00x | ≤ 5x | PASS | hashes 2500 → 10000 (4.00x, limit 5.00x) |
| scale | binary-chain fmt | 3.78x | ≤ 5x | PASS | allocs 545 → 2060 (3.78x, limit 5.00x) |
| scale | binary-chain build | 4.03x | ≤ 5x | PASS | hashes 22624 → 91252 (4.03x, limit 5.00x) |
| scale | interpolation check | 4.00x | ≤ 5x | PASS | trait_lookup 2501 → 10001 (4.00x, limit 5.00x) |
| scale | interpolation fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | interpolation build | 4.04x | ≤ 5x | PASS | hashes 3189681 → 12874754 (4.04x, limit 5.00x) |
| scale | list-literal check | 4.00x | ≤ 5x | PASS | hashes 7392 → 29580 (4.00x, limit 5.00x) |
| scale | list-literal fmt | 3.82x | ≤ 5x | PASS | allocs 533 → 2038 (3.82x, limit 5.00x) |
| scale | list-literal build | 4.04x | ≤ 5x | PASS | hashes 35198 → 142202 (4.04x, limit 5.00x) |
| scale | method-chain check | 4.00x | ≤ 5x | PASS | trait_lookup 500 → 2000 (4.00x, limit 5.00x) |
| scale | method-chain fmt | 4.04x | ≤ 10x | PASS | alloc_bytes 838129 → 3386711 (4.04x, limit 10.00x) |
| scale | method-chain build | 4.00x | ≤ 5x | PASS | trait_lookup 500 → 2000 (4.00x, limit 5.00x) |
| scale | pipe-chain check | 4.00x | ≤ 5x | PASS | hashes 15517 → 62017 (4.00x, limit 5.00x) |
| scale | pipe-chain fmt | 3.77x | ≤ 5x | PASS | allocs 546 → 2056 (3.77x, limit 5.00x) |
| scale | pipe-chain build | 3.97x | ≤ 5x | PASS | hashes 82321 → 327013 (3.97x, limit 5.00x) |
| scale | question-chain check | 4.00x | ≤ 5x | PASS | hashes 41028 → 164028 (4.00x, limit 5.00x) |
| scale | question-chain fmt | 3.94x | ≤ 5x | PASS | allocs 1533 → 6044 (3.94x, limit 5.00x) |
| scale | question-chain build | 3.99x | ≤ 5x | PASS | loan_queries 4010 → 16010 (3.99x, limit 5.00x) |
| scale | if-nest check | 3.92x | ≤ 15.11x | PASS | hashes 756 → 2964 (3.92x, limit 15.11x) |
| scale | if-nest fmt | - | ≤ 15.11x | PASS | no counter reached the floor |
| scale | if-nest build | 4.02x | ≤ 15.11x | PASS | hashes 5063 → 20375 (4.02x, limit 15.11x) |
| scale | block-nest check | 3.95x | ≤ 15.29x | PASS | hashes 814 → 3214 (3.95x, limit 15.29x) |
| scale | block-nest fmt | - | ≤ 15.29x | PASS | no counter reached the floor |
| scale | block-nest build | 3.99x | ≤ 15.29x | PASS | hashes 5546 → 22130 (3.99x, limit 15.29x) |
| scale | for-nest check | 5.23x | ≤ 14.5x | PASS | hashes 3499 → 18283 (5.23x, limit 14.50x) |
| scale | for-nest fmt | - | ≤ 14.5x | PASS | no counter reached the floor |
| scale | for-nest build | 4.65x | ≤ 14.5x | PASS | hashes 25673 → 119289 (4.65x, limit 14.50x) |
| scale | while-nest check | 3.88x | ≤ 15.11x | PASS | hashes 1033 → 4009 (3.88x, limit 15.11x) |
| scale | while-nest fmt | - | ≤ 15.11x | PASS | no counter reached the floor |
| scale | while-nest build | 4.00x | ≤ 15.11x | PASS | use_facts 1232 → 4928 (4.00x, limit 15.11x) |
| scale | match-nest check | 4.87x | ≤ 16.03x | PASS | hashes 1776 → 8652 (4.87x, limit 16.03x) |
| scale | match-nest fmt | - | ≤ 16.03x | PASS | no counter reached the floor |
| scale | match-nest build | 4.32x | ≤ 16.03x | PASS | hashes 7912 → 34177 (4.32x, limit 16.03x) |
| scale | closure-nest check | 3.95x | ≤ 11.45x | PASS | hashes 1807 → 7141 (3.95x, limit 11.45x) |
| scale | closure-nest fmt | - | ≤ 11.45x | PASS | no counter reached the floor |
| scale | closure-nest build | 4.00x | ≤ 11.45x | PASS | use_facts 490 → 1960 (4.00x, limit 11.45x) |
| scale | option-nest check | 4.18x | ≤ 5x | PASS | hashes 1775 → 7421 (4.18x, limit 5.00x) |
| scale | option-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | option-nest build | 4.13x | ≤ 5x | PASS | hashes 11369 → 46991 (4.13x, limit 5.00x) |
| scale | list-type-nest check | 4.28x | ≤ 5x | PASS | hashes 1253 → 5364 (4.28x, limit 5.00x) |
| scale | list-type-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | list-type-nest build | 4.32x | ≤ 5x | PASS | hashes 4335 → 18722 (4.32x, limit 5.00x) |
| scale | generic-nest check | 4.33x | ≤ 5x | PASS | hashes 1580 → 6842 (4.33x, limit 5.00x) |
| scale | generic-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | generic-nest build | 4.03x | ≤ 5x | PASS | allocs 6736 → 27178 (4.03x, limit 5.00x) |
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
| scale | tuple-nest build | 4.08x | ≤ 5x | PASS | hashes 10514 → 42864 (4.08x, limit 5.00x) |
| scale | if-expr-nest check | 3.76x | ≤ 5x | PASS | hashes 322 → 1210 (3.76x, limit 5.00x) |
| scale | if-expr-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | if-expr-nest build | 3.84x | ≤ 5x | PASS | hashes 2130 → 8180 (3.84x, limit 5.00x) |
| scale | call-nest check | 3.90x | ≤ 5x | PASS | hashes 513 → 2001 (3.90x, limit 5.00x) |
| scale | call-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | call-nest build | 3.13x | ≤ 5x | PASS | hashes 3664 → 11476 (3.13x, limit 5.00x) |
| scale | index-nest check | - | ≤ 5x | PASS | no counter reached the floor |
| scale | index-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | index-nest build | 4.75x | ≤ 5x | PASS | hashes 1597 → 7591 (4.75x, limit 5.00x) |
| scale | fn-nest check | 4.03x | ≤ 13.42x | PASS | hashes 2674 → 10782 (4.03x, limit 13.42x) |
| scale | fn-nest fmt | - | ≤ 13.42x | PASS | no counter reached the floor |
| scale | fn-nest build | 4.00x | ≤ 13.42x | PASS | use_facts 784 → 3136 (4.00x, limit 13.42x) |
| scale | module-path-nest check | 1.58x | ≤ 5x | PASS | allocs 2503 → 3965 (1.58x, limit 5.00x) |
| scale | module-path-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | module-path-nest build | 1.53x | ≤ 5x | PASS | allocs 2545 → 3894 (1.53x, limit 5.00x) |
| scale | coverage: node kinds | 119/119 | 119/119 | PASS | exempt: Error |
| scale | coverage: node kinds repeated | 110/119 | reported | PASS | in no repeat shape: ReturnStmt, RangeExpr, LogicalOrExpr, LogicalAndExpr, BitwiseOrExpr, BitwiseXorExpr, BitwiseAndExpr, ShiftExpr, WhileExpr; exempt: Error |
| scale | coverage: node kinds nested | 57/119 | reported | PASS | in no nest shape: UseGroup, UseItem, EnumDecl, TraitDecl, ImplDecl, TypeDecl, TestsBlock, Decorator, DefaultValue, ConstParameter, LifetimeParameter, BoundList, TypeDefault, ConstArgument, AssociatedTypeBinding, NamedArgument, EmbeddedField, EnumVariant, VariantSharedData, AssociatedTypeDecl, Derivation, DiscardStmt, BreakStmt, ContinueStmt, UsingStmt, ElseIfClause, MatchGuard, PipeExpr, RefExpr, DerefExpr, PowerExpr, TryExpr, SuspendExpr, PathExpr, TypeArgsExpr, TrailingCallExpr, PlaceholderExpr, MapExpr, MapEntry, SpreadExpr, ComprehensionExpr, ComprehensionFor, ComprehensionIf, ContextExpr, ContextEntry, Interpolation, LiteralPattern, NamedPattern, DataPattern, DataPatternField, SpreadPattern, RangePattern, DynType, ImplType, RefType, UnionType, ParenType, FunctionType, ProjectionType, ContextType, RestType, InferType; exempt: Error |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 69 of 70; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 37 of 37 safe |
| fmt | hd fmt, 10k lines | 23 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.41x | ≤ 1.30x | FAIL | checked 324 ms, unchecked 230 ms |
| dev-speed | geomean dev / optimized | 1.50x | ≤ 4.00x | PASS | fib 0.96x, sum 1.33x, string-build 1.38x, map 1.23x, sort 3.44x |
| dev-speed | worst case dev / optimized | 3.44x | ≤ 10.00x | PASS | sort |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 30 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 28.1 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.15 CPU-s, 39.7 MB | completes ≤ 30 s | PASS | wall 102 ms |
| resources | check, small, warm | 0.01 CPU-s, 20.3 MB | completes ≤ 30 s | PASS | wall 18 ms |
| resources | test, small, cold | 0.93 CPU-s, 122.3 MB | completes ≤ 30 s | PASS | wall 469 ms |
| resources | test, small, warm | 0.06 CPU-s, 56.9 MB | completes ≤ 30 s | PASS | wall 64 ms |
| resources | build, small, cold | 0.78 CPU-s, 88.9 MB | completes ≤ 30 s | PASS | wall 302 ms |
| resources | build, small, warm | 0.03 CPU-s, 32.5 MB | completes ≤ 30 s | PASS | wall 32 ms |
| resources | check, 10k, cold | 0.39 CPU-s, 61.1 MB | completes ≤ 30 s | PASS | wall 187 ms |
| resources | check, 10k, warm | 0.03 CPU-s, 28.1 MB | completes ≤ 30 s | PASS | wall 38 ms |
| resources | test, 10k, cold | 1.53 CPU-s, 170.0 MB | completes ≤ 30 s | PASS | wall 861 ms |
| resources | test, 10k, warm | 0.15 CPU-s, 72.0 MB | completes ≤ 30 s | PASS | wall 133 ms |
| resources | build, 10k, cold | 1.05 CPU-s, 110.8 MB | completes ≤ 30 s | PASS | wall 378 ms |
| resources | build, 10k, warm | 0.05 CPU-s, 37.2 MB | completes ≤ 30 s | PASS | wall 49 ms |
| resources | check, 50k, cold | 1.48 CPU-s, 166.4 MB | completes ≤ 150 s | PASS | wall 579 ms |
| resources | check, 50k, warm | 0.13 CPU-s, 58.9 MB | completes ≤ 150 s | PASS | wall 123 ms |
| resources | test, 50k, cold | 3.99 CPU-s, 378.5 MB | completes ≤ 150 s | PASS | wall 2451 ms |
| resources | test, 50k, warm | 0.45 CPU-s, 158.3 MB | completes ≤ 150 s | PASS | wall 376 ms |
| resources | build, 50k, cold | 2.17 CPU-s, 214.6 MB | completes ≤ 150 s | PASS | wall 752 ms |
| resources | build, 50k, warm | 0.14 CPU-s, 67.0 MB | completes ≤ 150 s | PASS | wall 130 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 71.0 MB | ≤ 22.0 MB | FAIL | RSS 105.1 MB at input 100, 172.4 MB at input 1000; build per input 80 ms at 50, 1469 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.6 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 5.2 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd, just written: wall time | 15 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, just written: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, just written: peak RSS | 19.2 MB | ≤ 10.0 MB | FAIL |  |
| startup | hd check empty.hd, settled: wall time | 5.8 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd check empty.hd, settled: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, settled: peak RSS | 10.2 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 1.43x | ≤ 1.50x | PASS | p95 103 ms at N = 1; 4 cores; total CPU 210 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 1.86x | ≤ 6.00x | PASS | p95 103 ms at N = 1; 4 cores; total CPU 440 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 0.36 | &lt; 1.00 (sublinear) | PASS | 160 ms at N = 1, 440 ms at N = 16 |
| disk | artifacts per worktree | 0.0 MB | ≤ 10.0 MB | PASS | 10 files; HD_CACHE 11.4 MB, shared per user, not counted |
| disk | toolchain, once per machine | 31.8 MB | recorded, no target | PASS | toolchain files: /home/runner/work/hd-lang/hd-lang/target/dist/hd |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.96x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 212 files | the files of one check's cache | FAIL | one check alone: 36 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 14.3 MB | ≤ 5.0 MB | FAIL | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 1 | ≤ 1 | PASS | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 113k/s | ≥ 100k/s | PASS | 6000 extra cases took 53 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 17 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 107 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed Timing change ≤10%: noise. |
| unit-test-perf | overhead per unit test | 0.1 ms | ≤ 1.0 ms | PASS | 100 tests: 45 ms |
| integration-test-perf | setup per integration test program | 21 ms | ≤ 20 ms | FAIL | 10 programs: 255 ms for 21 tests, 12 ms per test |
| integration-test-perf | total time growth in programs | 0.846 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 56 ms at 2, 219 ms at 10 programs |
| integration-test-perf | cost per doc test | 16 ms | ≤ 20 ms | PASS | 10 doc tests: 201 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 0.56x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.69x | ≤ 3.00x | PASS | hd 6.1 ms (p10 6.1, p90 6.4), Node 8.9 ms, Python 69 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 0.17x | ≤ 3.00x | PASS | hd 7.5 ms (p10 6.6, p90 8.1), Node 43 ms, Python 247 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.77x | ≤ 3.00x | PASS | hd 3.9 ms (p10 3.9, p90 4.0), Node 5.0 ms, Python 11 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 1.01x | ≤ 3.00x | PASS | hd 5.9 ms (p10 5.9, p90 6.0), Node 5.9 ms, Python 14 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.62x | ≤ 3.00x | PASS | hd 11 ms (p10 11, p90 11), Node 18 ms, Python 14 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | list loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | map loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | iterator chain, bytes per iteration | - | ≤ 1 allocation (≤ 32 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| size-startup-heap | tiny program, release Wasm size | 5017 B | ≤ 2048 B | FAIL | benchmarks: fib 5141 B, sum 4917 B, string-build 7800 B, map 7095 B, sort 13034 B |
| size-startup-heap | tiny program, start to first output | 7.5 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm Timing change ≤10%: noise. |
| size-startup-heap | peak heap, map: hd / Node | 0.81x | ≤ 2.00x | PASS | hd +11.7 MB, Node +14.5 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.40x | ≤ 2.00x | PASS | hd +8.0 MB, Node +20.1 MB over an empty program |
| host-call-overhead | Console println, per call | 3.0 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 11 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 17 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 9.7 ns | ≤ 100 ns | PASS | 9.7 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 8.2M/s | ≥ 1.0M/s | PASS | 243 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 6.7M/s | ≥ 1.0M/s | PASS | 299 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.13x | ≥ 0.50x | FAIL | hd 83 MB/s (45488 B document), Node 618 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.15x | ≥ 0.50x | FAIL | hd 34 MB/s (45488 B document), Node 230 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 1.53x | ≥ 0.50x | PASS | hd 228 MB/s, Node 149 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 0.70x | ≥ 0.50x | PASS | hd 336 MB/s, Node 482 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.05x | ≥ 0.50x | FAIL | hd 6.3 MB/s, Node 120 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 6516 B | ≤ 10240 B | PASS | 12590 B at 1075 lines, 71238 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 915 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 13800000 requests; RSS 64.2 MB after warm-up, 64.2 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 8990 of 8990 unlisted cases passed |
| conformance | listed known failures | 3 | ≤ 0 | FAIL | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
- [2026-11-07T17](daily/2026-11-07T17.json)
