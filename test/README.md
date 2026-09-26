# Test Layout

Language behavior is tested through the implementation-neutral fixtures in
`../spec/conformance/`. `portable/cases.tsv` selects the part implemented by
the MVP, and `run-portable.ts` executes those cases only through the public
`hd parse`, `hd check`, and `hd test` commands. The same fixtures can therefore
be used by another compiler without importing TypeScript modules.

The conformance fixture format and the command contract are defined in
[`../spec/conformance/README.md`](../spec/conformance/README.md); that file is
authoritative. The `test/fixtures` directives described below
(`# expect-result:`, `# expect:` without a manifest row, and the
`runtime-error` panic wildcard) belong only to this implementation's own
fixtures. Every promotable language fixture has moved into
`spec/conformance/`; the files that remain are listed in
[Held-Back Fixtures](#held-back-fixtures).

The TypeScript tests cover implementation details that are intentionally not
part of the language contract: AST and HIR shape, emitted WAT, Wasm host calls,
trace and replay plumbing, and the packaged CLI adapter. When a TypeScript test
finds a language-level regression, add or extend a `.hd` conformance fixture;
keep a TS assertion only when it verifies one of those implementation details.

Self-contained fixtures put their name and observable expectations in the hd
source. Compiler diagnostics are marked on the source line they must point to;
runtime panics have a distinct marker:

```text
# test: assignment rejects the wrong value type
let value: i32 = "text"  # diagnostic: type-mismatch

# test: integer division by zero traps
fn main() -> i32: 1 / 0  # panic: integer-division-by-zero
```

Use `# warning: CODE` for non-fatal compiler diagnostics and
`# expect-result: ENTRY = VALUE` for scalar exports. The portable harness checks
both the diagnostic code and the annotated line number. A `# panic: CODE`
fixture must fail with that exact runtime panic code; `runtime-error` is
reserved for backend traps that do not yet have a structured code.
`# expect: test` runs the fixture's `main` and named test blocks through the
public test command. A `# fixture-runtime-profile: NAME` directive supplies the
same named host profile to check and execution.

Run the portable behavior suite with:

```sh
npm run test:portable
```

To exercise a different implementation with the same command contract:

```sh
HD_TEST_COMMAND="other-hd" node --experimental-strip-types test/run-portable.ts
```

The runner uses up to eight cores by default. Override that with
`HD_TEST_JOBS=4` or `--jobs 4`.

The specification grammar oracle is also TypeScript and parses fixtures in a
worker-thread pool. It shares `HD_TEST_JOBS` by default; set `HD_SPEC_JOBS` to
tune that gate independently.

## Held-Back Fixtures

`test/fixtures` keeps only implementation-detail inputs and fixtures that
cannot be conformance cases yet. Each stays here for the reason given. The
TypeScript tests read promoted fixtures from `spec/conformance/` through
`conformance()` in `fixture.ts`.

- `compiler-types/collections/lists/empty-needs-context.hd`: marks empty-list-needs-context, which spec/README.md does not inventory (F-205).
- `compiler-types/collections/lists/void-element-type.hd`: marks unknown-type for list[void]; void resolves, and the spec names no code for a void type argument.
- `compiler-types/collections/maps/empty-needs-context.hd`: marks empty-map-needs-context, which spec/README.md does not inventory (F-205).
- `compiler-types/enums/generic/context-required.hd`: marks generic-enum-needs-context, which spec/README.md does not inventory (F-205).
- `compiler-types/enums/generic/unsaturated-type.hd`: marks unknown-type for a generic enum used without arguments; the name resolves, and no inventoried code fits.
- `compiler-types/generics/functions/unsaturated-function-value.hd`: marks generic-function-value-needs-arguments, which spec/README.md does not inventory (F-205).
- `compiler-types/identity/references/missing-implementation.hd`: marks missing-trait-implementation (unsatisfied bound), which spec/README.md does not inventory (F-205).
- `compiler-types/requirements/normalization/explicit-empty-row.hd`: uses `$()`, which chapter 02 prose defines but its EBNF (and the reference parser) does not derive.
- `compiler-types/traits/generic-bounds/missing-implementation.hd`: marks missing-trait-implementation (unsatisfied bound), which spec/README.md does not inventory (F-205).
- `compiler-types/traits/multiple-bounds/duplicate-bound.hd`: marks duplicate-trait-bound, which spec/README.md does not inventory (F-205); second duplicate bound removed so the case reports one error.
- `compiler-types/tuples/heterogeneous/index-out-of-range.hd`: marks tuple-index-range, which spec/README.md does not inventory (F-205).
- `compiler/07-checker-rejects-name-mutability-and-type-errors-diagnostic-4.hd`: marks invalid-binary-operands (float %), which spec/README.md does not inventory (F-205).
- `compiler/07-checker-rejects-name-mutability-and-type-errors-diagnostic-5.hd`: marks invalid-binary-operands (bool <), which spec/README.md does not inventory (F-205).
- `compiler/07-checker-rejects-name-mutability-and-type-errors-positional-spread-needs-vararg.hd`: marks positional-spread-needs-vararg, which spec/README.md does not inventory (F-205).
- `compiler/11-function-parameter-defaults-enforce-order-type-and-purity-diagnostic.hd`: marks parameter-default-order; chapter 07 states the rule but spec/README.md has no code for it (F-205).
- `compiler/14-named-enum-payloads-preserve-source-evaluation-order-duplicate-argument.hd`: marks duplicate-argument, which spec/README.md does not inventory (F-205).
- `compiler/14-named-enum-payloads-preserve-source-evaluation-order-unknown-named-argument.hd`: marks unknown-named-argument, which spec/README.md does not inventory (F-205).
- `compiler/15-named-enum-payload-patterns-resolve-bindings-by-field-name-unknown-variant-pattern-field.hd`: marks unknown-variant-pattern-field, which spec/README.md does not inventory (F-205).
- `compiler/24-for-loops-iterate-lists-and-maps-with-continue-destructuring-and-else-va-diagnostic-2.hd`: marks for-binding-arity, which spec/README.md does not inventory (F-205).
- `compiler/24-for-loops-iterate-lists-and-maps-with-continue-destructuring-and-else-va-diagnostic.hd`: marks not-iterable, which spec/README.md does not inventory (F-205).
- `compiler/44-defer-rejects-escaping-control-flow.hd`: marks defer-control-flow, which spec/README.md does not inventory (F-205).
- `compiler/48-match-pattern-arity.hd`: marks pattern-arity, which spec/README.md does not inventory (F-205).
- `compiler/48-match-unreachable-arm.hd`: marks unreachable-match-arm, which spec/README.md does not inventory (F-205).
- `compiler/51-boolean-matches-are-exhaustive-and-lower-to-scalar-tests-unreachable-match-arm.hd`: marks unreachable-match-arm, which spec/README.md does not inventory (F-205).
- `compiler/52-numeric-character-and-string-literal-patterns-require-a-catch-all-unreachable-match-arm.hd`: marks unreachable-match-arm, which spec/README.md does not inventory (F-205).
- `compiler/58-optional-and-result-context-errors-have-stable-diagnostics-diagnostic-2.hd`: marks result-constructor-needs-context, which spec/README.md does not inventory (F-205).
- `compiler/58-optional-and-result-context-errors-have-stable-diagnostics-diagnostic-3.hd`: marks invalid-result-propagation, which spec/README.md does not inventory (F-205).
- `compiler/58-optional-and-result-context-errors-have-stable-diagnostics-diagnostic.hd`: marks nil-needs-optional-type, which spec/README.md does not inventory (F-205).
- `compiler/61-expected-function-types-infer-inline-closure-parameters-and-results-diagnostic.hd`: marks closure-parameter-needs-annotation, which spec/README.md does not inventory (F-205).
- `compiler/62-nonrecursive-closures-infer-result-types-from-fallthrough-and-returns-closure-result-type.hd`: marks closure-result-type, which spec/README.md does not inventory (F-205).
- `compiler/66-associated-functions-use-qualified-static-calls.hd`: uses `Type::name[T](...)`, which the chapter-02 EBNF does not derive.
- `compiler/73-supertraits-reject-missing-parent-implementations.hd`: marks missing-supertrait-implementation, which spec/README.md does not inventory (F-205).
- `compiler/74-mutable-trait-bounds-reject-readonly-roots.hd`: marks mutable-bound-required, which spec/README.md does not inventory (F-205).
- `compiler/75-generic-trait-methods-use-static-erased-dispatch.hd`: uses `Trait::name[T](...)`, which the chapter-02 EBNF does not derive.
- `compiler/display-requires-implementation.hd`: marks missing-display, which spec/README.md does not inventory (F-205).
- `frontend/00-core-program.hd`: implementation detail: lexer and AST snapshot input.
- `requirements/closure-provider.hd`: implementation detail: explain-requirements output.
- `requirements/lexical-override.hd`: implementation detail: explain-requirements output.
- `requirements/loop-exit.hd`: implementation detail: explain-requirements output.
- `requirements/transitive-call-paths.hd`: implementation detail: explain-requirements output.
- `suspension/04-ordinary-suspending-calls-are-cold-values-and-bang-calls-need-a-driver-diagnostic-3.hd`: marks not-suspending, which spec/README.md does not inventory (F-205).
- `suspension/05-unresolved-race-task-combinator.hd`: marks unsupported-task-combinator; unsupported-* codes stay out of the inventory (the spec has no portable unsupported result).
- `suspension/05-unresolved-standard-task-combinators-have-a-dedicated-boundary-diagnosti-userdefined.hd`: declares its own all!; whether that is allowed next to the all! intrinsic (L12) is not specified.
- `suspension/05-unresolved-standard-task-combinators-have-a-dedicated-boundary-diagnosti.hd`: marks unsupported-task-combinator; unsupported-* codes stay out of the inventory (the spec has no portable unsupported result).
- `suspension/10-suspension-state-transitions-are-observable-through-the-trace-abi.hd`: implementation detail: trace ABI event order.
- `suspension/11-deterministic-host-pending.hd`: implementation detail: host pending across polls, driven from TypeScript.
- `suspension/12-development-drivers-reject-competing-and-reentrant-suspension-control.hd`: implementation detail: driver misuse through __hd_* exports.
- `suspension/26-replay-rejects-function-code-change.hd`: implementation detail: replay rejection, driven from TypeScript.
- `suspension/26-replay-survives-unrelated-declaration.hd`: implementation detail: replay stability, driven from TypeScript.
- `suspension/26-suspension-poll-decisions-record-and-replay-with-configuration-identity.hd`: implementation detail: record and replay configuration identity.
- `suspension/27-println-requires-console-and-streams-displayed-utf-8-through-the-host-bo.hd`: claims console output; stdout expectations are deferred (N5).
- `suspension/32-host-provider-polls-record-and-replay.hd`: implementation-only profile ready-gate (N2); check moved into `main!`.
- `suspension/33-host-provider-scalar-arguments-and-results.hd`: implementation-only profile ready-counter (N2); check moved into `main!`.
- `suspension/35-host-provider-f64-values-use-durable-bit-encoding.hd`: implementation-only profile ready-float (N2); sign-sensitive check moved into `main!`.
- `suspension/36-host-provider-strings-use-utf8-boundary.hd`: implementation-only profile ready-text (N2); check moved into `main!`.
- `suspension/42-suspending-associated-functions-preserve-generic-results.hd`: uses `Type::name[T]!(...)`, which the chapter-02 EBNF does not derive.
