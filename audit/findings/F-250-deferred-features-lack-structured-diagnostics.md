# F-250: Deferred features are rejected with generic or wrong diagnostics, not structured unsupported diagnostics
Severity: major
Area: coverage
Duplicates: F-312 (merged: every decorator is rejected by a hard-coded `decorator-not-top-level` in src/parser/parser.ts `parseStatement`)
Evidence: audit/evidence/03-fuzz/findings/F-312-impl-decorator-misreported.hd; the 51 cases tagged F-250 in test/portable/KNOWN_FAILURES.tsv; run them with `node --experimental-strip-types spec/tools/run-conformance.ts --manifest LIST`
Effect: Of the 51 conformance cases that exercise deferred features (annotations,
decorators, packs, GADTs, declared variance, shapes, packages), only 6 get a
structured code (`unsupported-generic-parameter`, all for packs). 10 get
`decorator-not-top-level` on a decorator that is at top level, a spec code with
a false meaning. 2 exit with usage text because the CLI has no
`--package-role` or `--dependency` option. The other 33 get generic parser or
checker codes (`expected-expression` 19, `expected-token` 9, `unknown-trait` 2,
`unknown-type` 2, `unknown-name` 1). A user writing `@derive(PartialEq)` or
`data Producer[+T]:` is told the syntax is malformed rather than unimplemented.
MVP goal 5 ("reject unimplemented language features with structured
diagnostics") is not met for these slices.
Recommendation: implementation change: recognise decorator, `annotate`, `shape(...)`,
variance markers, GADT variant results, and `pack.*` forms and report one stable
`unsupported-*` code per feature. OPEN_ISSUES question: should the spec define a
single portable "unsupported-feature" category so conformance runners can tell
"not implemented" from "wrong"?
