# Daily goal metrics

Finished: 2026-11-28T20:21:05.503Z. Commit: c97e90a3a7420d7c95cc828a223e1e22c937f7a1.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 7763 64-Core Processor).
Compiler: target/dist/hd.

290 passed, 23 failed, 5 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p95 check after an edit: pass 38 ms → pass 31 ms
- pathological: iterator-chain: time at 4x: pass 41 ms → pass 34 ms
- pathological: wide-list-literal: time at 4x: pass 56 ms → pass 66 ms
- pathological: deep-generic: time at 4x: pass 31 ms → pass 27 ms
- pathological: question-chain: time at 4x: pass 34 ms → pass 40 ms
- fmt: hd fmt, 10k lines: pass 34 ms → pass 45 ms
- startup: hd --version: wall time: pass 6.9 ms → pass 6.0 ms
- proptest-perf: property cases per second: fail 71k/s → fail 80k/s
- proptest-perf: time to shrink a known failure: pass 21 ms → pass 18 ms
- size-startup-heap: tiny program, start to first output: fail 11 ms → fail 9.2 ms
- size-startup-heap: peak heap, map: hd / Node: pass 0.53x → pass 0.95x
- size-startup-heap: peak heap, string-build: hd / Node: pass 0.20x → pass 0.16x
- suspension-overhead: await of a ready value, per await: pass 0.00 ns → pass 2.3 ns
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.12x → fail 0.10x
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.20x → fail 0.25x
- text-throughput: string build, hd / Node MB/s: pass 2.94x → pass 2.34x
- text-throughput: split, hd / Node MB/s: pass 1.05x → pass 1.29x

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 30 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 31 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 386 ms | ≤ 1000 ms | PASS | Timing change ≤10%: noise. |
| test-latency | median edit-then-test | 135 ms | ≤ 300 ms | PASS | Timing change ≤10%: noise. |
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
| pathological | overlapping-from: time at 4x | 61 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | overlapping-from: peak RSS at 4x | 50.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 47 → 61 ms, start-up 28 ms |
| pathological | deep-blocks: time at 4x | 27 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-blocks: peak RSS at 4x | 42.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 27 → 27 ms, start-up 28 ms |
| pathological | deep-parens: time at 4x | 25 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 42.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 29 → 25 ms, start-up 28 ms |
| pathological | method-chain: time at 4x | 33 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | method-chain: peak RSS at 4x | 43.0 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 29 → 33 ms, start-up 28 ms |
| pathological | iterator-chain: time at 4x | 34 ms | ≤ 2000 ms | PASS |  |
| pathological | iterator-chain: peak RSS at 4x | 42.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | iterator-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 31 → 34 ms, start-up 28 ms |
| pathological | wide-list-literal: time at 4x | 66 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 48.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 45 → 66 ms, start-up 28 ms |
| pathological | large-enum-match: time at 4x | 38 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | large-enum-match: peak RSS at 4x | 44.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 31 → 38 ms, start-up 28 ms |
| pathological | deep-generic: time at 4x | 27 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 42.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 29 → 27 ms, start-up 28 ms |
| pathological | question-chain: time at 4x | 40 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 46.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 35 → 40 ms, start-up 28 ms |
| pathological | big-file: time at 4x | 174 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | big-file: peak RSS at 4x | 92.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.07 | ≤ 1.30 | PASS | 2x→4x; 98 → 174 ms, start-up 28 ms |
| scale | fn-items check | 4.03x | ≤ 5x | PASS | hashes 196482 → 792398 (4.03x, limit 5.00x) |
| scale | fn-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | fn-items build | 4.02x | ≤ 5x | PASS | hashes 584813 → 2351548 (4.02x, limit 5.00x) |
| scale | bang-chain check | 4.03x | ≤ 5x | PASS | hashes 190506 → 767546 (4.03x, limit 5.00x) |
| scale | bang-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | bang-chain build | 4.03x | ≤ 5x | PASS | hashes 536743 → 2161275 (4.03x, limit 5.00x) |
| scale | struct-items check | 4.04x | ≤ 5x | PASS | hashes 199174 → 804502 (4.04x, limit 5.00x) |
| scale | struct-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | struct-items build | 4.01x | ≤ 5x | PASS | hashes 760281 → 3047813 (4.01x, limit 5.00x) |
| scale | enum-items check | 4.00x | ≤ 5x | PASS | scope_probes 25500 → 102000 (4.00x, limit 5.00x) |
| scale | enum-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | enum-items build | 4.00x | ≤ 5x | PASS | hashes 1643476 → 6574026 (4.00x, limit 5.00x) |
| scale | trait-items check | 4.01x | ≤ 5x | PASS | allocs 528877 → 2120321 (4.01x, limit 5.00x) |
| scale | trait-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | trait-items build | 4.00x | ≤ 5x | PASS | allocs 1253175 → 5015540 (4.00x, limit 5.00x) |
| scale | impl-methods check | 4.05x | ≤ 5x | PASS | allocs 214648 → 869084 (4.05x, limit 5.00x) |
| scale | impl-methods fmt | 3.97x | ≤ 5x | PASS | allocs 4045 → 16057 (3.97x, limit 5.00x) |
| scale | impl-methods build | 4.01x | ≤ 5x | PASS | hashes 842697 → 3379968 (4.01x, limit 5.00x) |
| scale | traits-of-one-struct check | 4.01x | ≤ 5x | PASS | allocs 404227 → 1619188 (4.01x, limit 5.00x) |
| scale | traits-of-one-struct fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | traits-of-one-struct build | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | impls-of-one-trait check | 4.02x | ≤ 5x | PASS | allocs 376688 → 1512501 (4.02x, limit 5.00x) |
| scale | impls-of-one-trait fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | impls-of-one-trait build | 4.00x | ≤ 5x | PASS | hashes 1703378 → 6819204 (4.00x, limit 5.00x) |
| scale | operator-impls check | 4.01x | ≤ 5x | PASS | allocs 453389 → 1818853 (4.01x, limit 5.00x) |
| scale | operator-impls fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | operator-impls build | 4.00x | ≤ 5x | PASS | scope_probes 25000 → 100000 (4.00x, limit 5.00x) |
| scale | derive-items check | 4.00x | ≤ 5x | PASS | trait_lookup 23498 → 93998 (4.00x, limit 5.00x) |
| scale | derive-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | derive-items build | 4.00x | ≤ 5x | PASS | trait_lookup 23498 → 93998 (4.00x, limit 5.00x) |
| scale | dyn-values check | 4.01x | ≤ 5x | PASS | allocs 458946 → 1838854 (4.01x, limit 5.00x) |
| scale | dyn-values fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | dyn-values build | 4.00x | ≤ 5x | PASS | scope_probes 15500 → 62000 (4.00x, limit 5.00x) |
| scale | consts check | 3.98x | ≤ 5x | PASS | hashes 63405 → 252265 (3.98x, limit 5.00x) |
| scale | consts fmt | 3.89x | ≤ 5x | PASS | allocs 2087 → 8109 (3.89x, limit 5.00x) |
| scale | consts build | 4.00x | ≤ 5x | PASS | loan_queries 4506 → 18006 (4.00x, limit 5.00x) |
| scale | local-items check | 4.00x | ≤ 5x | PASS | hashes 562267 → 2249315 (4.00x, limit 5.00x) |
| scale | local-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | local-items build | 4.00x | ≤ 5x | PASS | hashes 1530927 → 6125050 (4.00x, limit 5.00x) |
| scale | syntax-items check | 4.00x | ≤ 5x | PASS | item_body_checks 1700 → 6800 (4.00x, limit 5.00x) |
| scale | syntax-items fmt | 4.00x | ≤ 5x | PASS | hashes 17960 → 71864 (4.00x, limit 5.00x) |
| scale | syntax-items build | 4.00x | ≤ 5x | PASS | item_body_checks 1700 → 6800 (4.00x, limit 5.00x) |
| scale | struct-chain check | 4.05x | ≤ 5x | PASS | hashes 151442 → 613206 (4.05x, limit 5.00x) |
| scale | struct-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | struct-chain-values check | 4.02x | ≤ 5x | PASS | hashes 115912 → 466008 (4.02x, limit 5.00x) |
| scale | struct-chain-values fmt | 4.02x | ≤ 5x | PASS | hashes 2082 → 8370 (4.02x, limit 5.00x) |
| scale | struct-chain-values build | 4.00x | ≤ 5x | PASS | scope_probes 4900 → 19600 (4.00x, limit 5.00x) |
| scale | modules check | 4.00x | ≤ 5x | PASS | hashes 1154413 → 4622045 (4.00x, limit 5.00x) |
| scale | modules fmt | 4.00x | ≤ 5x | PASS | hashes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | modules build | 4.00x | ≤ 5x | PASS | hashes 1554923 → 6223312 (4.00x, limit 5.00x) |
| scale | use-names check | 4.03x | ≤ 5x | PASS | hashes 216717 → 874437 (4.03x, limit 5.00x) |
| scale | use-names fmt | 4.01x | ≤ 5x | PASS | hashes 3783 → 15159 (4.01x, limit 5.00x) |
| scale | use-names build | 4.01x | ≤ 5x | PASS | hashes 606908 → 2435335 (4.01x, limit 5.00x) |
| scale | generic-fns check | 4.01x | ≤ 5x | PASS | hashes 304935 → 1223807 (4.01x, limit 5.00x) |
| scale | generic-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-fns build | 4.01x | ≤ 5x | PASS | hashes 1072070 → 4301655 (4.01x, limit 5.00x) |
| scale | generic-params check | 4.02x | ≤ 5x | PASS | hashes 16173 → 65053 (4.02x, limit 5.00x) |
| scale | generic-params fmt | 4.10x | ≤ 10x | PASS | alloc_bytes 350594 → 1438988 (4.10x, limit 10.00x) |
| scale | generic-params build | 4.05x | ≤ 5x | PASS | hashes 25602 → 103620 (4.05x, limit 5.00x) |
| scale | param-bounds check | 4.00x | ≤ 5x | PASS | scope_probes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | param-bounds fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | param-bounds build | 4.01x | ≤ 5x | PASS | allocs 38100 → 152748 (4.01x, limit 5.00x) |
| scale | type-defaults check | 4.01x | ≤ 5x | PASS | allocs 692217 → 2774849 (4.01x, limit 5.00x) |
| scale | type-defaults fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | type-defaults build | 4.00x | ≤ 5x | PASS | allocs 1503578 → 6015434 (4.00x, limit 5.00x) |
| scale | alias-chain check | 4.00x | ≤ 5x | PASS | scope_probes 6600 → 26400 (4.00x, limit 5.00x) |
| scale | alias-chain fmt | 4.03x | ≤ 5x | PASS | hashes 1234 → 4978 (4.03x, limit 5.00x) |
| scale | alias-chain build | 4.00x | ≤ 5x | PASS | scope_probes 6600 → 26400 (4.00x, limit 5.00x) |
| scale | row-chain check | 4.03x | ≤ 5x | PASS | hashes 222760 → 897332 (4.03x, limit 5.00x) |
| scale | row-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | row-chain build | 4.02x | ≤ 5x | PASS | hashes 650177 → 2615441 (4.02x, limit 5.00x) |
| scale | row-traits check | 4.00x | ≤ 5x | PASS | scope_probes 2300 → 9200 (4.00x, limit 5.00x) |
| scale | row-traits fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | row-traits build | 4.00x | ≤ 5x | PASS | scope_probes 2300 → 9200 (4.00x, limit 5.00x) |
| scale | closures check | 4.00x | ≤ 5x | PASS | scope_probes 2000 → 8000 (4.00x, limit 5.00x) |
| scale | closures fmt | 3.94x | ≤ 5x | PASS | allocs 3061 → 12073 (3.94x, limit 5.00x) |
| scale | closures build | 4.00x | ≤ 5x | PASS | scope_probes 2000 → 8000 (4.00x, limit 5.00x) |
| scale | generic-local-fns check | 4.03x | ≤ 5x | PASS | hashes 239440 → 965900 (4.03x, limit 5.00x) |
| scale | generic-local-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-local-fns build | 4.01x | ≤ 5x | PASS | hashes 1205203 → 4833719 (4.01x, limit 5.00x) |
| scale | let-statements check | 4.00x | ≤ 5x | PASS | scope_probes 500 → 2000 (4.00x, limit 5.00x) |
| scale | let-statements fmt | 3.94x | ≤ 5x | PASS | allocs 2554 → 10066 (3.94x, limit 5.00x) |
| scale | let-statements build | 4.08x | ≤ 5x | PASS | hashes 290568 → 1185057 (4.08x, limit 5.00x) |
| scale | matches check | 4.00x | ≤ 5x | PASS | scope_probes 1000 → 4000 (4.00x, limit 5.00x) |
| scale | matches fmt | 3.97x | ≤ 5x | PASS | allocs 6057 → 24069 (3.97x, limit 5.00x) |
| scale | matches build | 4.00x | ≤ 5x | PASS | scope_probes 1000 → 4000 (4.00x, limit 5.00x) |
| scale | enum-arms check | 4.00x | ≤ 5x | PASS | scope_probes 5000 → 20000 (4.00x, limit 5.00x) |
| scale | enum-arms fmt | 3.91x | ≤ 5x | PASS | allocs 1551 → 6065 (3.91x, limit 5.00x) |
| scale | enum-arms build | 4.00x | ≤ 5x | PASS | scope_probes 5000 → 20000 (4.00x, limit 5.00x) |
| scale | tuple-patterns check | 4.00x | ≤ 5x | PASS | scope_probes 1000 → 4000 (4.00x, limit 5.00x) |
| scale | tuple-patterns fmt | 3.95x | ≤ 5x | PASS | allocs 3054 → 12066 (3.95x, limit 5.00x) |
| scale | tuple-patterns build | 4.00x | ≤ 5x | PASS | scope_probes 1000 → 4000 (4.00x, limit 5.00x) |
| scale | range-patterns check | 4.02x | ≤ 5x | PASS | hashes 256003 → 1030251 (4.02x, limit 5.00x) |
| scale | range-patterns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | range-patterns build | 4.01x | ≤ 5x | PASS | hashes 1002293 → 4021936 (4.01x, limit 5.00x) |
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
| scale | if-nest check | 3.91x | ≤ 15.11x | PASS | hashes 774 → 3030 (3.91x, limit 15.11x) |
| scale | if-nest fmt | - | ≤ 15.11x | PASS | no counter reached the floor |
| scale | if-nest build | 4.08x | ≤ 15.11x | PASS | hashes 6588 → 26887 (4.08x, limit 15.11x) |
| scale | block-nest check | 3.94x | ≤ 15.29x | PASS | hashes 832 → 3280 (3.94x, limit 15.29x) |
| scale | block-nest fmt | - | ≤ 15.29x | PASS | no counter reached the floor |
| scale | block-nest build | 4.04x | ≤ 15.29x | PASS | hashes 7118 → 28788 (4.04x, limit 15.29x) |
| scale | for-nest check | 5.56x | ≤ 14.5x | PASS | hashes 3730 → 20722 (5.56x, limit 14.50x) |
| scale | for-nest fmt | - | ≤ 14.5x | PASS | no counter reached the floor |
| scale | for-nest build | 4.13x | ≤ 14.5x | PASS | hashes 32264 → 133185 (4.13x, limit 14.50x) |
| scale | while-nest check | 3.87x | ≤ 15.11x | PASS | hashes 1053 → 4077 (3.87x, limit 15.11x) |
| scale | while-nest fmt | - | ≤ 15.11x | PASS | no counter reached the floor |
| scale | while-nest build | 4.00x | ≤ 15.11x | PASS | control 400 → 1600 (4.00x, limit 15.11x) |
| scale | match-nest check | 4.86x | ≤ 16.03x | PASS | hashes 1872 → 9092 (4.86x, limit 16.03x) |
| scale | match-nest fmt | - | ≤ 16.03x | PASS | no counter reached the floor |
| scale | match-nest build | 4.27x | ≤ 16.03x | PASS | hashes 8078 → 34459 (4.27x, limit 16.03x) |
| scale | closure-nest check | 4.10x | ≤ 11.45x | PASS | allocs 1462 → 5991 (4.10x, limit 11.45x) |
| scale | closure-nest fmt | - | ≤ 11.45x | PASS | no counter reached the floor |
| scale | closure-nest build | 4.00x | ≤ 11.45x | PASS | use_facts 960 → 3840 (4.00x, limit 11.45x) |
| scale | option-nest check | 4.18x | ≤ 5x | PASS | hashes 1864 → 7792 (4.18x, limit 5.00x) |
| scale | option-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | option-nest build | 4.11x | ≤ 5x | PASS | hashes 13394 → 55016 (4.11x, limit 5.00x) |
| scale | list-type-nest check | 4.28x | ≤ 5x | PASS | hashes 1297 → 5550 (4.28x, limit 5.00x) |
| scale | list-type-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | list-type-nest build | 4.00x | ≤ 5x | PASS | hashes 6175 → 24712 (4.00x, limit 5.00x) |
| scale | generic-nest check | 4.32x | ≤ 5x | PASS | hashes 1622 → 7010 (4.32x, limit 5.00x) |
| scale | generic-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | generic-nest build | 4.85x | ≤ 5x | PASS | hashes 7348 → 35636 (4.85x, limit 5.00x) |
| scale | paren-nest check | - | ≤ 5x | PASS | no counter reached the floor |
| scale | paren-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | paren-nest build | 3.79x | ≤ 5x | PASS | allocs 819 → 3106 (3.79x, limit 5.00x) |
| scale | arith-nest check | 4.00x | ≤ 5x | PASS | hashes 440 → 1760 (4.00x, limit 5.00x) |
| scale | arith-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | arith-nest build | 4.17x | ≤ 5x | PASS | hashes 7570 → 31563 (4.17x, limit 5.00x) |
| scale | bool-nest check | 3.95x | ≤ 5x | PASS | hashes 391 → 1543 (3.95x, limit 5.00x) |
| scale | bool-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | bool-nest build | 4.09x | ≤ 5x | PASS | use_facts 855 → 3495 (4.09x, limit 5.00x) |
| scale | bit-nest check | 3.99x | ≤ 5x | PASS | hashes 820 → 3268 (3.99x, limit 5.00x) |
| scale | bit-nest fmt | 5.84x | ≤ 10x | PASS | alloc_bytes 206985 → 1209058 (5.84x, limit 10.00x) |
| scale | bit-nest build | 4.24x | ≤ 5x | PASS | hashes 11197 → 47461 (4.24x, limit 5.00x) |
| scale | tuple-nest check | 4.04x | ≤ 5x | PASS | hashes 2058 → 8314 (4.04x, limit 5.00x) |
| scale | tuple-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | tuple-nest build | 4.06x | ≤ 5x | PASS | hashes 13162 → 53386 (4.06x, limit 5.00x) |
| scale | tuple-nest-string check | 4.01x | ≤ 5x | PASS | hashes 2079 → 8335 (4.01x, limit 5.00x) |
| scale | tuple-nest-string fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | tuple-nest-string build | 4.31x | ≤ 5x | PASS | control 406 → 1750 (4.31x, limit 5.00x) |
| scale | if-expr-nest check | 3.74x | ≤ 5x | PASS | hashes 324 → 1212 (3.74x, limit 5.00x) |
| scale | if-expr-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | if-expr-nest build | 3.83x | ≤ 5x | PASS | use_facts 628 → 2404 (3.83x, limit 5.00x) |
| scale | call-nest check | 3.86x | ≤ 5x | PASS | hashes 571 → 2203 (3.86x, limit 5.00x) |
| scale | call-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | call-nest build | 2.87x | ≤ 5x | PASS | hashes 5336 → 15335 (2.87x, limit 5.00x) |
| scale | index-nest check | - | ≤ 5x | PASS | no counter reached the floor |
| scale | index-nest fmt | - | ≤ 5x | PASS | no counter reached the floor |
| scale | index-nest build | 4.06x | ≤ 5x | PASS | hashes 3252 → 13210 (4.06x, limit 5.00x) |
| scale | fn-nest check | 3.94x | ≤ 13.42x | PASS | hashes 2956 → 11648 (3.94x, limit 13.42x) |
| scale | fn-nest fmt | - | ≤ 13.42x | PASS | no counter reached the floor |
| scale | fn-nest build | 4.04x | ≤ 13.42x | PASS | hashes 13222 → 53426 (4.04x, limit 13.42x) |
| scale | module-path-nest check | 5.07x | ≤ 14.36x | PASS | hashes 35340 → 179220 (5.07x, limit 14.36x) |
| scale | module-path-nest fmt | 9.99x | ≤ 28.72x | PASS | alloc_bytes 158233 → 1580610 (9.99x, limit 28.72x) |
| scale | module-path-nest build | 7.49x | ≤ 14.36x | PASS | hashes 1875 → 14043 (7.49x, limit 14.36x) |
| scale | coverage: node kinds | 122/122 | 122/122 | PASS | exempt: Error |
| scale | coverage: node kinds repeated | 113/122 | reported | PASS | in no repeat shape: ReturnStmt, RangeExpr, LogicalOrExpr, LogicalAndExpr, BitwiseOrExpr, BitwiseXorExpr, BitwiseAndExpr, ShiftExpr, WhileExpr; exempt: Error |
| scale | coverage: node kinds nested | 59/122 | reported | PASS | in no nest shape: EnumDecl, TraitDecl, ImplDecl, TypeDecl, TestsBlock, Decorator, DefaultValue, ConstParameter, LifetimeParameter, BoundList, WhereClause, WhereBound, TypeDefault, ConstArgument, AssociatedTypeBinding, NamedArgument, EmbeddedField, EnumVariant, VariantSharedData, AssociatedTypeDecl, Derivation, DiscardStmt, BreakStmt, ContinueStmt, UsingStmt, ElseIfClause, MatchGuard, PipeExpr, RefExpr, DerefExpr, PowerExpr, TryExpr, SuspendExpr, PathExpr, TypeArgsExpr, TrailingCallExpr, PlaceholderExpr, MapExpr, MapEntry, SpreadExpr, ComprehensionExpr, ComprehensionFor, ComprehensionIf, ContextExpr, ContextEntry, Interpolation, LiteralPattern, NamedPattern, DataPattern, DataPatternField, SpreadPattern, RangePattern, DynType, ImplType, RefType, UnionType, ParenType, FunctionType, ProjectionType, ContextType, RestType, InferType, MarkerBound; exempt: Error |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 4 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 68 of 69; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 38 of 38 safe |
| fmt | hd fmt, 10k lines | 45 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 2.16x | ≤ 1.30x | FAIL | checked 379 ms, unchecked 176 ms |
| dev-speed | geomean dev / optimized | 1.54x | ≤ 4.00x | PASS | fib 1.02x, sum 1.32x, string-build 1.92x, map 1.27x, sort 2.58x |
| dev-speed | worst case dev / optimized | 2.58x | ≤ 10.00x | PASS | sort |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 0.0 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 22.3 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.16 CPU-s, 59.4 MB | completes ≤ 30 s | PASS | wall 170 ms |
| resources | check, small, warm | 0.00 CPU-s, 22.2 MB | completes ≤ 30 s | PASS | wall 10 ms |
| resources | test, small, cold | 0.89 CPU-s, 123.6 MB | completes ≤ 30 s | PASS | wall 898 ms |
| resources | test, small, warm | 0.03 CPU-s, 45.6 MB | completes ≤ 30 s | PASS | wall 40 ms |
| resources | build, small, cold | 0.70 CPU-s, 107.4 MB | completes ≤ 30 s | PASS | wall 715 ms |
| resources | build, small, warm | 0.00 CPU-s, 22.5 MB | completes ≤ 30 s | PASS | wall 11 ms |
| resources | check, 10k, cold | 0.38 CPU-s, 91.4 MB | completes ≤ 30 s | PASS | wall 396 ms |
| resources | check, 10k, warm | 0.00 CPU-s, 22.3 MB | completes ≤ 30 s | PASS | wall 12 ms |
| resources | test, 10k, cold | 1.41 CPU-s, 164.8 MB | completes ≤ 30 s | PASS | wall 1423 ms |
| resources | test, 10k, warm | 0.06 CPU-s, 60.5 MB | completes ≤ 30 s | PASS | wall 75 ms |
| resources | build, 10k, cold | 0.70 CPU-s, 109.2 MB | completes ≤ 30 s | PASS | wall 712 ms |
| resources | build, 10k, warm | 0.00 CPU-s, 22.7 MB | completes ≤ 30 s | PASS | wall 12 ms |
| resources | check, 50k, cold | 1.32 CPU-s, 241.2 MB | completes ≤ 150 s | PASS | wall 1336 ms |
| resources | check, 50k, warm | 0.00 CPU-s, 28.3 MB | completes ≤ 150 s | PASS | wall 17 ms |
| resources | test, 50k, cold | 3.75 CPU-s, 393.3 MB | completes ≤ 150 s | PASS | wall 3757 ms |
| resources | test, 50k, warm | 0.24 CPU-s, 117.7 MB | completes ≤ 150 s | PASS | wall 244 ms |
| resources | build, 50k, cold | 0.73 CPU-s, 115.0 MB | completes ≤ 150 s | PASS | wall 746 ms |
| resources | build, 50k, warm | 0.00 CPU-s, 30.5 MB | completes ≤ 150 s | PASS | wall 18 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 675.4 MB | ≤ 22.0 MB | FAIL | RSS 93.8 MB at input 100, 769.2 MB at input 1000; build per input 127 ms at 50, 1354 ms at 1000; 4 KiB pages |
| startup | hd --version: wall time | 6.0 ms | ≤ 20 ms | PASS |  |
| startup | hd --version: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd --version: peak RSS | 9.5 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd, just written: wall time | 9.1 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, just written: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, just written: peak RSS | 21.4 MB | ≤ 10.0 MB | FAIL |  |
| startup | hd check empty.hd, settled: wall time | 9.7 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd check empty.hd, settled: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, settled: peak RSS | 21.3 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 1.60x | ≤ 1.50x | FAIL | p95 144 ms at N = 1; 4 cores; total CPU 350 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 2.82x | ≤ 6.00x | PASS | p95 144 ms at N = 1; 4 cores; total CPU 870 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 0.55 | &lt; 1.00 (sublinear) | PASS | 190 ms at N = 1, 870 ms at N = 16 |
| disk | artifacts per worktree | 0.0 MB | ≤ 10.0 MB | PASS | 10 files; HD_CACHE 13.5 MB, shared per user, not counted |
| disk | toolchain, once per machine | 35.6 MB | recorded, no target | PASS | toolchain files: /home/runner/work/hd-lang/hd-lang/target/dist/hd |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 2.06x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 319 files | the files of one check's cache | FAIL | one check alone: 36 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 13.6 MB | ≤ 5.0 MB | FAIL | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 1 | ≤ 1 | PASS | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 80k/s | ≥ 100k/s | FAIL | 6000 extra cases took 75 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 18 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 65 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed Timing change ≤10%: noise. |
| unit-test-perf | overhead per unit test | 0.0 ms | ≤ 1.0 ms | PASS | 100 tests: 25 ms |
| integration-test-perf | setup per integration test program | 8.3 ms | ≤ 20 ms | PASS | 10 programs: 106 ms for 21 tests, 5.1 ms per test |
| integration-test-perf | total time growth in programs | 0.898 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 21 ms at 2, 89 ms at 10 programs |
| integration-test-perf | cost per doc test | 5.7 ms | ≤ 20 ms | PASS | 10 doc tests: 75 ms; the run reports 11 of 11 passed Timing change ≤10%: noise. |
| runtime | geomean hd / Node | 0.47x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.79x | ≤ 3.00x | PASS | hd 7.3 ms (p10 7.1, p90 7.3), Node 9.2 ms, Python 100 ms; release build, run as hd run |
| runtime | sum: hd / Node | 0.12x | ≤ 3.00x | PASS | hd 6.3 ms (p10 6.1, p90 6.6), Node 53 ms, Python 283 ms; release build, run as hd run |
| runtime | string-build: hd / Node | 0.53x | ≤ 3.00x | PASS | hd 3.4 ms (p10 3.4, p90 3.5), Node 6.4 ms, Python 15 ms; release build, run as hd run |
| runtime | map: hd / Node | 0.97x | ≤ 3.00x | PASS | hd 7.3 ms (p10 7.1, p90 7.3), Node 7.4 ms, Python 18 ms; release build, run as hd run |
| runtime | sort: hd / Node | 0.46x | ≤ 3.00x | PASS | hd 11 ms (p10 11, p90 11), Node 24 ms, Python 19 ms; release build, run as hd run |
| allocations | counted loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | list loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | map loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | iterator chain, bytes per iteration | - | ≤ 1 allocation (≤ 32 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| size-startup-heap | tiny program, release Wasm size | - | ≤ 2048 B | N/A | not supported by this hd: the build wrote no .wasm file (release build, run as hd run) |
| size-startup-heap | tiny program, start to first output | 9.2 ms | ≤ 5.0 ms | FAIL | release build, run as hd run |
| size-startup-heap | peak heap, map: hd / Node | 0.95x | ≤ 2.00x | PASS | hd +13.1 MB, Node +13.8 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.16x | ≤ 2.00x | PASS | hd +3.2 MB, Node +19.8 MB over an empty program |
| host-call-overhead | Console println, per call | 2.8 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd run Timing change ≤10%: noise. |
| host-call-overhead | Fs read_text!, per call | 10 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd run |
| host-call-overhead | serde boundary (list_dir!), per call | 18 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd run |
| suspension-overhead | await of a ready value, per await | 2.3 ns | ≤ 100 ns | PASS | 2.3 ns per iteration; release build, run as hd run |
| suspension-overhead | all! of two ready tasks, throughput | 10.3M/s | ≥ 1.0M/s | PASS | 195 ns per iteration; release build, run as hd run |
| suspension-overhead | race! of two ready tasks, throughput | 7.9M/s | ≥ 1.0M/s | PASS | 254 ns per iteration; release build, run as hd run |
| serde-throughput | JSON encode, hd / Node MB/s | 0.10x | ≥ 0.50x | FAIL | hd 48 MB/s (45488 B document), Node 463 MB/s; release build, run as hd run |
| serde-throughput | JSON decode, hd / Node MB/s | 0.25x | ≥ 0.50x | FAIL | hd 37 MB/s (45488 B document), Node 150 MB/s; release build, run as hd run |
| text-throughput | string build, hd / Node MB/s | 2.34x | ≥ 0.50x | PASS | hd 296 MB/s, Node 127 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd run |
| text-throughput | split, hd / Node MB/s | 1.29x | ≥ 0.50x | PASS | hd 484 MB/s, Node 373 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd run |
| text-throughput | regex find_all, hd / Node MB/s | 0.44x | ≥ 0.50x | FAIL | hd 39 MB/s, Node 89 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd run |
| dead-code | Wasm bytes per 1,000 lines | - | ≤ 10240 B | FAIL | the build wrote no file to measure (release build, run as hd run) |
| dead-code | no unused std in the program | - | none | FAIL | the build wrote no file to measure (release build, run as hd run) |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 13662000 requests; RSS 41.5 MB after warm-up, 41.5 MB at the end; release build, run as hd run |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 9473 of 9473 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

- [2026-11-28T20](daily/2026-11-28T20.json)
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
