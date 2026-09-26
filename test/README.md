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

- `compiler-types/collections/lists/void-element-type.hd`: marks unknown-type for list[void]; void resolves, and the spec names no code for a void type argument.
- `compiler-types/enums/generic/unsaturated-type.hd`: marks unknown-type for a generic enum used without arguments; the name resolves, and no inventoried code fits.
- `frontend/00-core-program.hd`: implementation detail: lexer and AST snapshot input.
- `requirements/closure-provider.hd`: implementation detail: explain-requirements output.
- `requirements/lexical-override.hd`: implementation detail: explain-requirements output.
- `requirements/loop-exit.hd`: implementation detail: explain-requirements output.
- `requirements/transitive-call-paths.hd`: implementation detail: explain-requirements output.
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
