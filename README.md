# Daily goal metrics

Finished: 2026-10-25T12:05:45.747Z. Commit: 5b66837062f523b5debda0409af557ec6dcadafc.
Machine: ubuntu-latest, 4 CPUs (AMD EPYC 9V74 80-Core Processor).
Compiler: target/release/hd.

34 passed, 94 failed, 2 n/a.

Timing movements of 10% or less are flagged as noise, not regressions.

## Changed since the previous run

First recorded run.

## Pillar 1

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | - | ≤ 50 ms | FAIL | the warm-up check exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' &#124; error: src/m001.hd:6:5: syntax-error: expected a declaration, found 'data' |
| edit-latency | p95 check after an edit | - | ≤ 200 ms | FAIL | the warm-up check exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' &#124; error: src/m001.hd:6:5: syntax-error: expected a declaration, found 'data' |
| cold-check | median cold check, 10k lines | - | ≤ 1000 ms | FAIL | cold check 1 exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' &#124; error: src/m001.hd:6:5: syntax-error: expected a declaration, found 'data' |
| test-latency | median edit-then-test | - | ≤ 300 ms | FAIL | the warm-up test exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' |
| mistakes | programs with exactly one diagnostic | 87.5% | ≥ 95.0% | FAIL | 63 of 72 |
| mistakes | one diagnostic with the expected code | 66.7% | ≥ 95.0% | FAIL | 48 of 72; first misses: assert-without-import, bang-call-in-plain-fn, comma-row-separator, data-call-syntax, debug-as-statement, length-as-i32, let-mut-on-readonly-type, let-mut-primitive |
| mistakes | fix-it makes hd check pass | 26.4% | ≥ 80.0% | FAIL | 19 of 72 resolved; 19 offered a fix-it |
| mistakes | largest diagnostic text (bytes / 4) | 91 tokens | ≤ 60 tokens | FAIL | largest: some-on-result; median 40 tokens, 159 B; 67 of 72 within |
| answer-size | hd doc of a data type | - | ≤ 1000 B | FAIL | hd doc failed: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' &#124; error: src/m001.hd:6:5: syntax-error: expected a declaration, found 'data' |
| answer-size | one failing hd test, text | 75 B | ≤ 800 B | PASS |  |
| answer-size | one failing test, JSON object | - | ≤ 400 B | FAIL | no failed test object |
| answer-size | one diagnostic, JSON object | 168 B | ≤ 600 B | PASS |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | PASS |  |
| pathological | overlapping-from: time at 4x | - | ≤ 2000 ms | FAIL | 1x: hd check failed: error: overlapping_from.hd:3:1: syntax-error: expected an expression, found 'data' &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | overlapping-from: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 1x: hd check failed: error: overlapping_from.hd:3:1: syntax-error: expected an expression, found 'data' &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | overlapping-from: growth exponent | - | ≤ 1.3 | FAIL | 1x: hd check failed: error: overlapping_from.hd:3:1: syntax-error: expected an expression, found 'data' &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | deep-blocks: time at 4x | - | ≤ 2000 ms | FAIL | 4x: hd check failed: error: deep_blocks.hd:161:324: nesting-too-deep: source nests deeper than the implementation limit &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | deep-blocks: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 4x: hd check failed: error: deep_blocks.hd:161:324: nesting-too-deep: source nests deeper than the implementation limit &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | deep-blocks: growth exponent | noise | ≤ 1.3 | PASS | 1x→2x; 14 → 14 ms, start-up 14 ms |
| pathological | deep-parens: time at 4x | - | ≤ 2000 ms | FAIL | 1x: hd check failed: error: deep_parens.hd:2:164: nesting-too-deep: source nests deeper than the implementation limit &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | deep-parens: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 1x: hd check failed: error: deep_parens.hd:2:164: nesting-too-deep: source nests deeper than the implementation limit &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | deep-parens: growth exponent | - | ≤ 1.3 | FAIL | 1x: hd check failed: error: deep_parens.hd:2:164: nesting-too-deep: source nests deeper than the implementation limit &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | method-chain: time at 4x | - | ≤ 2000 ms | FAIL | 4x: hd check failed: thread '&lt;unknown>' (5974) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 4x: hd check failed: thread '&lt;unknown>' (5974) has overflowed its stack &#124; fatal runtime error: stack overflow, aborting |
| pathological | method-chain: growth exponent | noise | ≤ 1.3 | PASS | 1x→2x; 15 → 16 ms, start-up 14 ms |
| pathological | iterator-chain: time at 4x | - | ≤ 2000 ms | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | iterator-chain: growth exponent | - | ≤ 1.3 | FAIL | 2x: hd check failed: error: iterator_chain.hd:2:5: unsupported: stage Body: solver answer Overflow to a method lookup is not supported &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | wide-list-literal: time at 4x | 45 ms | ≤ 2000 ms | PASS |  |
| pathological | wide-list-literal: peak RSS at 4x | 19.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | wide-list-literal: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 28 → 45 ms, start-up 14 ms |
| pathological | large-enum-match: time at 4x | 374 ms | ≤ 2000 ms | PASS |  |
| pathological | large-enum-match: peak RSS at 4x | 18.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | large-enum-match: growth exponent | 2.26 | ≤ 1.30 | FAIL | 2x→4x; 89 → 374 ms, start-up 14 ms |
| pathological | deep-generic: time at 4x | - | ≤ 2000 ms | FAIL | 1x: hd check failed: error: deep_generic.hd:1:1: syntax-error: expected an expression, found 'data' &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | deep-generic: peak RSS at 4x | - | ≤ 200.0 MB | FAIL | 1x: hd check failed: error: deep_generic.hd:1:1: syntax-error: expected an expression, found 'data' &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | deep-generic: growth exponent | - | ≤ 1.3 | FAIL | 1x: hd check failed: error: deep_generic.hd:1:1: syntax-error: expected an expression, found 'data' &#124; check result: FAILED. errors: 1; warnings: 0 |
| pathological | question-chain: time at 4x | 21 ms | ≤ 2000 ms | PASS |  |
| pathological | question-chain: peak RSS at 4x | 18.4 MB | ≤ 200.0 MB | PASS |  |
| pathological | question-chain: growth exponent | noise | ≤ 1.3 | PASS | 2x→4x; 17 → 21 ms, start-up 14 ms |
| pathological | big-file: time at 4x | 132 ms | ≤ 2000 ms | PASS |  |
| pathological | big-file: peak RSS at 4x | 50.7 MB | ≤ 200.0 MB | PASS |  |
| pathological | big-file: growth exponent | 1.07 | ≤ 1.30 | PASS | 2x→4x; 70 → 132 ms, start-up 14 ms |
| recheck-precision | modules rechecked, private body edit | - | ≤ 1 | FAIL | the warm-up check exited 101: {"kind":"diagnostic","code":"syntax-error","severity":"error","message":"expected a declaration, found 'data'","file":"src/m000.hd","line":5,"column":5,"fixes":[]} &#124; {"kind":"diagnostic","code":"syntax-error","severity":"error","message":"expected a declaration, found 'data'","file":"src/m001.hd","l |
| recheck-precision | modules rechecked, public signature edit | - | ≤ 1 + dependents | FAIL | the warm-up check exited 101: {"kind":"diagnostic","code":"syntax-error","severity":"error","message":"expected a declaration, found 'data'","file":"src/m000.hd","line":5,"column":5,"fixes":[]} &#124; {"kind":"diagnostic","code":"syntax-error","severity":"error","message":"expected a declaration, found 'data'","file":"src/m001.hd","l |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | PASS | 0 reported twice or more; 0 on other lines |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | FAIL | 0 reported twice or more; 0 on other lines |
| diag-location | diagnostics on the mistake's line | 76.4% | ≥ 95.0% | FAIL | 55 of 72; misses: assert-without-import@none, bang-call-in-plain-fn@8, data-call-syntax@6, debug-as-statement@6, length-as-i32@6, let-mut-on-readonly-type@none |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | PASS | 19 of 19 safe |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | N/A | not supported by this hd: no hd callers |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | N/A | not supported by this hd: no hd needs |
| fmt | hd fmt, 10k lines | - | ≤ 200 ms | FAIL | hd fmt exited 101: error: src/m000.hd:5:5: syntax-error: the file does not parse, so `hd fmt` leaves it as it is &#124; error: src/m001.hd:6:5: syntax-error: the file does not parse, so `hd fmt` leaves it as it is |
| fmt | second hd fmt changes nothing | - | idempotent | FAIL | hd fmt exited 101: error: src/m000.hd:5:5: syntax-error: the file does not parse, so `hd fmt` leaves it as it is &#124; error: src/m001.hd:6:5: syntax-error: the file does not parse, so `hd fmt` leaves it as it is |
| check-cost | checked run time / unchecked run time | - | ≤ 1.30x | FAIL | hd test --release (100 rounds) exited 101: error: src/main.hd:4:17: mut-on-struct: List[i64] has no `mut` form &#124; error: src/main.hd:6:9: not-writable: this call changes `values` in place, but `values` is not `let mut`; bind it with `let mut` |
| dev-speed | geomean dev / optimized | - | ≤ 4.00x | FAIL | optimized work is within noise: fib (2.2 ms), sum (19.4 ms), string-build (23.2 ms), map (13.8 ms), sort (29.9 ms) |
| dev-speed | worst case dev / optimized | - | ≤ 10.00x | FAIL | optimized work is within noise: fib (2.2 ms), sum (19.4 ms), string-build (23.2 ms), map (13.8 ms), sort (29.9 ms) |

## Pillar 2

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| resources | warm check, 10k lines: CPU time | - | ≤ 200 ms | FAIL | the cold run failed: cold hd check exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' &#124; error: src/m001.hd:6:5: syntax-error: expected a declaration, found 'data' |
| resources | warm check, 10k lines: peak RSS | - | ≤ 50.0 MB | FAIL | the cold run failed: cold hd check exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' &#124; error: src/m001.hd:6:5: syntax-error: expected a declaration, found 'data' |
| resources | check, small, cold | - | completes ≤ 30 s | FAIL | cold hd check exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' &#124; error: src/m001.hd:6:5: syntax-error: expected a declaration, found 'data' |
| resources | check, small, warm | - | completes ≤ 30 s | FAIL | skipped: the cold run failed |
| resources | test, small, cold | - | completes ≤ 30 s | FAIL | cold hd test exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' |
| resources | test, small, warm | - | completes ≤ 30 s | FAIL | skipped: the cold run failed |
| resources | build, small, cold | - | completes ≤ 30 s | FAIL | cold hd build exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' |
| resources | build, small, warm | - | completes ≤ 30 s | FAIL | skipped: the cold run failed |
| resources | check, 10k, cold | - | completes ≤ 30 s | FAIL | cold hd check exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' &#124; error: src/m001.hd:6:5: syntax-error: expected a declaration, found 'data' |
| resources | check, 10k, warm | - | completes ≤ 30 s | FAIL | skipped: the cold run failed |
| resources | test, 10k, cold | - | completes ≤ 30 s | FAIL | cold hd test exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' |
| resources | test, 10k, warm | - | completes ≤ 30 s | FAIL | skipped: the cold run failed |
| resources | build, 10k, cold | - | completes ≤ 30 s | FAIL | cold hd build exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' |
| resources | build, 10k, warm | - | completes ≤ 30 s | FAIL | skipped: the cold run failed |
| resources | check, 50k, cold | - | completes ≤ 150 s | FAIL | cold hd check exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' &#124; error: src/m001.hd:6:5: syntax-error: expected a declaration, found 'data' |
| resources | check, 50k, warm | - | completes ≤ 150 s | FAIL | skipped: the cold run failed |
| resources | test, 50k, cold | - | completes ≤ 150 s | FAIL | cold hd test exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' |
| resources | test, 50k, warm | - | completes ≤ 150 s | FAIL | skipped: the cold run failed |
| resources | build, 50k, cold | - | completes ≤ 150 s | FAIL | cold hd build exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' |
| resources | build, 50k, warm | - | completes ≤ 150 s | FAIL | skipped: the cold run failed |
| long-session | RSS growth, REPL inputs 100 to 1000 | 371.1 MB | ≤ 22.0 MB | FAIL | RSS 142.0 MB at input 100, 513.1 MB at input 1000; build per input 42 ms at 50, 552 ms at 1000; 4 KiB pages |
| startup | hd help (no version command): wall time | 2.7 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd help (no version command): peak RSS | 3.6 MB | ≤ 10.0 MB | PASS |  |
| startup | hd check empty.hd: wall time | 10 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: CPU time | 0.0 ms | ≤ 20 ms | PASS |  |
| startup | hd check empty.hd: peak RSS | 12.5 MB | ≤ 10.0 MB | FAIL |  |
| concurrency | p95 latency at N = 4 vs N = 1 | - | ≤ 1.50x | FAIL | check 1 of 1 exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' &#124; error: src/m001.hd:6:5: syntax-error: expected a declaration, found 'data' |
| concurrency | p95 latency at N = 16 vs N = 1 | - | ≤ 6.00x | FAIL | check 1 of 1 exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' &#124; error: src/m001.hd:6:5: syntax-error: expected a declaration, found 'data' |
| concurrency | total CPU growth exponent, N = 1 to 16 | - | &lt; 1.00 | FAIL | check 1 of 1 exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' &#124; error: src/m001.hd:6:5: syntax-error: expected a declaration, found 'data' |
| disk | artifacts plus toolchain per worktree | - | ≤ 10.0 MB | FAIL | hd check exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' &#124; error: src/m001.hd:6:5: syntax-error: expected a declaration, found 'data' |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | - | ≥ 2.40x | FAIL | the 1-thread check of 50k exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' &#124; error: src/m001.hd:6:5: syntax-error: expected a declaration, found 'data' |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | PASS |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | FAIL | connections to the remote |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | - | no corruption | FAIL | the cache probe's hd check exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' &#124; error: src/m001.hd:6:5: syntax-error: expected a declaration, found 'data' |
| cache-contention | compilation cache, 8 concurrent checks: written once | - | computed once | FAIL | the cache probe's hd check exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' &#124; error: src/m001.hd:6:5: syntax-error: expected a declaration, found 'data' |
| cache-growth | cache size after 100 edits, cap 5 MB | - | ≤ 5.0 MB | FAIL | the cache probe's hd check exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' &#124; error: src/m001.hd:6:5: syntax-error: expected a declaration, found 'data' |
| io-per-check | source files read, warm check after one edit | - | ≤ 1 | FAIL | the cache probe's hd check exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' &#124; error: src/m001.hd:6:5: syntax-error: expected a declaration, found 'data' |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | PASS | the first worktree's: 2 |

## Pillar 3

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| proptest-perf | property cases per second | - | ≥ 100k/s | FAIL | the property suite (package 1, hd test --seed 7) exited 101: error: src/lib.hd:4:1: syntax-error: expected a declaration, found 'data' &#124; error: src/lib.hd:16:1: syntax-error: expected a declaration, found 'data' |
| proptest-perf | time to shrink a known failure | 0.0 ms | ≤ 1000 ms | PASS | shrink=500 minus shrink=0, median of 3; the failure did not name the property |
| unit-test-perf | 1,000 unit tests, warm | 848 ms | ≤ 1000 ms | PASS | the run reports 1000 of 1000 passed |
| unit-test-perf | overhead per unit test | 0.7 ms | ≤ 1.0 ms | PASS | 100 tests: 184 ms |
| integration-test-perf | setup per integration test program | - | ≤ 20 ms | FAIL | the integration tests (package 2, hd test) exited 101: error: tests/export0.hd:6:1: missing-capability: test `program 0 writes and reads export 0` requires `FsRead`, but neither `[capabilities]` nor `[dev-capabilities]` in hd.toml lists it &#124;     fix: list the missing capabilities in hd.toml |
| integration-test-perf | total time growth in programs | - | exponent &lt; 1 (sublinear) | FAIL | the integration tests (package 2, hd test) exited 101: error: tests/export0.hd:6:1: missing-capability: test `program 0 writes and reads export 0` requires `FsRead`, but neither `[capabilities]` nor `[dev-capabilities]` in hd.toml lists it &#124;     fix: list the missing capabilities in hd.toml |
| integration-test-perf | cost per doc test | 90 ms | ≤ 20 ms | FAIL | 10 doc tests: 990 ms; the run reports 11 of 11 passed |
| runtime | geomean hd / Node | 2.46x | ≤ 1.50x | FAIL | 5 cases |
| runtime | fib: hd / Node | 2.23x | ≤ 3.00x | PASS | hd 19 ms (p10 -1.3, p90 27), Node 8.5 ms, Python 89 ms; release build, run as hd FILE.wasm |
| runtime | sum: hd / Node | 2.93x | ≤ 3.00x | PASS | hd 24 ms (p10 21, p90 26), Node 8.2 ms, Python 275 ms; release build, run as hd FILE.wasm |
| runtime | string-build: hd / Node | 3.06x | ≤ 3.00x | FAIL | hd 27 ms (p10 22, p90 32), Node 8.9 ms, Python 12 ms; release build, run as hd FILE.wasm |
| runtime | map: hd / Node | 3.00x | ≤ 3.00x | FAIL | hd 24 ms (p10 22, p90 28), Node 8.1 ms, Python 14 ms; release build, run as hd FILE.wasm |
| runtime | sort: hd / Node | 1.52x | ≤ 3.00x | PASS | hd 29 ms (p10 22, p90 36), Node 19 ms, Python 17 ms; release build, run as hd FILE.wasm |
| allocations | counted loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | FAIL | hd build --release (alloc-0-small) exited 101: error: src/main.hd:1:23: missing-capability: `main` requires `Console` in `$ Console`, but `[capabilities]` in hd.toml does not list it &#124;     fix: list the missing capabilities in hd.toml |
| allocations | list loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | FAIL | hd build --release (alloc-1-small) exited 101: error: src/main.hd:1:23: missing-capability: `main` requires `Console` in `$ Console`, but `[capabilities]` in hd.toml does not list it &#124;     fix: list the missing capabilities in hd.toml |
| allocations | map loop, bytes per iteration | - | 0 allocations (&lt; 1 B/iter) | FAIL | hd build --release (alloc-2-small) exited 101: error: src/main.hd:1:23: missing-capability: `main` requires `Console` in `$ Console`, but `[capabilities]` in hd.toml does not list it &#124;     fix: list the missing capabilities in hd.toml |
| allocations | iterator chain, bytes per iteration | - | ≤ 1 allocation (≤ 32 B/iter) | FAIL | hd build --release (alloc-3-small) exited 101: error: src/main.hd:3:14: cannot-infer-type: a type argument here has no solution; write it explicitly or give an expected type |
| size-startup-heap | tiny program, release Wasm size | - | ≤ 2048 B | FAIL | hd build --release (tiny) exited 101: error: src/main.hd:1:23: missing-capability: `main` requires `Console` in `$ Console`, but `[capabilities]` in hd.toml does not list it &#124;     fix: list the missing capabilities in hd.toml |
| host-call-overhead | Console println, per call | - | ≤ 1.0 µs | FAIL | hd build --release (host-0) exited 101: error: src/main.hd:4:24: missing-capability: `main` requires `Console` in `$ Console + FsRead`, but `[capabilities]` in hd.toml does not list it &#124;     fix: list the missing capabilities in hd.toml |
| host-call-overhead | Fs read_text!, per call | - | ≤ 10 µs | FAIL | hd build --release (host-1) exited 101: error: src/main.hd:4:24: missing-capability: `main` requires `Console` in `$ Console + FsRead`, but `[capabilities]` in hd.toml does not list it &#124;     fix: list the missing capabilities in hd.toml |
| host-call-overhead | serde boundary (list_dir!), per call | - | ≤ 5.0 µs | FAIL | hd build --release (host-2) exited 101: error: src/main.hd:4:24: missing-capability: `main` requires `Console` in `$ Console + FsRead`, but `[capabilities]` in hd.toml does not list it &#124;     fix: list the missing capabilities in hd.toml |
| suspension-overhead | await of a ready value, per await | - | ≤ 100 ns | FAIL | hd build --release (suspend-empty) exited 101: error: src/main.hd:15:24: missing-capability: `main` requires `Console` in `$ Console`, but `[capabilities]` in hd.toml does not list it &#124;     fix: list the missing capabilities in hd.toml |
| suspension-overhead | all! of two ready tasks, throughput | - | ≥ 1.0M/s | FAIL | hd build --release (suspend-empty) exited 101: error: src/main.hd:15:24: missing-capability: `main` requires `Console` in `$ Console`, but `[capabilities]` in hd.toml does not list it &#124;     fix: list the missing capabilities in hd.toml |
| suspension-overhead | race! of two ready tasks, throughput | - | ≥ 1.0M/s | FAIL | hd build --release (suspend-empty) exited 101: error: src/main.hd:15:24: missing-capability: `main` requires `Console` in `$ Console`, but `[capabilities]` in hd.toml does not list it &#124;     fix: list the missing capabilities in hd.toml |
| serde-throughput | JSON encode, hd / Node MB/s | - | ≥ 0.50x | FAIL | hd build --release (serde-encode-small) exited 101: error: src/main.hd:5:1: syntax-error: expected a declaration, found 'data' &#124; error: src/main.hd:11:1: syntax-error: expected a declaration, found 'data' |
| serde-throughput | JSON decode, hd / Node MB/s | - | ≥ 0.50x | FAIL | hd build --release (serde-decode-small) exited 101: error: src/main.hd:5:1: syntax-error: expected a declaration, found 'data' &#124; error: src/main.hd:11:1: syntax-error: expected a declaration, found 'data' |
| text-throughput | string build, hd / Node MB/s | - | ≥ 0.50x | FAIL | hd build --release (text-build-small) exited 101: warning: src/main.hd:8:9: unused-local-binding: `k` is never read; name it `_k` to keep it &#124; error: src/main.hd:4:23: missing-capability: `main` requires `Console` in `$ Console`, but `[capabilities]` in hd.toml does not list it |
| text-throughput | split, hd / Node MB/s | - | ≥ 0.50x | FAIL | hd build --release (text-split-small) exited 101: warning: src/main.hd:12:9: unused-local-binding: `k` is never read; name it `_k` to keep it &#124; error: src/main.hd:4:23: missing-capability: `main` requires `Console` in `$ Console`, but `[capabilities]` in hd.toml does not list it |
| text-throughput | regex find_all, hd / Node MB/s | - | ≥ 0.50x | FAIL | hd build --release (text-regex-small) exited 101: warning: src/main.hd:13:9: unused-local-binding: `k` is never read; name it `_k` to keep it &#124; error: src/main.hd:4:23: missing-capability: `main` requires `Console` in `$ Console`, but `[capabilities]` in hd.toml does not list it |
| dead-code | Wasm bytes per 1,000 lines | - | ≤ 10240 B | FAIL | hd build --release (dead-small) exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' |
| dead-code | no unused std in the program | none | none | PASS | 743 B either way; checked sizes, and import and export names |
| long-run-memory | RSS growth after warm-up, 1 min | - | ≤ 10.0 MB | FAIL | hd build --release (service) exited 101: error: src/main.hd:5:1: syntax-error: expected a declaration, found 'data' &#124; error: src/main.hd:11:1: syntax-error: expected a declaration, found 'data' |

## Correctness gate

| Metric | Target | Value | Goal | Status | Note |
| --- | --- | --- | --- | --- | --- |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | PASS | 283 of 283 unlisted cases passed |
| conformance | listed known failures | 0 | ≤ 0 | PASS | KNOWN_FAILURES.tsv |
| incremental-soundness | incremental equals clean, after each edit | - | ≥ 100.0% | FAIL | the cache probe's hd check exited 101: error: src/m000.hd:5:5: syntax-error: expected a declaration, found 'data' &#124; error: src/m001.hd:6:5: syntax-error: expected a declaration, found 'data' |

## Recent daily files

- [2026-10-25](daily/2026-10-25.json)
