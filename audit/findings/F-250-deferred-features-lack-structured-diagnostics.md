# F-250: Deferred features are rejected with generic or wrong diagnostics, not structured unsupported diagnostics
Severity: major
Area: coverage
Evidence: the 12 cases tagged F-250 in test/portable/KNOWN_FAILURES.tsv; run them with `node --experimental-strip-types spec/tools/run-conformance.ts --manifest LIST` (re-counted 2026-09-30)
Effect: The deferred slices left are packs, GADT variant results, and
package roles. Packs now parse, and the value-pack placement codes and the
pack rule of dynamic safety are checked, but a pack function or an
implementation over a pack is `unsupported-generic-parameter` (6 cases),
and `pack.map` is an unknown name (1). GADT results get
`unsupported-gadt-result` (1) or generic parser codes (`syntax-error` 1,
`expected-expression` 2), and the root-orphan case is a usage error because
the CLI has no `--package-role`. A user writing a GADT variant result is
told the syntax is malformed rather than unimplemented. MVP goal 5
("reject unimplemented language features with structured diagnostics") is
not met for these slices. Decorators, derivation, declared variance, and
type-argument defaults are implemented.
Recommendation: implementation change: recognise GADT variant results and
`pack.*` forms and report one stable `unsupported-*` code per feature.
OPEN_ISSUES question: should the spec define a single portable
"unsupported-feature" category so conformance runners can tell "not
implemented" from "wrong"?
