# Job 4 audit: known-failure reasons recheck

> Posted in chat 2026-10-04 during the read-only audit session; logged here verbatim by owner request. HEAD 4636f546. Body below is the posted report, unedited.

Job 4 done. No repo files changed. HEAD `4636f546`.

## Method

Ran all 133 `test/portable/KNOWN_FAILURES.tsv` rows through the real `spec/tools/run-conformance.ts` with a manifest of exactly those paths and the in-process adapter — the same invocation `test/run-portable.ts` uses (`--adapter test/hd-adapter.ts --jobs 4`; `check` always gets `--tests` per the runner). Result: `conformance: 0 passed, 133 failed` — i.e. every row still fails its spec expectation. I then compared each verdict + output snippet against the row's reason text.

Definitions used: "now passes" = runner verdict pass (spec expectation met); "reason no longer matches" = runner still fails but the actual output contradicts the reason; "still matches" = actual output is what the reason describes.

## Rows that now pass: none (0 of 133)

## Rows whose reason no longer matches: none (0 of 133)

## Rows that still match: all 133

Spot-checked classes (each reason vs actual output):

- GADT rows (4, F-250): exact quoted behavior — `syntax-error` at 7:7, `unsupported-gadt-result` at 3:27/4:29, `expected-expression … found 'mut'`.
- Package-role rows (12) + `resource-disposed-result.hd` + `per-trait-self-line-unused-fact.hd`: reason says "check exits 2 with usage text"; actual is exit 2 with exactly that usage text (`unknown flag --package-role`; `--profile must be one of …`). The P2/EMB-S reasons' second clauses ("every trait/member is available/visible") are conditioned on the missing flags, so not exercisable — but nothing contradicts them.
- CLI-ENTRY rows (11): reason quotes the `IMPL FILE` gap verbatim; actual is exit 2 `unknown command '<file>'`. (Their `check` and `test` steps pass; only the bare-run step fails — the prototype has no `hd FILE`, per the command contract.)
- `relative-self-*` (2): actual is the quoted `unsupported-package-use` ×2; the reason's second clause (relative lookup now resolves) isn't observable past the rejection, but isn't contradicted.
- `retry-with-backoff.hd`: reason is a standing owner hold ("held: implementing it makes std.task import std.time"); actual `unknown-import` for `Backoff`/`retry_with` is consistent with the hold being in force.
- All literal-first-use / literal-var / sign-fallback / one-fit rows (~30): actual codes, lines, and messages are exactly the documented ones (including the `usize`-vs-`u32` rendering in `literal-first-use-generic-join.hd` — `usize` is a transparent alias of `u32` per `spec/lang/04-type-system.md:117`, so no conflict).
- Panic rows: `race-empty-at-run-time.hd` still hangs (runner killed it at 10 s); `literal-var-fallback-overflow.hd` reports `assertion-failed`, not `integer-overflow`, as documented; `sign-fallback-balance-underflow.hd` exits 0 with `1 passed`.
- `colon-line-after-if-suite.hd`: `hd parse` exits 0 as documented (my Job 3 `not-callable` came from `hd check` — different phase, no contradiction).
- `doc-test-failing-assert.hd`: prints `0 passed`, exit 0, exactly as documented.

Two rows I verified beyond the runner because the reason pointed elsewhere: `unconstrained-impl-parameter.hd` stays silent even under `hd check --tests` (recorded gap, `src/KNOWN_ISSUES.md:136`); `misplaced-test-case.hd` reports correctly under `--tests`, confirming the silent-33 bucket from Job 3 is a flags artifact, not a reason mismatch.

Ready for Job 5.