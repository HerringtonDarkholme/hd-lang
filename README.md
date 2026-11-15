# Daily goal metrics

Finished: 2026-11-15T03:45:03.896Z. Commit: 6c1ef4adfdfec468df6f8579522b4ba5168e9468.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V45 96-Core Processor).
Compiler: target/release/hd.

269 passed, 26 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 34 ms → pass 38 ms
- edit-latency: p95 check after an edit: pass 36 ms → pass 42 ms
- pathological: deep-blocks: time at 4x: pass 17 ms → pass 21 ms
- pathological: deep-parens: time at 4x: pass 17 ms → pass 19 ms
- pathological: method-chain: time at 4x: pass 22 ms → pass 25 ms
- pathological: large-enum-match: time at 4x: pass 24 ms → pass 29 ms
- pathological: deep-generic: time at 4x: pass 19 ms → pass 22 ms
- pathological: question-chain: time at 4x: pass 23 ms → pass 26 ms
- pathological: big-file: time at 4x: pass 139 ms → pass 115 ms
- check-cost: checked run time / unchecked run time: fail 1.68x → fail 2.12x
- resources: warm check, 10k lines: CPU time: pass 20 ms → pass 30 ms
- concurrency: p95 latency at N = 4 vs N = 1: fail 2.05x → pass 1.22x
- concurrency: p95 latency at N = 16 vs N = 1: fail 8.02x → pass 1.95x
- concurrency: total CPU growth exponent, N = 1 to 16: fail 1.06 → pass 0.33
- cache-growth: cache size after 100 edits, cap 5 MB: pass 4.5 MB → fail 14.1 MB
- unit-test-perf: overhead per unit test: pass 0.6 ms → pass 0.5 ms
- integration-test-perf: cost per doc test: fail 73 ms → fail 65 ms
- runtime: geomean hd / Node: pass 0.85x → pass 0.74x
- runtime: fib: hd / Node: pass 0.90x → pass 0.41x
- runtime: string-build: hd / Node: pass 0.88x → pass 1.00x
- runtime: map: hd / Node: pass 2.66x → fail 3.14x
- runtime: sort: hd / Node: pass 0.69x → pass 0.61x
- allocations: counted loop, bytes per iteration: pass 0.03 B/iter → pass 0.00 B/iter
- allocations: list loop, bytes per iteration: pass 0.00 B/iter → pass 0.01 B/iter
- allocations: map loop, bytes per iteration: pass 0.02 B/iter → pass 0.00 B/iter
- allocations: iterator chain, bytes per iteration: pass 0.00 B/iter → pass 0.01 B/iter
- host-call-overhead: Console println, per call: fail 2.6 µs → fail 3.0 µs
- host-call-overhead: Fs read_text!, per call: fail 10 µs → fail 14 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 21 µs → fail 26 µs
- suspension-overhead: await of a ready value, per await: pass 4.1 ns → pass 2.8 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 18.6M/s → pass 16.2M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 16.2M/s → pass 14.4M/s
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.21x → fail 0.17x
- text-throughput: split, hd / Node MB/s: pass 2.05x → pass 2.85x
- text-throughput: regex find_all, hd / Node MB/s: fail 0.09x → fail 0.08x

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 38 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 42 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 162 ms | ≤ 1000 ms | PASS | Timing change ≤10%: noise. |
| test-latency | median edit-then-test | 145 ms | ≤ 300 ms | PASS | Timing change ≤10%: noise. |
| mistakes | programs with exactly one diagnostic | 100.0% | ≥ 95.0% | PASS | 69 of 69 |
| mistakes | one diagnostic with the expected code | 100.0% | ≥ 95.0% | PASS | 69 of 69 |
| mistakes | fix-it makes hd check pass | 34.8% | ≥ 80.0% | FAIL | 24 of 69 resolved; 24 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 65 tokens | ≤ 60 tokens | FAIL | largest: mut-method-on-readonly-binding; median 43 tokens, 171 B; 67 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 40 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | overlapping-from: peak RSS at 4x | 26.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 31 → 40 ms, start-up 20 ms |
| pathological | deep-blocks: time at 4x | 21 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 22.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 21 ms, start-up 20 ms |
| pathological | deep-parens: time at 4x | 19 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 21.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 19 ms, start-up 20 ms |
| pathological | method-chain: time at 4x | 25 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 23.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 25 ms, start-up 20 ms |
| pathological | iterator-chain: time at 4x | 26 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | iterator-chain: peak RSS at 4x | 21.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | iterator-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 25 → 26 ms, start-up 20 ms |
| pathological | wide-list-literal: time at 4x | 37 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | wide-list-literal: peak RSS at 4x | 25.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 29 → 37 ms, start-up 20 ms |
| pathological | large-enum-match: time at 4x | 29 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 25.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 30 → 29 ms, start-up 20 ms |
| pathological | deep-generic: time at 4x | 22 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 21.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 22 ms, start-up 20 ms |
| pathological | question-chain: time at 4x | 26 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 25.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 26 ms, start-up 20 ms |
| pathological | big-file: time at 4x | 115 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 55.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 66 → 115 ms, start-up 20 ms |
| scale | fn-items check | 4.04x | ≤ 4.5x | PASS | hashes 182619 → 738587 (4.04x, limit 4.50x) |
| scale | fn-items fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | fn-items build | 4.02x | ≤ 4.5x | PASS | type_nodes 496 → 1996 (4.02x, limit 4.50x) |
| scale | struct-items check | 4.03x | ≤ 4.5x | PASS | hashes 186917 → 752829 (4.03x, limit 4.50x) |
| scale | struct-items fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | struct-items build | 4.02x | ≤ 4.5x | PASS | hashes 475379 → 1909328 (4.02x, limit 4.50x) |
| scale | enum-items check | 4.00x | ≤ 4.5x | PASS | type_nodes 3997 → 15997 (4.00x, limit 4.50x) |
| scale | enum-items fmt | 4.00x | ≤ 4.5x | PASS | hashes 7576 → 30328 (4.00x, limit 4.50x) |
| scale | enum-items build | 4.00x | ≤ 4.5x | PASS | type_nodes 3997 → 15997 (4.00x, limit 4.50x) |
| scale | trait-items check | 4.01x | ≤ 4.5x | PASS | hashes 1018388 → 4083023 (4.01x, limit 4.50x) |
| scale | trait-items fmt | 4.00x | ≤ 4.5x | PASS | hashes 7576 → 30328 (4.00x, limit 4.50x) |
| scale | trait-items build | 4.01x | ≤ 4.5x | PASS | hashes 1479273 → 5936114 (4.01x, limit 4.50x) |
| scale | impl-methods check | 4.00x | ≤ 4.5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 4.50x) |
| scale | impl-methods fmt | 3.92x | ≤ 4.5x | PASS | allocs 1545 → 6057 (3.92x, limit 4.50x) |
| scale | impl-methods build | 4.00x | ≤ 4.5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 4.50x) |
| scale | impls-of-one-trait check | 4.00x | ≤ 4.5x | PASS | hashes 539078 → 2157306 (4.00x, limit 4.50x) |
| scale | impls-of-one-trait fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | impls-of-one-trait build | 4.00x | ≤ 4.5x | PASS | type_nodes 5497 → 21997 (4.00x, limit 4.50x) |
| scale | operator-impls check | 4.00x | ≤ 4.5x | PASS | hashes 648557 → 2595905 (4.00x, limit 4.50x) |
| scale | operator-impls fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | operator-impls build | 4.00x | ≤ 4.5x | PASS | type_nodes 2498 → 9998 (4.00x, limit 4.50x) |
| scale | derive-items check | 4.00x | ≤ 4.5x | PASS | type_nodes 62498 → 249998 (4.00x, limit 4.50x) |
| scale | derive-items fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | derive-items build | 4.34x | ≤ 4.5x | PASS | hashes 42702897 → 185297814 (4.34x, limit 4.50x) |
| scale | dyn-values check | 4.00x | ≤ 4.5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 4.50x) |
| scale | dyn-values fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | dyn-values build | 6.49x | ≤ 4.5x | PASS | known superlinear, #1908: hashes 1502784 → 9750756 (6.49x, limit 4.50x); allocs 1067713 → 5947795 (5.57x, limit 4.50x) |
| scale | consts check | 4.02x | ≤ 4.5x | PASS | type_nodes 497 → 1997 (4.02x, limit 4.50x) |
| scale | consts fmt | 4.01x | ≤ 4.5x | PASS | alloc_bytes 2924047 → 11716540 (4.01x, limit 9.00x) |
| scale | consts build | 4.02x | ≤ 4.5x | PASS | type_nodes 497 → 1997 (4.02x, limit 4.50x) |
| scale | local-items check | 4.00x | ≤ 4.5x | PASS | type_nodes 2496 → 9996 (4.00x, limit 4.50x) |
| scale | local-items fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | local-items build | 4.00x | ≤ 4.5x | PASS | type_nodes 2496 → 9996 (4.00x, limit 4.50x) |
| scale | syntax-items check | 4.00x | ≤ 4.5x | PASS | type_nodes 26296 → 105196 (4.00x, limit 4.50x) |
| scale | syntax-items fmt | 4.00x | ≤ 4.5x | PASS | hashes 17160 → 68664 (4.00x, limit 4.50x) |
| scale | syntax-items build | 4.00x | ≤ 4.5x | PASS | type_nodes 26296 → 105196 (4.00x, limit 4.50x) |
| scale | struct-chain check | 4.06x | ≤ 4.5x | PASS | hashes 135214 → 549054 (4.06x, limit 4.50x) |
| scale | struct-chain fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | modules check | 4.02x | ≤ 4.5x | PASS | type_nodes 496 → 1996 (4.02x, limit 4.50x) |
| scale | modules fmt | 4.00x | ≤ 4.5x | PASS | hashes 1500 → 6000 (4.00x, limit 4.50x) |
| scale | modules build | 4.02x | ≤ 4.5x | PASS | type_nodes 496 → 1996 (4.02x, limit 4.50x) |
| scale | use-names check | 4.04x | ≤ 4.5x | PASS | hashes 212657 → 860005 (4.04x, limit 4.50x) |
| scale | use-names fmt | 4.01x | ≤ 4.5x | PASS | hashes 3783 → 15159 (4.01x, limit 4.50x) |
| scale | use-names build | 4.02x | ≤ 4.5x | PASS | type_nodes 496 → 1996 (4.02x, limit 4.50x) |
| scale | generic-fns check | 4.03x | ≤ 4.5x | PASS | hashes 292774 → 1179110 (4.03x, limit 4.50x) |
| scale | generic-fns fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | generic-fns build | 4.02x | ≤ 4.5x | PASS | hashes 742040 → 2979605 (4.02x, limit 4.50x) |
| scale | generic-params check | 4.23x | ≤ 4.5x | PASS | hashes 13330 → 56326 (4.23x, limit 4.50x) |
| scale | generic-params fmt | 4.11x | ≤ 4.5x | PASS | alloc_bytes 337742 → 1387004 (4.11x, limit 9.00x) |
| scale | generic-params build | 4.00x | ≤ 4.5x | PASS | scope_probes 800 → 3200 (4.00x, limit 4.50x) |
| scale | param-bounds check | 4.03x | ≤ 4.5x | PASS | hashes 53771 → 216507 (4.03x, limit 4.50x) |
| scale | param-bounds fmt | 4.04x | ≤ 4.5x | PASS | hashes 616 → 2488 (4.04x, limit 4.50x) |
| scale | param-bounds build | 4.00x | ≤ 4.5x | PASS | scope_probes 1500 → 6000 (4.00x, limit 4.50x) |
| scale | type-defaults check | 4.00x | ≤ 4.5x | PASS | type_nodes 2498 → 9998 (4.00x, limit 4.50x) |
| scale | type-defaults fmt | 4.00x | ≤ 4.5x | PASS | hashes 7576 → 30328 (4.00x, limit 4.50x) |
| scale | type-defaults build | 4.00x | ≤ 4.5x | PASS | type_nodes 2498 → 9998 (4.00x, limit 4.50x) |
| scale | row-chain check | 4.02x | ≤ 4.5x | PASS | hashes 215824 → 867640 (4.02x, limit 4.50x) |
| scale | row-chain fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | row-chain build | 11.72x | ≤ 4.5x | PASS | known superlinear, #1920: allocs 1941644 → 22763683 (11.72x, limit 4.50x) |
| scale | row-traits check | 4.00x | ≤ 4.5x | PASS | scope_probes 2100 → 8400 (4.00x, limit 4.50x) |
| scale | row-traits fmt | 4.04x | ≤ 4.5x | PASS | hashes 616 → 2488 (4.04x, limit 4.50x) |
| scale | row-traits build | 4.00x | ≤ 4.5x | PASS | scope_probes 2100 → 8400 (4.00x, limit 4.50x) |
| scale | closures check | 4.01x | ≤ 4.5x | PASS | type_nodes 1496 → 5996 (4.01x, limit 4.50x) |
| scale | closures fmt | 3.70x | ≤ 4.5x | PASS | allocs 561 → 2073 (3.70x, limit 4.50x) |
| scale | closures build | 15.89x | ≤ 4.5x | PASS | known superlinear, #1907: use_facts 3531504 → 56126004 (15.89x, limit 4.50x); hashes 22208274 → 349219756 (15.72x, limit 4.50x); allocs 1915004 → 25639658 (13.39x, limit 4.50x); alloc_bytes 1216291854 → 18201457588 (14.96x, limit 9.00x) |
| scale | let-statements check | 4.00x | ≤ 4.5x | PASS | type_nodes 6000 → 24000 (4.00x, limit 4.50x) |
| scale | let-statements fmt | 4.00x | ≤ 4.5x | PASS | alloc_bytes 4282286 → 17148665 (4.00x, limit 9.00x) |
| scale | let-statements build | 4.00x | ≤ 4.5x | PASS | control 72499 → 289999 (4.00x, limit 4.50x) |
| scale | matches check | 4.00x | ≤ 4.5x | PASS | decision_rows 6000 → 24000 (4.00x, limit 4.50x) |
| scale | matches fmt | 3.92x | ≤ 4.5x | PASS | allocs 2057 → 8069 (3.92x, limit 4.50x) |
| scale | matches build | 9.36x | ≤ 4.5x | PASS | known superlinear, #1917: alloc_bytes 258051661 → 2414257151 (9.36x, limit 9.00x) |
| scale | enum-arms check | 4.00x | ≤ 4.5x | PASS | hashes 49655 → 198659 (4.00x, limit 4.50x) |
| scale | enum-arms fmt | 3.75x | ≤ 4.5x | PASS | allocs 551 → 2065 (3.75x, limit 4.50x) |
| scale | enum-arms build | 11.23x | ≤ 4.5x | PASS | known superlinear, #1909: allocs 414187 → 4652070 (11.23x, limit 4.50x); hashes 436455 → 4738936 (10.86x, limit 4.50x) |
| scale | tuple-patterns check | 4.00x | ≤ 4.5x | PASS | decision_rows 1000 → 4000 (4.00x, limit 4.50x) |
| scale | tuple-patterns fmt | 4.00x | ≤ 4.5x | PASS | alloc_bytes 4868363 → 19493009 (4.00x, limit 9.00x) |
| scale | tuple-patterns build | 4.00x | ≤ 4.5x | PASS | control 94965 → 379965 (4.00x, limit 4.50x) |
| scale | range-patterns check | 4.03x | ≤ 4.5x | PASS | hashes 219588 → 884608 (4.03x, limit 4.50x) |
| scale | range-patterns fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | range-patterns build | 4.01x | ≤ 4.5x | PASS | hashes 639138 → 2565068 (4.01x, limit 4.50x) |
| scale | wide-struct check | 4.00x | ≤ 4.5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 4.50x) |
| scale | wide-struct fmt | 3.78x | ≤ 4.5x | PASS | allocs 544 → 2056 (3.78x, limit 4.50x) |
| scale | wide-struct build | 4.10x | ≤ 4.5x | PASS | allocs 117762 → 483003 (4.10x, limit 4.50x) |
| scale | call-args check | 4.35x | ≤ 4.5x | PASS | hashes 3932 → 17120 (4.35x, limit 4.50x) |
| scale | call-args fmt | 4.11x | ≤ 4.5x | PASS | alloc_bytes 348769 → 1431733 (4.11x, limit 9.00x) |
| scale | call-args build | 4.04x | ≤ 4.5x | PASS | hashes 14099 → 56939 (4.04x, limit 4.50x) |
| scale | binary-chain check | 4.00x | ≤ 4.5x | PASS | hashes 3500 → 14000 (4.00x, limit 4.50x) |
| scale | binary-chain fmt | 2.89x | ≤ 4.5x | PASS | alloc_bytes 576372 → 1665495 (2.89x, limit 9.00x) |
| scale | binary-chain build | 4.01x | ≤ 4.5x | PASS | hashes 18340 → 73528 (4.01x, limit 4.50x) |
| scale | interpolation check | 4.00x | ≤ 4.5x | PASS | type_nodes 2500 → 10000 (4.00x, limit 4.50x) |
| scale | interpolation fmt | 4.27x | ≤ 4.5x | PASS | alloc_bytes 6453 → 27573 (4.27x, limit 9.00x) |
| scale | interpolation build | 11.25x | ≤ 4.5x | PASS | known superlinear, #1914: alloc_bytes 912874699 → 10265421855 (11.25x, limit 9.00x) |
| scale | list-literal check | 4.00x | ≤ 4.5x | PASS | hashes 12000 → 48000 (4.00x, limit 4.50x) |
| scale | list-literal fmt | 2.87x | ≤ 4.5x | PASS | alloc_bytes 651597 → 1872219 (2.87x, limit 9.00x) |
| scale | list-literal build | 4.01x | ≤ 4.5x | PASS | hashes 29344 → 117532 (4.01x, limit 4.50x) |
| scale | method-chain check | 4.00x | ≤ 4.5x | PASS | hashes 13394 → 53582 (4.00x, limit 4.50x) |
| scale | method-chain fmt | 4.04x | ≤ 4.5x | PASS | alloc_bytes 838129 → 3386711 (4.04x, limit 9.00x) |
| scale | method-chain build | 3.88x | ≤ 4.5x | PASS | hashes 496449 → 1928397 (3.88x, limit 4.50x) |
| scale | pipe-chain check | 4.00x | ≤ 4.5x | PASS | hashes 15502 → 62002 (4.00x, limit 4.50x) |
| scale | pipe-chain fmt | 2.75x | ≤ 4.5x | PASS | alloc_bytes 637545 → 1750262 (2.75x, limit 9.00x) |
| scale | pipe-chain build | 3.98x | ≤ 4.5x | PASS | hashes 79293 → 315609 (3.98x, limit 4.50x) |
| scale | question-chain check | 4.00x | ≤ 4.5x | PASS | hashes 35415 → 141603 (4.00x, limit 4.50x) |
| scale | question-chain fmt | 3.97x | ≤ 4.5x | PASS | alloc_bytes 2362619 → 9386484 (3.97x, limit 9.00x) |
| scale | question-chain build | 3.99x | ≤ 4.5x | PASS | use_facts 3510 → 14010 (3.99x, limit 4.50x) |
| scale | if-nest check | 3.94x | ≤ 4.5x per 4x bytes (12.1x) | PASS | hashes 799 → 3151 (3.94x, limit 13.59x) |
| scale | if-nest fmt | 5.35x | ≤ 4.5x per 4x bytes (12.1x) | PASS | alloc_bytes 139251 → 745244 (5.35x, limit 27.19x) |
| scale | if-nest build | 4.00x | ≤ 4.5x per 4x bytes (12.1x) | PASS | hashes 3763 → 15035 (4.00x, limit 13.59x) |
| scale | block-nest check | 3.96x | ≤ 4.5x per 4x bytes (12.2x) | PASS | hashes 844 → 3340 (3.96x, limit 13.76x) |
| scale | block-nest fmt | 5.74x | ≤ 4.5x per 4x bytes (12.2x) | PASS | alloc_bytes 155418 → 892676 (5.74x, limit 27.51x) |
| scale | block-nest build | 3.95x | ≤ 4.5x per 4x bytes (12.2x) | PASS | hashes 3978 → 15730 (3.95x, limit 13.76x) |
| scale | for-nest check | 5.56x | ≤ 4.5x per 4x bytes (11.6x) | PASS | hashes 2800 → 15556 (5.56x, limit 13.05x) |
| scale | for-nest fmt | 5.44x | ≤ 4.5x per 4x bytes (11.6x) | PASS | alloc_bytes 133881 → 728551 (5.44x, limit 26.11x) |
| scale | for-nest build | 38.27x | ≤ 4.5x per 4x bytes (11.6x) | PASS | known superlinear, #1916: alloc_bytes 7427671 → 284258206 (38.27x, limit 26.11x); allocs 32197 → 562102 (17.46x, limit 13.05x) |
| scale | while-nest check | 3.89x | ≤ 4.5x per 4x bytes (12.1x) | PASS | hashes 946 → 3682 (3.89x, limit 13.59x) |
| scale | while-nest fmt | 5.36x | ≤ 4.5x per 4x bytes (12.1x) | PASS | alloc_bytes 137955 → 739820 (5.36x, limit 27.19x) |
| scale | while-nest build | 40.21x | ≤ 4.5x per 4x bytes (12.1x) | PASS | known superlinear, #1916: alloc_bytes 3188164 → 128201695 (40.21x, limit 27.19x); allocs 12970 → 243601 (18.78x, limit 13.59x) |
| scale | match-nest check | 5.01x | ≤ 4.5x per 4x bytes (12.8x) | PASS | hashes 1536 → 7692 (5.01x, limit 14.43x) |
| scale | match-nest fmt | 5.84x | ≤ 4.5x per 4x bytes (12.8x) | PASS | alloc_bytes 156189 → 912779 (5.84x, limit 28.86x) |
| scale | match-nest build | 4.28x | ≤ 4.5x per 4x bytes (12.8x) | PASS | hashes 6618 → 28332 (4.28x, limit 14.43x) |
| scale | closure-nest check | 3.99x | ≤ 4.5x per 4x bytes (9.2x) | PASS | hashes 1833 → 7317 (3.99x, limit 10.31x) |
| scale | closure-nest fmt | 5.56x | ≤ 4.5x per 4x bytes (9.2x) | PASS | alloc_bytes 72965 → 405829 (5.56x, limit 20.61x) |
| scale | closure-nest build | 14.37x | ≤ 4.5x per 4x bytes (9.2x) | PASS | known superlinear, #1906: use_facts 810 → 11640 (14.37x, limit 10.31x) |
| scale | option-nest check | 6.96x | ≤ 4.5x | PASS | known superlinear, #1918: hashes 1892 → 13164 (6.96x, limit 4.50x); allocs 1719 → 9868 (5.74x, limit 4.50x) |
| scale | option-nest fmt | 6.27x | ≤ 4.5x | PASS | alloc_bytes 72077 → 452212 (6.27x, limit 9.00x) |
| scale | option-nest build | 14.62x | ≤ 4.5x | PASS | known superlinear, #1918: alloc_bytes 686250 → 10035380 (14.62x, limit 9.00x); allocs 6971 → 40640 (5.83x, limit 4.50x); hashes 10680 → 62193 (5.82x, limit 4.50x) |
| scale | list-type-nest check | 4.30x | ≤ 4.5x | PASS | hashes 1016 → 4372 (4.30x, limit 4.50x) |
| scale | list-type-nest fmt | 6.09x | ≤ 4.5x | PASS | alloc_bytes 54304 → 330756 (6.09x, limit 9.00x) |
| scale | list-type-nest build | 4.30x | ≤ 4.5x | PASS | hashes 3424 → 14727 (4.30x, limit 4.50x) |
| scale | generic-nest check | 11.92x | ≤ 4.5x | PASS | known superlinear, #1912: type_nodes 598 → 7129 (11.92x, limit 4.50x); hashes 1914 → 21416 (11.19x, limit 4.50x); allocs 1342 → 11828 (8.81x, limit 4.50x) |
| scale | generic-nest fmt | 7.54x | ≤ 4.5x | PASS | alloc_bytes 10446 → 78809 (7.54x, limit 9.00x) |
| scale | generic-nest build | 11.92x | ≤ 4.5x | PASS | known superlinear, #1912: type_nodes 598 → 7129 (11.92x, limit 4.50x); allocs 7656 → 58640 (7.66x, limit 4.50x); hashes 6184 → 43108 (6.97x, limit 4.50x) |
| scale | paren-nest check | 4.46x | ≤ 4.5x | PASS | alloc_bytes 37368 → 166674 (4.46x, limit 9.00x) |
| scale | paren-nest fmt | 3.16x | ≤ 4.5x | PASS | alloc_bytes 32602 → 103167 (3.16x, limit 9.00x) |
| scale | paren-nest build | 4.10x | ≤ 4.5x | PASS | hashes 560 → 2296 (4.10x, limit 4.50x) |
| scale | arith-nest check | 4.00x | ≤ 4.5x | PASS | hashes 480 → 1920 (4.00x, limit 4.50x) |
| scale | arith-nest fmt | 2.88x | ≤ 4.5x | PASS | alloc_bytes 72870 → 209963 (2.88x, limit 9.00x) |
| scale | arith-nest build | 4.23x | ≤ 4.5x | PASS | allocs 3176 → 13441 (4.23x, limit 4.50x) |
| scale | bool-nest check | 3.97x | ≤ 4.5x | PASS | hashes 356 → 1412 (3.97x, limit 4.50x) |
| scale | bool-nest fmt | 5.20x | ≤ 4.5x | PASS | alloc_bytes 34532 → 179698 (5.20x, limit 9.00x) |
| scale | bool-nest build | 4.17x | ≤ 4.5x | PASS | hashes 2067 → 8615 (4.17x, limit 4.50x) |
| scale | bit-nest check | 4.00x | ≤ 4.5x | PASS | type_nodes 512 → 2048 (4.00x, limit 4.50x) |
| scale | bit-nest fmt | 2.85x | ≤ 4.5x | PASS | alloc_bytes 154367 → 439928 (2.85x, limit 9.00x) |
| scale | bit-nest build | 4.01x | ≤ 4.5x | PASS | hashes 5714 → 22890 (4.01x, limit 4.50x) |
| scale | tuple-nest check | 6.75x | ≤ 4.5x | PASS | known superlinear, #1924: hashes 1985 → 13389 (6.75x, limit 4.50x) |
| scale | tuple-nest fmt | 5.70x | ≤ 4.5x | PASS | alloc_bytes 120016 → 684496 (5.70x, limit 9.00x) |
| scale | tuple-nest build | 5.56x | ≤ 4.5x | PASS | known superlinear, #1924: control 429 → 2385 (5.56x, limit 4.50x); hashes 7637 → 38747 (5.07x, limit 4.50x) |
| scale | if-expr-nest check | 3.77x | ≤ 4.5x | PASS | hashes 398 → 1502 (3.77x, limit 4.50x) |
| scale | if-expr-nest fmt | 4.53x | ≤ 4.5x | PASS | alloc_bytes 66638 → 301792 (4.53x, limit 9.00x) |
| scale | if-expr-nest build | 3.79x | ≤ 4.5x | PASS | allocs 1224 → 4643 (3.79x, limit 4.50x) |
| scale | call-nest check | 4.01x | ≤ 4.5x | PASS | hashes 538 → 2158 (4.01x, limit 4.50x) |
| scale | call-nest fmt | 6.48x | ≤ 4.5x | PASS | alloc_bytes 32300 → 209204 (6.48x, limit 9.00x) |
| scale | call-nest build | 3.46x | ≤ 4.5x | PASS | hashes 3099 → 10723 (3.46x, limit 4.50x) |
| scale | index-nest check | 4.52x | ≤ 4.5x | PASS | alloc_bytes 31212 → 141004 (4.52x, limit 9.00x) |
| scale | index-nest fmt | 7.72x | ≤ 4.5x | PASS | alloc_bytes 22932 → 177018 (7.72x, limit 9.00x) |
| scale | index-nest build | 4.06x | ≤ 4.5x | PASS | hashes 1174 → 4766 (4.06x, limit 4.50x) |
| scale | fn-nest check | 3.95x | ≤ 4.5x per 4x bytes (10.7x) | PASS | hashes 2425 → 9589 (3.95x, limit 12.08x) |
| scale | fn-nest fmt | 5.66x | ≤ 4.5x per 4x bytes (10.7x) | PASS | alloc_bytes 130508 → 738328 (5.66x, limit 24.16x) |
| scale | fn-nest build | 15.02x | ≤ 4.5x per 4x bytes (10.7x) | PASS | known superlinear, #1911: use_facts 1952 → 29312 (15.02x, limit 12.08x); hashes 26576 → 321898 (12.11x, limit 12.08x) |
| scale | module-path-nest check | 1.95x | ≤ 4.5x | PASS | allocs 1539 → 3002 (1.95x, limit 4.50x) |
| scale | module-path-nest fmt | 7.90x | ≤ 4.5x | PASS | alloc_bytes 16030 → 126570 (7.90x, limit 9.00x) |
| scale | module-path-nest build | 1.85x | ≤ 4.5x | PASS | allocs 1566 → 2901 (1.85x, limit 4.50x) |
| scale | coverage: node kinds | 119/119 | 119/119 | PASS | exempt: Error, SkippedBody |
| scale | coverage: node kinds repeated | 110/119 | reported | PASS | in no repeat shape: ReturnStmt, RangeExpr, LogicalOrExpr, LogicalAndExpr, BitwiseOrExpr, BitwiseXorExpr, BitwiseAndExpr, ShiftExpr, WhileExpr; exempt: Error, SkippedBody |
| scale | coverage: node kinds nested | 57/119 | reported | PASS | in no nest shape: UseGroup, UseItem, EnumDecl, TraitDecl, ImplDecl, TypeDecl, TestsBlock, Decorator, DefaultValue, ConstParameter, LifetimeParameter, BoundList, TypeDefault, ConstArgument, AssociatedTypeBinding, NamedArgument, EmbeddedField, EnumVariant, VariantSharedData, AssociatedTypeDecl, Derivation, DiscardStmt, BreakStmt, ContinueStmt, UsingStmt, ElseIfClause, MatchGuard, PipeExpr, RefExpr, DerefExpr, PowerExpr, TryExpr, SuspendExpr, PathExpr, TypeArgsExpr, TrailingCallExpr, PlaceholderExpr, MapExpr, MapEntry, SpreadExpr, ComprehensionExpr, ComprehensionFor, ComprehensionIf, ContextExpr, ContextEntry, Interpolation, LiteralPattern, NamedPattern, DataPattern, DataPatternField, SpreadPattern, RangePattern, DynType, ImplType, RefType, UnionType, ParenType, FunctionType, ProjectionType, ContextType, RestType, InferType; exempt: Error, SkippedBody |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 24 of 24 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | 18 ms | ≤ 200 ms | PASS | Timing change ≤10%: noise. |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 2.12x | ≤ 1.30x | FAIL | checked 494 ms, unchecked 232 ms |
| dev-speed | geomean dev / optimized | - | ≤ 4.00x | FAIL | optimized work is within noise: string-build (30.8 ms) |
| dev-speed | worst case dev / optimized | - | ≤ 10.00x | FAIL | optimized work is within noise: string-build (30.8 ms) |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 30 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 26.1 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.13 CPU-s, 36.5 MB | completes ≤ 30 s | PASS | wall 86 ms |
| resources | check, small, warm | 0.00 CPU-s, 18.7 MB | completes ≤ 30 s | PASS | wall 17 ms |
| resources | test, small, cold | 1.07 CPU-s, 93.6 MB | completes ≤ 30 s | PASS | wall 459 ms |
| resources | test, small, warm | 0.44 CPU-s, 66.7 MB | completes ≤ 30 s | PASS | wall 192 ms |
| resources | build, small, cold | 0.59 CPU-s, 85.1 MB | completes ≤ 30 s | PASS | wall 244 ms |
| resources | build, small, warm | 0.02 CPU-s, 31.4 MB | completes ≤ 30 s | PASS | wall 30 ms |
| resources | check, 10k, cold | 0.33 CPU-s, 58.7 MB | completes ≤ 30 s | PASS | wall 160 ms |
| resources | check, 10k, warm | 0.03 CPU-s, 26.1 MB | completes ≤ 30 s | PASS | wall 35 ms |
| resources | test, 10k, cold | 1.67 CPU-s, 127.0 MB | completes ≤ 30 s | PASS | wall 763 ms |
| resources | test, 10k, warm | 0.64 CPU-s, 75.0 MB | completes ≤ 30 s | PASS | wall 276 ms |
| resources | build, 10k, cold | 0.87 CPU-s, 108.3 MB | completes ≤ 30 s | PASS | wall 328 ms |
| resources | build, 10k, warm | 0.04 CPU-s, 35.4 MB | completes ≤ 30 s | PASS | wall 45 ms |
| resources | check, 50k, cold | 1.23 CPU-s, 163.2 MB | completes ≤ 150 s | PASS | wall 501 ms |
| resources | check, 50k, warm | 0.11 CPU-s, 56.6 MB | completes ≤ 150 s | PASS | wall 104 ms |
| resources | test, 50k, cold | 4.77 CPU-s, 281.6 MB | completes ≤ 150 s | PASS | wall 2393 ms |
| resources | test, 50k, warm | 1.97 CPU-s, 105.5 MB | completes ≤ 150 s | PASS | wall 758 ms |
| resources | build, 50k, cold | 1.72 CPU-s, 209.8 MB | completes ≤ 150 s | PASS | wall 623 ms |
| resources | build, 50k, warm | 0.12 CPU-s, 64.3 MB | completes ≤ 150 s | PASS | wall 110 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 90.8 MB | ≤ 22.0 MB | FAIL | RSS 107.5 MB at input 100, 198.4 MB at input 1000; build per input 57 ms at 50, 592 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.2 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 4.2 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 14 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 17.4 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 1.22x | ≤ 1.50x | PASS | p95 83 ms at N = 1; 4 cores; total CPU 180 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 1.95x | ≤ 6.00x | PASS | p95 83 ms at N = 1; 4 cores; total CPU 300 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 0.33 | &lt; 1.00 (sublinear) | PASS | 120 ms at N = 1, 300 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 21.7 MB | ≤ 10.0 MB | FAIL | artifacts 8052 B in 10 files; toolchain 21.7 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 10.8 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.94x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 133 files | the files of one check's cache | FAIL | one check alone: 17 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 14.1 MB | ≤ 5.0 MB | FAIL | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 79 | ≤ 1 | FAIL | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 57k/s | ≥ 100k/s | FAIL | 6000 extra cases took 105 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 81 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 Timing change ≤10%: noise. |
| unit-test-perf | 1,000 unit tests, warm | 615 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed Timing change ≤10%: noise. |
| unit-test-perf | overhead per unit test | 0.5 ms | ≤ 1.0 ms | PASS | 100 tests: 133 ms |
| integration-test-perf | setup per integration test program | 77 ms | ≤ 20 ms | FAIL | 10 programs: 852 ms for 21 tests, 41 ms per test Timing change ≤10%: noise. |
| integration-test-perf | total time growth in programs | 0.981 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 159 ms at 2, 771 ms at 10 programs |
| integration-test-perf | cost per doc test | 65 ms | ≤ 20 ms | FAIL | 10 doc tests: 727 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 0.74x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.41x | ≤ 3.00x | PASS | hd 2.0 ms (p10 0.1, p90 5.3), Node 5.0 ms, Python 49 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 0.29x | ≤ 3.00x | PASS | hd 11 ms (p10 6.3, p90 20), Node 37 ms, Python 156 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 1.00x | ≤ 3.00x | PASS | hd 6.0 ms (p10 4.4, p90 7.9), Node 6.0 ms, Python 8.0 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 3.14x | ≤ 3.00x | FAIL | hd 19 ms (p10 16, p90 22), Node 6.1 ms, Python 10 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.61x | ≤ 3.00x | PASS | hd 8.2 ms (p10 5.0, p90 9.9), Node 13 ms, Python 9.7 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.01 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.01 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 5028 B | ≤ 2048 B | FAIL | benchmarks: fib 5157 B, sum 4933 B, string-build 7813 B, map 6664 B, sort 12721 B |
| size-startup-heap | tiny program, start to first output | 35 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm Timing change ≤10%: noise. |
| size-startup-heap | peak heap, map: hd / Node | 1.06x | ≤ 2.00x | PASS | hd +15.0 MB, Node +14.2 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.43x | ≤ 2.00x | PASS | hd +8.7 MB, Node +20.0 MB over an empty program |
| host-call-overhead | Console println, per call | 3.0 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 14 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 26 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 2.8 ns | ≤ 100 ns | PASS | 2.8 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 16.2M/s | ≥ 1.0M/s | PASS | 124 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 14.4M/s | ≥ 1.0M/s | PASS | 139 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.12x | ≥ 0.50x | FAIL | hd 104 MB/s (45488 B document), Node 893 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.17x | ≥ 0.50x | FAIL | hd 57 MB/s (45488 B document), Node 343 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 3.20x | ≥ 0.50x | PASS | hd 553 MB/s, Node 173 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 2.85x | ≥ 0.50x | PASS | hd 1055 MB/s, Node 371 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.08x | ≥ 0.50x | FAIL | hd 13 MB/s, Node 166 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 6493 B | ≤ 10240 B | PASS | 12590 B at 1075 lines, 71031 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 915 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 2.5 MB | ≤ 10.0 MB | PASS | 35402000 requests; RSS 68.9 MB after warm-up, 72.0 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 7752 of 7752 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
- [2026-11-07T03](daily/2026-11-07T03.json)
- [2026-11-06T19](daily/2026-11-06T19.json)
- [2026-11-06T04](daily/2026-11-06T04.json)
- [2026-11-05T20](daily/2026-11-05T20.json)
- [2026-11-05T04](daily/2026-11-05T04.json)
- [2026-11-04T19](daily/2026-11-04T19.json)
- [2026-11-04T04](daily/2026-11-04T04.json)
- [2026-11-03T19](daily/2026-11-03T19.json)
- [2026-11-03T04](daily/2026-11-03T04.json)
- [2026-11-02T21](daily/2026-11-02T21.json)
- [2026-11-02T03](daily/2026-11-02T03.json)
- [2026-11-01T18](daily/2026-11-01T18.json)
