# F-250: Deferred features are rejected with generic or wrong diagnostics, not structured unsupported diagnostics
Severity: major
Area: coverage
Duplicates: F-405 (decorator part only) (found independently by another worker; phase-2 evidence corroborates)
Evidence: audit/evidence/02-coverage/unselected-classified.tsv (command: `node --experimental-strip-types audit/scripts/coverage/runner.ts conformance audit/evidence/02-coverage/unselected-manifest.tsv audit/evidence/02-coverage/unselected.tsv`, each case run with the phase command used by test/run-portable.ts)
Effect: Of 51 unselected conformance cases that exercise deferred features
(annotations 28, packs 8, GADTs 4, declared variance 6, plus mixed), only 7 get a
structured `unsupported-*` code (`unsupported-generic-parameter` for packs; one
`unsupported-gadt-result`, and that one is not the first diagnostic). 34 get generic parser or
checker codes (`expected-expression` 21, `expected-token` 9, `unknown-trait` 2,
`unknown-type` 1, `unknown-name` 1). 10 get `decorator-not-top-level` on a decorator
that is at top level, a spec code with a false meaning. A user writing `@derive(PartialEq)`
or `data Producer[+T]:` is told the syntax is malformed rather than unimplemented.
MVP goal 5 ("reject unimplemented language features with structured diagnostics")
and src/README.md lines 5-6 and 270-272 are not met for these slices.
Recommendation: implementation change: recognise decorator, `annotate`, `shape(...)`,
variance markers, GADT variant results, and `pack.*` forms and report one stable
`unsupported-*` code per feature. OPEN_ISSUES question: should the spec define a
single portable "unsupported-feature" category so conformance runners can tell
"not implemented" from "wrong"?
