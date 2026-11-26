# Daily goal metrics

Finished: 2026-11-26T21:36:59.102Z. Commit: 6f11a5b3ba7ea1ac2183f68426a1452733234576.
Machine: ubuntu-latest, 4 CPUs (Intel(R) Xeon(R) 6975P-C).
Compiler: target/dist/hd.

286 passed, 21 failed, 5 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p95 check after an edit: pass 30 ms → pass 22 ms
- pathological: overlapping-from: time at 4x: pass 43 ms → pass 37 ms
- pathological: deep-blocks: peak RSS at 4x: pass 39.7 MB → pass 34.4 MB
- pathological: deep-parens: peak RSS at 4x: pass 39.6 MB → pass 34.5 MB
- pathological: large-enum-match: time at 4x: pass 26 ms → pass 23 ms
- pathological: question-chain: time at 4x: pass 26 ms → pass 23 ms
- pathological: big-file: time at 4x: pass 118 ms → pass 104 ms
- scale: module-path-nest build: pass 4.73x → pass 11.83x
- fmt: hd fmt, 10k lines: pass 26 ms → pass 21 ms
- check-cost: checked run time / unchecked run time: fail 1.43x → pass 0.72x
- dev-speed: geomean dev / optimized: pass 1.44x → pass 1.62x
- dev-speed: worst case dev / optimized: pass 2.83x → pass 2.52x
- long-session: RSS growth, REPL inputs 100 to 1000: fail 95.9 MB → fail 108.3 MB
- startup: hd help (no version command): wall time: pass 4.4 ms → pass 3.7 ms
- startup: hd check empty.hd, just written: wall time: pass 7.6 ms → pass 6.0 ms
- startup: hd check empty.hd, settled: wall time: pass 6.7 ms → pass 5.5 ms
- concurrency: p95 latency at N = 4 vs N = 1: fail 1.60x → fail 2.43x
- concurrency: p95 latency at N = 16 vs N = 1: pass 4.47x → pass 2.41x
- concurrency: total CPU growth exponent, N = 1 to 16: pass 0.56 → pass 0.36
- proptest-perf: time to shrink a known failure: pass 14 ms → pass 8.0 ms
- unit-test-perf: 1,000 unit tests, warm: pass 49 ms → pass 37 ms
- integration-test-perf: setup per integration test program: pass 8.6 ms → pass 5.8 ms
- integration-test-perf: cost per doc test: pass 5.0 ms → pass 3.5 ms
- runtime: sum: hd / Node: pass 0.18x → pass 0.46x
- runtime: string-build: hd / Node: pass 0.66x → pass 0.52x
- runtime: map: hd / Node: pass 0.82x → pass 0.62x
- size-startup-heap: tiny program, release Wasm size: fail 4970 B → n/a -
- size-startup-heap: tiny program, start to first output: fail 7.2 ms → fail 5.6 ms
- size-startup-heap: peak heap, map: hd / Node: pass 0.96x → pass 0.50x
- size-startup-heap: peak heap, string-build: hd / Node: pass 0.50x → pass 0.16x
- host-call-overhead: Console println, per call: fail 2.8 µs → fail 1.4 µs
- host-call-overhead: Fs read_text!, per call: fail 10 µs → pass 3.1 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 16 µs → fail 6.2 µs
- suspension-overhead: await of a ready value, per await: pass 3.5 ns → pass 6.5 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 10.2M/s → pass 16.7M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 8.0M/s → pass 11.2M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.18x → fail 0.13x
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.19x → fail 0.23x
- text-throughput: string build, hd / Node MB/s: pass 2.30x → pass 3.11x
- text-throughput: split, hd / Node MB/s: pass 0.86x → pass 1.47x
- text-throughput: regex find_all, hd / Node MB/s: fail 0.20x → fail 0.45x
- dead-code: Wasm bytes per 1,000 lines: pass 6516 B → fail -
- dead-code: no unused std in the program: pass none → fail -

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 21 ms | ≤ 50 ms | PASS | Timing change ≤10%: noise. |
| edit-latency | p95 check after an edit | 22 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 288 ms | ≤ 1000 ms | PASS | Timing change ≤10%: noise. |
| test-latency | median edit-then-test | 94 ms | ≤ 300 ms | PASS | Timing change ≤10%: noise. |
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
| pathological | overlapping-from: time at 4x | 37 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 45.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 26 → 37 ms, start-up 16 ms |
| pathological | deep-blocks: time at 4x | 18 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-blocks: peak RSS at 4x | 34.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 16 → 18 ms, start-up 16 ms |
| pathological | deep-parens: time at 4x | 19 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 34.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 17 → 19 ms, start-up 16 ms |
| pathological | method-chain: time at 4x | 25 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | method-chain: peak RSS at 4x | 41.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 25 ms, start-up 16 ms |
| pathological | iterator-chain: time at 4x | 23 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | iterator-chain: peak RSS at 4x | 41.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | iterator-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 23 ms, start-up 16 ms |
| pathological | wide-list-literal: time at 4x | 37 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | wide-list-literal: peak RSS at 4x | 45.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 25 → 37 ms, start-up 16 ms |
| pathological | large-enum-match: time at 4x | 23 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 41.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 23 ms, start-up 16 ms |
| pathological | deep-generic: time at 4x | 18 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-generic: peak RSS at 4x | 39.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 17 → 18 ms, start-up 16 ms |
| pathological | question-chain: time at 4x | 23 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 41.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 23 ms, start-up 16 ms |
| pathological | big-file: time at 4x | 104 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 83.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 60 → 104 ms, start-up 16 ms |
| scale | fn-items check | 4.03x | ≤ 5x | PASS | hashes 205467 → 828799 (4.03x, limit 5.00x) |
| scale | fn-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | fn-items build | 4.02x | ≤ 5x | PASS | hashes 556233 → 2235526 (4.02x, limit 5.00x) |
| scale | struct-items check | 4.02x | ≤ 5x | PASS | hashes 211907 → 852475 (4.02x, limit 5.00x) |
| scale | struct-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | struct-items build | 4.01x | ≤ 5x | PASS | hashes 710521 → 2848756 (4.01x, limit 5.00x) |
| scale | enum-items check | 4.00x | ≤ 5x | PASS | scope_probes 25500 → 102000 (4.00x, limit 5.00x) |
| scale | enum-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | enum-items build | 4.00x | ≤ 5x | PASS | hashes 1472213 → 5895466 (4.00x, limit 5.00x) |
| scale | trait-items check | 4.00x | ≤ 5x | PASS | scope_probes 20000 → 80000 (4.00x, limit 5.00x) |
| scale | trait-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | trait-items build | 4.01x | ≤ 5x | PASS | hashes 1942776 → 7798179 (4.01x, limit 5.00x) |
| scale | impl-methods check | 4.00x | ≤ 5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 5.00x) |
| scale | impl-methods fmt | 3.97x | ≤ 5x | PASS | allocs 4045 → 16057 (3.97x, limit 5.00x) |
| scale | impl-methods build | 4.01x | ≤ 5x | PASS | hashes 779538 → 3123542 (4.01x, limit 5.00x) |
| scale | traits-of-one-struct check | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | traits-of-one-struct fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | traits-of-one-struct build | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | impls-of-one-trait check | 4.01x | ≤ 5x | PASS | hashes 626669 → 2511865 (4.01x, limit 5.00x) |
| scale | impls-of-one-trait fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | impls-of-one-trait build | 4.00x | ≤ 5x | PASS | hashes 1572809 → 6294521 (4.00x, limit 5.00x) |
| scale | operator-impls check | 4.00x | ≤ 5x | PASS | hashes 755213 → 3022669 (4.00x, limit 5.00x) |
| scale | operator-impls fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | operator-impls build | 4.00x | ≤ 5x | PASS | hashes 1552548 → 6213616 (4.00x, limit 5.00x) |
| scale | derive-items check | 4.00x | ≤ 5x | PASS | trait_lookup 16998 → 67998 (4.00x, limit 5.00x) |
| scale | derive-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | derive-items build | 4.00x | ≤ 5x | PASS | trait_lookup 16998 → 67998 (4.00x, limit 5.00x) |
| scale | dyn-values check | 4.01x | ≤ 5x | PASS | hashes 806876 → 3234158 (4.01x, limit 5.00x) |
| scale | dyn-values fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | dyn-values build | 4.00x | ≤ 5x | PASS | hashes 2195945 → 8788042 (4.00x, limit 5.00x) |
| scale | consts check | 3.98x | ≤ 5x | PASS | hashes 63166 → 251198 (3.98x, limit 5.00x) |
| scale | consts fmt | 3.89x | ≤ 5x | PASS | allocs 2087 → 8109 (3.89x, limit 5.00x) |
| scale | consts build | 4.00x | ≤ 5x | PASS | loan_queries 4506 → 18006 (4.00x, limit 5.00x) |
| scale | local-items check | 4.00x | ≤ 5x | PASS | hashes 566919 → 2270459 (4.00x, limit 5.00x) |
| scale | local-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | local-items build | 4.01x | ≤ 5x | PASS | hashes 1402265 → 5617026 (4.01x, limit 5.00x) |
| scale | syntax-items check | 4.00x | ≤ 5x | PASS | item_body_checks 1700 → 6800 (4.00x, limit 5.00x) |
| scale | syntax-items fmt | 4.00x | ≤ 5x | PASS | hashes 17960 → 71864 (4.00x, limit 5.00x) |
| scale | syntax-items build | 4.00x | ≤ 5x | PASS | item_body_checks 1700 → 6800 (4.00x, limit 5.00x) |
| scale | struct-chain check | 4.05x | ≤ 5x | PASS | hashes 152086 → 615486 (4.05x, limit 5.00x) |
| scale | struct-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | struct-chain-values check | 4.01x | ≤ 5x | PASS | hashes 121729 → 488093 (4.01x, limit 5.00x) |
| scale | struct-chain-values fmt | 4.02x | ≤ 5x | PASS | hashes 2082 → 8370 (4.02x, limit 5.00x) |
| scale | struct-chain-values build | 4.00x | ≤ 5x | PASS | scope_probes 4900 → 19600 (4.00x, limit 5.00x) |
| scale | modules check | 4.00x | ≤ 5x | PASS | hashes 5398370 → 21595826 (4.00x, limit 5.00x) |
| scale | modules fmt | 4.00x | ≤ 5x | PASS | hashes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | modules build | 4.00x | ≤ 5x | PASS | hashes 5753018 → 23025075 (4.00x, limit 5.00x) |
| scale | use-names check | 4.03x | ≤ 5x | PASS | hashes 217852 → 878764 (4.03x, limit 5.00x) |
| scale | use-names fmt | 4.01x | ≤ 5x | PASS | hashes 3783 → 15159 (4.01x, limit 5.00x) |
| scale | use-names build | 4.00x | ≤ 5x | PASS | scope_probes 10500 → 42000 (4.00x, limit 5.00x) |
| scale | generic-fns check | 4.00x | ≤ 5x | PASS | hashes 313161 → 1253885 (4.00x, limit 5.00x) |
| scale | generic-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-fns build | 4.01x | ≤ 5x | PASS | hashes 989479 → 3969724 (4.01x, limit 5.00x) |
| scale | generic-params check | 4.05x | ≤ 5x | PASS | hashes 15718 → 63622 (4.05x, limit 5.00x) |
| scale | generic-params fmt | 4.10x | ≤ 10x | PASS | alloc_bytes 350594 → 1438988 (4.10x, limit 10.00x) |
| scale | generic-params build | 4.08x | ≤ 5x | PASS | hashes 21106 → 86019 (4.08x, limit 5.00x) |
| scale | param-bounds check | 4.00x | ≤ 5x | PASS | scope_probes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | param-bounds fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | param-bounds build | 4.04x | ≤ 5x | PASS | hashes 61387 → 247700 (4.04x, limit 5.00x) |
| scale | type-defaults check | 4.00x | ≤ 5x | PASS | scope_probes 31000 → 124000 (4.00x, limit 5.00x) |
| scale | type-defaults fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | type-defaults build | 4.01x | ≤ 5x | PASS | control 138021 → 553521 (4.01x, limit 5.00x) |
| scale | alias-chain check | 4.00x | ≤ 5x | PASS | scope_probes 6600 → 26400 (4.00x, limit 5.00x) |
| scale | alias-chain fmt | 4.03x | ≤ 5x | PASS | hashes 1234 → 4978 (4.03x, limit 5.00x) |
| scale | alias-chain build | 4.00x | ≤ 5x | PASS | scope_probes 6600 → 26400 (4.00x, limit 5.00x) |
| scale | row-chain check | 4.02x | ≤ 5x | PASS | hashes 229460 → 923500 (4.02x, limit 5.00x) |
| scale | row-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | row-chain build | 4.02x | ≤ 5x | PASS | hashes 601525 → 2420819 (4.02x, limit 5.00x) |
| scale | row-traits check | 4.00x | ≤ 5x | PASS | scope_probes 2300 → 9200 (4.00x, limit 5.00x) |
| scale | row-traits fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | row-traits build | 4.00x | ≤ 5x | PASS | scope_probes 2300 → 9200 (4.00x, limit 5.00x) |
| scale | closures check | 4.00x | ≤ 5x | PASS | scope_probes 2000 → 8000 (4.00x, limit 5.00x) |
| scale | closures fmt | 3.94x | ≤ 5x | PASS | allocs 3061 → 12073 (3.94x, limit 5.00x) |
| scale | closures build | 4.00x | ≤ 5x | PASS | scope_probes 2000 → 8000 (4.00x, limit 5.00x) |
| scale | generic-local-fns check | 4.03x | ≤ 5x | PASS | hashes 237144 → 956520 (4.03x, limit 5.00x) |
| scale | generic-local-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-local-fns build | 4.01x | ≤ 5x | PASS | hashes 1073660 → 4309125 (4.01x, limit 5.00x) |
| scale | let-statements check | 4.00x | ≤ 5x | PASS | scope_probes 500 → 2000 (4.00x, limit 5.00x) |
| scale | let-statements fmt | 3.94x | ≤ 5x | PASS | allocs 2554 → 10066 (3.94x, limit 5.00x) |
| scale | let-statements build | 4.00x | ≤ 5x | PASS | control 68499 → 273999 (4.00x, limit 5.00x) |
| scale | matches check | 4.00x | ≤ 5x | PASS | scope_probes 1000 → 4000 (4.00x, limit 5.00x) |
| scale | matches fmt | 3.97x | ≤ 5x | PASS | allocs 6057 → 24069 (3.97x, limit 5.00x) |
| scale | matches build | 4.05x | ≤ 5x | PASS | hashes 1123237 → 4546296 (4.05x, limit 5.00x) |
| scale | enum-arms check | 4.00x | ≤ 5x | PASS | scope_probes 5000 → 20000 (4.00x, limit 5.00x) |
| scale | enum-arms fmt | 3.91x | ≤ 5x | PASS | allocs 1551 → 6065 (3.91x, limit 5.00x) |
| scale | enum-arms build | 4.00x | ≤ 5x | PASS | scope_probes 5000 → 20000 (4.00x, limit 5.00x) |
| scale | tuple-patterns check | 4.00x | ≤ 5x | PASS | scope_probes 1000 → 4000 (4.00x, limit 5.00x) |
| scale | tuple-patterns fmt | 3.95x | ≤ 5x | PASS | allocs 3054 → 12066 (3.95x, limit 5.00x) |
| scale | tuple-patterns build | 4.00x | ≤ 5x | PASS | control 92471 → 369971 (4.00x, limit 5.00x) |
| scale | range-patterns check | 4.02x | ≤ 5x | PASS | hashes 263380 → 1058708 (4.02x, limit 5.00x) |
| scale | range-patterns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | range-patterns build | 4.01x | ≤ 5x | PASS | hashes 929930 → 3731023 (4.01x, limit 5.00x) |
| scale | wide-struct check | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | wide-struct fmt | 3.95x | ≤ 5x | PASS | allocs 2544 → 10056 (3.95x, limit 5.00x) |
| scale | wide-struct build | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | call-args check | 4.00x | ≤ 5x | PASS | scope_probes 1100 → 4400 (4.00x, limit 5.00x) |
| scale | call-args fmt | 4.10x | ≤ 10x | PASS | alloc_bytes 361569 → 1482933 (4.10x, limit 10.00x) |
| scale | call-args build | 4.05x | ≤ 5x | PASS | hashes 20203 → 81737 (4.05x, limit 5.00x) |
| scale | binary-chain check | 4.00x | ≤ 5x | PASS | hashes 2500 → 10000 (4.00x, limit 5.00x) |
| scale | binary-chain fmt | 3.78x | ≤ 5x | PASS | allocs 545 → 2060 (3.78x, limit 5.00x) |
| scale | binary-chain build | 4.03x | ≤ 5x | PASS | hashes 27506 → 110822 (4.03x, limit 5.00x) |
| scale | interpolation check | 4.00x | ≤ 5x | PASS | trait_lookup 2501 → 10001 (4.00x, limit 5.00x) |
| scale | interpolation fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | interpolation build | 4.03x | ≤ 5x | PASS | hashes 4232412 → 17043737 (4.03x, limit 5.00x) |
| scale | list-literal check | 4.00x | ≤ 5x | PASS | hashes 7392 → 29580 (4.00x, limit 5.00x) |
| scale | list-literal fmt | 3.82x | ≤ 5x | PASS | allocs 533 → 2038 (3.82x, limit 5.00x) |
| scale | list-literal build | 4.03x | ≤ 5x | PASS | hashes 44574 → 179766 (4.03x, limit 5.00x) |
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
| scale | if-nest build | 4.03x | ≤ 15.11x | PASS | hashes 5801 → 23369 (4.03x, limit 15.11x) |
| scale | block-nest check | 3.95x | ≤ 15.29x | PASS | hashes 814 → 3214 (3.95x, limit 15.29x) |
| scale | block-nest fmt | - | ≤ 15.29x | PASS | no counter reached the floor |
| scale | block-nest build | 3.99x | ≤ 15.29x | PASS | hashes 6308 → 25196 (3.99x, limit 15.29x) |
| scale | for-nest check | 5.27x | ≤ 14.5x | PASS | hashes 3515 → 18515 (5.27x, limit 14.50x) |
| scale | for-nest fmt | - | ≤ 14.5x | PASS | no counter reached the floor |
| scale | for-nest build | 4.62x | ≤ 14.5x | PASS | hashes 27455 → 126775 (4.62x, limit 14.50x) |
| scale | while-nest check | 3.88x | ≤ 15.11x | PASS | hashes 1033 → 4009 (3.88x, limit 15.11x) |
| scale | while-nest fmt | - | ≤ 15.11x | PASS | no counter reached the floor |
| scale | while-nest build | 4.00x | ≤ 15.11x | PASS | use_facts 1232 → 4928 (4.00x, limit 15.11x) |
| scale | match-nest check | 4.83x | ≤ 16.03x | PASS | hashes 1928 → 9316 (4.83x, limit 16.03x) |
| scale | match-nest fmt | - | ≤ 16.03x | PASS | no counter reached the floor |
| scale | match-nest build | 4.27x | ≤ 16.03x | PASS | hashes 7608 → 32457 (4.27x, limit 16.03x) |
| scale | closure-nest check | 4.03x | ≤ 11.45x | PASS | hashes 1964 → 7922 (4.03x, limit 11.45x) |
| scale | closure-nest fmt | - | ≤ 11.45x | PASS | no counter reached the floor |
| scale | closure-nest build | 4.13x | ≤ 11.45x | PASS | hashes 8139 → 33653 (4.13x, limit 11.45x) |
| scale | option-nest check | 4.19x | ≤ 5x | PASS | hashes 1816 → 7600 (4.19x, limit 5.00x) |
| scale | option-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | option-nest build | 4.13x | ≤ 5x | PASS | hashes 12313 → 50813 (4.13x, limit 5.00x) |
| scale | list-type-nest check | 4.27x | ≤ 5x | PASS | hashes 1254 → 5357 (4.27x, limit 5.00x) |
| scale | list-type-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | list-type-nest build | 148.43x | ≤ 5x | FAIL | control 14 → 2078 (148.43x, limit 5.00x); allocs 1345 → 7924 (5.89x, limit 5.00x) |
| scale | generic-nest check | 4.32x | ≤ 5x | PASS | hashes 1599 → 6901 (4.32x, limit 5.00x) |
| scale | generic-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | generic-nest build | 4.19x | ≤ 5x | PASS | hashes 6769 → 28351 (4.19x, limit 5.00x) |
| scale | paren-nest check | - | ≤ 5x | PASS | no counter reached the floor |
| scale | paren-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | paren-nest build | 4.08x | ≤ 5x | PASS | control 795 → 3243 (4.08x, limit 5.00x) |
| scale | arith-nest check | 4.00x | ≤ 5x | PASS | hashes 440 → 1760 (4.00x, limit 5.00x) |
| scale | arith-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | arith-nest build | 4.04x | ≤ 5x | PASS | hashes 6121 → 24704 (4.04x, limit 5.00x) |
| scale | bool-nest check | 3.95x | ≤ 5x | PASS | hashes 391 → 1543 (3.95x, limit 5.00x) |
| scale | bool-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | bool-nest build | 4.09x | ≤ 5x | PASS | use_facts 490 → 2002 (4.09x, limit 5.00x) |
| scale | bit-nest check | 3.99x | ≤ 5x | PASS | hashes 818 → 3266 (3.99x, limit 5.00x) |
| scale | bit-nest fmt | 5.84x | ≤ 10x | PASS | alloc_bytes 206985 → 1209058 (5.84x, limit 10.00x) |
| scale | bit-nest build | 4.11x | ≤ 5x | PASS | hashes 8441 → 34703 (4.11x, limit 5.00x) |
| scale | tuple-nest check | 4.00x | ≤ 5x | PASS | hashes 2024 → 8088 (4.00x, limit 5.00x) |
| scale | tuple-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | tuple-nest build | 4.00x | ≤ 5x | PASS | hashes 12736 → 50980 (4.00x, limit 5.00x) |
| scale | if-expr-nest check | 3.76x | ≤ 5x | PASS | hashes 322 → 1210 (3.76x, limit 5.00x) |
| scale | if-expr-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | if-expr-nest build | 3.82x | ≤ 5x | PASS | use_facts 357 → 1365 (3.82x, limit 5.00x) |
| scale | call-nest check | 3.91x | ≤ 5x | PASS | hashes 544 → 2128 (3.91x, limit 5.00x) |
| scale | call-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | call-nest build | 3.06x | ≤ 5x | PASS | hashes 5544 → 16944 (3.06x, limit 5.00x) |
| scale | index-nest check | - | ≤ 5x | PASS | no counter reached the floor |
| scale | index-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | index-nest build | 4.40x | ≤ 5x | PASS | hashes 2858 → 12587 (4.40x, limit 5.00x) |
| scale | fn-nest check | 3.93x | ≤ 13.42x | PASS | hashes 3006 → 11810 (3.93x, limit 13.42x) |
| scale | fn-nest fmt | - | ≤ 13.42x | PASS | no counter reached the floor |
| scale | fn-nest build | 4.11x | ≤ 13.42x | PASS | hashes 11874 → 48786 (4.11x, limit 13.42x) |
| scale | module-path-nest check | 4.32x | ≤ 14.36x | PASS | allocs 57060 → 246669 (4.32x, limit 14.36x) |
| scale | module-path-nest fmt | 9.99x | ≤ 28.72x | PASS | alloc_bytes 158233 → 1580610 (9.99x, limit 28.72x) |
| scale | module-path-nest build | 11.83x | ≤ 14.36x | PASS | hashes 1064 → 12588 (11.83x, limit 14.36x) |
| scale | coverage: node kinds | 122/122 | 122/122 | PASS | exempt: Error |
| scale | coverage: node kinds repeated | 113/122 | reported | PASS | in no repeat shape: ReturnStmt, RangeExpr, LogicalOrExpr, LogicalAndExpr, BitwiseOrExpr, BitwiseXorExpr, BitwiseAndExpr, ShiftExpr, WhileExpr; exempt: Error |
| scale | coverage: node kinds nested | 59/122 | reported | PASS | in no nest shape: EnumDecl, TraitDecl, ImplDecl, TypeDecl, TestsBlock, Decorator, DefaultValue, ConstParameter, LifetimeParameter, BoundList, WhereClause, WhereBound, TypeDefault, ConstArgument, AssociatedTypeBinding, NamedArgument, EmbeddedField, EnumVariant, VariantSharedData, AssociatedTypeDecl, Derivation, DiscardStmt, BreakStmt, ContinueStmt, UsingStmt, ElseIfClause, MatchGuard, PipeExpr, RefExpr, DerefExpr, PowerExpr, TryExpr, SuspendExpr, PathExpr, TypeArgsExpr, TrailingCallExpr, PlaceholderExpr, MapExpr, MapEntry, SpreadExpr, ComprehensionExpr, ComprehensionFor, ComprehensionIf, ContextExpr, ContextEntry, Interpolation, LiteralPattern, NamedPattern, DataPattern, DataPatternField, SpreadPattern, RangePattern, DynType, ImplType, RefType, UnionType, ParenType, FunctionType, ProjectionType, ContextType, RestType, InferType, MarkerBound; exempt: Error |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 4 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 38 of 38 safe |
| fmt | hd fmt, 10k lines | 21 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 0.72x | ≤ 1.30x | PASS | checked 282 ms, unchecked 390 ms |
| dev-speed | geomean dev / optimized | 1.62x | ≤ 4.00x | PASS | fib 0.98x, sum 2.52x, string-build 1.65x, map 1.12x, sort 2.42x |
| dev-speed | worst case dev / optimized | 2.52x | ≤ 10.00x | PASS | sum |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 0.0 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 22.1 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.11 CPU-s, 56.3 MB | completes ≤ 30 s | PASS | wall 122 ms |
| resources | check, small, warm | 0.00 CPU-s, 22.1 MB | completes ≤ 30 s | PASS | wall 8.0 ms |
| resources | test, small, cold | 0.56 CPU-s, 111.1 MB | completes ≤ 30 s | PASS | wall 575 ms |
| resources | test, small, warm | 0.01 CPU-s, 45.2 MB | completes ≤ 30 s | PASS | wall 22 ms |
| resources | build, small, cold | 0.43 CPU-s, 95.7 MB | completes ≤ 30 s | PASS | wall 445 ms |
| resources | build, small, warm | 0.00 CPU-s, 21.9 MB | completes ≤ 30 s | PASS | wall 11 ms |
| resources | check, 10k, cold | 0.28 CPU-s, 104.1 MB | completes ≤ 30 s | PASS | wall 291 ms |
| resources | check, 10k, warm | 0.00 CPU-s, 22.1 MB | completes ≤ 30 s | PASS | wall 8.1 ms |
| resources | test, 10k, cold | 0.99 CPU-s, 164.1 MB | completes ≤ 30 s | PASS | wall 997 ms |
| resources | test, 10k, warm | 0.03 CPU-s, 58.3 MB | completes ≤ 30 s | PASS | wall 42 ms |
| resources | build, 10k, cold | 0.44 CPU-s, 97.7 MB | completes ≤ 30 s | PASS | wall 453 ms |
| resources | build, 10k, warm | 0.00 CPU-s, 21.8 MB | completes ≤ 30 s | PASS | wall 8.8 ms |
| resources | check, 50k, cold | 1.07 CPU-s, 322.0 MB | completes ≤ 150 s | PASS | wall 1091 ms |
| resources | check, 50k, warm | 0.00 CPU-s, 24.1 MB | completes ≤ 150 s | PASS | wall 11 ms |
| resources | test, 50k, cold | 2.90 CPU-s, 445.6 MB | completes ≤ 150 s | PASS | wall 2899 ms |
| resources | test, 50k, warm | 0.14 CPU-s, 117.3 MB | completes ≤ 150 s | PASS | wall 141 ms |
| resources | build, 50k, cold | 0.47 CPU-s, 103.8 MB | completes ≤ 150 s | PASS | wall 476 ms |
| resources | build, 50k, warm | 0.00 CPU-s, 24.1 MB | completes ≤ 150 s | PASS | wall 11 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 108.3 MB | ≤ 22.0 MB | FAIL | RSS 80.9 MB at input 100, 189.2 MB at input 1000; build per input 88 ms at 50, 898 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.7 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 9.9 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd, just written: wall time | 6.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, just written: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, just written: peak RSS | 21.3 MB | ≤ 10.0 MB | FAIL |  |
| startup | hd check empty.hd, settled: wall time | 5.5 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, settled: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, settled: peak RSS | 21.3 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.43x | ≤ 1.50x | FAIL | p95 91 ms at N = 1; 4 cores; total CPU 260 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 2.41x | ≤ 6.00x | PASS | p95 91 ms at N = 1; 4 cores; total CPU 380 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 0.36 | &lt; 1.00 (sublinear) | PASS | 140 ms at N = 1, 380 ms at N = 16 |
| disk | artifacts per worktree | 0.0 MB | ≤ 10.0 MB | PASS | 10 files; HD_CACHE 11.1 MB, shared per user, not counted |
| disk | toolchain, once per machine | 33.6 MB | recorded, no target | PASS | toolchain files: /home/runner/work/hd-lang/hd-lang/target/dist/hd |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 2.15x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 327 files | the files of one check's cache | FAIL | one check alone: 36 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 13.2 MB | ≤ 5.0 MB | FAIL | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 1 | ≤ 1 | PASS | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 112k/s | ≥ 100k/s | PASS | 6000 extra cases took 54 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 8.0 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 37 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.0 ms | ≤ 1.0 ms | PASS | 100 tests: 15 ms |
| integration-test-perf | setup per integration test program | 5.8 ms | ≤ 20 ms | PASS | 10 programs: 72 ms for 21 tests, 3.4 ms per test |
| integration-test-perf | total time growth in programs | 0.913 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 14 ms at 2, 60 ms at 10 programs |
| integration-test-perf | cost per doc test | 3.5 ms | ≤ 20 ms | PASS | 10 doc tests: 47 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 0.57x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.80x | ≤ 3.00x | PASS | hd 4.9 ms (p10 4.9, p90 5.0), Node 6.2 ms, Python 59 ms; release build, run as hd run |
| runtime | sum: hd / Node | 0.46x | ≤ 3.00x | PASS | hd 19 ms (p10 19, p90 19), Node 42 ms, Python 200 ms; release build, run as hd run |
| runtime | string-build: hd / Node | 0.52x | ≤ 3.00x | PASS | hd 2.1 ms (p10 2.1, p90 2.1), Node 4.1 ms, Python 8.3 ms; release build, run as hd run |
| runtime | map: hd / Node | 0.62x | ≤ 3.00x | PASS | hd 3.5 ms (p10 3.4, p90 3.5), Node 5.6 ms, Python 12 ms; release build, run as hd run |
| runtime | sort: hd / Node | 0.53x | ≤ 3.00x | PASS | hd 8.4 ms (p10 8.3, p90 8.4), Node 16 ms, Python 13 ms; release build, run as hd run |
| allocations | counted loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | list loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | map loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | iterator chain, bytes per iteration | - | ≤ 1 allocation (≤ 32 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| size-startup-heap | tiny program, release Wasm size | - | ≤ 2048 B | N/A | not supported by this hd: the build wrote no .wasm file (release build, run as hd run) |
| size-startup-heap | tiny program, start to first output | 5.6 ms | ≤ 5.0 ms | FAIL | release build, run as hd run |
| size-startup-heap | peak heap, map: hd / Node | 0.50x | ≤ 2.00x | PASS | hd +7.1 MB, Node +14.3 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.16x | ≤ 2.00x | PASS | hd +3.2 MB, Node +20.2 MB over an empty program |
| host-call-overhead | Console println, per call | 1.4 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd run |
| host-call-overhead | Fs read_text!, per call | 3.1 µs | ≤ 10 µs | PASS | 10000 calls; release build, run as hd run |
| host-call-overhead | serde boundary (list_dir!), per call | 6.2 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd run |
| suspension-overhead | await of a ready value, per await | 6.5 ns | ≤ 100 ns | PASS | 6.5 ns per iteration; release build, run as hd run |
| suspension-overhead | all! of two ready tasks, throughput | 16.7M/s | ≥ 1.0M/s | PASS | 120 ns per iteration; release build, run as hd run |
| suspension-overhead | race! of two ready tasks, throughput | 11.2M/s | ≥ 1.0M/s | PASS | 179 ns per iteration; release build, run as hd run |
| serde-throughput | JSON encode, hd / Node MB/s | 0.13x | ≥ 0.50x | FAIL | hd 92 MB/s (45488 B document), Node 718 MB/s; release build, run as hd run |
| serde-throughput | JSON decode, hd / Node MB/s | 0.23x | ≥ 0.50x | FAIL | hd 63 MB/s (45488 B document), Node 271 MB/s; release build, run as hd run |
| text-throughput | string build, hd / Node MB/s | 3.11x | ≥ 0.50x | PASS | hd 530 MB/s, Node 171 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd run |
| text-throughput | split, hd / Node MB/s | 1.47x | ≥ 0.50x | PASS | hd 643 MB/s, Node 438 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd run |
| text-throughput | regex find_all, hd / Node MB/s | 0.45x | ≥ 0.50x | FAIL | hd 53 MB/s, Node 117 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd run |
| dead-code | Wasm bytes per 1,000 lines | - | ≤ 10240 B | FAIL | the build wrote no file to measure (release build, run as hd run) |
| dead-code | no unused std in the program | - | none | FAIL | the build wrote no file to measure (release build, run as hd run) |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 22362000 requests; RSS 41.1 MB after warm-up, 41.1 MB at the end; release build, run as hd run |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 9421 of 9421 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
- [2026-11-13T04](daily/2026-11-13T04.json)
