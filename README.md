# Daily goal metrics

Finished: 2026-11-15T19:02:13.131Z. Commit: 108369669d4557733cfda43e9dd261cd92087d6a.
Machine: ubuntu-latest, 4 CPUs (Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz).
Compiler: target/release/hd.

267 passed, 28 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

- edit-latency: p50 check after an edit: pass 38 ms → pass 47 ms
- edit-latency: p95 check after an edit: pass 42 ms → pass 49 ms
- cold-check: median cold check, 10k lines: pass 162 ms → pass 227 ms
- test-latency: median edit-then-test: pass 145 ms → pass 193 ms
- pathological: overlapping-from: time at 4x: pass 40 ms → pass 58 ms
- pathological: deep-parens: time at 4x: pass 19 ms → pass 21 ms
- pathological: iterator-chain: time at 4x: pass 26 ms → pass 30 ms
- pathological: wide-list-literal: time at 4x: pass 37 ms → pass 56 ms
- pathological: large-enum-match: time at 4x: pass 29 ms → pass 35 ms
- pathological: deep-generic: time at 4x: pass 22 ms → pass 25 ms
- pathological: question-chain: time at 4x: pass 26 ms → pass 31 ms
- pathological: big-file: time at 4x: pass 115 ms → pass 175 ms
- scale: dyn-values build: pass 6.49x → pass 5.54x
- scale: consts build: pass 4.02x → pass 4.43x
- scale: row-chain build: pass 11.72x → pass 4.01x
- scale: closures build: pass 15.89x → pass 4.19x
- scale: let-statements build: pass 4.00x → fail 4.68x
- scale: matches build: pass 9.36x → pass 4.28x
- scale: binary-chain fmt: pass 2.89x → pass 3.78x
- scale: interpolation build: pass 11.25x → pass 4.40x
- scale: list-literal fmt: pass 2.87x → pass 3.82x
- scale: pipe-chain fmt: pass 2.75x → pass 3.77x
- scale: for-nest build: pass 38.27x → pass 9.41x
- scale: while-nest build: pass 40.21x → pass 4.00x
- scale: closure-nest build: pass 14.37x → pass 4.01x
- scale: option-nest check: pass 6.96x → pass 4.18x
- scale: option-nest build: pass 14.62x → pass 4.15x
- scale: generic-nest check: pass 11.92x → pass 4.30x
- scale: generic-nest build: pass 11.92x → pass 4.03x
- scale: arith-nest build: pass 4.23x → fail 4.50x
- scale: bit-nest build: pass 4.01x → fail 4.53x
- scale: tuple-nest check: pass 6.75x → pass 4.07x
- scale: tuple-nest build: pass 5.56x → pass 6.95x
- scale: call-nest build: pass 3.46x → pass 2.89x
- scale: fn-nest build: pass 15.02x → pass 4.02x
- fmt: hd fmt, 10k lines: pass 18 ms → pass 24 ms
- dev-speed: geomean dev / optimized: fail - → pass 1.46x
- dev-speed: worst case dev / optimized: fail - → pass 4.12x
- resources: warm check, 10k lines: CPU time: pass 30 ms → pass 40 ms
- startup: hd check empty.hd: wall time: pass 14 ms → pass 16 ms
- concurrency: p95 latency at N = 4 vs N = 1: pass 1.22x → pass 1.38x
- concurrency: p95 latency at N = 16 vs N = 1: pass 1.95x → pass 2.30x
- concurrency: total CPU growth exponent, N = 1 to 16: pass 0.33 → pass 0.41
- proptest-perf: property cases per second: fail 57k/s → fail 29k/s
- proptest-perf: time to shrink a known failure: pass 81 ms → pass 110 ms
- unit-test-perf: 1,000 unit tests, warm: pass 615 ms → pass 945 ms
- unit-test-perf: overhead per unit test: pass 0.5 ms → pass 0.8 ms
- integration-test-perf: setup per integration test program: fail 77 ms → fail 97 ms
- integration-test-perf: total time growth in programs: pass 0.981 → fail 1.006
- integration-test-perf: cost per doc test: fail 65 ms → fail 91 ms
- runtime: geomean hd / Node: pass 0.74x → pass 0.90x
- runtime: fib: hd / Node: pass 0.41x → pass 0.77x
- runtime: sum: hd / Node: pass 0.29x → pass 0.45x
- runtime: string-build: hd / Node: pass 1.00x → pass 0.89x
- allocations: map loop, bytes per iteration: pass 0.00 B/iter → pass 0.01 B/iter
- allocations: iterator chain, bytes per iteration: pass 0.01 B/iter → pass 0.00 B/iter
- size-startup-heap: tiny program, start to first output: fail 35 ms → fail 48 ms
- host-call-overhead: Console println, per call: fail 3.0 µs → fail 3.8 µs
- host-call-overhead: Fs read_text!, per call: fail 14 µs → fail 17 µs
- host-call-overhead: serde boundary (list_dir!), per call: fail 26 µs → fail 33 µs
- suspension-overhead: await of a ready value, per await: pass 2.8 ns → pass 0.00 ns
- suspension-overhead: all! of two ready tasks, throughput: pass 16.2M/s → pass 12.1M/s
- suspension-overhead: race! of two ready tasks, throughput: pass 14.4M/s → pass 8.8M/s
- serde-throughput: JSON encode, hd / Node MB/s: fail 0.12x → fail 0.10x
- serde-throughput: JSON decode, hd / Node MB/s: fail 0.17x → fail 0.20x
- text-throughput: split, hd / Node MB/s: pass 2.85x → pass 1.71x

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 47 ms | ≤ 50 ms | PASS |  |
| edit-latency | p95 check after an edit | 49 ms | ≤ 200 ms | PASS |  |
| cold-check | median cold check, 10k lines | 227 ms | ≤ 1000 ms | PASS |  |
| test-latency | median edit-then-test | 193 ms | ≤ 300 ms | PASS |  |
| mistakes | programs with exactly one diagnostic | 100.0% | ≥ 95.0% | PASS | 69 of 69 |
| mistakes | one diagnostic with the expected code | 100.0% | ≥ 95.0% | PASS | 69 of 69 |
| mistakes | fix-it makes hd check pass | 34.8% | ≥ 80.0% | FAIL | 24 of 69 resolved; 24 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 65 tokens | ≤ 60 tokens | FAIL | largest: mut-method-on-readonly-binding; median 43 tokens, 171 B; 67 of 69 within |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | PASS |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | PASS |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | 58 ms | ≤ 2000 ms | PASS |  |
| pathological | overlapping-from: peak RSS at 4x | 27.3 MB | ≤ 200.0 MB | PASS |  |
| pathological | overlapping-from: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 38 → 58 ms, start-up 22 ms |
| pathological | deep-blocks: time at 4x | 22 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | deep-blocks: peak RSS at 4x | 21.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 22 ms, start-up 22 ms |
| pathological | deep-parens: time at 4x | 21 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-parens: peak RSS at 4x | 21.8 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-parens: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 21 → 21 ms, start-up 22 ms |
| pathological | method-chain: time at 4x | 27 ms | ≤ 2000 ms | PASS | Timing change ≤10%: noise. |
| pathological | method-chain: peak RSS at 4x | 23.1 MB | ≤ 200.0 MB | PASS |  |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 24 → 27 ms, start-up 22 ms |
| pathological | iterator-chain: time at 4x | 30 ms | ≤ 2000 ms | PASS |  |
| pathological | iterator-chain: peak RSS at 4x | 22.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | iterator-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 26 → 30 ms, start-up 22 ms |
| pathological | wide-list-literal: time at 4x | 56 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 26.5 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 36 → 56 ms, start-up 22 ms |
| pathological | large-enum-match: time at 4x | 35 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 24.9 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 29 → 35 ms, start-up 22 ms |
| pathological | deep-generic: time at 4x | 25 ms | ≤ 2000 ms | PASS |  |
| pathological | deep-generic: peak RSS at 4x | 21.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | deep-generic: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 22 → 25 ms, start-up 22 ms |
| pathological | question-chain: time at 4x | 31 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 24.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 29 → 31 ms, start-up 22 ms |
| pathological | big-file: time at 4x | 175 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 54.6 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.01 | ≤ 1.30 | PASS | 2x→4x; 98 → 175 ms, start-up 22 ms |
| scale | fn-items check | 4.04x | ≤ 4.5x | PASS | hashes 192130 → 776598 (4.04x, limit 4.50x) |
| scale | fn-items fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | fn-items build | 4.12x | ≤ 4.5x | PASS | hashes 1067553 → 4393800 (4.12x, limit 4.50x) |
| scale | struct-items check | 4.02x | ≤ 4.5x | PASS | hashes 208210 → 837998 (4.02x, limit 4.50x) |
| scale | struct-items fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | struct-items build | 4.19x | ≤ 4.5x | PASS | hashes 2104023 → 8817169 (4.19x, limit 4.50x) |
| scale | enum-items check | 4.00x | ≤ 4.5x | PASS | scope_probes 23500 → 94000 (4.00x, limit 4.50x) |
| scale | enum-items fmt | 4.00x | ≤ 4.5x | PASS | hashes 7576 → 30328 (4.00x, limit 4.50x) |
| scale | enum-items build | 4.10x | ≤ 4.5x | PASS | hashes 2516188 → 10319333 (4.10x, limit 4.50x) |
| scale | trait-items check | 4.01x | ≤ 4.5x | PASS | hashes 1061749 → 4256512 (4.01x, limit 4.50x) |
| scale | trait-items fmt | 4.00x | ≤ 4.5x | PASS | hashes 7576 → 30328 (4.00x, limit 4.50x) |
| scale | trait-items build | 4.32x | ≤ 4.5x | PASS | hashes 3176666 → 13719786 (4.32x, limit 4.50x) |
| scale | impl-methods check | 4.00x | ≤ 4.5x | PASS | scope_probes 10000 → 40000 (4.00x, limit 4.50x) |
| scale | impl-methods fmt | 3.97x | ≤ 4.5x | PASS | allocs 4045 → 16057 (3.97x, limit 4.50x) |
| scale | impl-methods build | 4.30x | ≤ 4.5x | PASS | hashes 1347225 → 5793394 (4.30x, limit 4.50x) |
| scale | impls-of-one-trait check | 4.00x | ≤ 4.5x | PASS | hashes 571934 → 2288790 (4.00x, limit 4.50x) |
| scale | impls-of-one-trait fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | impls-of-one-trait build | 4.10x | ≤ 4.5x | PASS | hashes 2305784 → 9463351 (4.10x, limit 4.50x) |
| scale | operator-impls check | 4.00x | ≤ 4.5x | PASS | hashes 690129 → 2762229 (4.00x, limit 4.50x) |
| scale | operator-impls fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | operator-impls build | 4.12x | ≤ 4.5x | PASS | hashes 2469984 → 10178555 (4.12x, limit 4.50x) |
| scale | derive-items check | 4.00x | ≤ 4.5x | PASS | item_body_checks 1000 → 4000 (4.00x, limit 4.50x) |
| scale | derive-items fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | derive-items build | 4.25x | ≤ 4.5x | PASS | hashes 59711364 → 253676039 (4.25x, limit 4.50x) |
| scale | dyn-values check | 4.00x | ≤ 4.5x | PASS | scope_probes 15000 → 60000 (4.00x, limit 4.50x) |
| scale | dyn-values fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | dyn-values build | 5.54x | ≤ 4.5x | PASS | known superlinear, #1908: allocs 1092257 → 6049570 (5.54x, limit 4.50x); hashes 4091114 → 20976797 (5.13x, limit 4.50x) |
| scale | consts check | 3.99x | ≤ 4.5x | PASS | hashes 63471 → 253003 (3.99x, limit 4.50x) |
| scale | consts fmt | 3.89x | ≤ 4.5x | PASS | allocs 2087 → 8109 (3.89x, limit 4.50x) |
| scale | consts build | 4.43x | ≤ 4.5x | PASS | hashes 1049384 → 4647135 (4.43x, limit 4.50x) |
| scale | local-items check | 4.00x | ≤ 4.5x | PASS | hashes 585074 → 2340862 (4.00x, limit 4.50x) |
| scale | local-items fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | local-items build | 4.06x | ≤ 4.5x | PASS | hashes 1939193 → 7866913 (4.06x, limit 4.50x) |
| scale | syntax-items check | 4.00x | ≤ 4.5x | PASS | item_body_checks 1500 → 6000 (4.00x, limit 4.50x) |
| scale | syntax-items fmt | 4.00x | ≤ 4.5x | PASS | hashes 17160 → 68664 (4.00x, limit 4.50x) |
| scale | syntax-items build | 4.00x | ≤ 4.5x | PASS | item_body_checks 1500 → 6000 (4.00x, limit 4.50x) |
| scale | struct-chain check | 4.06x | ≤ 4.5x | PASS | hashes 147499 → 598215 (4.06x, limit 4.50x) |
| scale | struct-chain fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | modules check | 4.00x | ≤ 4.5x | PASS | hashes 1177319 → 4713332 (4.00x, limit 4.50x) |
| scale | modules fmt | 4.00x | ≤ 4.5x | PASS | hashes 1500 → 6000 (4.00x, limit 4.50x) |
| scale | modules build | 4.06x | ≤ 4.5x | PASS | hashes 2058280 → 8346696 (4.06x, limit 4.50x) |
| scale | use-names check | 4.04x | ≤ 4.5x | PASS | hashes 222170 → 898018 (4.04x, limit 4.50x) |
| scale | use-names fmt | 4.01x | ≤ 4.5x | PASS | hashes 3783 → 15159 (4.01x, limit 4.50x) |
| scale | use-names build | 4.11x | ≤ 4.5x | PASS | hashes 1097651 → 4515264 (4.11x, limit 4.50x) |
| scale | generic-fns check | 4.03x | ≤ 4.5x | PASS | hashes 296667 → 1194691 (4.03x, limit 4.50x) |
| scale | generic-fns fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | generic-fns build | 4.05x | ≤ 4.5x | PASS | hashes 1078596 → 4366752 (4.05x, limit 4.50x) |
| scale | generic-params check | 4.23x | ≤ 4.5x | PASS | hashes 13354 → 56458 (4.23x, limit 4.50x) |
| scale | generic-params fmt | 4.10x | ≤ 4.5x | PASS | alloc_bytes 344142 → 1412604 (4.10x, limit 9.00x) |
| scale | generic-params build | 4.00x | ≤ 4.5x | PASS | scope_probes 800 → 3200 (4.00x, limit 4.50x) |
| scale | param-bounds check | 4.03x | ≤ 4.5x | PASS | hashes 55071 → 221707 (4.03x, limit 4.50x) |
| scale | param-bounds fmt | 4.04x | ≤ 4.5x | PASS | hashes 616 → 2488 (4.04x, limit 4.50x) |
| scale | param-bounds build | 4.00x | ≤ 4.5x | PASS | scope_probes 1500 → 6000 (4.00x, limit 4.50x) |
| scale | type-defaults check | 4.00x | ≤ 4.5x | PASS | scope_probes 33500 → 134000 (4.00x, limit 4.50x) |
| scale | type-defaults fmt | 4.00x | ≤ 4.5x | PASS | hashes 7576 → 30328 (4.00x, limit 4.50x) |
| scale | type-defaults build | 4.08x | ≤ 4.5x | PASS | hashes 3030156 → 12351889 (4.08x, limit 4.50x) |
| scale | row-chain check | 4.02x | ≤ 4.5x | PASS | hashes 224324 → 901640 (4.02x, limit 4.50x) |
| scale | row-chain fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | row-chain build | 4.01x | ≤ 4.5x | PASS | hashes 549230 → 2203784 (4.01x, limit 4.50x) |
| scale | row-traits check | 4.00x | ≤ 4.5x | PASS | scope_probes 2100 → 8400 (4.00x, limit 4.50x) |
| scale | row-traits fmt | 4.04x | ≤ 4.5x | PASS | hashes 616 → 2488 (4.04x, limit 4.50x) |
| scale | row-traits build | 4.00x | ≤ 4.5x | PASS | scope_probes 2100 → 8400 (4.00x, limit 4.50x) |
| scale | closures check | 4.00x | ≤ 4.5x | PASS | local_body_checks 1000 → 4000 (4.00x, limit 4.50x) |
| scale | closures fmt | 3.94x | ≤ 4.5x | PASS | allocs 3061 → 12073 (3.94x, limit 4.50x) |
| scale | closures build | 4.19x | ≤ 4.5x | PASS | hashes 1452072 → 6084932 (4.19x, limit 4.50x) |
| scale | let-statements check | 3.96x | ≤ 4.5x | PASS | allocs 25331 → 100426 (3.96x, limit 4.50x) |
| scale | let-statements fmt | 3.94x | ≤ 4.5x | PASS | allocs 2554 → 10066 (3.94x, limit 4.50x) |
| scale | let-statements build | 4.68x | ≤ 4.5x | FAIL | hashes 482336 → 2259720 (4.68x, limit 4.50x) |
| scale | matches check | 4.00x | ≤ 4.5x | PASS | decision_rows 6000 → 24000 (4.00x, limit 4.50x) |
| scale | matches fmt | 3.97x | ≤ 4.5x | PASS | allocs 6057 → 24069 (3.97x, limit 4.50x) |
| scale | matches build | 4.28x | ≤ 4.5x | PASS | hashes 3587191 → 15345539 (4.28x, limit 4.50x) |
| scale | enum-arms check | 4.00x | ≤ 4.5x | PASS | hashes 60656 → 242660 (4.00x, limit 4.50x) |
| scale | enum-arms fmt | 3.91x | ≤ 4.5x | PASS | allocs 1551 → 6065 (3.91x, limit 4.50x) |
| scale | enum-arms build | 11.14x | ≤ 4.5x | PASS | known superlinear, #1909: allocs 419741 → 4674055 (11.14x, limit 4.50x); hashes 498721 → 4987958 (10.00x, limit 4.50x) |
| scale | tuple-patterns check | 4.00x | ≤ 4.5x | PASS | decision_rows 1000 → 4000 (4.00x, limit 4.50x) |
| scale | tuple-patterns fmt | 3.95x | ≤ 4.5x | PASS | allocs 3054 → 12066 (3.95x, limit 4.50x) |
| scale | tuple-patterns build | 4.10x | ≤ 4.5x | PASS | hashes 877141 → 3599392 (4.10x, limit 4.50x) |
| scale | range-patterns check | 4.03x | ≤ 4.5x | PASS | hashes 248099 → 998619 (4.03x, limit 4.50x) |
| scale | range-patterns fmt | 4.01x | ≤ 4.5x | PASS | hashes 3784 → 15160 (4.01x, limit 4.50x) |
| scale | range-patterns build | 4.15x | ≤ 4.5x | PASS | hashes 1780522 → 7387325 (4.15x, limit 4.50x) |
| scale | wide-struct check | 4.00x | ≤ 4.5x | PASS | scope_probes 4500 → 18000 (4.00x, limit 4.50x) |
| scale | wide-struct fmt | 3.95x | ≤ 4.5x | PASS | allocs 2544 → 10056 (3.95x, limit 4.50x) |
| scale | wide-struct build | 4.23x | ≤ 4.5x | PASS | hashes 1330519 → 5625960 (4.23x, limit 4.50x) |
| scale | call-args check | 4.31x | ≤ 4.5x | PASS | hashes 4432 → 19120 (4.31x, limit 4.50x) |
| scale | call-args fmt | 4.10x | ≤ 4.5x | PASS | alloc_bytes 361569 → 1482933 (4.10x, limit 9.00x) |
| scale | call-args build | 4.13x | ≤ 4.5x | PASS | hashes 26321 → 108819 (4.13x, limit 4.50x) |
| scale | binary-chain check | 4.00x | ≤ 4.5x | PASS | hashes 2500 → 10000 (4.00x, limit 4.50x) |
| scale | binary-chain fmt | 3.78x | ≤ 4.5x | PASS | allocs 545 → 2060 (3.78x, limit 4.50x) |
| scale | binary-chain build | 4.14x | ≤ 4.5x | PASS | hashes 101278 → 419002 (4.14x, limit 4.50x) |
| scale | interpolation check | 4.00x | ≤ 4.5x | PASS | hashes 59034 → 236034 (4.00x, limit 4.50x) |
| scale | interpolation fmt | 4.27x | ≤ 4.5x | PASS | alloc_bytes 6453 → 27573 (4.27x, limit 9.00x) |
| scale | interpolation build | 4.40x | ≤ 4.5x | PASS | hashes 12051223 → 53046595 (4.40x, limit 4.50x) |
| scale | list-literal check | 4.00x | ≤ 4.5x | PASS | hashes 7392 → 29580 (4.00x, limit 4.50x) |
| scale | list-literal fmt | 3.82x | ≤ 4.5x | PASS | allocs 533 → 2038 (3.82x, limit 4.50x) |
| scale | list-literal build | 4.11x | ≤ 4.5x | PASS | hashes 77552 → 319126 (4.11x, limit 4.50x) |
| scale | method-chain check | 4.00x | ≤ 4.5x | PASS | hashes 14394 → 57582 (4.00x, limit 4.50x) |
| scale | method-chain fmt | 4.04x | ≤ 4.5x | PASS | alloc_bytes 838129 → 3386711 (4.04x, limit 9.00x) |
| scale | method-chain build | 4.09x | ≤ 4.5x | PASS | hashes 1648986 → 6737628 (4.09x, limit 4.50x) |
| scale | pipe-chain check | 4.00x | ≤ 4.5x | PASS | hashes 16513 → 66013 (4.00x, limit 4.50x) |
| scale | pipe-chain fmt | 3.77x | ≤ 4.5x | PASS | allocs 546 → 2056 (3.77x, limit 4.50x) |
| scale | pipe-chain build | 4.02x | ≤ 4.5x | PASS | hashes 107279 → 431704 (4.02x, limit 4.50x) |
| scale | question-chain check | 4.00x | ≤ 4.5x | PASS | hashes 44420 → 177608 (4.00x, limit 4.50x) |
| scale | question-chain fmt | 3.94x | ≤ 4.5x | PASS | allocs 1533 → 6044 (3.94x, limit 4.50x) |
| scale | question-chain build | 3.99x | ≤ 4.5x | PASS | use_facts 24570 → 98070 (3.99x, limit 4.50x) |
| scale | if-nest check | 3.94x | ≤ 4.5x per 4x bytes (12.1x) | PASS | hashes 849 → 3345 (3.94x, limit 13.59x) |
| scale | if-nest fmt | 5.30x | ≤ 4.5x per 4x bytes (12.1x) | PASS | alloc_bytes 144371 → 765724 (5.30x, limit 27.19x) |
| scale | if-nest build | 4.00x | ≤ 4.5x per 4x bytes (12.1x) | PASS | use_facts 768 → 3072 (4.00x, limit 13.59x) |
| scale | block-nest check | 3.95x | ≤ 4.5x per 4x bytes (12.2x) | PASS | hashes 910 → 3598 (3.95x, limit 13.76x) |
| scale | block-nest fmt | 5.69x | ≤ 4.5x per 4x bytes (12.2x) | PASS | alloc_bytes 160538 → 913156 (5.69x, limit 27.51x) |
| scale | block-nest build | 3.95x | ≤ 4.5x per 4x bytes (12.2x) | PASS | use_facts 878 → 3470 (3.95x, limit 13.76x) |
| scale | for-nest check | 5.21x | ≤ 4.5x per 4x bytes (11.6x) | PASS | hashes 3561 → 18537 (5.21x, limit 13.05x) |
| scale | for-nest fmt | 5.39x | ≤ 4.5x per 4x bytes (11.6x) | PASS | alloc_bytes 139001 → 749031 (5.39x, limit 26.11x) |
| scale | for-nest build | 9.41x | ≤ 4.5x per 4x bytes (11.6x) | PASS | hashes 83964 → 790434 (9.41x, limit 13.05x) |
| scale | while-nest check | 3.90x | ≤ 4.5x per 4x bytes (12.1x) | PASS | hashes 1126 → 4390 (3.90x, limit 13.59x) |
| scale | while-nest fmt | 5.31x | ≤ 4.5x per 4x bytes (12.1x) | PASS | alloc_bytes 143075 → 760300 (5.31x, limit 27.19x) |
| scale | while-nest build | 4.00x | ≤ 4.5x per 4x bytes (12.1x) | PASS | use_facts 1184 → 4736 (4.00x, limit 13.59x) |
| scale | match-nest check | 4.76x | ≤ 4.5x per 4x bytes (12.8x) | PASS | hashes 2032 → 9676 (4.76x, limit 14.43x) |
| scale | match-nest fmt | 5.82x | ≤ 4.5x per 4x bytes (12.8x) | PASS | alloc_bytes 158237 → 920971 (5.82x, limit 28.86x) |
| scale | match-nest build | 4.23x | ≤ 4.5x per 4x bytes (12.8x) | PASS | hashes 13594 → 57563 (4.23x, limit 14.43x) |
| scale | closure-nest check | 3.98x | ≤ 4.5x per 4x bytes (9.2x) | PASS | hashes 1998 → 7962 (3.98x, limit 10.31x) |
| scale | closure-nest fmt | 5.54x | ≤ 4.5x per 4x bytes (9.2x) | PASS | alloc_bytes 74245 → 410949 (5.54x, limit 20.61x) |
| scale | closure-nest build | 4.01x | ≤ 4.5x per 4x bytes (9.2x) | PASS | use_facts 479 → 1919 (4.01x, limit 10.31x) |
| scale | option-nest check | 4.18x | ≤ 4.5x | PASS | hashes 1879 → 7849 (4.18x, limit 4.50x) |
| scale | option-nest fmt | 6.24x | ≤ 4.5x | PASS | alloc_bytes 73101 → 456308 (6.24x, limit 9.00x) |
| scale | option-nest build | 4.15x | ≤ 4.5x | PASS | hashes 12199 → 50572 (4.15x, limit 4.50x) |
| scale | list-type-nest check | 4.27x | ≤ 4.5x | PASS | hashes 1269 → 5417 (4.27x, limit 4.50x) |
| scale | list-type-nest fmt | 6.05x | ≤ 4.5x | PASS | alloc_bytes 55328 → 334852 (6.05x, limit 9.00x) |
| scale | list-type-nest build | 4.32x | ≤ 4.5x | PASS | hashes 12349 → 53310 (4.32x, limit 4.50x) |
| scale | generic-nest check | 4.30x | ≤ 4.5x | PASS | hashes 1589 → 6825 (4.30x, limit 4.50x) |
| scale | generic-nest fmt | 7.40x | ≤ 4.5x | PASS | alloc_bytes 10894 → 80601 (7.40x, limit 9.00x) |
| scale | generic-nest build | 4.03x | ≤ 4.5x | PASS | allocs 4981 → 20091 (4.03x, limit 4.50x) |
| scale | paren-nest check | 4.45x | ≤ 4.5x | PASS | alloc_bytes 38264 → 170258 (4.45x, limit 9.00x) |
| scale | paren-nest fmt | 3.21x | ≤ 4.5x | PASS | alloc_bytes 34650 → 111359 (3.21x, limit 9.00x) |
| scale | paren-nest build | 4.07x | ≤ 4.5x | PASS | control 859 → 3499 (4.07x, limit 4.50x) |
| scale | arith-nest check | 4.00x | ≤ 4.5x | PASS | hashes 440 → 1760 (4.00x, limit 4.50x) |
| scale | arith-nest fmt | 2.94x | ≤ 4.5x | PASS | alloc_bytes 76966 → 226347 (2.94x, limit 9.00x) |
| scale | arith-nest build | 4.50x | ≤ 4.5x | FAIL | hashes 20689 → 93173 (4.50x, limit 4.50x) |
| scale | bool-nest check | 3.97x | ≤ 4.5x | PASS | hashes 388 → 1540 (3.97x, limit 4.50x) |
| scale | bool-nest fmt | 5.11x | ≤ 4.5x | PASS | alloc_bytes 37604 → 191986 (5.11x, limit 9.00x) |
| scale | bool-nest build | 4.09x | ≤ 4.5x | PASS | use_facts 474 → 1938 (4.09x, limit 4.50x) |
| scale | bit-nest check | 3.99x | ≤ 4.5x | PASS | hashes 818 → 3266 (3.99x, limit 4.50x) |
| scale | bit-nest fmt | 2.91x | ≤ 4.5x | PASS | alloc_bytes 163583 → 476792 (2.91x, limit 9.00x) |
| scale | bit-nest build | 4.53x | ≤ 4.5x | FAIL | hashes 21258 → 96205 (4.53x, limit 4.50x) |
| scale | tuple-nest check | 4.07x | ≤ 4.5x | PASS | hashes 1957 → 7963 (4.07x, limit 4.50x) |
| scale | tuple-nest fmt | 5.67x | ≤ 4.5x | PASS | alloc_bytes 122064 → 692688 (5.67x, limit 9.00x) |
| scale | tuple-nest build | 6.95x | ≤ 4.5x | PASS | accepted superlinear, bounded by the parser's MAX_NESTING = 160 (owner 2026-11-15, #1924): +37 MB allocated at depth 150, no measured time: hashes 45179 → 313830 (6.95x, limit 4.50x); control 431 → 2384 (5.53x, limit 4.50x) |
| scale | if-expr-nest check | 3.76x | ≤ 4.5x | PASS | hashes 322 → 1210 (3.76x, limit 4.50x) |
| scale | if-expr-nest fmt | 4.51x | ≤ 4.5x | PASS | alloc_bytes 68686 → 309984 (4.51x, limit 9.00x) |
| scale | if-expr-nest build | 3.82x | ≤ 4.5x | PASS | use_facts 341 → 1301 (3.82x, limit 4.50x) |
| scale | call-nest check | 3.95x | ≤ 4.5x | PASS | hashes 581 → 2297 (3.95x, limit 4.50x) |
| scale | call-nest fmt | 6.40x | ≤ 4.5x | PASS | alloc_bytes 33324 → 213300 (6.40x, limit 9.00x) |
| scale | call-nest build | 2.89x | ≤ 4.5x | PASS | allocs 1702 → 4926 (2.89x, limit 4.50x) |
| scale | index-nest check | 4.50x | ≤ 4.5x | PASS | alloc_bytes 32492 → 146124 (4.50x, limit 9.00x) |
| scale | index-nest fmt | 7.56x | ≤ 4.5x | PASS | alloc_bytes 23956 → 181114 (7.56x, limit 9.00x) |
| scale | index-nest build | 3.98x | ≤ 4.5x | PASS | control 1384 → 5512 (3.98x, limit 4.50x) |
| scale | fn-nest check | 3.95x | ≤ 4.5x per 4x bytes (10.7x) | PASS | hashes 2689 → 10621 (3.95x, limit 12.08x) |
| scale | fn-nest fmt | 5.63x | ≤ 4.5x per 4x bytes (10.7x) | PASS | alloc_bytes 132556 → 746520 (5.63x, limit 24.16x) |
| scale | fn-nest build | 4.02x | ≤ 4.5x per 4x bytes (10.7x) | PASS | hashes 12535 → 50433 (4.02x, limit 12.08x) |
| scale | module-path-nest check | 1.95x | ≤ 4.5x | PASS | allocs 1540 → 3003 (1.95x, limit 4.50x) |
| scale | module-path-nest fmt | 7.90x | ≤ 4.5x | PASS | alloc_bytes 16030 → 126570 (7.90x, limit 9.00x) |
| scale | module-path-nest build | 1.85x | ≤ 4.5x | PASS | allocs 1567 → 2902 (1.85x, limit 4.50x) |
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
| fmt | hd fmt, 10k lines | 24 ms | ≤ 200 ms | PASS |  |
| fmt | second hd fmt changes nothing | no change | idempotent | PASS |  |
| check-cost | checked run time / unchecked run time | 1.97x | ≤ 1.30x | FAIL | checked 848 ms, unchecked 431 ms |
| dev-speed | geomean dev / optimized | 1.46x | ≤ 4.00x | PASS | fib 0.89x, sum 4.12x, string-build 1.24x, map 1.00x, sort 1.46x |
| dev-speed | worst case dev / optimized | 4.12x | ≤ 10.00x | PASS | sum |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | 40 ms | ≤ 200 ms | PASS |  |
| resources | warm check, 10k lines: peak RSS | 26.2 MB | ≤ 50.0 MB | PASS |  |
| resources | check, small, cold | 0.19 CPU-s, 36.9 MB | completes ≤ 30 s | PASS | wall 121 ms |
| resources | check, small, warm | 0.01 CPU-s, 18.8 MB | completes ≤ 30 s | PASS | wall 19 ms |
| resources | test, small, cold | 1.65 CPU-s, 92.2 MB | completes ≤ 30 s | PASS | wall 681 ms |
| resources | test, small, warm | 0.61 CPU-s, 66.6 MB | completes ≤ 30 s | PASS | wall 258 ms |
| resources | build, small, cold | 0.97 CPU-s, 85.7 MB | completes ≤ 30 s | PASS | wall 375 ms |
| resources | build, small, warm | 0.03 CPU-s, 31.7 MB | completes ≤ 30 s | PASS | wall 36 ms |
| resources | check, 10k, cold | 0.54 CPU-s, 58.6 MB | completes ≤ 30 s | PASS | wall 232 ms |
| resources | check, 10k, warm | 0.04 CPU-s, 26.2 MB | completes ≤ 30 s | PASS | wall 42 ms |
| resources | test, 10k, cold | 2.61 CPU-s, 126.5 MB | completes ≤ 30 s | PASS | wall 1206 ms |
| resources | test, 10k, warm | 0.87 CPU-s, 74.7 MB | completes ≤ 30 s | PASS | wall 357 ms |
| resources | build, 10k, cold | 1.35 CPU-s, 108.0 MB | completes ≤ 30 s | PASS | wall 459 ms |
| resources | build, 10k, warm | 0.06 CPU-s, 35.2 MB | completes ≤ 30 s | PASS | wall 56 ms |
| resources | check, 50k, cold | 2.00 CPU-s, 163.5 MB | completes ≤ 150 s | PASS | wall 723 ms |
| resources | check, 50k, warm | 0.14 CPU-s, 56.5 MB | completes ≤ 150 s | PASS | wall 135 ms |
| resources | test, 50k, cold | 7.71 CPU-s, 282.4 MB | completes ≤ 150 s | PASS | wall 4052 ms |
| resources | test, 50k, warm | 2.71 CPU-s, 94.2 MB | completes ≤ 150 s | PASS | wall 1008 ms |
| resources | build, 50k, cold | 2.92 CPU-s, 210.1 MB | completes ≤ 150 s | PASS | wall 957 ms |
| resources | build, 50k, warm | 0.17 CPU-s, 65.3 MB | completes ≤ 150 s | PASS | wall 151 ms |
| long-session | RSS growth, REPL inputs 100 to 1000 | 96.6 MB | ≤ 22.0 MB | FAIL | RSS 101.2 MB at input 100, 197.5 MB at input 1000; build per input 92 ms at 50, 1070 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 3.0 ms | ≤ 20 ms | PASS | Timing change ≤10%: noise. |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 4.1 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 16 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 17.5 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | 1.38x | ≤ 1.50x | PASS | p95 120 ms at N = 1; 4 cores; total CPU 340 ms |
| concurrency | p95 latency at N = 16 vs N = 1 | 2.30x | ≤ 6.00x | PASS | p95 120 ms at N = 1; 4 cores; total CPU 620 ms |
| concurrency | total CPU growth exponent, N = 1 to 16 | 0.41 | &lt; 1.00 (sublinear) | PASS | 200 ms at N = 1, 620 ms at N = 16 |
| disk | artifacts plus toolchain per worktree | 22.0 MB | ≤ 10.0 MB | FAIL | artifacts 8045 B in 10 files; toolchain 22.0 MB; toolchain files: /home/runner/work/hd-lang/hd-lang/target/release/hd; HD_CACHE 10.8 MB, shared per user, not counted |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.91x | ≥ 2.40x | FAIL | by --jobs |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | PASS |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 201 files | the files of one check's cache | FAIL | one check alone: 35 files |
| cache-growth | cache size after 100 edits, cap 5 MB | 14.1 MB | ≤ 5.0 MB | FAIL | cap set by HD_CACHE_MAX_SIZE |
| io-per-check | source files read, warm check after one edit | 79 | ≤ 1 | FAIL | 80 stat'ed; 79 source files; edited src/m039.hd |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | 29k/s | ≥ 100k/s | FAIL | 6000 extra cases took 209 ms; hd test, median of 3 |
| proptest-perf | time to shrink a known failure | 110 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3 |
| unit-test-perf | 1,000 unit tests, warm | 945 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.8 ms | ≤ 1.0 ms | PASS | 100 tests: 165 ms |
| integration-test-perf | setup per integration test program | 97 ms | ≤ 20 ms | FAIL | 10 programs: 1049 ms for 21 tests, 50 ms per test |
| integration-test-perf | total time growth in programs | 1.006 | exponent &lt; 1 (sublinear) | FAIL | beyond the 0-program run: 191 ms at 2, 964 ms at 10 programs |
| integration-test-perf | cost per doc test | 91 ms | ≤ 20 ms | FAIL | 10 doc tests: 1019 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 0.90x | ≤ 1.50x | PASS | 5 cases |
| runtime | fib: hd / Node | 0.77x | ≤ 3.00x | PASS | hd 6.6 ms (p10 4.5, p90 9.0), Node 8.5 ms, Python 86 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 0.45x | ≤ 3.00x | PASS | hd 26 ms (p10 24, p90 33), Node 57 ms, Python 288 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 0.89x | ≤ 3.00x | PASS | hd 7.8 ms (p10 5.0, p90 16), Node 8.8 ms, Python 12 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 3.24x | ≤ 3.00x | FAIL | hd 26 ms (p10 23, p90 34), Node 8.0 ms, Python 16 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 0.60x | ≤ 3.00x | PASS | hd 14 ms (p10 8.9, p90 19), Node 24 ms, Python 17 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | list loop, bytes per iteration | 0.01 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | map loop, bytes per iteration | 0.01 B/iter | 0 allocations (&lt; 1 B/iter) | PASS | release build, run as hd FILE.wasm |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | PASS | release build, run as hd FILE.wasm |
| size-startup-heap | tiny program, release Wasm size | 5024 B | ≤ 2048 B | FAIL | benchmarks: fib 5153 B, sum 4929 B, string-build 7809 B, map 7056 B, sort 12679 B |
| size-startup-heap | tiny program, start to first output | 48 ms | ≤ 5.0 ms | FAIL | release build, run as hd FILE.wasm |
| size-startup-heap | peak heap, map: hd / Node | 1.09x | ≤ 2.00x | PASS | hd +15.4 MB, Node +14.1 MB over an empty program |
| size-startup-heap | peak heap, string-build: hd / Node | 0.44x | ≤ 2.00x | PASS | hd +8.7 MB, Node +20.0 MB over an empty program |
| host-call-overhead | Console println, per call | 3.8 µs | ≤ 1.0 µs | FAIL | 100000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | Fs read_text!, per call | 17 µs | ≤ 10 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| host-call-overhead | serde boundary (list_dir!), per call | 33 µs | ≤ 5.0 µs | FAIL | 10000 calls; release build, run as hd FILE.wasm |
| suspension-overhead | await of a ready value, per await | 0.00 ns | ≤ 100 ns | PASS | -9.4 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | all! of two ready tasks, throughput | 12.1M/s | ≥ 1.0M/s | PASS | 166 ns per iteration; release build, run as hd FILE.wasm |
| suspension-overhead | race! of two ready tasks, throughput | 8.8M/s | ≥ 1.0M/s | PASS | 227 ns per iteration; release build, run as hd FILE.wasm |
| serde-throughput | JSON encode, hd / Node MB/s | 0.10x | ≥ 0.50x | FAIL | hd 54 MB/s (45488 B document), Node 534 MB/s; release build, run as hd FILE.wasm |
| serde-throughput | JSON decode, hd / Node MB/s | 0.20x | ≥ 0.50x | FAIL | hd 35 MB/s (45488 B document), Node 175 MB/s; release build, run as hd FILE.wasm |
| text-throughput | string build, hd / Node MB/s | 2.88x | ≥ 0.50x | PASS | hd 355 MB/s, Node 123 MB/s; 100000 words, 1088890 bytes per text; hd StringBuilder, Node Array.join; release build, run as hd FILE.wasm |
| text-throughput | split, hd / Node MB/s | 1.71x | ≥ 0.50x | PASS | hd 610 MB/s, Node 357 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| text-throughput | regex find_all, hd / Node MB/s | 0.08x | ≥ 0.50x | FAIL | hd 7.5 MB/s, Node 90 MB/s; 100000 words, 1088890 bytes per text; release build, run as hd FILE.wasm |
| dead-code | Wasm bytes per 1,000 lines | 6431 B | ≤ 10240 B | PASS | 12516 B at 1075 lines, 70405 B at 10076 lines; release build, run as hd FILE.wasm |
| dead-code | no unused std in the program | none | none | PASS | 915 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | 2.7 MB | ≤ 10.0 MB | PASS | 22114000 requests; RSS 67.4 MB after warm-up, 70.1 MB at the end; release build, run as hd FILE.wasm |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 7747 of 7747 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | PASS | 20 of 20 runs matched over 16 edits |

## Recent runs

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
- [2026-11-02T03](daily/2026-11-02T03.json)
