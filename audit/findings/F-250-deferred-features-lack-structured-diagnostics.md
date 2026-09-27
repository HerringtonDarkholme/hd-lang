# F-250: Deferred features are rejected with generic or wrong diagnostics, not structured unsupported diagnostics
Severity: major
Area: coverage
Duplicates: F-312 (merged: every decorator is rejected by a hard-coded `decorator-not-top-level` in src/parser/parser.ts `parseStatement`)
Evidence: audit/evidence/03-fuzz/findings/F-312-impl-decorator-misreported.hd; the 44 cases tagged F-250 in test/portable/KNOWN_FAILURES.tsv; run them with `node --experimental-strip-types spec/tools/run-conformance.ts --manifest LIST` (re-counted 2026-09-27)
Effect: Of the 44 conformance cases that exercise deferred features
(annotations, decorators, packs, GADTs, metadata types, packages), only 6 get
a structured code (`unsupported-generic-parameter`, all for packs). 11 get
`decorator-not-top-level` on a decorator that is at top level, a spec code
with a false meaning. The other 27 get generic parser or checker codes
(`expected-expression` 18, `syntax-error` 4, `unknown-trait` 2,
`unknown-type` 2, `unknown-name` 1); the two package cases also need
`--package-role`. A user writing `@derive(PartialEq)` is told the syntax is
malformed rather than unimplemented. MVP goal 5 ("reject unimplemented
language features with structured diagnostics") is not met for these slices.
Declared variance is implemented.
Recommendation: implementation change: recognise decorator, `annotate`,
GADT variant results, and `pack.*` forms and report one
stable `unsupported-*` code per feature. OPEN_ISSUES question: should the spec
define a single portable "unsupported-feature" category so conformance runners
can tell "not implemented" from "wrong"?
