<!-- commit bd985d7; command: node --experimental-strip-types audit/scripts/arch/bench-fixtures.ts; date: 2026-09-25T20:50:49.957Z -->

Cases: 551 (209 selected conformance + 342 test/fixtures). Sequential; CPU shared with other audit workers.

| measure | n | median ms | p90 ms | p99 ms | max ms | over 1000 ms |
| --- | --- | --- | --- | --- | --- | --- |
| CLI wall (all) | 551 | 687 | 1324 | 2864 | 5095 | 136 |
| CLI wall (parse) | 63 | 571 | 909 | 1176 | 1176 | 2 |
| CLI wall (check) | 291 | 501 | 882 | 1348 | 1363 | 17 |
| CLI wall (test) | 47 | 1170 | 2440 | 5095 | 5095 | 38 |
| CLI wall (run) | 150 | 1037 | 1647 | 2864 | 2984 | 79 |
| in-process compile+validate+Wasm compile (emitting cases) | 301 | 11.1 | 26.6 | 72.3 | 90.0 | 0 |

Slowest 8 CLI invocations:

- test/fixtures/suspension/43-blanket-implementations-materialize-suspending-dictionaries.hd | test | 0 | 5095
- test/fixtures/suspension/44-named-local-suspending-functions-capture-recurse-and-forward-requirements.hd | test | 0 | 3465
- test/fixtures/suspension/42-suspending-associated-functions-preserve-generic-results.hd | test | 0 | 3232
- spec/conformance/runtime/valid/defer-order.hd | test | 0 | 3191
- test/fixtures/compiler/73-supertraits-require-and-expose-parent-implementations.hd | run --entry formatted_length | 0 | 2984
- test/fixtures/compiler-types/collections/maps/lookup-replacement.hd | run --entry main | 0 | 2864
- test/fixtures/compiler/74-mutable-trait-bounds-preserve-root-access.hd | run --entry cleared_value | 0 | 2655
- spec/conformance/runtime/valid/map-insertion-order.hd | test | 0 | 2440
