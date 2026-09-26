# commit bd985d7 | command: node --experimental-strip-types audit/scripts/compiler/import-graph.ts src | date 2026-09-25
| file | lines | fan-in | fan-out | % of 1500 cap |
|---|---:|---:|---:|---:|
| src/hir.ts | 816 | 31 | 1 | 54% |
| src/types.ts | 277 | 25 | 1 | 18% |
| src/ast.ts | 449 | 23 | 1 | 30% |
| src/checker/shared.ts | 1225 | 16 | 4 | 82% |
| src/checker/context.ts | 1181 | 15 | 5 | 79% |
| src/diagnostics.ts | 35 | 15 | 0 | 2% |
| src/checker/program-context.ts | 31 | 8 | 4 | 2% |
| src/emitter/shared.ts | 140 | 6 | 2 | 9% |
| src/compiler.ts | 496 | 5 | 7 | 33% |
| src/runtime-panic.ts | 33 | 5 | 0 | 2% |
| src/lexer.ts | 617 | 4 | 1 | 41% |
| src/parser/index.ts | 1 | 3 | 1 | 0% |
| src/emitter/suspension.ts | 1191 | 2 | 2 | 79% |
| src/emitter/context.ts | 368 | 2 | 5 | 25% |
| src/requirements.ts | 300 | 2 | 1 | 20% |
| src/parser/base.ts | 120 | 2 | 3 | 8% |
| src/checker/program-effects.ts | 110 | 2 | 1 | 7% |
| src/wasm.ts | 39 | 2 | 0 | 3% |
| src/emitter/function-body.ts | 1487 | 1 | 5 | 99% |
| src/emitter/emitter.ts | 1460 | 1 | 8 | 97% |
| src/checker/expression-calls.ts | 1313 | 1 | 5 | 88% |
| src/parser/parser.ts | 1219 | 1 | 5 | 81% |
| src/parser/expression.ts | 964 | 1 | 3 | 64% |
| src/checker/calls.ts | 904 | 1 | 7 | 60% |
| src/checker/expression-control.ts | 582 | 1 | 6 | 39% |
| src/checker/program-types.ts | 567 | 1 | 5 | 38% |
| src/checker/program-implementations.ts | 566 | 1 | 6 | 38% |
| src/checker/statements.ts | 551 | 1 | 5 | 37% |
| src/checker/expression-data.ts | 503 | 1 | 6 | 34% |
| src/checker/patterns.ts | 474 | 1 | 7 | 32% |
| src/emitter/value-comparison.ts | 451 | 1 | 4 | 30% |
| src/checker/expression-operators.ts | 359 | 1 | 6 | 24% |
| src/checker/program-declarations.ts | 322 | 1 | 3 | 21% |
| src/emitter/host-providers.ts | 292 | 1 | 2 | 19% |
| src/checker/checker.ts | 287 | 1 | 6 | 19% |
| src/emitter/iterator.ts | 281 | 1 | 4 | 19% |
| src/checker/program-signatures.ts | 273 | 1 | 6 | 18% |
| src/checker/expression-suspensions.ts | 257 | 1 | 5 | 17% |
| src/checker/expression-literals.ts | 203 | 1 | 5 | 14% |
| src/checker/expression-comprehensions.ts | 184 | 1 | 7 | 12% |
| src/checker/program-lower.ts | 155 | 1 | 7 | 10% |
| src/checker/program-validation.ts | 133 | 1 | 4 | 9% |
| src/emitter/stored-suspension.ts | 124 | 1 | 4 | 8% |
| src/checker/module-initialization.ts | 98 | 1 | 2 | 7% |
| src/toolchain-gate.ts | 78 | 1 | 1 | 5% |
| src/checker/program.ts | 46 | 1 | 10 | 3% |
| src/checker/host-capabilities.ts | 26 | 1 | 1 | 2% |
| src/emitter/runtime/index.ts | 12 | 1 | 0 | 1% |
| src/checker/index.ts | 3 | 1 | 2 | 0% |
| src/emitter/index.ts | 1 | 1 | 1 | 0% |
| src/cli.ts | 313 | 0 | 5 | 21% |

boundary violations (0):

importers of src/checker/shared.ts: src/checker/calls.ts, src/checker/checker.ts, src/checker/context.ts, src/checker/expression-calls.ts, src/checker/expression-comprehensions.ts, src/checker/expression-control.ts, src/checker/expression-data.ts, src/checker/expression-operators.ts, src/checker/expression-suspensions.ts, src/checker/patterns.ts, src/checker/program-implementations.ts, src/checker/program-lower.ts, src/checker/program-signatures.ts, src/checker/program-types.ts, src/checker/program-validation.ts, src/checker/statements.ts

importers of src/checker/context.ts: src/checker/calls.ts, src/checker/checker.ts, src/checker/expression-comprehensions.ts, src/checker/expression-control.ts, src/checker/expression-literals.ts, src/checker/expression-operators.ts, src/checker/index.ts, src/checker/patterns.ts, src/checker/program-context.ts, src/checker/program-lower.ts, src/checker/program-signatures.ts, src/checker/program-types.ts, src/checker/program-validation.ts, src/checker/program.ts, src/checker/statements.ts

importers of src/checker/expression-calls.ts: src/checker/expression-suspensions.ts

## class inheritance chains (grep 'class .* extends')
src/checker/calls.ts:57:export abstract class CallChecker extends StatementChecker {
src/checker/expression-comprehensions.ts:23:export abstract class ExpressionComprehensionChecker extends ExpressionDataChecker {
src/checker/checker.ts:8:export class FunctionChecker extends ExpressionControlChecker {
src/checker/expression-operators.ts:14:export abstract class ExpressionOperatorChecker extends ExpressionLiteralChecker {
src/checker/statements.ts:18:export abstract class StatementChecker extends CheckerContext {
src/checker/expression-control.ts:52:export abstract class ExpressionControlChecker extends ExpressionComprehensionChecker {
src/checker/context.ts:99:export class CheckFailure extends Error {}
src/checker/expression-suspensions.ts:13:export abstract class ExpressionSuspensionChecker extends ExpressionCallChecker {
src/checker/expression-data.ts:28:export abstract class ExpressionDataChecker extends ExpressionSuspensionChecker {
src/checker/patterns.ts:33:export abstract class PatternChecker extends CallChecker {
src/checker/expression-calls.ts:47:export abstract class ExpressionCallChecker extends ExpressionOperatorChecker {
src/checker/expression-literals.ts:16:export abstract class ExpressionLiteralChecker extends PatternChecker {
src/parser/expression.ts:48:export abstract class ExpressionParser extends ParserBase {
src/emitter/emitter.ts:65:class FunctionEmitter extends FunctionBodyEmitter {
src/emitter/iterator.ts:14:export abstract class IteratorEmitter extends ValueComparisonEmitter {
src/emitter/value-comparison.ts:12:export abstract class ValueComparisonEmitter extends EmitterContext {
src/parser/parser.ts:39:class Parser extends ExpressionParser {
src/emitter/function-body.ts:47:export abstract class FunctionBodyEmitter extends IteratorEmitter {
src/parser/base.ts:5:export class ParseFailure extends Error {}
