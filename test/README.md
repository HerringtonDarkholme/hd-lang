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
fixtures. They are being retired as those fixtures move into
`spec/conformance/` (see `audit/proposals/CONFORMANCE_PLAN.md`, W6).

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
