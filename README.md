# Daily goal metrics

Finished: 2026-11-16T03:47:08.573Z. Commit: f67c8607cd07e528c0afe6eeb459e106633f196b.
Machine: ubuntu-latest, 4 CPUs (Intel(R) Xeon(R) 6975P-C).
Compiler: target/release/hd.

272 passed, 23 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 47 ms → pass 37 ms
- edit-latency: p95 check after an edit: pass 49 ms → pass 39 ms
- cold-check: median cold check, 10k lines: pass 227 ms → pass 175 ms
- test-latency: median edit-then-test: pass 193 ms → pass 149 ms
- pathological: overlapping-from: time at 4x: pass 58 ms → pass 40 ms
- pathological: deep-blocks: time at 4x: pass 22 ms → pass 18 ms
- pathological: deep-parens: time at 4x: pass 21 ms → pass 18 ms
- pathological: method-chain: time at 4x: pass 27 ms → pass 22 ms
- pathological: iterator-chain: time at 4x: pass 30 ms → pass 24 ms
- pathological: wide-list-literal: time at 4x: pass 56 ms → pass 37 ms
- pathological: large-enum-match: time at 4x: pass 35 ms → pass 26 ms
- pathological: deep-generic: time at 4x: pass 25 ms → pass 21 ms
- pathological: question-chain: time at 4x: pass 31 ms → pass 24 ms
- pathological: big-file: time at 4x: pass 175 ms → pass 127 ms
- scale: dyn-values build: pass 5.54x → pass 4.00x
- scale: let-statements build: fail 4.68x → pass 4.00x
- scale: enum-arms build: pass 11.14x → pass 4.00x
- scale: for-nest build: pass 9.41x → pass 4.64x
- scale: arith-nest build: fail 4.50x → pass 4.13x
- scale: bit-nest build: fail 4.53x → pass 4.08x
- scale: tuple-nest build: pass 6.95x → pass 5.53x
- scale: call-nest build: pass 2.89x → pass 3.26x
- scale: module-path-nest check: pass 1.95x → pass 1.64x
- scale: module-path-nest build: pass 1.85x → pass 1.58x
- fmt: hd fmt, 10k lines: pass 24 ms → pass 20 ms
- dev-speed: worst case dev / optimized: pass 4.12x → pass 4.66x
- resources: warm check, 10k lines: CPU time: pass 40 ms → pass 30 ms
- long-session: RSS growth, REPL inputs 100 to 1000: fail 96.6 MB → fail 74.3 MB
- startup: hd check empty.hd: wall time: pass 16 ms → pass 13 ms
- proptest-perf: property cases per second: fail 29k/s → fail 54k/s
- proptest-perf: time to shrink a known failure: pass 110 ms → pass 84 ms
- unit-test-perf: 1,000 unit tests, warm: pass 945 ms → pass 298 ms
- unit-test-perf: overhead per unit test: pass 0.8 ms → pass 0.2 ms
- integration-test-perf: setup per integration test program: fail 97 ms → fail 73 ms
- integration-test-perf: total time growth in programs: fail 1.006 → pass 0.939
- integration-test-perf: cost per doc test: fail 91 ms → fail 74 ms
- runtime: fib: hd / Node: pass 0.77x → pass 1.15x
- runtime: sum: hd / Node: pass 0.45x → pass 0.23x
- runtime: string-build: hd / Node: pass 0.89x → pass 1.13x
- runtime: map: hd / Node: fail 3.24x → pass 2.89x
- allocations: counted loop, bytes per iteration: pass 0.00 B/iter → pass 0.02 B/iter
- allocations: map loop, bytes per iteration: pass 0.01 B/iter → pass 0.00 B/iter
- size-startup-heap: tiny program, start to first output: fail 48 ms → fail 39 ms
- host-call-overhead: Console println, per call: fail 3.8 µs → fail 2.6 µs
- host-call-overhead: Fs read_text!, per call: fail 17 µs → fail 11 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 33 µs → fail 21 µs
- suspension-overhead: all! of two ready tasks, throughput: pass 12.1M/s → pass 19.6M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 8.8M/s → pass 13.4M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.10x → fail 0.14x
- text-throughput: string build, hd / Node MB/s: pass 2.88x → pass 3.76x
- text-throughput: split, hd / Node MB/s: pass 1.71x → pass 3.25x
- text-throughput: regex find_all, hd / Node MB/s: fail 0.08x → fail 0.10x

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 37 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 39 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 175 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 149 ms | ≤ 300 ms | PASS |  |
| mistakes | programs with exactly one diagnostic | 100.0% | ≥ 95.0% | PASS | 69 of 69 |
| mistakes | one diagnostic with the expected code | 100.0% | ≥ 95.0% | PASS | 69 of 69 |
| mistakes | fix-it makes hd check pass | 34.8% | ≥ 80.0% | FAIL | 24 of 69 resolved; 24 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 65 tokens | ≤ 60 tokens | FAIL | largest: mut-method-on-readonly-binding; median 43 tokens, 171 B; 67 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 40 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 27.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 29 → 40 ms, start-up 17 ms |
| pathological | deep-blocks: time at 4x | 18 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-blocks: peak RSS at 4x | 22.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 18 → 18 ms, start-up 17 ms |
| pathological | deep-parens: time at 4x | 18 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 21.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 17 → 18 ms, start-up 17 ms |
| pathological | method-chain: time at 4x | 22 ms | ≤ 2000 ms | PASS |  |
| pathological | method-chain: peak RSS at 4x | 23.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 22 ms, start-up 17 ms |
| pathological | iterator-chain: time at 4x | 24 ms | ≤ 2000 ms | PASS |  |
| pathological | iterator-chain: peak RSS at 4x | 23.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | iterator-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 20 → 24 ms, start-up 17 ms |
| pathological | wide-list-literal: time at 4x | 37 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 26.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 27 → 37 ms, start-up 17 ms |
| pathological | large-enum-match: time at 4x | 26 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 25.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 21 → 26 ms, start-up 17 ms |
| pathological | deep-generic: time at 4x | 21 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 22.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 18 → 21 ms, start-up 17 ms |
| pathological | question-chain: time at 4x | 24 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 25.2 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 24 ms, start-up 17 ms |
| pathological | big-file: time at 4x | 127 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 54.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.09 | ≤ 1.30 | PASS | 2x→4x; 69 → 127 ms, start-up 17 ms |
| scale | fn-items check | 4.04x | ≤ 5x | PASS | hashes 196629 → 794597 (4.04x, limit 5.00x) |
| scale | fn-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | fn-items build | 4.01x | ≤ 5x | PASS | hashes 496060 → 1991377 (4.01x, limit 5.00x) |
| scale | struct-items check | 4.02x | ≤ 5x | PASS | hashes 211598 → 851574 (4.02x, limit 5.00x) |
| scale | struct-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | struct-items build | 4.00x | ≤ 5x | PASS | scope_probes 9000 → 36000 (4.00x, limit 5.00x) |
| scale | enum-items check | 4.00x | ≤ 5x | PASS | scope_probes 23500 → 94000 (4.00x, limit 5.00x) |
| scale | enum-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | enum-items build | 4.00x | ≤ 5x | PASS | scope_probes 23500 → 94000 (4.00x, limit 5.00x) |
| scale | trait-items check | 4.01x | ≤ 5x | PASS | hashes 1073637 → 4304088 (4.01x, limit 5.00x) |
| scale | trait-items fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | trait-items build | 4.01x | ≤ 5x | PASS | hashes 1699717 → 6823996 (4.01x, limit 5.00x) |
| scale | impl-methods check | 4.00x | ≤ 5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 5.00x) |
| scale | impl-methods fmt | 3.97x | ≤ 5x | PASS | allocs 4045 → 16057 (3.97x, limit 5.00x) |
| scale | impl-methods build | 4.00x | ≤ 5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 5.00x) |
| scale | impls-of-one-trait check | 4.00x | ≤ 5x | PASS | hashes 581822 → 2328366 (4.00x, limit 5.00x) |
| scale | impls-of-one-trait fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | impls-of-one-trait build | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | operator-impls check | 4.00x | ≤ 5x | PASS | hashes 708017 → 2833805 (4.00x, limit 5.00x) |
| scale | operator-impls fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | operator-impls build | 4.00x | ≤ 5x | PASS | scope_probes 25000 → 100000 (4.00x, limit 5.00x) |
| scale | derive-items check | 4.00x | ≤ 5x | PASS | trait_lookup 16998 → 67998 (4.00x, limit 5.00x) |
| scale | derive-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | derive-items build | 4.29x | ≤ 5x | PASS | hashes 45019939 → 192964703 (4.29x, limit 5.00x) |
| scale | dyn-values check | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | dyn-values fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | dyn-values build | 4.00x | ≤ 5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 5.00x) |
| scale | consts check | 3.99x | ≤ 5x | PASS | hashes 63470 → 253002 (3.99x, limit 5.00x) |
| scale | consts fmt | 3.89x | ≤ 5x | PASS | allocs 2087 → 8109 (3.89x, limit 5.00x) |
| scale | consts build | 4.00x | ≤ 5x | PASS | use_facts 23531 → 94031 (4.00x, limit 5.00x) |
| scale | local-items check | 4.00x | ≤ 5x | PASS | hashes 595073 → 2380861 (4.00x, limit 5.00x) |
| scale | local-items fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | local-items build | 4.00x | ≤ 5x | PASS | scope_probes 19000 → 76000 (4.00x, limit 5.00x) |
| scale | syntax-items check | 4.00x | ≤ 5x | PASS | trait_lookup 9898 → 39598 (4.00x, limit 5.00x) |
| scale | syntax-items fmt | 4.00x | ≤ 5x | PASS | hashes 17160 → 68664 (4.00x, limit 5.00x) |
| scale | syntax-items build | 4.00x | ≤ 5x | PASS | trait_lookup 9898 → 39598 (4.00x, limit 5.00x) |
| scale | struct-chain check | 4.06x | ≤ 5x | PASS | hashes 148499 → 602215 (4.06x, limit 5.00x) |
| scale | struct-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | modules check | 4.00x | ≤ 5x | PASS | hashes 1181818 → 4731331 (4.00x, limit 5.00x) |
| scale | modules fmt | 4.00x | ≤ 5x | PASS | hashes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | modules build | 4.00x | ≤ 5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 5.00x) |
| scale | use-names check | 4.04x | ≤ 5x | PASS | hashes 226669 → 916017 (4.04x, limit 5.00x) |
| scale | use-names fmt | 4.01x | ≤ 5x | PASS | hashes 3783 → 15159 (4.01x, limit 5.00x) |
| scale | use-names build | 4.02x | ≤ 5x | PASS | hashes 526158 → 2112841 (4.02x, limit 5.00x) |
| scale | generic-fns check | 4.03x | ≤ 5x | PASS | hashes 301670 → 1214694 (4.03x, limit 5.00x) |
| scale | generic-fns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | generic-fns build | 4.01x | ≤ 5x | PASS | hashes 826307 → 3317192 (4.01x, limit 5.00x) |
| scale | generic-params check | 4.22x | ≤ 5x | PASS | hashes 13862 → 58502 (4.22x, limit 5.00x) |
| scale | generic-params fmt | 4.10x | ≤ 5x | PASS | alloc_bytes 344142 → 1412604 (4.10x, limit 10.00x) |
| scale | generic-params build | 4.00x | ≤ 5x | PASS | scope_probes 800 → 3200 (4.00x, limit 5.00x) |
| scale | param-bounds check | 4.03x | ≤ 5x | PASS | hashes 55471 → 223307 (4.03x, limit 5.00x) |
| scale | param-bounds fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | param-bounds build | 4.00x | ≤ 5x | PASS | scope_probes 1500 → 6000 (4.00x, limit 5.00x) |
| scale | type-defaults check | 4.00x | ≤ 5x | PASS | scope_probes 33500 → 134000 (4.00x, limit 5.00x) |
| scale | type-defaults fmt | 4.00x | ≤ 5x | PASS | hashes 7576 → 30328 (4.00x, limit 5.00x) |
| scale | type-defaults build | 4.00x | ≤ 5x | PASS | scope_probes 33500 → 134000 (4.00x, limit 5.00x) |
| scale | row-chain check | 4.02x | ≤ 5x | PASS | hashes 229324 → 921640 (4.02x, limit 5.00x) |
| scale | row-chain fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | row-chain build | 4.01x | ≤ 5x | PASS | hashes 535230 → 2147784 (4.01x, limit 5.00x) |
| scale | row-traits check | 4.00x | ≤ 5x | PASS | scope_probes 2100 → 8400 (4.00x, limit 5.00x) |
| scale | row-traits fmt | 4.04x | ≤ 5x | PASS | hashes 616 → 2488 (4.04x, limit 5.00x) |
| scale | row-traits build | 4.00x | ≤ 5x | PASS | scope_probes 2100 → 8400 (4.00x, limit 5.00x) |
| scale | closures check | 4.00x | ≤ 5x | PASS | local_body_checks 1000 → 4000 (4.00x, limit 5.00x) |
| scale | closures fmt | 3.94x | ≤ 5x | PASS | allocs 3061 → 12073 (3.94x, limit 5.00x) |
| scale | closures build | 4.00x | ≤ 5x | PASS | local_body_checks 1000 → 4000 (4.00x, limit 5.00x) |
| scale | let-statements check | 3.96x | ≤ 5x | PASS | allocs 25332 → 100428 (3.96x, limit 5.00x) |
| scale | let-statements fmt | 3.94x | ≤ 5x | PASS | allocs 2554 → 10066 (3.94x, limit 5.00x) |
| scale | let-statements build | 4.00x | ≤ 5x | PASS | control 72499 → 289999 (4.00x, limit 5.00x) |
| scale | matches check | 4.00x | ≤ 5x | PASS | decision_rows 6000 → 24000 (4.00x, limit 5.00x) |
| scale | matches fmt | 3.97x | ≤ 5x | PASS | allocs 6057 → 24069 (3.97x, limit 5.00x) |
| scale | matches build | 4.09x | ≤ 5x | PASS | hashes 892823 → 3650590 (4.09x, limit 5.00x) |
| scale | enum-arms check | 4.00x | ≤ 5x | PASS | hashes 61156 → 244660 (4.00x, limit 5.00x) |
| scale | enum-arms fmt | 3.91x | ≤ 5x | PASS | allocs 1551 → 6065 (3.91x, limit 5.00x) |
| scale | enum-arms build | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | tuple-patterns check | 4.00x | ≤ 5x | PASS | decision_rows 1000 → 4000 (4.00x, limit 5.00x) |
| scale | tuple-patterns fmt | 3.95x | ≤ 5x | PASS | allocs 3054 → 12066 (3.95x, limit 5.00x) |
| scale | tuple-patterns build | 4.00x | ≤ 5x | PASS | control 95965 → 383965 (4.00x, limit 5.00x) |
| scale | range-patterns check | 4.02x | ≤ 5x | PASS | hashes 252598 → 1016618 (4.02x, limit 5.00x) |
| scale | range-patterns fmt | 4.01x | ≤ 5x | PASS | hashes 3784 → 15160 (4.01x, limit 5.00x) |
| scale | range-patterns build | 4.01x | ≤ 5x | PASS | hashes 860004 → 3448807 (4.01x, limit 5.00x) |
| scale | wide-struct check | 4.00x | ≤ 5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 5.00x) |
| scale | wide-struct fmt | 3.95x | ≤ 5x | PASS | allocs 2544 → 10056 (3.95x, limit 5.00x) |
| scale | wide-struct build | 4.01x | ≤ 5x | PASS | hashes 273292 → 1094577 (4.01x, limit 5.00x) |
| scale | call-args check | 4.31x | ≤ 5x | PASS | hashes 4532 → 19520 (4.31x, limit 5.00x) |
| scale | call-args fmt | 4.10x | ≤ 5x | PASS | alloc_bytes 361569 → 1482933 (4.10x, limit 10.00x) |
| scale | call-args build | 4.07x | ≤ 5x | PASS | hashes 20851 → 84829 (4.07x, limit 5.00x) |
| scale | binary-chain check | 4.00x | ≤ 5x | PASS | hashes 2500 → 10000 (4.00x, limit 5.00x) |
| scale | binary-chain fmt | 3.78x | ≤ 5x | PASS | allocs 545 → 2060 (3.78x, limit 5.00x) |
| scale | binary-chain build | 4.02x | ≤ 5x | PASS | hashes 23050 → 92614 (4.02x, limit 5.00x) |
| scale | interpolation check | 4.00x | ≤ 5x | PASS | trait_lookup 2501 → 10001 (4.00x, limit 5.00x) |
| scale | interpolation fmt | 4.27x | ≤ 5x | PASS | alloc_bytes 6453 → 27573 (4.27x, limit 10.00x) |
| scale | interpolation build | 4.04x | ≤ 5x | PASS | hashes 2855351 → 11536792 (4.04x, limit 5.00x) |
| scale | list-literal check | 4.00x | ≤ 5x | PASS | hashes 7392 → 29580 (4.00x, limit 5.00x) |
| scale | list-literal fmt | 3.82x | ≤ 5x | PASS | allocs 533 → 2038 (3.82x, limit 5.00x) |
| scale | list-literal build | 4.01x | ≤ 5x | PASS | hashes 35442 → 142194 (4.01x, limit 5.00x) |
| scale | method-chain check | 4.00x | ≤ 5x | PASS | hashes 14394 → 57582 (4.00x, limit 5.00x) |
| scale | method-chain fmt | 4.04x | ≤ 5x | PASS | alloc_bytes 838129 → 3386711 (4.04x, limit 10.00x) |
| scale | method-chain build | 4.00x | ≤ 5x | PASS | trait_lookup 500 → 2000 (4.00x, limit 5.00x) |
| scale | pipe-chain check | 4.00x | ≤ 5x | PASS | hashes 16509 → 66009 (4.00x, limit 5.00x) |
| scale | pipe-chain fmt | 3.77x | ≤ 5x | PASS | allocs 546 → 2056 (3.77x, limit 5.00x) |
| scale | pipe-chain build | 3.97x | ≤ 5x | PASS | hashes 87830 → 349022 (3.97x, limit 5.00x) |
| scale | question-chain check | 4.00x | ≤ 5x | PASS | hashes 44420 → 177608 (4.00x, limit 5.00x) |
| scale | question-chain fmt | 3.94x | ≤ 5x | PASS | allocs 1533 → 6044 (3.94x, limit 5.00x) |
| scale | question-chain build | 3.99x | ≤ 5x | PASS | use_facts 24570 → 98070 (3.99x, limit 5.00x) |
| scale | if-nest check | 3.93x | ≤ 5x per 4x bytes (12.1x) | PASS | hashes 852 → 3348 (3.93x, limit 15.11x) |
| scale | if-nest fmt | 5.30x | ≤ 5x per 4x bytes (12.1x) | PASS | alloc_bytes 144371 → 765724 (5.30x, limit 30.21x) |
| scale | if-nest build | 4.00x | ≤ 5x per 4x bytes (12.1x) | PASS | use_facts 768 → 3072 (4.00x, limit 15.11x) |
| scale | block-nest check | 3.95x | ≤ 5x per 4x bytes (12.2x) | PASS | hashes 910 → 3598 (3.95x, limit 15.29x) |
| scale | block-nest fmt | 5.69x | ≤ 5x per 4x bytes (12.2x) | PASS | alloc_bytes 160538 → 913156 (5.69x, limit 30.57x) |
| scale | block-nest build | 3.95x | ≤ 5x per 4x bytes (12.2x) | PASS | use_facts 878 → 3470 (3.95x, limit 15.29x) |
| scale | for-nest check | 5.20x | ≤ 5x per 4x bytes (11.6x) | PASS | hashes 3567 → 18543 (5.20x, limit 14.50x) |
| scale | for-nest fmt | 5.39x | ≤ 5x per 4x bytes (11.6x) | PASS | alloc_bytes 139001 → 749031 (5.39x, limit 29.01x) |
| scale | for-nest build | 4.64x | ≤ 5x per 4x bytes (11.6x) | PASS | hashes 24850 → 115422 (4.64x, limit 14.50x) |
| scale | while-nest check | 3.89x | ≤ 5x per 4x bytes (12.1x) | PASS | hashes 1129 → 4393 (3.89x, limit 15.11x) |
| scale | while-nest fmt | 5.31x | ≤ 5x per 4x bytes (12.1x) | PASS | alloc_bytes 143075 → 760300 (5.31x, limit 30.21x) |
| scale | while-nest build | 4.00x | ≤ 5x per 4x bytes (12.1x) | PASS | use_facts 1184 → 4736 (4.00x, limit 15.11x) |
| scale | match-nest check | 4.76x | ≤ 5x per 4x bytes (12.8x) | PASS | hashes 2032 → 9676 (4.76x, limit 16.03x) |
| scale | match-nest fmt | 5.82x | ≤ 5x per 4x bytes (12.8x) | PASS | alloc_bytes 158237 → 920971 (5.82x, limit 32.07x) |
| scale | match-nest build | 4.19x | ≤ 5x per 4x bytes (12.8x) | PASS | hashes 10200 → 42762 (4.19x, limit 16.03x) |
| scale | closure-nest check | 3.98x | ≤ 5x per 4x bytes (9.2x) | PASS | hashes 2001 → 7965 (3.98x, limit 11.45x) |
| scale | closure-nest fmt | 5.54x | ≤ 5x per 4x bytes (9.2x) | PASS | alloc_bytes 74245 → 410949 (5.54x, limit 22.90x) |
| scale | closure-nest build | 4.01x | ≤ 5x per 4x bytes (9.2x) | PASS | use_facts 479 → 1919 (4.01x, limit 11.45x) |
| scale | option-nest check | 4.18x | ≤ 5x | PASS | hashes 1879 → 7849 (4.18x, limit 5.00x) |
| scale | option-nest fmt | 6.24x | ≤ 5x | PASS | alloc_bytes 73101 → 456308 (6.24x, limit 10.00x) |
| scale | option-nest build | 4.00x | ≤ 5x | PASS | use_facts 608 → 2432 (4.00x, limit 5.00x) |
| scale | list-type-nest check | 4.27x | ≤ 5x | PASS | hashes 1269 → 5417 (4.27x, limit 5.00x) |
| scale | list-type-nest fmt | 6.05x | ≤ 5x | PASS | alloc_bytes 55328 → 334852 (6.05x, limit 10.00x) |
| scale | list-type-nest build | 4.29x | ≤ 5x | PASS | hashes 4276 → 18346 (4.29x, limit 5.00x) |
| scale | generic-nest check | 4.30x | ≤ 5x | PASS | hashes 1617 → 6958 (4.30x, limit 5.00x) |
| scale | generic-nest fmt | 7.40x | ≤ 5x | PASS | alloc_bytes 10894 → 80601 (7.40x, limit 10.00x) |
| scale | generic-nest build | 4.03x | ≤ 5x | PASS | allocs 5085 → 20473 (4.03x, limit 5.00x) |
| scale | paren-nest check | 4.45x | ≤ 5x | PASS | alloc_bytes 38264 → 170258 (4.45x, limit 10.00x) |
| scale | paren-nest fmt | 3.21x | ≤ 5x | PASS | alloc_bytes 34650 → 111359 (3.21x, limit 10.00x) |
| scale | paren-nest build | 4.07x | ≤ 5x | PASS | control 859 → 3499 (4.07x, limit 5.00x) |
| scale | arith-nest check | 4.00x | ≤ 5x | PASS | hashes 440 → 1760 (4.00x, limit 5.00x) |
| scale | arith-nest fmt | 2.94x | ≤ 5x | PASS | alloc_bytes 76966 → 226347 (2.94x, limit 10.00x) |
| scale | arith-nest build | 4.13x | ≤ 5x | PASS | hashes 5505 → 22752 (4.13x, limit 5.00x) |
| scale | bool-nest check | 3.95x | ≤ 5x | PASS | hashes 391 → 1543 (3.95x, limit 5.00x) |
| scale | bool-nest fmt | 5.11x | ≤ 5x | PASS | alloc_bytes 37604 → 191986 (5.11x, limit 10.00x) |
| scale | bool-nest build | 4.21x | ≤ 5x | PASS | hashes 2617 → 11013 (4.21x, limit 5.00x) |
| scale | bit-nest check | 3.99x | ≤ 5x | PASS | hashes 818 → 3266 (3.99x, limit 5.00x) |
| scale | bit-nest fmt | 2.91x | ≤ 5x | PASS | alloc_bytes 163583 → 476792 (2.91x, limit 10.00x) |
| scale | bit-nest build | 4.08x | ≤ 5x | PASS | hashes 7570 → 30856 (4.08x, limit 5.00x) |
| scale | tuple-nest check | 4.06x | ≤ 5x | PASS | hashes 1960 → 7966 (4.06x, limit 5.00x) |
| scale | tuple-nest fmt | 5.67x | ≤ 5x | PASS | alloc_bytes 122064 → 692688 (5.67x, limit 10.00x) |
| scale | tuple-nest build | 5.53x | ≤ 5x | PASS | accepted superlinear, bounded by the parser's MAX_NESTING = 160 (owner 2026-11-15, #1924): +37 MB allocated at depth 150, no measured time: control 431 → 2384 (5.53x, limit 5.00x) |
| scale | if-expr-nest check | 3.76x | ≤ 5x | PASS | hashes 322 → 1210 (3.76x, limit 5.00x) |
| scale | if-expr-nest fmt | 4.51x | ≤ 5x | PASS | alloc_bytes 68686 → 309984 (4.51x, limit 10.00x) |
| scale | if-expr-nest build | 3.82x | ≤ 5x | PASS | use_facts 341 → 1301 (3.82x, limit 5.00x) |
| scale | call-nest check | 3.97x | ≤ 5x | PASS | hashes 577 → 2293 (3.97x, limit 5.00x) |
| scale | call-nest fmt | 6.40x | ≤ 5x | PASS | alloc_bytes 33324 → 213300 (6.40x, limit 10.00x) |
| scale | call-nest build | 3.26x | ≤ 5x | PASS | hashes 3681 → 12013 (3.26x, limit 5.00x) |
| scale | index-nest check | 4.50x | ≤ 5x | PASS | alloc_bytes 32492 → 146124 (4.50x, limit 10.00x) |
| scale | index-nest fmt | 7.56x | ≤ 5x | PASS | alloc_bytes 23956 → 181114 (7.56x, limit 10.00x) |
| scale | index-nest build | 4.16x | ≤ 5x | PASS | hashes 1798 → 7484 (4.16x, limit 5.00x) |
| scale | fn-nest check | 3.94x | ≤ 5x per 4x bytes (10.7x) | PASS | hashes 2698 → 10630 (3.94x, limit 13.42x) |
| scale | fn-nest fmt | 5.63x | ≤ 5x per 4x bytes (10.7x) | PASS | alloc_bytes 132556 → 746520 (5.63x, limit 26.84x) |
| scale | fn-nest build | 4.03x | ≤ 5x per 4x bytes (10.7x) | PASS | hashes 10360 → 41730 (4.03x, limit 13.42x) |
| scale | module-path-nest check | 1.64x | ≤ 5x | PASS | allocs 2270 → 3733 (1.64x, limit 5.00x) |
| scale | module-path-nest fmt | 7.90x | ≤ 5x | PASS | alloc_bytes 16030 → 126570 (7.90x, limit 10.00x) |
| scale | module-path-nest build | 1.58x | ≤ 5x | PASS | allocs 2297 → 3632 (1.58x, limit 5.00x) |
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
| fmt | hd fmt, 10k lines | 20 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 2.05x | ≤ 1.30x | FAIL | checked 553 ms, unchecked 269 ms |
| dev-speed | geomean dev / optimized | 1.40x | ≤ 4.00x | PASS | fib 0.98x, sum 4.66x, string-build 0.84x, map 1.09x, sort 1.30x |
| dev-speed | worst case dev / optimized | 4.66x | ≤ 10.00x | PASS | sum |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 30 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 25.9 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.15 CPU-s, 36.9 MB | completes ≤ 30 s | PASS | wall 95 ms |
| resources | check, small, warm | 0.01 CPU-s, 18.8 MB | completes ≤ 30 s | PASS | wall 17 ms |
| resources | test, small, cold | 1.21 CPU-s, 93.4 MB | completes ≤ 30 s | PASS | wall 509 ms |
| resources | test, small, warm | 0.46 CPU-s, 66.6 MB | completes ≤ 30 s | PASS | wall 204 ms |
| resources | build, small, cold | 0.71 CPU-s, 86.5 MB | completes ≤ 30 s | PASS | wall 284 ms |
| resources | build, small, warm | 0.02 CPU-s, 31.4 MB | completes ≤ 30 s | PASS | wall 29 ms |
| resources | check, 10k, cold | 0.40 CPU-s, 58.8 MB | completes ≤ 30 s | PASS | wall 179 ms |
| resources | check, 10k, warm | 0.03 CPU-s, 25.9 MB | completes ≤ 30 s | PASS | wall 35 ms |
| resources | test, 10k, cold | 1.83 CPU-s, 126.2 MB | completes ≤ 30 s | PASS | wall 845 ms |
| resources | test, 10k, warm | 0.58 CPU-s, 71.3 MB | completes ≤ 30 s | PASS | wall 254 ms |
| resources | build, 10k, cold | 0.98 CPU-s, 110.3 MB | completes ≤ 30 s | PASS | wall 364 ms |
| resources | build, 10k, warm | 0.04 CPU-s, 35.2 MB | completes ≤ 30 s | PASS | wall 46 ms |
| resources | check, 50k, cold | 1.55 CPU-s, 164.2 MB | completes ≤ 150 s | PASS | wall 578 ms |
| resources | check, 50k, warm | 0.12 CPU-s, 56.8 MB | completes ≤ 150 s | PASS | wall 110 ms |
| resources | test, 50k, cold | 4.68 CPU-s, 281.8 MB | completes ≤ 150 s | PASS | wall 2560 ms |
| resources | test, 50k, warm | 1.12 CPU-s, 97.6 MB | completes ≤ 150 s | PASS | wall 479 ms |
| resources | build, 50k, cold | 2.16 CPU-s, 210.9 MB | completes ≤ 150 s | PASS | wall 725 ms |
| resources | build, 50k, warm | 0.13 CPU-s, 65.5 MB | completes ≤ 150 s | PASS | wall 122 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 74.3 MB | ≤ 22.0 MB | FAIL | RSS 106.5 MB at input 100, 174.3 MB at input 1000; build per input 63 ms at 50, 689 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.1 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 4.3 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 13 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 17.4 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 1.28x | ≤ 1.50x | PASS | p95 98 ms at N = 1; 4 cores; total CPU 250 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 2.16x | ≤ 6.00x | PASS | p95 98 ms at N = 1; 4 cores; total CPU 450 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 0.40 | &lt; 1.00 (sublinear) | PASS | 150 ms at N = 1, 450 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 22.3 MB | ≤ 10.0 MB | FAIL | artifacts 8045 B in 10 files; toolchain 22.3 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 10.8 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 2.01x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 211 files | the files of one check's cache | FAIL | one check alone: 35 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 14.1 MB | ≤ 5.0 MB | FAIL | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 79 | ≤ 1 | FAIL | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 54k/s | ≥ 100k/s | FAIL | 6000 extra cases took 112 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 84 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 298 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.2 ms | ≤ 1.0 ms | PASS | 100 tests: 138 ms |
| integration-test-perf | setup per integration test program | 73 ms | ≤ 20 ms | FAIL | 10 programs: 810 ms for 21 tests, 39 ms per test |
| integration-test-perf | total time growth in programs | 0.939 | exponent &lt; 1 (sublinear) | PASS | beyond the 0-program run: 159 ms at 2, 722 ms at 10 programs |
| integration-test-perf | cost per doc test | 74 ms | ≤ 20 ms | FAIL | 10 doc tests: 832 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 0.89x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 1.15x | ≤ 3.00x | PASS | hd 7.1 ms (p10 3.5, p90 14), Node 6.2 ms, Python 59 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 0.23x | ≤ 3.00x | PASS | hd 9.6 ms (p10 7.6, p90 23), Node 42 ms, Python 201 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 1.13x | ≤ 3.00x | PASS | hd 8.0 ms (p10 3.5, p90 12), Node 7.0 ms, Python 8.8 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 2.89x | ≤ 3.00x | PASS | hd 20 ms (p10 14, p90 23), Node 7.0 ms, Python 12 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.66x | ≤ 3.00x | PASS | hd 11 ms (p10 8.4, p90 15), Node 17 ms, Python 13 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.02 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.01 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 5024 B | ≤ 2048 B | FAIL | benchmarks: fib 5153 B, sum 4929 B, string-build 7809 B, map 7056 B, sort 12679 B |
| size-startup-heap | tiny program, start to first output | 39 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 1.05x | ≤ 2.00x | PASS | hd +15.2 MB, Node +14.5 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.42x | ≤ 2.00x | PASS | hd +8.5 MB, Node +20.1 MB over an empty program |
| host-call-overhead | Console println, per call | 2.6 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 11 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 21 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 0.00 ns | ≤ 100 ns | PASS | -13 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 19.6M/s | ≥ 1.0M/s | PASS | 102 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 13.4M/s | ≥ 1.0M/s | PASS | 149 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.14x | ≥ 0.50x | FAIL | hd 99 MB/s (45488 B document), Node 727 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.19x | ≥ 0.50x | FAIL | hd 52 MB/s (45488 B document), Node 271 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 3.76x | ≥ 0.50x | PASS | hd 543 MB/s, Node 144 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 3.25x | ≥ 0.50x | PASS | hd 1072 MB/s, Node 330 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.10x | ≥ 0.50x | FAIL | hd 12 MB/s, Node 121 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 6431 B | ≤ 10240 B | PASS | 12516 B at 1075 lines, 70405 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 915 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 2.7 MB | ≤ 10.0 MB | PASS | 33710000 requests; RSS 68.7 MB after warm-up, 71.5 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 7758 of 7758 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
- [2026-11-03T19](daily/2026-11-03T19.json)
- [2026-11-03T04](daily/2026-11-03T04.json)
- [2026-11-02T21](daily/2026-11-02T21.json)
