# Daily goal metrics

Finished: 2026-11-27T06:10:13.991Z. Commit: 8744354ab90c0fbe1def5c2908dcb2ab8802d7cb.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V74 80-Core Processor).
Compiler: target/dist/hd.

286 passed, 21 failed, 5 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 21 ms → pass 25 ms
- edit-latency: p95 check after an edit: pass 22 ms → pass 31 ms
- pathological: overlapping-from: time at 4x: pass 37 ms → pass 53 ms
- pathological: deep-blocks: time at 4x: pass 18 ms → pass 27 ms
- pathological: deep-blocks: peak RSS at 4x: pass 34.4 MB → pass 41.5 MB
- pathological: deep-parens: time at 4x: pass 19 ms → pass 25 ms
- pathological: deep-parens: peak RSS at 4x: pass 34.5 MB → pass 41.7 MB
- pathological: method-chain: time at 4x: pass 25 ms → pass 30 ms
- pathological: iterator-chain: time at 4x: pass 23 ms → pass 28 ms
- pathological: wide-list-literal: time at 4x: pass 37 ms → pass 44 ms
- pathological: large-enum-match: time at 4x: pass 23 ms → pass 30 ms
- pathological: deep-generic: time at 4x: pass 18 ms → pass 22 ms
- pathological: question-chain: time at 4x: pass 23 ms → pass 27 ms
- pathological: big-file: time at 4x: pass 104 ms → pass 116 ms
- scale: for-nest build: pass 4.62x → pass 6.32x
- scale: list-type-nest build: fail 148.43x → pass 3.99x
- scale: module-path-nest check: pass 4.32x → pass 5.07x
- scale: module-path-nest build: pass 11.83x → pass 8.02x
- fmt: hd fmt, 10k lines: pass 21 ms → pass 26 ms
- check-cost: checked run time / unchecked run time: pass 0.72x → fail 1.89x
- startup: hd help (no version command): wall time: pass 3.7 ms → pass 5.9 ms
- startup: hd check empty.hd, just written: wall time: pass 6.0 ms → pass 8.6 ms
- startup: hd check empty.hd, settled: wall time: pass 5.5 ms → pass 8.1 ms
- concurrency: p95 latency at N = 4 vs N = 1: fail 2.43x → fail 1.63x
- concurrency: total CPU growth exponent, N = 1 to 16: pass 0.36 → pass 0.45
- proptest-perf: property cases per second: pass 112k/s → fail 78k/s
- proptest-perf: time to shrink a known failure: pass 8.0 ms → pass 16 ms
- unit-test-perf: 1,000 unit tests, warm: pass 37 ms → pass 56 ms
- integration-test-perf: setup per integration test program: pass 5.8 ms → pass 6.5 ms
- integration-test-perf: cost per doc test: pass 3.5 ms → pass 4.3 ms
- runtime: geomean hd / Node: pass 0.57x → pass 0.44x
- runtime: sum: hd / Node: pass 0.46x → pass 0.13x
- runtime: map: hd / Node: pass 0.62x → pass 0.73x
- runtime: sort: hd / Node: pass 0.53x → pass 0.41x
- size-startup-heap: tiny program, start to first output: fail 5.6 ms → fail 8.3 ms
- size-startup-heap: peak heap, map: hd / Node: pass 0.50x → pass 0.77x
- host-call-overhead: Console println, per call: fail 1.4 µs → fail 2.3 µs
- host-call-overhead: Fs read_text!, per call: pass 3.1 µs → pass 9.8 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 6.2 µs → fail 16 µs
- suspension-overhead: all! of two ready tasks, throughput: pass 16.7M/s → pass 13.1M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 11.2M/s → pass 8.6M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.13x → fail 0.17x
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.23x → fail 0.19x
- text-throughput: string build, hd / Node MB/s: pass 3.11x → pass 2.75x
- text-throughput: split, hd / Node MB/s: pass 1.47x → pass 1.63x
- text-throughput: regex find_all, hd / Node MB/s: fail 0.45x → pass 0.65x

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 25 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 31 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 294 ms | ≤ 1000 ms | PASS | Timing change ≤10%: noise. |
| test-latency | median edit-then-test | 98 ms | ≤ 300 ms | PASS | Timing change ≤10%: noise. |
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
| pathological | overlapping-from: time at 4x | 53 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 46.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 34 → 53 ms, start-up 22 ms |
| pathological | deep-blocks: time at 4x | 27 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 41.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 21 → 27 ms, start-up 22 ms |
| pathological | deep-parens: time at 4x | 25 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 41.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 25 → 25 ms, start-up 22 ms |
| pathological | method-chain: time at 4x | 30 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 41.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 24 → 30 ms, start-up 22 ms |
| pathological | iterator-chain: time at 4x | 28 ms | ≤ 2000 ms | PASS |  |
| pathological | iterator-chain: peak RSS at 4x | 41.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | iterator-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 25 → 28 ms, start-up 22 ms |
| pathological | wide-list-literal: time at 4x | 44 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 48.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 33 → 44 ms, start-up 22 ms |
| pathological | large-enum-match: time at 4x | 30 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 42.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 24 → 30 ms, start-up 22 ms |
| pathological | deep-generic: time at 4x | 22 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 41.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 29 → 22 ms, start-up 22 ms |
| pathological | question-chain: time at 4x | 27 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 43.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 24 → 27 ms, start-up 22 ms |
| pathological | big-file: time at 4x | 116 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 85.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 67 → 116 ms, start-up 22 ms |
| scale | fn-items check | 4.04x | ≤ 5x | PASS | hashes 195617 → 789541 (4.04x, limit 5.00x) |
| scale | fn-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | fn-items build | 4.02x | ≤ 5x | PASS | hashes 545842 → 2195009 (4.02x, limit 5.00x) |
| scale | struct-items check | 4.02x | ≤ 5x | PASS | hashes 198591 → 799251 (4.02x, limit 5.00x) |
| scale | struct-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | struct-items build | 4.01x | ≤ 5x | PASS | hashes 728217 → 2920061 (4.01x, limit 5.00x) |
| scale | enum-items check | 4.00x | ≤ 5x | PASS | scope_probes 25500 → 102000 (4.00x, limit 5.00x) |
| scale | enum-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | enum-items build | 4.00x | ≤ 5x | PASS | scope_probes 25500 → 102000 (4.00x, limit 5.00x) |
| scale | trait-items check | 4.01x | ≤ 5x | PASS | allocs 525673 → 2108078 (4.01x, limit 5.00x) |
| scale | trait-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | trait-items build | 4.00x | ≤ 5x | PASS | allocs 1090433 → 4365282 (4.00x, limit 5.00x) |
| scale | impl-methods check | 4.05x | ≤ 5x | PASS | allocs 212215 → 859232 (4.05x, limit 5.00x) |
| scale | impl-methods fmt | 3.97x | ≤ 5x | PASS | allocs 4045 → 16057 (3.97x, limit 5.00x) |
| scale | impl-methods build | 4.01x | ≤ 5x | PASS | hashes 774346 → 3104875 (4.01x, limit 5.00x) |
| scale | traits-of-one-struct check | 4.01x | ≤ 5x | PASS | allocs 401092 → 1606647 (4.01x, limit 5.00x) |
| scale | traits-of-one-struct fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | traits-of-one-struct build | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | impls-of-one-trait check | 4.01x | ≤ 5x | PASS | allocs 374808 → 1504619 (4.01x, limit 5.00x) |
| scale | impls-of-one-trait fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | impls-of-one-trait build | 4.00x | ≤ 5x | PASS | hashes 1544848 → 6185694 (4.00x, limit 5.00x) |
| scale | operator-impls check | 4.01x | ≤ 5x | PASS | allocs 450421 → 1806727 (4.01x, limit 5.00x) |
| scale | operator-impls fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | operator-impls build | 4.00x | ≤ 5x | PASS | scope_probes 25000 → 100000 (4.00x, limit 5.00x) |
| scale | derive-items check | 4.00x | ≤ 5x | PASS | trait_lookup 23498 → 93998 (4.00x, limit 5.00x) |
| scale | derive-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | derive-items build | 4.00x | ≤ 5x | PASS | trait_lookup 23498 → 93998 (4.00x, limit 5.00x) |
| scale | dyn-values check | 4.01x | ≤ 5x | PASS | allocs 457037 → 1830966 (4.01x, limit 5.00x) |
| scale | dyn-values fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | dyn-values build | 4.00x | ≤ 5x | PASS | scope_probes 15500 → 62000 (4.00x, limit 5.00x) |
| scale | consts check | 3.95x | ≤ 5x | PASS | hashes 63544 → 251240 (3.95x, limit 5.00x) |
| scale | consts fmt | 3.89x | ≤ 5x | PASS | allocs 2087 → 8109 (3.89x, limit 5.00x) |
| scale | consts build | 4.00x | ≤ 5x | PASS | loan_queries 4506 → 18006 (4.00x, limit 5.00x) |
| scale | local-items check | 4.00x | ≤ 5x | PASS | scope_probes 21500 → 86000 (4.00x, limit 5.00x) |
| scale | local-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | local-items build | 4.00x | ≤ 5x | PASS | hashes 1422656 → 5694819 (4.00x, limit 5.00x) |
| scale | syntax-items check | 4.00x | ≤ 5x | PASS | item_body_checks 1700 → 6800 (4.00x, limit 5.00x) |
| scale | syntax-items fmt | 4.00x | ≤ 5x | PASS | hashes 17960 → 71864 (4.00x, limit 5.00x) |
| scale | syntax-items build | 4.00x | ≤ 5x | PASS | item_body_checks 1700 → 6800 (4.00x, limit 5.00x) |
| scale | struct-chain check | 4.04x | ≤ 5x | PASS | hashes 150918 → 610130 (4.04x, limit 5.00x) |
| scale | struct-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | struct-chain-values check | 4.00x | ≤ 5x | PASS | hashes 115300 → 461628 (4.00x, limit 5.00x) |
| scale | struct-chain-values fmt | 4.02x | ≤ 5x | PASS | hashes 2082 → 8370 (4.02x, limit 5.00x) |
| scale | struct-chain-values build | 4.00x | ≤ 5x | PASS | scope_probes 4900 → 19600 (4.00x, limit 5.00x) |
| scale | modules check | 4.00x | ≤ 5x | PASS | hashes 1153554 → 4616350 (4.00x, limit 5.00x) |
| scale | modules fmt | 4.00x | ≤ 5x | PASS | hashes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | modules build | 4.00x | ≤ 5x | PASS | hashes 1511330 → 6050519 (4.00x, limit 5.00x) |
| scale | use-names check | 4.03x | ≤ 5x | PASS | hashes 215522 → 869190 (4.03x, limit 5.00x) |
| scale | use-names fmt | 4.01x | ≤ 5x | PASS | hashes 3783 → 15159 (4.01x, limit 5.00x) |
| scale | use-names build | 4.01x | ≤ 5x | PASS | hashes 567490 → 2276793 (4.01x, limit 5.00x) |
| scale | generic-fns check | 4.01x | ≤ 5x | PASS | hashes 303240 → 1214500 (4.01x, limit 5.00x) |
| scale | generic-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-fns build | 4.01x | ≤ 5x | PASS | hashes 976040 → 3916641 (4.01x, limit 5.00x) |
| scale | generic-params check | 4.00x | ≤ 5x | PASS | scope_probes 800 → 3200 (4.00x, limit 5.00x) |
| scale | generic-params fmt | 4.10x | ≤ 10x | PASS | alloc_bytes 350594 → 1438988 (4.10x, limit 10.00x) |
| scale | generic-params build | 4.07x | ≤ 5x | PASS | hashes 21307 → 86825 (4.07x, limit 5.00x) |
| scale | param-bounds check | 4.00x | ≤ 5x | PASS | scope_probes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | param-bounds fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | param-bounds build | 4.01x | ≤ 5x | PASS | hashes 56454 → 226262 (4.01x, limit 5.00x) |
| scale | type-defaults check | 4.01x | ≤ 5x | PASS | allocs 689207 → 2762654 (4.01x, limit 5.00x) |
| scale | type-defaults fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | type-defaults build | 4.00x | ≤ 5x | PASS | allocs 1325031 → 5301066 (4.00x, limit 5.00x) |
| scale | alias-chain check | 4.00x | ≤ 5x | PASS | scope_probes 6600 → 26400 (4.00x, limit 5.00x) |
| scale | alias-chain fmt | 4.03x | ≤ 5x | PASS | hashes 1234 → 4978 (4.03x, limit 5.00x) |
| scale | alias-chain build | 4.00x | ≤ 5x | PASS | scope_probes 6600 → 26400 (4.00x, limit 5.00x) |
| scale | row-chain check | 4.03x | ≤ 5x | PASS | hashes 219344 → 883808 (4.03x, limit 5.00x) |
| scale | row-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | row-chain build | 4.03x | ≤ 5x | PASS | hashes 582236 → 2344288 (4.03x, limit 5.00x) |
| scale | row-traits check | 4.00x | ≤ 5x | PASS | scope_probes 2300 → 9200 (4.00x, limit 5.00x) |
| scale | row-traits fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | row-traits build | 4.00x | ≤ 5x | PASS | scope_probes 2300 → 9200 (4.00x, limit 5.00x) |
| scale | closures check | 4.00x | ≤ 5x | PASS | scope_probes 2000 → 8000 (4.00x, limit 5.00x) |
| scale | closures fmt | 3.94x | ≤ 5x | PASS | allocs 3061 → 12073 (3.94x, limit 5.00x) |
| scale | closures build | 4.00x | ≤ 5x | PASS | scope_probes 2000 → 8000 (4.00x, limit 5.00x) |
| scale | generic-local-fns check | 4.03x | ≤ 5x | PASS | hashes 235963 → 951151 (4.03x, limit 5.00x) |
| scale | generic-local-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-local-fns build | 4.01x | ≤ 5x | PASS | hashes 1140348 → 4575611 (4.01x, limit 5.00x) |
| scale | let-statements check | 4.00x | ≤ 5x | PASS | scope_probes 500 → 2000 (4.00x, limit 5.00x) |
| scale | let-statements fmt | 3.94x | ≤ 5x | PASS | allocs 2554 → 10066 (3.94x, limit 5.00x) |
| scale | let-statements build | 4.08x | ≤ 5x | PASS | hashes 284903 → 1161760 (4.08x, limit 5.00x) |
| scale | matches check | 4.00x | ≤ 5x | PASS | scope_probes 1000 → 4000 (4.00x, limit 5.00x) |
| scale | matches fmt | 3.97x | ≤ 5x | PASS | allocs 6057 → 24069 (3.97x, limit 5.00x) |
| scale | matches build | 4.00x | ≤ 5x | PASS | scope_probes 1000 → 4000 (4.00x, limit 5.00x) |
| scale | enum-arms check | 4.00x | ≤ 5x | PASS | scope_probes 5000 → 20000 (4.00x, limit 5.00x) |
| scale | enum-arms fmt | 3.91x | ≤ 5x | PASS | allocs 1551 → 6065 (3.91x, limit 5.00x) |
| scale | enum-arms build | 4.00x | ≤ 5x | PASS | scope_probes 5000 → 20000 (4.00x, limit 5.00x) |
| scale | tuple-patterns check | 4.00x | ≤ 5x | PASS | scope_probes 1000 → 4000 (4.00x, limit 5.00x) |
| scale | tuple-patterns fmt | 3.95x | ≤ 5x | PASS | allocs 3054 → 12066 (3.95x, limit 5.00x) |
| scale | tuple-patterns build | 4.00x | ≤ 5x | PASS | scope_probes 1000 → 4000 (4.00x, limit 5.00x) |
| scale | range-patterns check | 4.02x | ≤ 5x | PASS | hashes 253642 → 1019450 (4.02x, limit 5.00x) |
| scale | range-patterns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | range-patterns build | 4.01x | ≤ 5x | PASS | hashes 940990 → 3775397 (4.01x, limit 5.00x) |
| scale | wide-struct check | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | wide-struct fmt | 3.95x | ≤ 5x | PASS | allocs 2544 → 10056 (3.95x, limit 5.00x) |
| scale | wide-struct build | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | call-args check | 4.00x | ≤ 5x | PASS | scope_probes 1100 → 4400 (4.00x, limit 5.00x) |
| scale | call-args fmt | 4.10x | ≤ 10x | PASS | alloc_bytes 361569 → 1482933 (4.10x, limit 10.00x) |
| scale | call-args build | 4.03x | ≤ 5x | PASS | hashes 23464 → 94636 (4.03x, limit 5.00x) |
| scale | binary-chain check | 4.00x | ≤ 5x | PASS | hashes 2500 → 10000 (4.00x, limit 5.00x) |
| scale | binary-chain fmt | 3.78x | ≤ 5x | PASS | allocs 545 → 2060 (3.78x, limit 5.00x) |
| scale | binary-chain build | 4.04x | ≤ 5x | PASS | hashes 35420 → 143021 (4.04x, limit 5.00x) |
| scale | interpolation check | 4.00x | ≤ 5x | PASS | trait_lookup 2501 → 10001 (4.00x, limit 5.00x) |
| scale | interpolation fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | interpolation build | 4.03x | ≤ 5x | PASS | hashes 4446857 → 17940254 (4.03x, limit 5.00x) |
| scale | list-literal check | 4.00x | ≤ 5x | PASS | hashes 7392 → 29580 (4.00x, limit 5.00x) |
| scale | list-literal fmt | 3.82x | ≤ 5x | PASS | allocs 533 → 2038 (3.82x, limit 5.00x) |
| scale | list-literal build | 4.11x | ≤ 5x | PASS | hashes 59401 → 244397 (4.11x, limit 5.00x) |
| scale | method-chain check | 4.00x | ≤ 5x | PASS | trait_lookup 500 → 2000 (4.00x, limit 5.00x) |
| scale | method-chain fmt | 4.04x | ≤ 10x | PASS | alloc_bytes 838129 → 3386711 (4.04x, limit 10.00x) |
| scale | method-chain build | 4.00x | ≤ 5x | PASS | trait_lookup 500 → 2000 (4.00x, limit 5.00x) |
| scale | pipe-chain check | 4.00x | ≤ 5x | PASS | scope_probes 500 → 2000 (4.00x, limit 5.00x) |
| scale | pipe-chain fmt | 3.77x | ≤ 5x | PASS | allocs 546 → 2056 (3.77x, limit 5.00x) |
| scale | pipe-chain build | 4.00x | ≤ 5x | PASS | scope_probes 500 → 2000 (4.00x, limit 5.00x) |
| scale | question-chain check | 4.00x | ≤ 5x | PASS | scope_probes 500 → 2000 (4.00x, limit 5.00x) |
| scale | question-chain fmt | 3.94x | ≤ 5x | PASS | allocs 1533 → 6044 (3.94x, limit 5.00x) |
| scale | question-chain build | 4.00x | ≤ 5x | PASS | scope_probes 500 → 2000 (4.00x, limit 5.00x) |
| scale | if-nest check | 3.92x | ≤ 15.11x | PASS | hashes 756 → 2964 (3.92x, limit 15.11x) |
| scale | if-nest fmt | - | ≤ 15.11x | PASS | no counter reached the floor |
| scale | if-nest build | 4.08x | ≤ 15.11x | PASS | hashes 6426 → 26245 (4.08x, limit 15.11x) |
| scale | block-nest check | 3.95x | ≤ 15.29x | PASS | hashes 814 → 3214 (3.95x, limit 15.29x) |
| scale | block-nest fmt | - | ≤ 15.29x | PASS | no counter reached the floor |
| scale | block-nest build | 4.05x | ≤ 15.29x | PASS | hashes 6938 → 28080 (4.05x, limit 15.29x) |
| scale | for-nest check | 5.30x | ≤ 14.5x | PASS | hashes 3515 → 18627 (5.30x, limit 14.50x) |
| scale | for-nest fmt | - | ≤ 14.5x | PASS | no counter reached the floor |
| scale | for-nest build | 6.32x | ≤ 14.5x | PASS | hashes 34866 → 220228 (6.32x, limit 14.50x) |
| scale | while-nest check | 3.88x | ≤ 15.11x | PASS | hashes 1033 → 4009 (3.88x, limit 15.11x) |
| scale | while-nest fmt | - | ≤ 15.11x | PASS | no counter reached the floor |
| scale | while-nest build | 4.00x | ≤ 15.11x | PASS | control 400 → 1600 (4.00x, limit 15.11x) |
| scale | match-nest check | 4.83x | ≤ 16.03x | PASS | hashes 1928 → 9316 (4.83x, limit 16.03x) |
| scale | match-nest fmt | - | ≤ 16.03x | PASS | no counter reached the floor |
| scale | match-nest build | 4.27x | ≤ 16.03x | PASS | hashes 7798 → 33271 (4.27x, limit 16.03x) |
| scale | closure-nest check | 4.13x | ≤ 11.45x | PASS | hashes 1940 → 8010 (4.13x, limit 11.45x) |
| scale | closure-nest fmt | - | ≤ 11.45x | PASS | no counter reached the floor |
| scale | closure-nest build | 4.00x | ≤ 11.45x | PASS | use_facts 560 → 2240 (4.00x, limit 11.45x) |
| scale | option-nest check | 4.19x | ≤ 5x | PASS | hashes 1816 → 7600 (4.19x, limit 5.00x) |
| scale | option-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | option-nest build | 4.09x | ≤ 5x | PASS | hashes 13078 → 53460 (4.09x, limit 5.00x) |
| scale | list-type-nest check | 4.27x | ≤ 5x | PASS | hashes 1254 → 5351 (4.27x, limit 5.00x) |
| scale | list-type-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | list-type-nest build | 3.99x | ≤ 5x | PASS | hashes 5861 → 23381 (3.99x, limit 5.00x) |
| scale | generic-nest check | 4.32x | ≤ 5x | PASS | hashes 1597 → 6899 (4.32x, limit 5.00x) |
| scale | generic-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | generic-nest build | 4.28x | ≤ 5x | PASS | hashes 6490 → 27779 (4.28x, limit 5.00x) |
| scale | paren-nest check | - | ≤ 5x | PASS | no counter reached the floor |
| scale | paren-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | paren-nest build | 3.80x | ≤ 5x | PASS | allocs 794 → 3019 (3.80x, limit 5.00x) |
| scale | arith-nest check | 4.00x | ≤ 5x | PASS | hashes 440 → 1760 (4.00x, limit 5.00x) |
| scale | arith-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | arith-nest build | 4.17x | ≤ 5x | PASS | hashes 7466 → 31147 (4.17x, limit 5.00x) |
| scale | bool-nest check | 3.95x | ≤ 5x | PASS | hashes 391 → 1543 (3.95x, limit 5.00x) |
| scale | bool-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | bool-nest build | 4.09x | ≤ 5x | PASS | use_facts 490 → 2002 (4.09x, limit 5.00x) |
| scale | bit-nest check | 3.99x | ≤ 5x | PASS | hashes 818 → 3266 (3.99x, limit 5.00x) |
| scale | bit-nest fmt | 5.84x | ≤ 10x | PASS | alloc_bytes 206985 → 1209058 (5.84x, limit 10.00x) |
| scale | bit-nest build | 4.25x | ≤ 5x | PASS | hashes 10997 → 46685 (4.25x, limit 5.00x) |
| scale | tuple-nest check | 3.96x | ≤ 5x | PASS | hashes 2048 → 8112 (3.96x, limit 5.00x) |
| scale | tuple-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | tuple-nest build | 4.00x | ≤ 5x | PASS | hashes 12844 → 51420 (4.00x, limit 5.00x) |
| scale | if-expr-nest check | 3.76x | ≤ 5x | PASS | hashes 322 → 1210 (3.76x, limit 5.00x) |
| scale | if-expr-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | if-expr-nest build | 3.82x | ≤ 5x | PASS | use_facts 357 → 1365 (3.82x, limit 5.00x) |
| scale | call-nest check | 3.87x | ≤ 5x | PASS | hashes 552 → 2136 (3.87x, limit 5.00x) |
| scale | call-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | call-nest build | 2.90x | ≤ 5x | PASS | hashes 5162 → 14969 (2.90x, limit 5.00x) |
| scale | index-nest check | - | ≤ 5x | PASS | no counter reached the floor |
| scale | index-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | index-nest build | 4.06x | ≤ 5x | PASS | hashes 3236 → 13146 (4.06x, limit 5.00x) |
| scale | fn-nest check | 3.72x | ≤ 13.42x | PASS | hashes 3174 → 11810 (3.72x, limit 13.42x) |
| scale | fn-nest fmt | - | ≤ 13.42x | PASS | no counter reached the floor |
| scale | fn-nest build | 4.00x | ≤ 13.42x | PASS | use_facts 896 → 3584 (4.00x, limit 13.42x) |
| scale | module-path-nest check | 5.07x | ≤ 14.36x | PASS | hashes 35284 → 178940 (5.07x, limit 14.36x) |
| scale | module-path-nest fmt | 9.99x | ≤ 28.72x | PASS | alloc_bytes 158233 → 1580610 (9.99x, limit 28.72x) |
| scale | module-path-nest build | 8.02x | ≤ 14.36x | PASS | hashes 1590 → 12748 (8.02x, limit 14.36x) |
| scale | coverage: node kinds | 122/122 | 122/122 | PASS | exempt: Error |
| scale | coverage: node kinds repeated | 113/122 | reported | PASS | in no repeat shape: ReturnStmt, RangeExpr, LogicalOrExpr, LogicalAndExpr, BitwiseOrExpr, BitwiseXorExpr, BitwiseAndExpr, ShiftExpr, WhileExpr; exempt: Error |
| scale | coverage: node kinds nested | 59/122 | reported | PASS | in no nest shape: EnumDecl, TraitDecl, ImplDecl, TypeDecl, TestsBlock, Decorator, DefaultValue, ConstParameter, LifetimeParameter, BoundList, WhereClause, WhereBound, TypeDefault, ConstArgument, AssociatedTypeBinding, NamedArgument, EmbeddedField, EnumVariant, VariantSharedData, AssociatedTypeDecl, Derivation, DiscardStmt, BreakStmt, ContinueStmt, UsingStmt, ElseIfClause, MatchGuard, PipeExpr, RefExpr, DerefExpr, PowerExpr, TryExpr, SuspendExpr, PathExpr, TypeArgsExpr, TrailingCallExpr, PlaceholderExpr, MapExpr, MapEntry, SpreadExpr, ComprehensionExpr, ComprehensionFor, ComprehensionIf, ContextExpr, ContextEntry, Interpolation, LiteralPattern, NamedPattern, DataPattern, DataPatternField, SpreadPattern, RangePattern, DynType, ImplType, RefType, UnionType, ParenType, FunctionType, ProjectionType, ContextType, RestType, InferType, MarkerBound; exempt: Error |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 4 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 38 of 38 safe |
| fmt | hd fmt, 10k lines | 26 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.89x | ≤ 1.30x | FAIL | checked 326 ms, unchecked 172 ms |
| dev-speed | geomean dev / optimized | 1.49x | ≤ 4.00x | PASS | fib 0.95x, sum 1.31x, string-build 1.82x, map 1.17x, sort 2.76x |
| dev-speed | worst case dev / optimized | 2.76x | ≤ 10.00x | PASS | sort |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 0.0 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 21.9 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.12 CPU-s, 56.1 MB | completes ≤ 30 s | PASS | wall 137 ms |
| resources | check, small, warm | 0.00 CPU-s, 22.2 MB | completes ≤ 30 s | PASS | wall 8.8 ms |
| resources | test, small, cold | 0.67 CPU-s, 111.0 MB | completes ≤ 30 s | PASS | wall 686 ms |
| resources | test, small, warm | 0.02 CPU-s, 43.7 MB | completes ≤ 30 s | PASS | wall 36 ms |
| resources | build, small, cold | 0.51 CPU-s, 96.4 MB | completes ≤ 30 s | PASS | wall 532 ms |
| resources | build, small, warm | 0.00 CPU-s, 21.9 MB | completes ≤ 30 s | PASS | wall 9.6 ms |
| resources | check, 10k, cold | 0.30 CPU-s, 90.0 MB | completes ≤ 30 s | PASS | wall 311 ms |
| resources | check, 10k, warm | 0.00 CPU-s, 21.9 MB | completes ≤ 30 s | PASS | wall 9.9 ms |
| resources | test, 10k, cold | 1.09 CPU-s, 159.9 MB | completes ≤ 30 s | PASS | wall 1107 ms |
| resources | test, 10k, warm | 0.04 CPU-s, 60.0 MB | completes ≤ 30 s | PASS | wall 58 ms |
| resources | build, 10k, cold | 0.54 CPU-s, 98.2 MB | completes ≤ 30 s | PASS | wall 555 ms |
| resources | build, 10k, warm | 0.00 CPU-s, 22.1 MB | completes ≤ 30 s | PASS | wall 11 ms |
| resources | check, 50k, cold | 1.03 CPU-s, 238.1 MB | completes ≤ 150 s | PASS | wall 1034 ms |
| resources | check, 50k, warm | 0.00 CPU-s, 30.6 MB | completes ≤ 150 s | PASS | wall 15 ms |
| resources | test, 50k, cold | 2.94 CPU-s, 372.1 MB | completes ≤ 150 s | PASS | wall 2949 ms |
| resources | test, 50k, warm | 0.19 CPU-s, 118.8 MB | completes ≤ 150 s | PASS | wall 202 ms |
| resources | build, 50k, cold | 0.54 CPU-s, 106.2 MB | completes ≤ 150 s | PASS | wall 553 ms |
| resources | build, 50k, warm | 0.00 CPU-s, 31.0 MB | completes ≤ 150 s | PASS | wall 14 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 108.4 MB | ≤ 22.0 MB | FAIL | RSS 87.8 MB at input 100, 186.0 MB at input 1000; build per input 109 ms at 50, 1537 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 5.9 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 9.7 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd, just written: wall time | 8.6 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, just written: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, just written: peak RSS | 21.4 MB | ≤ 10.0 MB | FAIL |  |
| startup | hd check empty.hd, settled: wall time | 8.1 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, settled: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, settled: peak RSS | 21.4 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 1.63x | ≤ 1.50x | FAIL | p95 104 ms at N = 1; 4 cores; total CPU 270 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 2.65x | ≤ 6.00x | PASS | p95 104 ms at N = 1; 4 cores; total CPU 550 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 0.45 | &lt; 1.00 (sublinear) | PASS | 160 ms at N = 1, 550 ms at N = 16 |
| disk | artifacts per worktree | 0.0 MB | ≤ 10.0 MB | PASS | 10 files; HD_CACHE 11.4 MB, shared per user, not counted |
| disk | toolchain, once per machine | 34.0 MB | recorded, no target | PASS | toolchain files: /home/runner/work/hd-lang/hd-lang/target/dist/hd |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 2.10x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 311 files | the files of one check's cache | FAIL | one check alone: 36 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 13.1 MB | ≤ 5.0 MB | FAIL | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 1 | ≤ 1 | PASS | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 78k/s | ≥ 100k/s | FAIL | 6000 extra cases took 77 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 16 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 56 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.0 ms | ≤ 1.0 ms | PASS | 100 tests: 24 ms |
| integration-test-perf | setup per integration test program | 6.5 ms | ≤ 20 ms | PASS | 10 programs: 84 ms for 21 tests, 4.0 ms per test |
| integration-test-perf | total time growth in programs | 0.842 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 18 ms at 2, 69 ms at 10 programs |
| integration-test-perf | cost per doc test | 4.3 ms | ≤ 20 ms | PASS | 10 doc tests: 59 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 0.44x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.76x | ≤ 3.00x | PASS | hd 6.1 ms (p10 6.1, p90 6.3), Node 8.1 ms, Python 76 ms; release build, run as hd run |
| runtime | sum: hd / Node | 0.13x | ≤ 3.00x | PASS | hd 5.5 ms (p10 5.4, p90 7.0), Node 43 ms, Python 237 ms; release build, run as hd run |
| runtime | string-build: hd / Node | 0.55x | ≤ 3.00x | PASS | hd 2.7 ms (p10 2.7, p90 2.7), Node 5.0 ms, Python 11 ms; release build, run as hd run |
| runtime | map: hd / Node | 0.73x | ≤ 3.00x | PASS | hd 4.3 ms (p10 4.2, p90 4.4), Node 5.9 ms, Python 15 ms; release build, run as hd run |
| runtime | sort: hd / Node | 0.41x | ≤ 3.00x | PASS | hd 8.5 ms (p10 8.4, p90 8.6), Node 21 ms, Python 15 ms; release build, run as hd run |
| allocations | counted loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | list loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | map loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | iterator chain, bytes per iteration | - | ≤ 1 allocation (≤ 32 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| size-startup-heap | tiny program, release Wasm size | - | ≤ 2048 B | N/A | not supported by this hd: the build wrote no .wasm file (release build, run as hd run) |
| size-startup-heap | tiny program, start to first output | 8.3 ms | ≤ 5.0 ms | FAIL | release build, run as hd run |
| size-startup-heap | peak heap, map: hd / Node | 0.77x | ≤ 2.00x | PASS | hd +10.9 MB, Node +14.3 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.16x | ≤ 2.00x | PASS | hd +3.2 MB, Node +20.2 MB over an empty program |
| host-call-overhead | Console println, per call | 2.3 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd run |
| host-call-overhead | Fs read_text!, per call | 9.8 µs | ≤ 10 µs | PASS | 10000 calls; release build, run as hd run |
| host-call-overhead | serde boundary (list_dir!), per call | 16 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd run |
| suspension-overhead | await of a ready value, per await | 6.5 ns | ≤ 100 ns | PASS | 6.5 ns per iteration; release build, run as hd run |
| suspension-overhead | all! of two ready tasks, throughput | 13.1M/s | ≥ 1.0M/s | PASS | 153 ns per iteration; release build, run as hd run |
| suspension-overhead | race! of two ready tasks, throughput | 8.6M/s | ≥ 1.0M/s | PASS | 232 ns per iteration; release build, run as hd run |
| serde-throughput | JSON encode, hd / Node MB/s | 0.17x | ≥ 0.50x | FAIL | hd 103 MB/s (45488 B document), Node 617 MB/s; release build, run as hd run |
| serde-throughput | JSON decode, hd / Node MB/s | 0.19x | ≥ 0.50x | FAIL | hd 43 MB/s (45488 B document), Node 230 MB/s; release build, run as hd run |
| text-throughput | string build, hd / Node MB/s | 2.75x | ≥ 0.50x | PASS | hd 440 MB/s, Node 160 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd run |
| text-throughput | split, hd / Node MB/s | 1.63x | ≥ 0.50x | PASS | hd 485 MB/s, Node 298 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd run |
| text-throughput | regex find_all, hd / Node MB/s | 0.65x | ≥ 0.50x | PASS | hd 49 MB/s, Node 74 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd run |
| dead-code | Wasm bytes per 1,000 lines | - | ≤ 10240 B | FAIL | the build wrote no file to measure (release build, run as hd run) |
| dead-code | no unused std in the program | - | none | FAIL | the build wrote no file to measure (release build, run as hd run) |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 17702000 requests; RSS 40.4 MB after warm-up, 40.4 MB at the end; release build, run as hd run |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 9434 of 9434 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
- [2026-11-13T19](daily/2026-11-13T19.json)
