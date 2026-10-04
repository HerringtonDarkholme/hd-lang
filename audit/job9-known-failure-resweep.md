# Job 9 audit: known-failure re-sweep (Job 4 rerun)

> Read-only sweep. No repo files were changed to produce it. HEAD `4fac80ae`.

## Method

Ran all 164 `test/portable/KNOWN_FAILURES.tsv` rows (down from 133 in
Job 4: the 13 pending-first-poll rows moved to `cases.tsv` since) through
the real `spec/tools/run-conformance.ts` with the in-process adapter —
the same invocation `test/run-portable.ts` uses (`--adapter
test/hd-adapter.ts --jobs 4`), over a manifest of exactly the KF paths
(language/type/runtime rows from `spec/conformance/cases.tsv`, `cli/*`
rows from `spec/conformance/cli-cases.tsv`). Result: `conformance: 5
passed, 159 failed, 164 selected (language: 0 of 126; stdlib: 0 of 10;
cli: 5 of 28)`. Each verdict + output snippet was then compared against
the row's reason text.

Definitions (as in Job 4): "now passes" = runner verdict pass (spec
expectation met); "reason no longer matches" = runner still fails but
the actual output contradicts the reason; "still matches" = actual
output is what the reason describes.

## Rows that now pass: 5 of 164

- `cli/new-existing`
- `cli/new-no-kind`
- `cli/doc-outside-package`
- `cli/doc-check-error`
- `cli/new-pages-existing`

All five are CLI-NEW / CLI-DOC rows whose reason is "no `hd new` (or
`hd doc`) command". Both commands exist now (recent CLI batches), so
these invocations succeed and the reasons are obsolete. Recommended
follow-up (not done here): move these 5 rows to `cases.tsv`.

## Rows whose reason needs an update: 41 of 164

46 KF rows cite exit code 2 ("check exits 2 with usage text",
"rejects with usage text and exit 2", "exits 2 as an unknown
command"). Rejected command lines now exit 101 (CLI-EXIT batch
`c08f4a7d`); 5 of the 46 are the passes above. The remaining 41 still
fail, with the same usage text but exit status 101:

`cli/doc-broken-link`, `cli/doc-index-module`, `cli/doc-main-page`,
`cli/doc-name`, `cli/doc-out`, `cli/doc-private`,
`cli/exe-missing-module`, `cli/exe-unselected-main`,
`cli/exit-outside-package`, `cli/exit-package-check`,
`cli/exit-program-status`, `cli/json-run`, `cli/new-app`,
`cli/new-lib`, `cli/new-no-pages`, `cli/new-pages`, `cli/new-path`,
`cli/new-vcs`, `runtime/valid/available-trait-method-across-packages.hd`,
`runtime/valid/block-on-inside-driver.hd`,
`runtime/valid/nested-block-on.hd`,
`runtime/valid/println-console-stdout.hd`,
`runtime/valid/println-provider-suspending-body.hd`,
`runtime/valid/println-recording-provider.hd`,
`runtime/valid/println-under-main-driver.hd`,
`runtime/valid/provider-scope-dynamic-callback.hd`,
`runtime/valid/provider-scope-lexical-capture.hd`,
`runtime/valid/pub-own-member-hides-promoted-other-module.hd`,
`runtime/valid/resource-disposed-result.hd`,
`runtime/valid/script-empty-run.hd`,
`runtime/valid/unavailable-trait-method-invisible.hd`,
`runtime/valid/write-line-around-suspending-provider-scope.hd`,
`runtime/valid/write-line-suspending-call-argument.hd`,
`typing/invalid/available-trait-method-beside-promoted.hd`,
`typing/invalid/orphan-impl-foreign-trait-argument.hd`,
`typing/invalid/private-embedded-field-nothing-visible.hd`,
`typing/invalid/private-field-nothing-visible.hd`,
`typing/invalid/private-own-method-nothing-visible.hd`,
`typing/invalid/private-promoted-method-nothing-visible.hd`,
`typing/invalid/unavailable-trait-method-not-found.hd`,
`typing/warnings/per-trait-self-line-unused-fact.hd`

For 40 of the 41, only the exit-code detail is stale: the documented
underlying gaps still hold verbatim (no `--package-role` flag, no
disposed-file profile, no `hd FILE` command, no whole-package
`hd check`, no `--out` doc flag, no `hd new` for the remaining forms).
One row is stale more deeply:

- `cli/doc-private`: the reason says "no `hd doc` command: exits 2 as
  an unknown command". `hd doc` exists now — this invocation runs and
  exits 1 (`doc-comment-without-target` plus "no symbol named
  text.helper"). The row still fails, but for a new reason, not the
  recorded one.

Note on scope, to avoid misreading the CLI-EXIT rows (`cli/exit-hd-failure`,
`cli/json-check-error`, `cli/json-file-location`, `cli/json-file-single`,
`cli/exit-test-empty`): those document the 1-vs-101 tension for rejected
*programs* under the conformance Command Contract, and they still match —
rejected programs still exit 1; only rejected *command lines* moved to
101. No change recommended there until the owner reconciles the contract.

## Rows that still match: 118 of 164

Every other row fails exactly as its reason describes (same codes,
lines, and messages). Spot-checked classes:

- GADT rows (4, F-250): exact quoted behavior — `syntax-error` at 7:7,
  `unsupported-gadt-result` at 3:27/4:29, `expected-expression …
  found 'mut'`.
- TYPE-GAPS rows (generic-inference conflicts, supertrait-impl-bounds,
  bound-implies-supertrait, any-void-never, unconstrained-impl-parameter):
  same acceptances and same `unsatisfied-trait-bound` / `type-mismatch`
  outputs as documented.
- All literal-first-use / literal-var / sign-fallback / one-fit rows
  (~30): actual codes, lines, and messages are exactly the documented
  ones (including the `usize`-vs-`u32` rendering and the
  `no-common-type: i32, i16` join).
- FACT-PATTERN rows (9): same `type-mismatch … must be one of the type
  parameters of …` at the `@annotate` decorator, same wrong-line
  locations.
- SHADOW-TPARAM, ALIAS-MISSING, PRIVATE-STD, DERIVE-MISSING,
  BOUND-AMBIGUOUS, RESERVE-PKG, UNCOVERED-*, MODULE-DOC, SELF-CURRENT,
  ROOTS, FOLDER-SELF, NONPKG, TASK-PROGRAMS, DC7, MHP-1, M29,
  DERIVE-DEFAULT, QUALIFIED-PATH, TEST-REG-ID, VOID-UNIT,
  METHOD-DEFAULT, VARIANCE-MUT-SELF, CLI-57 rows: each actual output
  matches its reason (verified against the full per-row output, not
  just the verdict).
- `retry-with-backoff.hd`: still `unknown-import` for `Backoff` /
  `retry_with`, consistent with the standing owner hold.
- `race-empty-at-run-time.hd`: still hangs (runner killed it at 10 s),
  as in Job 4.
- `doc-test-failing-assert.hd`: prints `0 passed`, exit 0, exactly as
  documented.

## Standing watch

Next known-failure sweep reruns this method when `KNOWN_FAILURES.tsv`
or the CLI exit contract changes.
