# commit bd985d7 | command: node --experimental-strip-types audit/scripts/compiler/duplication.ts 6 | date 2026-09-25
## Shingle duplication (window 6 normalized lines of >= 12 chars)
| file | kept lines | lines inside a duplicated window | % |
|---|---:|---:|---:|
| src/ast.ts | 332 | 26 | 7.8% |
| src/checker/calls.ts | 612 | 82 | 13.4% |
| src/checker/checker.ts | 201 | 38 | 18.9% |
| src/checker/context.ts | 805 | 121 | 15.0% |
| src/checker/expression-calls.ts | 922 | 159 | 17.2% |
| src/checker/expression-comprehensions.ts | 126 | 22 | 17.5% |
| src/checker/expression-control.ts | 393 | 19 | 4.8% |
| src/checker/expression-data.ts | 330 | 24 | 7.3% |
| src/checker/expression-literals.ts | 140 | 15 | 10.7% |
| src/checker/expression-operators.ts | 227 | 7 | 3.1% |
| src/checker/expression-suspensions.ts | 170 | 60 | 35.3% |
| src/checker/patterns.ts | 319 | 40 | 12.5% |
| src/checker/program-declarations.ts | 224 | 24 | 10.7% |
| src/checker/program-implementations.ts | 386 | 7 | 1.8% |
| src/checker/program-signatures.ts | 183 | 6 | 3.3% |
| src/checker/program-types.ts | 384 | 89 | 23.2% |
| src/checker/program-validation.ts | 92 | 12 | 13.0% |
| src/checker/shared.ts | 879 | 106 | 12.1% |
| src/checker/statements.ts | 381 | 27 | 7.1% |
| src/emitter/emitter.ts | 1084 | 342 | 31.5% |
| src/emitter/function-body.ts | 1087 | 80 | 7.4% |
| src/emitter/shared.ts | 108 | 11 | 10.2% |
| src/emitter/stored-suspension.ts | 91 | 6 | 6.6% |
| src/emitter/suspension.ts | 830 | 33 | 4.0% |
| src/emitter/value-comparison.ts | 330 | 30 | 9.1% |
| src/hir.ts | 647 | 46 | 7.1% |
| src/parser/expression.ts | 625 | 6 | 1.0% |
| src/parser/parser.ts | 831 | 52 | 6.3% |
| src/requirements.ts | 205 | 8 | 3.9% |
| **total** | 15330 | 1498 | 9.8% |

| file pair | duplicated windows |
|---|---:|
| src/emitter/emitter.ts (self) | 85 |
| src/checker/context.ts <-> src/checker/shared.ts | 68 |
| src/checker/calls.ts <-> src/checker/expression-calls.ts <-> src/checker/expression-suspensions.ts | 17 |
| src/emitter/function-body.ts (self) | 15 |
| src/checker/expression-calls.ts <-> src/checker/expression-suspensions.ts | 13 |
| src/checker/calls.ts <-> src/checker/context.ts | 9 |
| src/checker/expression-calls.ts (self) | 9 |
| src/checker/checker.ts (self) | 8 |
| src/hir.ts (self) | 8 |
| src/parser/parser.ts (self) | 8 |
| src/emitter/value-comparison.ts (self) | 7 |
| src/checker/calls.ts <-> src/checker/expression-calls.ts | 6 |
| src/checker/program-types.ts (self) | 6 |
| src/emitter/shared.ts <-> src/emitter/suspension.ts | 6 |
| src/checker/expression-control.ts <-> src/checker/patterns.ts | 5 |

## HIR walker census (80 HIR expression kinds)
| file | distinct HIR expression kinds in `case` labels |
|---|---:|
| src/checker/shared.ts | 25 |
| src/emitter/function-body.ts | 80 |
| src/emitter/suspension.ts | 79 |
| src/requirements.ts | 57 |

## Variant-kind references per file
| file | suspend-*/suspension-*/trait-suspend-* | trait-call/closure-call/call |
|---|---:|---:|
| src/checker/calls.ts | 6 | 6 |
| src/checker/expression-calls.ts | 9 | 7 |
| src/checker/expression-suspensions.ts | 11 | 2 |
| src/emitter/function-body.ts | 9 | 3 |
| src/emitter/shared.ts | 12 | 0 |
| src/emitter/suspension.ts | 25 | 3 |
| src/hir.ts | 9 | 3 |
| src/requirements.ts | 6 | 3 |
