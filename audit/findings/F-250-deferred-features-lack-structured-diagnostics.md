# F-250: Deferred features are rejected with generic or wrong diagnostics, not structured unsupported diagnostics
Severity: major
Area: coverage
Evidence: the 15 cases tagged F-250 in test/portable/KNOWN_FAILURES.tsv; run them with `node --experimental-strip-types spec/tools/run-conformance.ts --manifest LIST` (re-counted 2026-09-29)
Effect: The deferred slices left are packs, GADT variant results, and
package roles. Of the 15 cases, 10 get a structured code
(`unsupported-generic-parameter` 9, `unsupported-gadt-result` 1). The other
5 get generic parser or checker codes (`syntax-error` 2,
`expected-expression` 1, `unknown-name` 1), or a usage error because the
CLI has no `--package-role`. A user writing a GADT variant result is told
the syntax is malformed rather than unimplemented. MVP goal 5 ("reject
unimplemented language features with structured diagnostics") is not met
for these slices. Decorators, derivation, and declared variance are
implemented.
Recommendation: implementation change: recognise GADT variant results and
`pack.*` forms and report one stable `unsupported-*` code per feature.
OPEN_ISSUES question: should the spec define a single portable
"unsupported-feature" category so conformance runners can tell "not
implemented" from "wrong"?
