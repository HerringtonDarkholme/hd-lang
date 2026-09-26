<!-- commit bd985d7; command: repeated CLI timings (see rows); date: 2026-09-25T20:51:01Z -->

| command | wall ms, 5 runs |
| --- | --- |
| node -e 0 | 140 146 89 222 168  |
| hd parse minimal.hd | 1504 2285 1943 1958 2619  |
| hd run minimal.hd | 3460 4167 4038 3094 3429  |
| hd test test/fixtures/suspension/43-blanket-implementations-materialize-suspending-dictionaries.hd | 3814 2570 2657  |
| hd test test/fixtures/suspension/44-named-local-suspending-functions-capture-recurse-and-forward-requirements.hd | 3487 3493 3157  |
| hd test spec/conformance/runtime/valid/defer-order.hd | 3173 2736 2371  |
| hd test test/fixtures/compiler-types/collections/maps/lookup-replacement.hd | 2257 1984 1824  |

Rerun at 2026-09-25T20:55:28Z, load averages: 67.13 64.06 43.84

| command | wall ms, 5 runs |
| --- | --- |
| node -e 0 | 194 424 590 404 386  |
| node --experimental-strip-types -e 0 | 506 215 313 145 201  |
| hd parse minimal.hd | 2548 2143 2468 3322 2262  |
| hd run minimal.hd | 3325 3086 2979 4598 4447  |
| node --input-type=module -e "await import('binaryen')" | 1769 1328 1300 900 1306  |
| node --experimental-strip-types -e "await import('./src/parser/index.ts')" | 201 427 305 388 354  |

load averages: 57.37 61.74 44.24
