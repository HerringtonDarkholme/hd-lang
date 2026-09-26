# F-205: Half of test/fixtures reject markers use diagnostic codes that spec/README.md does not define
Severity: major
Area: spec
Evidence: audit/evidence/01-test-quality/selfcontain.tsv (flag `nonspec-code`: 69 of 140 markers, 47 distinct codes); spec/README.md code table
Effect: these fixtures cannot be promoted to spec/conformance without inventing codes. The missing codes include the most common checker errors: `type-mismatch` (12 uses), `unknown-name`, `unknown-type`, `generic-type-mismatch`, `unreachable-match-arm`, `missing-trait-implementation`, and lexer errors `expected-token`, `inconsistent-dedent`, `invalid-integer-literal`. The reference parser also emits unlisted codes (`syntax-error`, `invalid-dedent`, `unterminated-string`).
Recommendation: OPEN_ISSUES question: should the stable inventory cover general type mismatch, unknown names and types, and generic syntax errors, or should conformance cases avoid those failure classes?
