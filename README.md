# Daily goal metrics

Finished: 2026-11-17T21:02:15.600Z. Commit: 9db0af8b6577609fb83433adb48ea79d14d7cf98.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V45 96-Core Processor).
Compiler: target/dist/hd.

281 passed, 27 failed, 0 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p95 check after an edit: pass 29 ms → pass 32 ms
- cold-check: median cold check, 10k lines: pass 166 ms → pass 148 ms
- mistakes: largest diagnostic text (bytes / 4): fail 65 tokens → fail 83 tokens
- pathological: deep-blocks: time at 4x: pass 16 ms → pass 20 ms
- pathological: deep-parens: time at 4x: pass 16 ms → pass 18 ms
- pathological: method-chain: time at 4x: pass 19 ms → pass 22 ms
- pathological: iterator-chain: time at 4x: pass 21 ms → pass 24 ms
- pathological: wide-list-literal: time at 4x: pass 33 ms → pass 38 ms
- pathological: deep-generic: time at 4x: pass 17 ms → pass 20 ms
- scale: trait-items build: pass 4.00x → fail 5.75x
- scale: derive-items build: pass 4.28x → fail 5.06x
- scale: type-defaults build: pass 4.00x → fail 5.50x
- fmt: hd fmt, 10k lines: pass 19 ms → pass 17 ms
- check-cost: checked run time / unchecked run time: fail 2.03x → fail 2.52x
- dev-speed: geomean dev / optimized: pass 1.37x → fail -
- dev-speed: worst case dev / optimized: pass 4.50x → fail -
- long-session: RSS growth, REPL inputs 100 to 1000: fail 77.8 MB → fail 61.3 MB
- startup: hd help (no version command): wall time: pass 3.0 ms → pass 3.4 ms
- concurrency: p95 latency at N = 16 vs N = 1: pass 1.81x → pass 2.07x
- concurrency: total CPU growth exponent, N = 1 to 16: pass 0.26 → pass 0.29
- proptest-perf: property cases per second: fail 45k/s → fail 59k/s
- runtime: fib: hd / Node: pass 0.35x → pass 0.31x
- runtime: sum: hd / Node: pass 0.07x → pass 0.06x
- runtime: string-build: hd / Node: pass 0.20x → pass 0.23x
- runtime: sort: hd / Node: pass 0.28x → pass 0.25x
- allocations: list loop, bytes per iteration: pass 0.08 B/iter → pass 0.00 B/iter
- allocations: iterator chain, bytes per iteration: pass 0.02 B/iter → pass 0.01 B/iter
- host-call-overhead: Console println, per call: fail 2.5 µs → fail 3.1 µs
- host-call-overhead: Fs read_text!, per call: pass 8.6 µs → fail 10 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 17 µs → fail 22 µs
- suspension-overhead: all! of two ready tasks, throughput: pass 16.5M/s → pass 13.9M/s
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.20x → fail 0.15x
- text-throughput: split, hd / Node MB/s: pass 2.56x → pass 2.97x
- text-throughput: regex find_all, hd / Node MB/s: fail 0.10x → fail 0.08x
- conformance: listed known failures: pass 0 → fail 1

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 30 ms | ≤ 50 ms | PASS | Timing change ≤10%: noise. |
| edit-latency | p95 check after an edit | 32 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 148 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 143 ms | ≤ 300 ms | PASS | Timing change ≤10%: noise. |
| mistakes | programs with exactly one diagnostic | 100.0% | ≥ 95.0% | PASS | 70 of 70 |
| mistakes | one diagnostic with the expected code | 100.0% | ≥ 95.0% | PASS | 70 of 70 |
| mistakes | fix-it makes hd check pass | 35.7% | ≥ 80.0% | FAIL | 25 of 70 resolved; 25 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 83 tokens | ≤ 60 tokens | FAIL | largest: map-get-store-back; median 43 tokens, 171 B; 67 of 70 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 36 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | overlapping-from: peak RSS at 4x | 27.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 28 → 36 ms, start-up 19 ms |
| pathological | deep-blocks: time at 4x | 20 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 22.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 20 ms, start-up 19 ms |
| pathological | deep-parens: time at 4x | 18 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 21.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 18 ms, start-up 19 ms |
| pathological | method-chain: time at 4x | 22 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 23.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 22 ms, start-up 19 ms |
| pathological | iterator-chain: time at 4x | 24 ms | ≤ 2000 ms | PASS |  |
| pathological | iterator-chain: peak RSS at 4x | 23.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | iterator-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 24 ms, start-up 19 ms |
| pathological | wide-list-literal: time at 4x | 38 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 27.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 28 → 38 ms, start-up 19 ms |
| pathological | large-enum-match: time at 4x | 26 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | large-enum-match: peak RSS at 4x | 24.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 23 → 26 ms, start-up 19 ms |
| pathological | deep-generic: time at 4x | 20 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 22.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 19 → 20 ms, start-up 19 ms |
| pathological | question-chain: time at 4x | 23 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | question-chain: peak RSS at 4x | 25.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 23 ms, start-up 19 ms |
| pathological | big-file: time at 4x | 109 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | big-file: peak RSS at 4x | 54.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 63 → 109 ms, start-up 19 ms |
| scale | fn-items check | 4.04x | ≤ 5x | PASS | hashes 191646 → 774614 (4.04x, limit 5.00x) |
| scale | fn-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | fn-items build | 4.01x | ≤ 5x | PASS | hashes 546448 → 2191099 (4.01x, limit 5.00x) |
| scale | struct-items check | 4.03x | ≤ 5x | PASS | hashes 206516 → 832076 (4.03x, limit 5.00x) |
| scale | struct-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | struct-items build | 4.00x | ≤ 5x | PASS | scope_probes 9000 → 36000 (4.00x, limit 5.00x) |
| scale | enum-items check | 4.00x | ≤ 5x | PASS | hashes 520244 → 2081272 (4.00x, limit 5.00x) |
| scale | enum-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | enum-items build | 4.00x | ≤ 5x | PASS | scope_probes 23500 → 94000 (4.00x, limit 5.00x) |
| scale | trait-items check | 4.00x | ≤ 5x | PASS | scope_probes 19500 → 78000 (4.00x, limit 5.00x) |
| scale | trait-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | trait-items build | 5.75x | ≤ 5x | FAIL | hashes 1927503 → 11075174 (5.75x, limit 5.00x) |
| scale | impl-methods check | 4.00x | ≤ 5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 5.00x) |
| scale | impl-methods fmt | 3.97x | ≤ 5x | PASS | allocs 4045 → 16057 (3.97x, limit 5.00x) |
| scale | impl-methods build | 4.00x | ≤ 5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 5.00x) |
| scale | traits-of-one-struct check | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | traits-of-one-struct fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | traits-of-one-struct build | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | impls-of-one-trait check | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | impls-of-one-trait fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | impls-of-one-trait build | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | operator-impls check | 4.00x | ≤ 5x | PASS | hashes 699772 → 2799300 (4.00x, limit 5.00x) |
| scale | operator-impls fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | operator-impls build | 4.00x | ≤ 5x | PASS | scope_probes 25000 → 100000 (4.00x, limit 5.00x) |
| scale | derive-items check | 4.00x | ≤ 5x | PASS | trait_lookup 16998 → 67998 (4.00x, limit 5.00x) |
| scale | derive-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | derive-items build | 5.06x | ≤ 5x | FAIL | hashes 52599881 → 266059895 (5.06x, limit 5.00x) |
| scale | dyn-values check | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | dyn-values fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | dyn-values build | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | consts check | 3.97x | ≤ 5x | PASS | hashes 61711 → 245019 (3.97x, limit 5.00x) |
| scale | consts fmt | 3.89x | ≤ 5x | PASS | allocs 2087 → 8109 (3.89x, limit 5.00x) |
| scale | consts build | 4.00x | ≤ 5x | PASS | use_facts 23531 → 94031 (4.00x, limit 5.00x) |
| scale | local-items check | 4.00x | ≤ 5x | PASS | scope_probes 19000 → 76000 (4.00x, limit 5.00x) |
| scale | local-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | local-items build | 4.00x | ≤ 5x | PASS | scope_probes 19000 → 76000 (4.00x, limit 5.00x) |
| scale | syntax-items check | 4.00x | ≤ 5x | PASS | trait_lookup 9898 → 39598 (4.00x, limit 5.00x) |
| scale | syntax-items fmt | 4.00x | ≤ 5x | PASS | hashes 17160 → 68664 (4.00x, limit 5.00x) |
| scale | syntax-items build | 4.00x | ≤ 5x | PASS | trait_lookup 9898 → 39598 (4.00x, limit 5.00x) |
| scale | struct-chain check | 4.04x | ≤ 5x | PASS | hashes 152339 → 615795 (4.04x, limit 5.00x) |
| scale | struct-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | struct-chain-values check | 4.04x | ≤ 5x | PASS | hashes 112418 → 453786 (4.04x, limit 5.00x) |
| scale | struct-chain-values fmt | 4.02x | ≤ 5x | PASS | hashes 2082 → 8370 (4.02x, limit 5.00x) |
| scale | struct-chain-values build | 4.00x | ≤ 5x | PASS | scope_probes 4700 → 18800 (4.00x, limit 5.00x) |
| scale | modules check | 4.00x | ≤ 5x | PASS | hashes 1168560 → 4677348 (4.00x, limit 5.00x) |
| scale | modules fmt | 4.00x | ≤ 5x | PASS | hashes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | modules build | 4.00x | ≤ 5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 5.00x) |
| scale | use-names check | 4.04x | ≤ 5x | PASS | hashes 220189 → 890037 (4.04x, limit 5.00x) |
| scale | use-names fmt | 4.01x | ≤ 5x | PASS | hashes 3783 → 15159 (4.01x, limit 5.00x) |
| scale | use-names build | 4.01x | ≤ 5x | PASS | hashes 574947 → 2306582 (4.01x, limit 5.00x) |
| scale | generic-fns check | 4.03x | ≤ 5x | PASS | hashes 295543 → 1190619 (4.03x, limit 5.00x) |
| scale | generic-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-fns build | 4.01x | ≤ 5x | PASS | hashes 900376 → 3613667 (4.01x, limit 5.00x) |
| scale | generic-params check | 4.09x | ≤ 5x | PASS | hashes 14618 → 59746 (4.09x, limit 5.00x) |
| scale | generic-params fmt | 4.10x | ≤ 5x | PASS | alloc_bytes 344142 → 1412604 (4.10x, limit 10.00x) |
| scale | generic-params build | 4.00x | ≤ 5x | PASS | scope_probes 800 → 3200 (4.00x, limit 5.00x) |
| scale | param-bounds check | 4.01x | ≤ 5x | PASS | hashes 55295 → 221707 (4.01x, limit 5.00x) |
| scale | param-bounds fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | param-bounds build | 4.00x | ≤ 5x | PASS | scope_probes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | type-defaults check | 4.00x | ≤ 5x | PASS | scope_probes 33500 → 134000 (4.00x, limit 5.00x) |
| scale | type-defaults fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | type-defaults build | 5.50x | ≤ 5x | FAIL | hashes 2221398 → 12224451 (5.50x, limit 5.00x) |
| scale | alias-chain check | 4.01x | ≤ 5x | PASS | hashes 121467 → 486932 (4.01x, limit 5.00x) |
| scale | alias-chain fmt | 4.03x | ≤ 5x | PASS | hashes 1234 → 4978 (4.03x, limit 5.00x) |
| scale | alias-chain build | 4.00x | ≤ 5x | PASS | scope_probes 6600 → 26400 (4.00x, limit 5.00x) |
| scale | row-chain check | 4.02x | ≤ 5x | PASS | hashes 219548 → 881640 (4.02x, limit 5.00x) |
| scale | row-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | row-chain build | 4.01x | ≤ 5x | PASS | hashes 584236 → 2344418 (4.01x, limit 5.00x) |
| scale | row-traits check | 4.00x | ≤ 5x | PASS | scope_probes 2100 → 8400 (4.00x, limit 5.00x) |
| scale | row-traits fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | row-traits build | 4.00x | ≤ 5x | PASS | scope_probes 2100 → 8400 (4.00x, limit 5.00x) |
| scale | closures check | 4.00x | ≤ 5x | PASS | local_body_checks 1000 → 4000 (4.00x, limit 5.00x) |
| scale | closures fmt | 3.94x | ≤ 5x | PASS | allocs 3061 → 12073 (3.94x, limit 5.00x) |
| scale | closures build | 4.00x | ≤ 5x | PASS | local_body_checks 1000 → 4000 (4.00x, limit 5.00x) |
| scale | let-statements check | 3.96x | ≤ 5x | PASS | allocs 25334 → 100428 (3.96x, limit 5.00x) |
| scale | let-statements fmt | 3.94x | ≤ 5x | PASS | allocs 2554 → 10066 (3.94x, limit 5.00x) |
| scale | let-statements build | 4.00x | ≤ 5x | PASS | control 68499 → 273999 (4.00x, limit 5.00x) |
| scale | matches check | 4.00x | ≤ 5x | PASS | decision_rows 6000 → 24000 (4.00x, limit 5.00x) |
| scale | matches fmt | 3.97x | ≤ 5x | PASS | allocs 6057 → 24069 (3.97x, limit 5.00x) |
| scale | matches build | 4.32x | ≤ 5x | PASS | hashes 914652 → 3954188 (4.32x, limit 5.00x) |
| scale | enum-arms check | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | enum-arms fmt | 3.91x | ≤ 5x | PASS | allocs 1551 → 6065 (3.91x, limit 5.00x) |
| scale | enum-arms build | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | tuple-patterns check | 4.00x | ≤ 5x | PASS | decision_rows 1000 → 4000 (4.00x, limit 5.00x) |
| scale | tuple-patterns fmt | 3.95x | ≤ 5x | PASS | allocs 3054 → 12066 (3.95x, limit 5.00x) |
| scale | tuple-patterns build | 4.00x | ≤ 5x | PASS | control 93471 → 373971 (4.00x, limit 5.00x) |
| scale | range-patterns check | 4.02x | ≤ 5x | PASS | hashes 247615 → 996635 (4.02x, limit 5.00x) |
| scale | range-patterns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | range-patterns build | 4.01x | ≤ 5x | PASS | hashes 959684 → 3845697 (4.01x, limit 5.00x) |
| scale | wide-struct check | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | wide-struct fmt | 3.95x | ≤ 5x | PASS | allocs 2544 → 10056 (3.95x, limit 5.00x) |
| scale | wide-struct build | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | call-args check | 4.41x | ≤ 5x | PASS | hashes 4532 → 19968 (4.41x, limit 5.00x) |
| scale | call-args fmt | 4.10x | ≤ 5x | PASS | alloc_bytes 361569 → 1482933 (4.10x, limit 10.00x) |
| scale | call-args build | 4.00x | ≤ 5x | PASS | control 4999 → 19999 (4.00x, limit 5.00x) |
| scale | binary-chain check | 4.00x | ≤ 5x | PASS | hashes 2500 → 10000 (4.00x, limit 5.00x) |
| scale | binary-chain fmt | 3.78x | ≤ 5x | PASS | allocs 545 → 2060 (3.78x, limit 5.00x) |
| scale | binary-chain build | 4.02x | ≤ 5x | PASS | hashes 27284 → 109724 (4.02x, limit 5.00x) |
| scale | interpolation check | 4.00x | ≤ 5x | PASS | trait_lookup 2501 → 10001 (4.00x, limit 5.00x) |
| scale | interpolation fmt | 4.27x | ≤ 5x | PASS | alloc_bytes 6453 → 27573 (4.27x, limit 10.00x) |
| scale | interpolation build | 4.03x | ≤ 5x | PASS | hashes 3380443 → 13636469 (4.03x, limit 5.00x) |
| scale | list-literal check | 4.00x | ≤ 5x | PASS | hashes 7392 → 29580 (4.00x, limit 5.00x) |
| scale | list-literal fmt | 3.82x | ≤ 5x | PASS | allocs 533 → 2038 (3.82x, limit 5.00x) |
| scale | list-literal build | 4.01x | ≤ 5x | PASS | hashes 41678 → 167306 (4.01x, limit 5.00x) |
| scale | method-chain check | 4.00x | ≤ 5x | PASS | hashes 14394 → 57582 (4.00x, limit 5.00x) |
| scale | method-chain fmt | 4.04x | ≤ 5x | PASS | alloc_bytes 838129 → 3386711 (4.04x, limit 10.00x) |
| scale | method-chain build | 4.00x | ≤ 5x | PASS | trait_lookup 500 → 2000 (4.00x, limit 5.00x) |
| scale | pipe-chain check | 4.00x | ≤ 5x | PASS | hashes 13517 → 54017 (4.00x, limit 5.00x) |
| scale | pipe-chain fmt | 3.77x | ≤ 5x | PASS | allocs 546 → 2056 (3.77x, limit 5.00x) |
| scale | pipe-chain build | 3.97x | ≤ 5x | PASS | hashes 107673 → 427053 (3.97x, limit 5.00x) |
| scale | question-chain check | 4.00x | ≤ 5x | PASS | hashes 39420 → 157608 (4.00x, limit 5.00x) |
| scale | question-chain fmt | 3.94x | ≤ 5x | PASS | allocs 1533 → 6044 (3.94x, limit 5.00x) |
| scale | question-chain build | 3.99x | ≤ 5x | PASS | use_facts 24570 → 98070 (3.99x, limit 5.00x) |
| scale | if-nest check | 3.92x | ≤ 5x per 4x bytes (12.1x) | PASS | hashes 756 → 2964 (3.92x, limit 15.11x) |
| scale | if-nest fmt | 5.30x | ≤ 5x per 4x bytes (12.1x) | PASS | alloc_bytes 144371 → 765724 (5.30x, limit 30.21x) |
| scale | if-nest build | 4.00x | ≤ 5x per 4x bytes (12.1x) | PASS | use_facts 768 → 3072 (4.00x, limit 15.11x) |
| scale | block-nest check | 3.95x | ≤ 5x per 4x bytes (12.2x) | PASS | hashes 814 → 3214 (3.95x, limit 15.29x) |
| scale | block-nest fmt | 5.69x | ≤ 5x per 4x bytes (12.2x) | PASS | alloc_bytes 160538 → 913156 (5.69x, limit 30.57x) |
| scale | block-nest build | 3.95x | ≤ 5x per 4x bytes (12.2x) | PASS | use_facts 878 → 3470 (3.95x, limit 15.29x) |
| scale | for-nest check | 5.23x | ≤ 5x per 4x bytes (11.6x) | PASS | hashes 3473 → 18161 (5.23x, limit 14.50x) |
| scale | for-nest fmt | 5.39x | ≤ 5x per 4x bytes (11.6x) | PASS | alloc_bytes 139001 → 749031 (5.39x, limit 29.01x) |
| scale | for-nest build | 4.60x | ≤ 5x per 4x bytes (11.6x) | PASS | hashes 27114 → 124666 (4.60x, limit 14.50x) |
| scale | while-nest check | 3.88x | ≤ 5x per 4x bytes (12.1x) | PASS | hashes 1033 → 4009 (3.88x, limit 15.11x) |
| scale | while-nest fmt | 5.31x | ≤ 5x per 4x bytes (12.1x) | PASS | alloc_bytes 143075 → 760300 (5.31x, limit 30.21x) |
| scale | while-nest build | 4.00x | ≤ 5x per 4x bytes (12.1x) | PASS | use_facts 1184 → 4736 (4.00x, limit 15.11x) |
| scale | match-nest check | 4.87x | ≤ 5x per 4x bytes (12.8x) | PASS | hashes 1776 → 8652 (4.87x, limit 16.03x) |
| scale | match-nest fmt | 5.82x | ≤ 5x per 4x bytes (12.8x) | PASS | alloc_bytes 158237 → 920971 (5.82x, limit 32.07x) |
| scale | match-nest build | 4.27x | ≤ 5x per 4x bytes (12.8x) | PASS | hashes 8939 → 38204 (4.27x, limit 16.03x) |
| scale | closure-nest check | 3.95x | ≤ 5x per 4x bytes (9.2x) | PASS | hashes 1818 → 7182 (3.95x, limit 11.45x) |
| scale | closure-nest fmt | 5.54x | ≤ 5x per 4x bytes (9.2x) | PASS | alloc_bytes 74245 → 410949 (5.54x, limit 22.90x) |
| scale | closure-nest build | 4.01x | ≤ 5x per 4x bytes (9.2x) | PASS | use_facts 479 → 1919 (4.01x, limit 11.45x) |
| scale | option-nest check | 4.18x | ≤ 5x | PASS | hashes 1871 → 7829 (4.18x, limit 5.00x) |
| scale | option-nest fmt | 6.24x | ≤ 5x | PASS | alloc_bytes 73101 → 456308 (6.24x, limit 10.00x) |
| scale | option-nest build | 4.04x | ≤ 5x | PASS | hashes 11112 → 44856 (4.04x, limit 5.00x) |
| scale | list-type-nest check | 4.26x | ≤ 5x | PASS | hashes 1260 → 5364 (4.26x, limit 5.00x) |
| scale | list-type-nest fmt | 6.05x | ≤ 5x | PASS | alloc_bytes 55328 → 334852 (6.05x, limit 10.00x) |
| scale | list-type-nest build | 4.26x | ≤ 5x | PASS | hashes 4442 → 18905 (4.26x, limit 5.00x) |
| scale | generic-nest check | 4.34x | ≤ 5x | PASS | hashes 1584 → 6871 (4.34x, limit 5.00x) |
| scale | generic-nest fmt | 7.40x | ≤ 5x | PASS | alloc_bytes 10894 → 80601 (7.40x, limit 10.00x) |
| scale | generic-nest build | 4.00x | ≤ 5x | PASS | allocs 5500 → 22014 (4.00x, limit 5.00x) |
| scale | paren-nest check | 4.45x | ≤ 5x | PASS | alloc_bytes 38264 → 170258 (4.45x, limit 10.00x) |
| scale | paren-nest fmt | 3.21x | ≤ 5x | PASS | alloc_bytes 34650 → 111359 (3.21x, limit 10.00x) |
| scale | paren-nest build | 4.08x | ≤ 5x | PASS | control 795 → 3243 (4.08x, limit 5.00x) |
| scale | arith-nest check | 4.00x | ≤ 5x | PASS | hashes 440 → 1760 (4.00x, limit 5.00x) |
| scale | arith-nest fmt | 2.94x | ≤ 5x | PASS | alloc_bytes 76966 → 226347 (2.94x, limit 10.00x) |
| scale | arith-nest build | 4.08x | ≤ 5x | PASS | hashes 6007 → 24486 (4.08x, limit 5.00x) |
| scale | bool-nest check | 3.95x | ≤ 5x | PASS | hashes 391 → 1543 (3.95x, limit 5.00x) |
| scale | bool-nest fmt | 5.11x | ≤ 5x | PASS | alloc_bytes 37604 → 191986 (5.11x, limit 10.00x) |
| scale | bool-nest build | 4.26x | ≤ 5x | PASS | hashes 2853 → 12161 (4.26x, limit 5.00x) |
| scale | bit-nest check | 3.99x | ≤ 5x | PASS | hashes 818 → 3266 (3.99x, limit 5.00x) |
| scale | bit-nest fmt | 2.91x | ≤ 5x | PASS | alloc_bytes 163583 → 476792 (2.91x, limit 10.00x) |
| scale | bit-nest build | 4.09x | ≤ 5x | PASS | hashes 8366 → 34196 (4.09x, limit 5.00x) |
| scale | tuple-nest check | 4.07x | ≤ 5x | PASS | hashes 1924 → 7822 (4.07x, limit 5.00x) |
| scale | tuple-nest fmt | 5.67x | ≤ 5x | PASS | alloc_bytes 122064 → 692688 (5.67x, limit 10.00x) |
| scale | tuple-nest build | 4.08x | ≤ 5x | PASS | hashes 11156 → 45476 (4.08x, limit 5.00x) |
| scale | if-expr-nest check | 3.76x | ≤ 5x | PASS | hashes 322 → 1210 (3.76x, limit 5.00x) |
| scale | if-expr-nest fmt | 4.51x | ≤ 5x | PASS | alloc_bytes 68686 → 309984 (4.51x, limit 10.00x) |
| scale | if-expr-nest build | 3.82x | ≤ 5x | PASS | use_facts 341 → 1301 (3.82x, limit 5.00x) |
| scale | call-nest check | 3.92x | ≤ 5x | PASS | hashes 489 → 1917 (3.92x, limit 5.00x) |
| scale | call-nest fmt | 6.40x | ≤ 5x | PASS | alloc_bytes 33324 → 213300 (6.40x, limit 10.00x) |
| scale | call-nest build | 3.13x | ≤ 5x | PASS | hashes 4771 → 14935 (3.13x, limit 5.00x) |
| scale | index-nest check | 4.50x | ≤ 5x | PASS | alloc_bytes 32492 → 146124 (4.50x, limit 10.00x) |
| scale | index-nest fmt | 7.56x | ≤ 5x | PASS | alloc_bytes 23956 → 181114 (7.56x, limit 10.00x) |
| scale | index-nest build | 4.16x | ≤ 5x | PASS | hashes 2132 → 8878 (4.16x, limit 5.00x) |
| scale | fn-nest check | 4.03x | ≤ 5x per 4x bytes (10.7x) | PASS | hashes 2691 → 10847 (4.03x, limit 13.42x) |
| scale | fn-nest fmt | 5.63x | ≤ 5x per 4x bytes (10.7x) | PASS | alloc_bytes 132556 → 746520 (5.63x, limit 26.84x) |
| scale | fn-nest build | 4.00x | ≤ 5x per 4x bytes (10.7x) | PASS | use_facts 767 → 3071 (4.00x, limit 13.42x) |
| scale | module-path-nest check | 1.58x | ≤ 5x | PASS | allocs 2514 → 3977 (1.58x, limit 5.00x) |
| scale | module-path-nest fmt | 7.90x | ≤ 5x | PASS | alloc_bytes 16030 → 126570 (7.90x, limit 10.00x) |
| scale | module-path-nest build | 1.55x | ≤ 5x | PASS | allocs 2522 → 3908 (1.55x, limit 5.00x) |
| scale | coverage: node kinds | 119/119 | 119/119 | PASS | exempt: Error |
| scale | coverage: node kinds repeated | 110/119 | reported | PASS | in no repeat shape: ReturnStmt, RangeExpr, LogicalOrExpr, LogicalAndExpr, BitwiseOrExpr, BitwiseXorExpr, BitwiseAndExpr, ShiftExpr, WhileExpr; exempt: Error |
| scale | coverage: node kinds nested | 57/119 | reported | PASS | in no nest shape: UseGroup, UseItem, EnumDecl, TraitDecl, ImplDecl, TypeDecl, TestsBlock, Decorator, DefaultValue, ConstParameter, LifetimeParameter, BoundList, TypeDefault, ConstArgument, AssociatedTypeBinding, NamedArgument, EmbeddedField, EnumVariant, VariantSharedData, AssociatedTypeDecl, Derivation, DiscardStmt, BreakStmt, ContinueStmt, UsingStmt, ElseIfClause, MatchGuard, PipeExpr, RefExpr, DerefExpr, PowerExpr, TryExpr, SuspendExpr, PathExpr, TypeArgsExpr, TrailingCallExpr, PlaceholderExpr, MapExpr, MapEntry, SpreadExpr, ComprehensionExpr, ComprehensionFor, ComprehensionIf, ContextExpr, ContextEntry, Interpolation, LiteralPattern, NamedPattern, DataPattern, DataPatternField, SpreadPattern, RangePattern, DynType, ImplType, RefType, UnionType, ParenType, FunctionType, ProjectionType, ContextType, RestType, InferType; exempt: Error |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 69 of 70; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 25 of 25 safe |
| fmt | hd fmt, 10k lines | 17 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 2.52x | ≤ 1.30x | FAIL | checked 521 ms, unchecked 207 ms |
| dev-speed | geomean dev / optimized | - | ≤ 4.00x | FAIL | optimized work is within noise: string-build (31.3 ms) |
| dev-speed | worst case dev / optimized | - | ≤ 10.00x | FAIL | optimized work is within noise: string-build (31.3 ms) |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 20 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 26.2 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.12 CPU-s, 37.6 MB | completes ≤ 30 s | PASS | wall 85 ms |
| resources | check, small, warm | 0.00 CPU-s, 18.9 MB | completes ≤ 30 s | PASS | wall 16 ms |
| resources | test, small, cold | 0.97 CPU-s, 94.2 MB | completes ≤ 30 s | PASS | wall 440 ms |
| resources | test, small, warm | 0.43 CPU-s, 66.6 MB | completes ≤ 30 s | PASS | wall 199 ms |
| resources | build, small, cold | 0.54 CPU-s, 87.2 MB | completes ≤ 30 s | PASS | wall 233 ms |
| resources | build, small, warm | 0.02 CPU-s, 31.6 MB | completes ≤ 30 s | PASS | wall 28 ms |
| resources | check, 10k, cold | 0.30 CPU-s, 59.3 MB | completes ≤ 30 s | PASS | wall 152 ms |
| resources | check, 10k, warm | 0.02 CPU-s, 26.2 MB | completes ≤ 30 s | PASS | wall 32 ms |
| resources | test, 10k, cold | 1.51 CPU-s, 125.6 MB | completes ≤ 30 s | PASS | wall 715 ms |
| resources | test, 10k, warm | 0.55 CPU-s, 71.8 MB | completes ≤ 30 s | PASS | wall 247 ms |
| resources | build, 10k, cold | 0.77 CPU-s, 109.7 MB | completes ≤ 30 s | PASS | wall 295 ms |
| resources | build, 10k, warm | 0.04 CPU-s, 34.3 MB | completes ≤ 30 s | PASS | wall 41 ms |
| resources | check, 50k, cold | 1.14 CPU-s, 164.4 MB | completes ≤ 150 s | PASS | wall 467 ms |
| resources | check, 50k, warm | 0.10 CPU-s, 56.8 MB | completes ≤ 150 s | PASS | wall 98 ms |
| resources | test, 50k, cold | 3.55 CPU-s, 289.0 MB | completes ≤ 150 s | PASS | wall 1881 ms |
| resources | test, 50k, warm | 1.07 CPU-s, 96.2 MB | completes ≤ 150 s | PASS | wall 486 ms |
| resources | build, 50k, cold | 1.62 CPU-s, 211.9 MB | completes ≤ 150 s | PASS | wall 593 ms |
| resources | build, 50k, warm | 0.11 CPU-s, 65.0 MB | completes ≤ 150 s | PASS | wall 105 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 61.3 MB | ≤ 22.0 MB | FAIL | RSS 106.9 MB at input 100, 161.6 MB at input 1000; build per input 62 ms at 50, 1111 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.4 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 4.1 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd, just written: wall time | 12 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd check empty.hd, just written: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, just written: peak RSS | 17.2 MB | ≤ 10.0 MB | FAIL |  |
| startup | hd check empty.hd, settled: wall time | 4.9 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd check empty.hd, settled: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, settled: peak RSS | 8.8 MB | ≤ 10.0 MB | PASS |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 1.34x | ≤ 1.50x | PASS | p95 79 ms at N = 1; 4 cores; total CPU 150 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 2.07x | ≤ 6.00x | PASS | p95 79 ms at N = 1; 4 cores; total CPU 270 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 0.29 | &lt; 1.00 (sublinear) | PASS | 120 ms at N = 1, 270 ms at N = 16 |
| disk | artifacts per worktree | 0.0 MB | ≤ 10.0 MB | PASS | 10 files; HD_CACHE 11.0 MB, shared per user, not counted |
| disk | toolchain, once per machine | 17.9 MB | recorded, no target | PASS | toolchain files: /home/runner/work/hd-lang/hd-lang/target/dist/hd |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.99x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 214 files | the files of one check's cache | FAIL | one check alone: 36 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 14.9 MB | ≤ 5.0 MB | FAIL | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 1 | ≤ 1 | PASS | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 59k/s | ≥ 100k/s | FAIL | 6000 extra cases took 101 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 85 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 Timing change ≤10%: noise. |
| unit-test-perf | 1,000 unit tests, warm | 291 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed Timing change ≤10%: noise. |
| unit-test-perf | overhead per unit test | 0.2 ms | ≤ 1.0 ms | PASS | 100 tests: 130 ms |
| integration-test-perf | setup per integration test program | 73 ms | ≤ 20 ms | FAIL | 10 programs: 834 ms for 21 tests, 40 ms per test |
| integration-test-perf | total time growth in programs | 0.989 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 154 ms at 2, 756 ms at 10 programs |
| integration-test-perf | cost per doc test | 67 ms | ≤ 20 ms | FAIL | 10 doc tests: 756 ms; the run reports 11 of 11 passed Timing change ≤10%: noise. |
| runtime | geomean hd / Node | 0.21x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.31x | ≤ 3.00x | PASS | hd 1.6 ms (p10 1.6, p90 1.6), Node 5.2 ms, Python 49 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 0.06x | ≤ 3.00x | PASS | hd 2.2 ms (p10 2.2, p90 2.4), Node 37 ms, Python 176 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.23x | ≤ 3.00x | PASS | hd 0.8 ms (p10 0.8, p90 0.8), Node 3.4 ms, Python 7.9 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 0.42x | ≤ 3.00x | PASS | hd 2.0 ms (p10 1.9, p90 2.0), Node 4.7 ms, Python 11 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.25x | ≤ 3.00x | PASS | hd 3.4 ms (p10 3.4, p90 3.6), Node 14 ms, Python 9.9 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.01 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 5014 B | ≤ 2048 B | FAIL | benchmarks: fib 5141 B, sum 4917 B, string-build 7785 B, map 7034 B, sort 12632 B |
| size-startup-heap | tiny program, start to first output | 36 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm Timing change ≤10%: noise. |
| size-startup-heap | peak heap, map: hd / Node | 1.06x | ≤ 2.00x | PASS | hd +15.0 MB, Node +14.1 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.42x | ≤ 2.00x | PASS | hd +8.5 MB, Node +20.1 MB over an empty program |
| host-call-overhead | Console println, per call | 3.1 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 10 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 22 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 0.00 ns | ≤ 100 ns | PASS | -2.3 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 13.9M/s | ≥ 1.0M/s | PASS | 144 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 14.8M/s | ≥ 1.0M/s | PASS | 135 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.13x | ≥ 0.50x | FAIL | hd 109 MB/s (45488 B document), Node 845 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.15x | ≥ 0.50x | FAIL | hd 49 MB/s (45488 B document), Node 327 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 2.96x | ≥ 0.50x | PASS | hd 530 MB/s, Node 179 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 2.97x | ≥ 0.50x | PASS | hd 1083 MB/s, Node 365 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.08x | ≥ 0.50x | FAIL | hd 13 MB/s, Node 169 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 6316 B | ≤ 10240 B | PASS | 12386 B at 1075 lines, 69240 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 915 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 2.7 MB | ≤ 10.0 MB | PASS | 32866000 requests; RSS 67.9 MB after warm-up, 70.6 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 7770 of 7770 unlisted cases passed |
| conformance | listed known failures | 1 | ≤ 0 | FAIL | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
- [2026-11-07T03](daily/2026-11-07T03.json)
- [2026-11-06T19](daily/2026-11-06T19.json)
- [2026-11-06T04](daily/2026-11-06T04.json)
- [2026-11-05T20](daily/2026-11-05T20.json)
- [2026-11-05T04](daily/2026-11-05T04.json)
- [2026-11-04T19](daily/2026-11-04T19.json)
- [2026-11-04T04](daily/2026-11-04T04.json)
