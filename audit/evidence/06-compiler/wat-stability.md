# commit bd985d7 | command: node --experimental-strip-types audit/scripts/compiler/wat-stability.ts audit/probes/compiler/scale-100.hd | date 2026-09-25
base: 628 WAT functions
| edit | WAT functions after | function bodies with changed text |
|---|---:|---:|
| no edit (determinism) | 628 | 0 |
| change one literal in the middle unit | 628 | 1 |
| add one blank line + comment inside a function | 628 | 0 |
| insert a new function before all others | 630 | 308 |
| insert one closure in the first function | 629 | 204 |
