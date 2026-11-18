# Daily goal metrics

Finished: 2026-11-18T05:29:10.419Z. Commit: b2e43db001c13ccaab6f31fc8bfdf1e43e70ecd9.
Machine: ubuntu-latest, 4 CPUs (INTEL(R) XEON(R) PLATINUM 8573C).
Compiler: target/dist/hd.

285 passed, 22 failed, 4 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 30 ms → pass 39 ms
- edit-latency: p95 check after an edit: pass 32 ms → pass 41 ms
- cold-check: median cold check, 10k lines: pass 148 ms → pass 225 ms
- mistakes: fix-it makes hd check pass: fail 35.7% → fail 52.9%
- pathological: overlapping-from: time at 4x: pass 36 ms → pass 47 ms
- pathological: deep-blocks: time at 4x: pass 20 ms → pass 23 ms
- pathological: deep-parens: time at 4x: pass 18 ms → pass 22 ms
- pathological: deep-parens: peak RSS at 4x: pass 21.4 MB → pass 24.2 MB
- pathological: method-chain: time at 4x: pass 22 ms → pass 27 ms
- pathological: iterator-chain: time at 4x: pass 24 ms → pass 29 ms
- pathological: wide-list-literal: time at 4x: pass 38 ms → pass 49 ms
- pathological: large-enum-match: time at 4x: pass 26 ms → pass 33 ms
- pathological: deep-generic: time at 4x: pass 20 ms → pass 24 ms
- pathological: question-chain: time at 4x: pass 23 ms → pass 29 ms
- pathological: big-file: time at 4x: pass 109 ms → pass 150 ms
- scale: trait-items build: fail 5.75x → pass 4.02x
- scale: derive-items build: fail 5.06x → pass 4.00x
- scale: type-defaults build: fail 5.50x → pass 4.01x
- fmt: hd fmt, 10k lines: pass 17 ms → pass 26 ms
- check-cost: checked run time / unchecked run time: fail 2.52x → fail 1.91x
- dev-speed: geomean dev / optimized: fail - → pass 1.69x
- dev-speed: worst case dev / optimized: fail - → pass 3.28x
- resources: warm check, 10k lines: CPU time: pass 20 ms → pass 40 ms
- long-session: RSS growth, REPL inputs 100 to 1000: fail 61.3 MB → fail 73.8 MB
- startup: hd help (no version command): peak RSS: pass 4.1 MB → pass 5.2 MB
- startup: hd check empty.hd, just written: wall time: pass 12 ms → pass 16 ms
- startup: hd check empty.hd, just written: peak RSS: fail 17.2 MB → fail 19.0 MB
- startup: hd check empty.hd, settled: peak RSS: pass 8.8 MB → pass 9.8 MB
- concurrency: p95 latency at N = 4 vs N = 1: pass 1.34x → fail 1.87x
- disk: toolchain, once per machine: pass 17.9 MB → pass 31.7 MB
- proptest-perf: property cases per second: fail 59k/s → fail 93k/s
- proptest-perf: time to shrink a known failure: pass 85 ms → pass 15 ms
- unit-test-perf: 1,000 unit tests, warm: pass 291 ms → pass 120 ms
- unit-test-perf: overhead per unit test: pass 0.2 ms → pass 0.1 ms
- integration-test-perf: setup per integration test program: fail 73 ms → fail 26 ms
- integration-test-perf: cost per doc test: fail 67 ms → fail 21 ms
- runtime: geomean hd / Node: pass 0.21x → pass 0.54x
- runtime: fib: hd / Node: pass 0.31x → pass 0.83x
- runtime: sum: hd / Node: pass 0.06x → pass 0.16x
- runtime: string-build: hd / Node: pass 0.23x → pass 0.88x
- runtime: map: hd / Node: pass 0.42x → pass 0.73x
- runtime: sort: hd / Node: pass 0.25x → pass 0.56x
- allocations: counted loop, bytes per iteration: pass 0.00 B/iter → n/a -
- allocations: list loop, bytes per iteration: pass 0.00 B/iter → n/a -
- allocations: map loop, bytes per iteration: pass 0.00 B/iter → n/a -
- allocations: iterator chain, bytes per iteration: pass 0.01 B/iter → n/a -
- size-startup-heap: tiny program, start to first output: fail 36 ms → fail 6.8 ms
- size-startup-heap: peak heap, map: hd / Node: pass 1.06x → pass 0.84x
- host-call-overhead: Console println, per call: fail 3.1 µs → fail 1.8 µs
- host-call-overhead: Fs read_text!, per call: fail 10 µs → pass 5.9 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 22 µs → fail 10 µs
- suspension-overhead: await of a ready value, per await: pass 0.00 ns → pass 15 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 13.9M/s → pass 10.1M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 14.8M/s → pass 6.2M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.13x → fail 0.10x
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.15x → fail 0.13x
- text-throughput: string build, hd / Node MB/s: pass 2.96x → pass 1.47x
- text-throughput: split, hd / Node MB/s: pass 2.97x → pass 0.90x
- text-throughput: regex find_all, hd / Node MB/s: fail 0.08x → fail 0.06x
- long-run-memory: RSS growth after warm-up, 1 min: pass 2.7 MB → pass 0.0 MB

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 39 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 41 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 225 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 146 ms | ≤ 300 ms | PASS | Timing change ≤10%: noise. |
| mistakes | programs with exactly one diagnostic | 100.0% | ≥ 95.0% | PASS | 70 of 70 |
| mistakes | one diagnostic with the expected code | 100.0% | ≥ 95.0% | PASS | 70 of 70 |
| mistakes | fix-it makes hd check pass | 52.9% | ≥ 80.0% | FAIL | 37 of 70 resolved; 37 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 85 tokens | ≤ 60 tokens | FAIL | largest: interpolate-optional; median 45 tokens, 178 B; 56 of 70 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 47 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 28.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 35 → 47 ms, start-up 23 ms |
| pathological | deep-blocks: time at 4x | 23 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 23.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 23 ms, start-up 23 ms |
| pathological | deep-parens: time at 4x | 22 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 24.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 23 → 22 ms, start-up 23 ms |
| pathological | method-chain: time at 4x | 27 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 25.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 25 → 27 ms, start-up 23 ms |
| pathological | iterator-chain: time at 4x | 29 ms | ≤ 2000 ms | PASS |  |
| pathological | iterator-chain: peak RSS at 4x | 24.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | iterator-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 26 → 29 ms, start-up 23 ms |
| pathological | wide-list-literal: time at 4x | 49 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 28.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 34 → 49 ms, start-up 23 ms |
| pathological | large-enum-match: time at 4x | 33 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 26.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 27 → 33 ms, start-up 23 ms |
| pathological | deep-generic: time at 4x | 24 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 23.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 24 ms, start-up 23 ms |
| pathological | question-chain: time at 4x | 29 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 26.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 26 → 29 ms, start-up 23 ms |
| pathological | big-file: time at 4x | 150 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 57.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.12 | ≤ 1.30 | PASS | 2x→4x; 82 → 150 ms, start-up 23 ms |
| scale | fn-items check | 4.04x | ≤ 5x | PASS | hashes 192646 → 778614 (4.04x, limit 5.00x) |
| scale | fn-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | fn-items build | 4.01x | ≤ 5x | PASS | hashes 473284 → 1899559 (4.01x, limit 5.00x) |
| scale | struct-items check | 4.03x | ≤ 5x | PASS | hashes 210516 → 848076 (4.03x, limit 5.00x) |
| scale | struct-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | struct-items build | 4.00x | ≤ 5x | PASS | scope_probes 9000 → 36000 (4.00x, limit 5.00x) |
| scale | enum-items check | 4.00x | ≤ 5x | PASS | hashes 526744 → 2107272 (4.00x, limit 5.00x) |
| scale | enum-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | enum-items build | 4.00x | ≤ 5x | PASS | scope_probes 23500 → 94000 (4.00x, limit 5.00x) |
| scale | trait-items check | 4.00x | ≤ 5x | PASS | scope_probes 19500 → 78000 (4.00x, limit 5.00x) |
| scale | trait-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | trait-items build | 4.02x | ≤ 5x | PASS | control 245363 → 986363 (4.02x, limit 5.00x) |
| scale | impl-methods check | 4.00x | ≤ 5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 5.00x) |
| scale | impl-methods fmt | 3.97x | ≤ 5x | PASS | allocs 4045 → 16057 (3.97x, limit 5.00x) |
| scale | impl-methods build | 4.00x | ≤ 5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 5.00x) |
| scale | traits-of-one-struct check | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | traits-of-one-struct fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | traits-of-one-struct build | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | impls-of-one-trait check | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | impls-of-one-trait fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | impls-of-one-trait build | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | operator-impls check | 4.00x | ≤ 5x | PASS | hashes 712353 → 2849633 (4.00x, limit 5.00x) |
| scale | operator-impls fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | operator-impls build | 4.00x | ≤ 5x | PASS | scope_probes 25000 → 100000 (4.00x, limit 5.00x) |
| scale | derive-items check | 4.00x | ≤ 5x | PASS | trait_lookup 16998 → 67998 (4.00x, limit 5.00x) |
| scale | derive-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | derive-items build | 4.00x | ≤ 5x | PASS | trait_lookup 16998 → 67998 (4.00x, limit 5.00x) |
| scale | dyn-values check | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | dyn-values fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | dyn-values build | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | consts check | 3.97x | ≤ 5x | PASS | hashes 61711 → 245019 (3.97x, limit 5.00x) |
| scale | consts fmt | 3.89x | ≤ 5x | PASS | allocs 2087 → 8109 (3.89x, limit 5.00x) |
| scale | consts build | 4.00x | ≤ 5x | PASS | use_facts 21027 → 84027 (4.00x, limit 5.00x) |
| scale | local-items check | 4.00x | ≤ 5x | PASS | scope_probes 19000 → 76000 (4.00x, limit 5.00x) |
| scale | local-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | local-items build | 4.00x | ≤ 5x | PASS | scope_probes 19000 → 76000 (4.00x, limit 5.00x) |
| scale | syntax-items check | 4.00x | ≤ 5x | PASS | trait_lookup 9898 → 39598 (4.00x, limit 5.00x) |
| scale | syntax-items fmt | 4.00x | ≤ 5x | PASS | hashes 17160 → 68664 (4.00x, limit 5.00x) |
| scale | syntax-items build | 4.00x | ≤ 5x | PASS | trait_lookup 9898 → 39598 (4.00x, limit 5.00x) |
| scale | struct-chain check | 4.04x | ≤ 5x | PASS | hashes 155339 → 627795 (4.04x, limit 5.00x) |
| scale | struct-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | struct-chain-values check | 4.04x | ≤ 5x | PASS | hashes 113418 → 457786 (4.04x, limit 5.00x) |
| scale | struct-chain-values fmt | 4.02x | ≤ 5x | PASS | hashes 2082 → 8370 (4.02x, limit 5.00x) |
| scale | struct-chain-values build | 4.00x | ≤ 5x | PASS | scope_probes 4700 → 18800 (4.00x, limit 5.00x) |
| scale | modules check | 4.00x | ≤ 5x | PASS | hashes 1172349 → 4692513 (4.00x, limit 5.00x) |
| scale | modules fmt | 4.00x | ≤ 5x | PASS | hashes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | modules build | 4.00x | ≤ 5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 5.00x) |
| scale | use-names check | 4.04x | ≤ 5x | PASS | hashes 223978 → 905202 (4.04x, limit 5.00x) |
| scale | use-names fmt | 4.01x | ≤ 5x | PASS | hashes 3783 → 15159 (4.01x, limit 5.00x) |
| scale | use-names build | 4.02x | ≤ 5x | PASS | hashes 504586 → 2026221 (4.02x, limit 5.00x) |
| scale | generic-fns check | 4.03x | ≤ 5x | PASS | hashes 296543 → 1194619 (4.03x, limit 5.00x) |
| scale | generic-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-fns build | 4.01x | ≤ 5x | PASS | hashes 809116 → 3248531 (4.01x, limit 5.00x) |
| scale | generic-params check | 4.09x | ≤ 5x | PASS | hashes 14618 → 59746 (4.09x, limit 5.00x) |
| scale | generic-params fmt | 4.10x | ≤ 5x | PASS | alloc_bytes 344142 → 1412604 (4.10x, limit 10.00x) |
| scale | generic-params build | 4.00x | ≤ 5x | PASS | scope_probes 800 → 3200 (4.00x, limit 5.00x) |
| scale | param-bounds check | 4.01x | ≤ 5x | PASS | hashes 55695 → 223307 (4.01x, limit 5.00x) |
| scale | param-bounds fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | param-bounds build | 4.00x | ≤ 5x | PASS | scope_probes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | type-defaults check | 4.00x | ≤ 5x | PASS | scope_probes 33500 → 134000 (4.00x, limit 5.00x) |
| scale | type-defaults fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | type-defaults build | 4.01x | ≤ 5x | PASS | control 226942 → 910942 (4.01x, limit 5.00x) |
| scale | alias-chain check | 4.01x | ≤ 5x | PASS | hashes 121867 → 488532 (4.01x, limit 5.00x) |
| scale | alias-chain fmt | 4.03x | ≤ 5x | PASS | hashes 1234 → 4978 (4.03x, limit 5.00x) |
| scale | alias-chain build | 4.00x | ≤ 5x | PASS | scope_probes 6600 → 26400 (4.00x, limit 5.00x) |
| scale | row-chain check | 4.02x | ≤ 5x | PASS | hashes 220548 → 885640 (4.02x, limit 5.00x) |
| scale | row-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | row-chain build | 4.01x | ≤ 5x | PASS | hashes 534239 → 2144421 (4.01x, limit 5.00x) |
| scale | row-traits check | 4.00x | ≤ 5x | PASS | scope_probes 2100 → 8400 (4.00x, limit 5.00x) |
| scale | row-traits fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | row-traits build | 4.00x | ≤ 5x | PASS | scope_probes 2100 → 8400 (4.00x, limit 5.00x) |
| scale | closures check | 4.00x | ≤ 5x | PASS | local_body_checks 1000 → 4000 (4.00x, limit 5.00x) |
| scale | closures fmt | 3.94x | ≤ 5x | PASS | allocs 3061 → 12073 (3.94x, limit 5.00x) |
| scale | closures build | 4.00x | ≤ 5x | PASS | local_body_checks 1000 → 4000 (4.00x, limit 5.00x) |
| scale | generic-local-fns check | 4.03x | ≤ 5x | PASS | hashes 243208 → 979140 (4.03x, limit 5.00x) |
| scale | generic-local-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-local-fns build | 4.01x | ≤ 5x | PASS | hashes 809345 → 3246450 (4.01x, limit 5.00x) |
| scale | let-statements check | 3.96x | ≤ 5x | PASS | allocs 25334 → 100428 (3.96x, limit 5.00x) |
| scale | let-statements fmt | 3.94x | ≤ 5x | PASS | allocs 2554 → 10066 (3.94x, limit 5.00x) |
| scale | let-statements build | 4.00x | ≤ 5x | PASS | control 68499 → 273999 (4.00x, limit 5.00x) |
| scale | matches check | 4.00x | ≤ 5x | PASS | decision_rows 6000 → 24000 (4.00x, limit 5.00x) |
| scale | matches fmt | 3.97x | ≤ 5x | PASS | allocs 6057 → 24069 (3.97x, limit 5.00x) |
| scale | matches build | 4.43x | ≤ 5x | PASS | hashes 689411 → 3054439 (4.43x, limit 5.00x) |
| scale | enum-arms check | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | enum-arms fmt | 3.91x | ≤ 5x | PASS | allocs 1551 → 6065 (3.91x, limit 5.00x) |
| scale | enum-arms build | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | tuple-patterns check | 4.00x | ≤ 5x | PASS | decision_rows 1000 → 4000 (4.00x, limit 5.00x) |
| scale | tuple-patterns fmt | 3.95x | ≤ 5x | PASS | allocs 3054 → 12066 (3.95x, limit 5.00x) |
| scale | tuple-patterns build | 4.00x | ≤ 5x | PASS | control 93471 → 373971 (4.00x, limit 5.00x) |
| scale | range-patterns check | 4.02x | ≤ 5x | PASS | hashes 248615 → 1000635 (4.02x, limit 5.00x) |
| scale | range-patterns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | range-patterns build | 4.01x | ≤ 5x | PASS | hashes 772730 → 3098994 (4.01x, limit 5.00x) |
| scale | wide-struct check | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | wide-struct fmt | 3.95x | ≤ 5x | PASS | allocs 2544 → 10056 (3.95x, limit 5.00x) |
| scale | wide-struct build | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | call-args check | 4.41x | ≤ 5x | PASS | hashes 4532 → 19968 (4.41x, limit 5.00x) |
| scale | call-args fmt | 4.10x | ≤ 5x | PASS | alloc_bytes 361569 → 1482933 (4.10x, limit 10.00x) |
| scale | call-args build | 4.00x | ≤ 5x | PASS | control 4999 → 19999 (4.00x, limit 5.00x) |
| scale | binary-chain check | 4.00x | ≤ 5x | PASS | hashes 2500 → 10000 (4.00x, limit 5.00x) |
| scale | binary-chain fmt | 3.78x | ≤ 5x | PASS | allocs 545 → 2060 (3.78x, limit 5.00x) |
| scale | binary-chain build | 4.02x | ≤ 5x | PASS | hashes 17944 → 72196 (4.02x, limit 5.00x) |
| scale | interpolation check | 4.00x | ≤ 5x | PASS | trait_lookup 2501 → 10001 (4.00x, limit 5.00x) |
| scale | interpolation fmt | 4.27x | ≤ 5x | PASS | alloc_bytes 6453 → 27573 (4.27x, limit 10.00x) |
| scale | interpolation build | 4.04x | ≤ 5x | PASS | hashes 2638930 → 10671440 (4.04x, limit 5.00x) |
| scale | list-literal check | 4.00x | ≤ 5x | PASS | hashes 7392 → 29580 (4.00x, limit 5.00x) |
| scale | list-literal fmt | 3.82x | ≤ 5x | PASS | allocs 533 → 2038 (3.82x, limit 5.00x) |
| scale | list-literal build | 4.01x | ≤ 5x | PASS | hashes 28340 → 113780 (4.01x, limit 5.00x) |
| scale | method-chain check | 4.00x | ≤ 5x | PASS | hashes 14394 → 57582 (4.00x, limit 5.00x) |
| scale | method-chain fmt | 4.04x | ≤ 5x | PASS | alloc_bytes 838129 → 3386711 (4.04x, limit 10.00x) |
| scale | method-chain build | 4.00x | ≤ 5x | PASS | trait_lookup 500 → 2000 (4.00x, limit 5.00x) |
| scale | pipe-chain check | 4.00x | ≤ 5x | PASS | hashes 13517 → 54017 (4.00x, limit 5.00x) |
| scale | pipe-chain fmt | 3.77x | ≤ 5x | PASS | allocs 546 → 2056 (3.77x, limit 5.00x) |
| scale | pipe-chain build | 3.97x | ≤ 5x | PASS | hashes 85301 → 338993 (3.97x, limit 5.00x) |
| scale | question-chain check | 4.00x | ≤ 5x | PASS | hashes 39420 → 157608 (4.00x, limit 5.00x) |
| scale | question-chain fmt | 3.94x | ≤ 5x | PASS | allocs 1533 → 6044 (3.94x, limit 5.00x) |
| scale | question-chain build | 3.99x | ≤ 5x | PASS | use_facts 21060 → 84060 (3.99x, limit 5.00x) |
| scale | if-nest check | 3.92x | ≤ 5x per 4x bytes (12.1x) | PASS | hashes 756 → 2964 (3.92x, limit 15.11x) |
| scale | if-nest fmt | 5.30x | ≤ 5x per 4x bytes (12.1x) | PASS | alloc_bytes 144371 → 765724 (5.30x, limit 30.21x) |
| scale | if-nest build | 4.00x | ≤ 5x per 4x bytes (12.1x) | PASS | use_facts 672 → 2688 (4.00x, limit 15.11x) |
| scale | block-nest check | 3.95x | ≤ 5x per 4x bytes (12.2x) | PASS | hashes 814 → 3214 (3.95x, limit 15.29x) |
| scale | block-nest fmt | 5.69x | ≤ 5x per 4x bytes (12.2x) | PASS | alloc_bytes 160538 → 913156 (5.69x, limit 30.57x) |
| scale | block-nest build | 3.95x | ≤ 5x per 4x bytes (12.2x) | PASS | use_facts 780 → 3084 (3.95x, limit 15.29x) |
| scale | for-nest check | 5.23x | ≤ 5x per 4x bytes (11.6x) | PASS | hashes 3473 → 18161 (5.23x, limit 14.50x) |
| scale | for-nest fmt | 5.39x | ≤ 5x per 4x bytes (11.6x) | PASS | alloc_bytes 139001 → 749031 (5.39x, limit 29.01x) |
| scale | for-nest build | 4.74x | ≤ 5x per 4x bytes (11.6x) | PASS | hashes 21701 → 102789 (4.74x, limit 14.50x) |
| scale | while-nest check | 3.88x | ≤ 5x per 4x bytes (12.1x) | PASS | hashes 1033 → 4009 (3.88x, limit 15.11x) |
| scale | while-nest fmt | 5.31x | ≤ 5x per 4x bytes (12.1x) | PASS | alloc_bytes 143075 → 760300 (5.31x, limit 30.21x) |
| scale | while-nest build | 4.00x | ≤ 5x per 4x bytes (12.1x) | PASS | use_facts 1056 → 4224 (4.00x, limit 15.11x) |
| scale | match-nest check | 4.87x | ≤ 5x per 4x bytes (12.8x) | PASS | hashes 1776 → 8652 (4.87x, limit 16.03x) |
| scale | match-nest fmt | 5.82x | ≤ 5x per 4x bytes (12.8x) | PASS | alloc_bytes 158237 → 920971 (5.82x, limit 32.07x) |
| scale | match-nest build | 4.34x | ≤ 5x per 4x bytes (12.8x) | PASS | hashes 7017 → 30474 (4.34x, limit 16.03x) |
| scale | closure-nest check | 3.95x | ≤ 5x per 4x bytes (9.2x) | PASS | hashes 1818 → 7182 (3.95x, limit 11.45x) |
| scale | closure-nest fmt | 5.54x | ≤ 5x per 4x bytes (9.2x) | PASS | alloc_bytes 74245 → 410949 (5.54x, limit 22.90x) |
| scale | closure-nest build | 4.00x | ≤ 5x per 4x bytes (9.2x) | PASS | use_facts 430 → 1720 (4.00x, limit 11.45x) |
| scale | option-nest check | 4.18x | ≤ 5x | PASS | hashes 1871 → 7829 (4.18x, limit 5.00x) |
| scale | option-nest fmt | 6.24x | ≤ 5x | PASS | alloc_bytes 73101 → 456308 (6.24x, limit 10.00x) |
| scale | option-nest build | 4.05x | ≤ 5x | PASS | hashes 10770 → 43574 (4.05x, limit 5.00x) |
| scale | list-type-nest check | 4.26x | ≤ 5x | PASS | hashes 1260 → 5364 (4.26x, limit 5.00x) |
| scale | list-type-nest fmt | 6.05x | ≤ 5x | PASS | alloc_bytes 55328 → 334852 (6.05x, limit 10.00x) |
| scale | list-type-nest build | 4.26x | ≤ 5x | PASS | hashes 4071 → 17335 (4.26x, limit 5.00x) |
| scale | generic-nest check | 4.34x | ≤ 5x | PASS | hashes 1584 → 6871 (4.34x, limit 5.00x) |
| scale | generic-nest fmt | 7.40x | ≤ 5x | PASS | alloc_bytes 10894 → 80601 (7.40x, limit 10.00x) |
| scale | generic-nest build | 4.03x | ≤ 5x | PASS | allocs 6271 → 25299 (4.03x, limit 5.00x) |
| scale | paren-nest check | 4.45x | ≤ 5x | PASS | alloc_bytes 38264 → 170258 (4.45x, limit 10.00x) |
| scale | paren-nest fmt | 3.21x | ≤ 5x | PASS | alloc_bytes 34650 → 111359 (3.21x, limit 10.00x) |
| scale | paren-nest build | 4.16x | ≤ 5x | PASS | hashes 520 → 2164 (4.16x, limit 5.00x) |
| scale | arith-nest check | 4.00x | ≤ 5x | PASS | hashes 440 → 1760 (4.00x, limit 5.00x) |
| scale | arith-nest fmt | 2.94x | ≤ 5x | PASS | alloc_bytes 76966 → 226347 (2.94x, limit 10.00x) |
| scale | arith-nest build | 4.16x | ≤ 5x | PASS | hashes 4527 → 18846 (4.16x, limit 5.00x) |
| scale | bool-nest check | 3.95x | ≤ 5x | PASS | hashes 391 → 1543 (3.95x, limit 5.00x) |
| scale | bool-nest fmt | 5.11x | ≤ 5x | PASS | alloc_bytes 37604 → 191986 (5.11x, limit 10.00x) |
| scale | bool-nest build | 4.24x | ≤ 5x | PASS | hashes 2033 → 8629 (4.24x, limit 5.00x) |
| scale | bit-nest check | 3.99x | ≤ 5x | PASS | hashes 818 → 3266 (3.99x, limit 5.00x) |
| scale | bit-nest fmt | 2.91x | ≤ 5x | PASS | alloc_bytes 163583 → 476792 (2.91x, limit 10.00x) |
| scale | bit-nest build | 4.10x | ≤ 5x | PASS | hashes 5782 → 23692 (4.10x, limit 5.00x) |
| scale | tuple-nest check | 4.07x | ≤ 5x | PASS | hashes 1924 → 7822 (4.07x, limit 5.00x) |
| scale | tuple-nest fmt | 5.67x | ≤ 5x | PASS | alloc_bytes 122064 → 692688 (5.67x, limit 10.00x) |
| scale | tuple-nest build | 4.17x | ≤ 5x | PASS | hashes 9168 → 38260 (4.17x, limit 5.00x) |
| scale | if-expr-nest check | 3.76x | ≤ 5x | PASS | hashes 322 → 1210 (3.76x, limit 5.00x) |
| scale | if-expr-nest fmt | 4.51x | ≤ 5x | PASS | alloc_bytes 68686 → 309984 (4.51x, limit 10.00x) |
| scale | if-expr-nest build | 3.82x | ≤ 5x | PASS | use_facts 306 → 1170 (3.82x, limit 5.00x) |
| scale | call-nest check | 3.92x | ≤ 5x | PASS | hashes 489 → 1917 (3.92x, limit 5.00x) |
| scale | call-nest fmt | 6.40x | ≤ 5x | PASS | alloc_bytes 33324 → 213300 (6.40x, limit 10.00x) |
| scale | call-nest build | 3.31x | ≤ 5x | PASS | hashes 3529 → 11677 (3.31x, limit 5.00x) |
| scale | index-nest check | 4.50x | ≤ 5x | PASS | alloc_bytes 32492 → 146124 (4.50x, limit 10.00x) |
| scale | index-nest fmt | 7.56x | ≤ 5x | PASS | alloc_bytes 23956 → 181114 (7.56x, limit 10.00x) |
| scale | index-nest build | 4.16x | ≤ 5x | PASS | hashes 1582 → 6586 (4.16x, limit 5.00x) |
| scale | fn-nest check | 4.03x | ≤ 5x per 4x bytes (10.7x) | PASS | hashes 2691 → 10847 (4.03x, limit 13.42x) |
| scale | fn-nest fmt | 5.63x | ≤ 5x per 4x bytes (10.7x) | PASS | alloc_bytes 132556 → 746520 (5.63x, limit 26.84x) |
| scale | fn-nest build | 4.00x | ≤ 5x per 4x bytes (10.7x) | PASS | use_facts 688 → 2752 (4.00x, limit 13.42x) |
| scale | module-path-nest check | 1.58x | ≤ 5x | PASS | allocs 2515 → 3978 (1.58x, limit 5.00x) |
| scale | module-path-nest fmt | 7.90x | ≤ 5x | PASS | alloc_bytes 16030 → 126570 (7.90x, limit 10.00x) |
| scale | module-path-nest build | 1.55x | ≤ 5x | PASS | allocs 2524 → 3913 (1.55x, limit 5.00x) |
| scale | coverage: node kinds | 119/119 | 119/119 | PASS | exempt: Error |
| scale | coverage: node kinds repeated | 110/119 | reported | PASS | in no repeat shape: ReturnStmt, RangeExpr, LogicalOrExpr, LogicalAndExpr, BitwiseOrExpr, BitwiseXorExpr, BitwiseAndExpr, ShiftExpr, WhileExpr; exempt: Error |
| scale | coverage: node kinds nested | 57/119 | reported | PASS | in no nest shape: UseGroup, UseItem, EnumDecl, TraitDecl, ImplDecl, TypeDecl, TestsBlock, Decorator, DefaultValue, ConstParameter, LifetimeParameter, BoundList, TypeDefault, ConstArgument, AssociatedTypeBinding, NamedArgument, EmbeddedField, EnumVariant, VariantSharedData, AssociatedTypeDecl, Derivation, DiscardStmt, BreakStmt, ContinueStmt, UsingStmt, ElseIfClause, MatchGuard, PipeExpr, RefExpr, DerefExpr, PowerExpr, TryExpr, SuspendExpr, PathExpr, TypeArgsExpr, TrailingCallExpr, PlaceholderExpr, MapExpr, MapEntry, SpreadExpr, ComprehensionExpr, ComprehensionFor, ComprehensionIf, ContextExpr, ContextEntry, Interpolation, LiteralPattern, NamedPattern, DataPattern, DataPatternField, SpreadPattern, RangePattern, DynType, ImplType, RefType, UnionType, ParenType, FunctionType, ProjectionType, ContextType, RestType, InferType; exempt: Error |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | PASS |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | FAIL | m004 and its dependents |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | PASS | 69 of 70; misses: operator-line-break@9 |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 37 of 37 safe |
| fmt | hd fmt, 10k lines | 26 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.91x | ≤ 1.30x | FAIL | checked 394 ms, unchecked 206 ms |
| dev-speed | geomean dev / optimized | 1.69x | ≤ 4.00x | PASS | fib 0.98x, sum 2.14x, string-build 1.68x, map 1.18x, sort 3.28x |
| dev-speed | worst case dev / optimized | 3.28x | ≤ 10.00x | PASS | sort |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 40 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 27.4 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.19 CPU-s, 39.0 MB | completes ≤ 30 s | PASS | wall 119 ms |
| resources | check, small, warm | 0.01 CPU-s, 20.5 MB | completes ≤ 30 s | PASS | wall 20 ms |
| resources | test, small, cold | 1.08 CPU-s, 118.1 MB | completes ≤ 30 s | PASS | wall 525 ms |
| resources | test, small, warm | 0.07 CPU-s, 52.5 MB | completes ≤ 30 s | PASS | wall 68 ms |
| resources | build, small, cold | 0.92 CPU-s, 89.0 MB | completes ≤ 30 s | PASS | wall 351 ms |
| resources | build, small, warm | 0.03 CPU-s, 33.0 MB | completes ≤ 30 s | PASS | wall 35 ms |
| resources | check, 10k, cold | 0.52 CPU-s, 60.8 MB | completes ≤ 30 s | PASS | wall 229 ms |
| resources | check, 10k, warm | 0.04 CPU-s, 27.4 MB | completes ≤ 30 s | PASS | wall 42 ms |
| resources | test, 10k, cold | 1.80 CPU-s, 162.6 MB | completes ≤ 30 s | PASS | wall 944 ms |
| resources | test, 10k, warm | 0.16 CPU-s, 72.2 MB | completes ≤ 30 s | PASS | wall 153 ms |
| resources | build, 10k, cold | 1.26 CPU-s, 110.5 MB | completes ≤ 30 s | PASS | wall 436 ms |
| resources | build, 10k, warm | 0.05 CPU-s, 37.2 MB | completes ≤ 30 s | PASS | wall 57 ms |
| resources | check, 50k, cold | 1.96 CPU-s, 166.6 MB | completes ≤ 150 s | PASS | wall 719 ms |
| resources | check, 50k, warm | 0.15 CPU-s, 58.6 MB | completes ≤ 150 s | PASS | wall 139 ms |
| resources | test, 50k, cold | 4.79 CPU-s, 382.0 MB | completes ≤ 150 s | PASS | wall 2809 ms |
| resources | test, 50k, warm | 0.49 CPU-s, 159.0 MB | completes ≤ 150 s | PASS | wall 421 ms |
| resources | build, 50k, cold | 2.73 CPU-s, 215.3 MB | completes ≤ 150 s | PASS | wall 907 ms |
| resources | build, 50k, warm | 0.17 CPU-s, 66.9 MB | completes ≤ 150 s | PASS | wall 151 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 73.8 MB | ≤ 22.0 MB | FAIL | RSS 101.3 MB at input 100, 175.1 MB at input 1000; build per input 93 ms at 50, 1575 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.5 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 5.2 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd, just written: wall time | 16 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, just written: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, just written: peak RSS | 19.0 MB | ≤ 10.0 MB | FAIL |  |
| startup | hd check empty.hd, settled: wall time | 5.2 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd check empty.hd, settled: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd, settled: peak RSS | 9.8 MB | ≤ 10.0 MB | PASS |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 1.87x | ≤ 1.50x | FAIL | p95 119 ms at N = 1; 4 cores; total CPU 230 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 2.13x | ≤ 6.00x | PASS | p95 119 ms at N = 1; 4 cores; total CPU 440 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 0.30 | &lt; 1.00 (sublinear) | PASS | 190 ms at N = 1, 440 ms at N = 16 |
| disk | artifacts per worktree | 0.0 MB | ≤ 10.0 MB | PASS | 10 files; HD_CACHE 11.4 MB, shared per user, not counted |
| disk | toolchain, once per machine | 31.7 MB | recorded, no target | PASS | toolchain files: /home/runner/work/hd-lang/hd-lang/target/dist/hd |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 2.01x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 212 files | the files of one check's cache | FAIL | one check alone: 36 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 14.2 MB | ≤ 5.0 MB | FAIL | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 1 | ≤ 1 | PASS | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 93k/s | ≥ 100k/s | FAIL | 6000 extra cases took 64 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 15 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 120 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.1 ms | ≤ 1.0 ms | PASS | 100 tests: 56 ms |
| integration-test-perf | setup per integration test program | 26 ms | ≤ 20 ms | FAIL | 10 programs: 314 ms for 21 tests, 15 ms per test |
| integration-test-perf | total time growth in programs | 0.968 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 56 ms at 2, 265 ms at 10 programs |
| integration-test-perf | cost per doc test | 21 ms | ≤ 20 ms | FAIL | 10 doc tests: 249 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 0.54x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.83x | ≤ 3.00x | PASS | hd 6.5 ms (p10 6.5, p90 6.6), Node 7.8 ms, Python 75 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 0.16x | ≤ 3.00x | PASS | hd 8.4 ms (p10 8.3, p90 8.4), Node 52 ms, Python 252 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.88x | ≤ 3.00x | PASS | hd 5.0 ms (p10 5.0, p90 5.1), Node 5.7 ms, Python 11 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 0.73x | ≤ 3.00x | PASS | hd 5.4 ms (p10 5.3, p90 5.4), Node 7.4 ms, Python 15 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.56x | ≤ 3.00x | PASS | hd 11 ms (p10 11, p90 12), Node 20 ms, Python 16 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | list loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | map loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| allocations | iterator chain, bytes per iteration | - | ≤ 1 allocation (≤ 32 B/iter) | N/A | not supported by this hd: the program does not run on Node, so V8 heap statistics are out of reach, and no other allocation counter is known |
| size-startup-heap | tiny program, release Wasm size | 5014 B | ≤ 2048 B | FAIL | benchmarks: fib 5141 B, sum 4917 B, string-build 7811 B, map 7102 B, sort 12983 B |
| size-startup-heap | tiny program, start to first output | 6.8 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 0.84x | ≤ 2.00x | PASS | hd +12.0 MB, Node +14.3 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.41x | ≤ 2.00x | PASS | hd +8.3 MB, Node +20.1 MB over an empty program |
| host-call-overhead | Console println, per call | 1.8 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 5.9 µs | ≤ 10 µs | PASS | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 10 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 15 ns | ≤ 100 ns | PASS | 15 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 10.1M/s | ≥ 1.0M/s | PASS | 198 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 6.2M/s | ≥ 1.0M/s | PASS | 321 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.10x | ≥ 0.50x | FAIL | hd 60 MB/s (45488 B document), Node 586 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.13x | ≥ 0.50x | FAIL | hd 26 MB/s (45488 B document), Node 208 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 1.47x | ≥ 0.50x | PASS | hd 194 MB/s, Node 132 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 0.90x | ≥ 0.50x | PASS | hd 363 MB/s, Node 403 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.06x | ≥ 0.50x | FAIL | hd 6.2 MB/s, Node 95 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 6516 B | ≤ 10240 B | PASS | 12594 B at 1075 lines, 71242 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 915 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 0.0 MB | ≤ 10.0 MB | PASS | 12170000 requests; RSS 63.6 MB after warm-up, 63.6 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 8155 of 8155 unlisted cases passed |
| conformance | listed known failures | 1 | ≤ 0 | FAIL | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
- [2026-11-07T03](daily/2026-11-07T03.json)
- [2026-11-06T19](daily/2026-11-06T19.json)
- [2026-11-06T04](daily/2026-11-06T04.json)
- [2026-11-05T20](daily/2026-11-05T20.json)
- [2026-11-05T04](daily/2026-11-05T04.json)
- [2026-11-04T19](daily/2026-11-04T19.json)
