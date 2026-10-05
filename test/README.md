# Test Layout

Language behavior is tested through the implementation-neutral fixtures in
`../spec/conformance/`. `portable/cases.tsv` selects the part implemented by
the MVP, and `run-portable.ts` executes those cases only through the
`hd parse`, `hd check`, and `hd test` command lines of the conformance
command contract (`hd parse` is that contract's spelling of `hd debug parse`).
By default it runs them in-process: `hd-adapter.ts` hands each command line
to a pool of worker threads that import `src/cli.ts` once, so no case starts
a process. `--compiler` or `HD_TEST_COMMAND` spawns another implementation
instead, so the same fixtures serve another compiler without importing
TypeScript modules.

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
trace and replay plumbing, the packaged CLI, and the in-process adapter
(`hd-adapter.test.ts` checks that it reports what a spawned `hd` reports, and
that a hung case times out). A test that runs an `hd` command line calls
`runHd` or `hd` from `hd-in-process.ts`, which call `main` in the test's own
process and pass the directory as `cwd` instead of changing the process's.
Only three tests start `bin/hd.js`: the executable smoke test in
`cli.test.ts`, the REPL's standard-input test in `ui/repl.test.ts` (run by `pnpm run test:ui`, in CI and once per merge, not by `pnpm run check`), and the
adapter comparison. When a TypeScript test
finds a language-level regression, add or extend a `.hd` conformance fixture;
keep a TS assertion only when it verifies one of those implementation details.
[`MIGRATED.md`](MIGRATED.md) maps TypeScript tests that now have conformance
fixtures to those fixtures, so the tests can be deleted.

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
`# expect: test` runs the fixture's `main` and its test cases through the
public test command. A `# fixture-runtime-profile: NAME` directive supplies the
same named host profile to check and execution.

Run the portable behavior suite with `pnpm run test:portable`. Add `--changed [REVISION]` to run only the conformance cases whose
fixture differs from that revision (default `origin/main`), or `--phase
parse|type|runtime` to run one phase. Fetch first (`git fetch origin`):
`--changed` compares with the local `origin/main` ref.
[`portable/README.md`](portable/README.md) covers the in-process adapter,
running it against another implementation, selecting a tier, and the
`HD_TEST_JOBS` setting.

The specification grammar oracle is also TypeScript and parses fixtures in a
worker-thread pool. It shares `HD_TEST_JOBS` by default; set `HD_SPEC_JOBS` to
tune that gate independently.

`pnpm run test:ui` runs the REPL, website, and playground tests
(`test/ui`, `website/test`, `website/playground/test`). It is not part of
`pnpm run check`; the Pages workflow runs it before it builds and deploys
the site, and the merge subagent runs it once per merge. `pnpm run test:all` runs both `pnpm test` and
`pnpm run test:ui`.

`pnpm run perf:check` is the type checker's performance gate. It is not part
of `pnpm run check`, since it takes about a minute; CI runs it as its own
job. [`perf/inference/README.md`](perf/inference/README.md) describes the
cases, the rules, and how to update the baseline.

## Held-Back Fixtures

`test/fixtures` keeps only implementation-detail inputs and fixtures that
cannot be conformance cases yet. Each stays here for the reason given. The
TypeScript tests read promoted fixtures from `spec/conformance/` through
`conformance()` in `fixture.ts`.

- `compiler-types/enums/generic/unsaturated-type.hd`: marks partial-generic-arguments for a generic enum written without arguments (types.generic.default.bare); it waits for a conformance fixture of that case.
- `frontend/00-core-program.hd`: implementation detail: lexer and AST snapshot input.
- `suspension/05-all-task-combinator.hd`: implementation detail: `test/compiler-suspension.test.ts` drives it with host pending and checks the poll order through the trace ABI.
- `suspension/05-race-task-combinator.hd`: implementation detail: `test/compiler-suspension.test.ts` drives it with host pending and checks that the losers are cancelled through the trace ABI.
- `suspension/05-user-defined-all.hd`: declares its own all!; whether that is allowed next to the all! intrinsic (L12) is not specified.
- `suspension/10-suspension-state-transitions-are-observable-through-the-trace-abi.hd`: implementation detail: trace ABI event order.
- `suspension/11-deterministic-host-pending.hd`: implementation detail: host pending across polls, driven from TypeScript.
- `suspension/12-development-drivers-reject-competing-and-reentrant-suspension-control.hd`: implementation detail: driver misuse through __hd_* exports.
- `suspension/26-replay-rejects-function-code-change.hd`: implementation detail: replay rejection, driven from TypeScript.
- `suspension/26-replay-survives-unrelated-declaration.hd`: implementation detail: replay stability, driven from TypeScript.
- `suspension/26-suspension-poll-decisions-record-and-replay-with-configuration-identity.hd`: implementation detail: record and replay configuration identity.
- `suspension/27-println-requires-console-and-streams-displayed-utf-8-through-the-host-bo.hd`: implementation detail: console provider plumbing, a non-`pub` `main` returning `i32`, and `bool` and `char` rendering, which the specification does not define. Its portable part is `runtime/valid/println-console-stdout.hd`, checked with `# expect-stdout:`.
- `suspension/32-host-provider-polls-record-and-replay.hd`: the ready-gate profile is an implementation test, because the conformance suite lists only its own profiles; check moved into `main!`.
- `suspension/33-host-provider-scalar-arguments-and-results.hd`: the ready-counter profile is an implementation test, because the conformance suite lists only its own profiles; check moved into `main!`.
- `suspension/35-host-provider-f64-values-use-durable-bit-encoding.hd`: the ready-float profile is an implementation test, because the conformance suite lists only its own profiles; sign-sensitive check moved into `main!`.
- `suspension/36-host-provider-strings-use-utf8-boundary.hd`: the ready-text profile is an implementation test, because the conformance suite lists only its own profiles; check moved into `main!`.
