# F-252: The parser rejects core grammar forms
Severity: major
Area: correctness
Duplicates: F-309 (same-line if/else, found independently by the fuzzer)
Evidence: the 3 cases tagged F-252 in test/portable/KNOWN_FAILURES.tsv
Status: same-line `if c: a else: b` and relative `use self...` / `use super...`
parse since the grammar decisions GQ9 and GQ17 were applied; nested-layout.hd,
grammar-disambiguation.hd, binding-same-line-if-else.hd, and uses.hd moved to
test/portable/cases.tsv.
Effect: Three core, non-deferred forms fail to parse:
- unnamed positional enum payloads such as `Circle(f64)` (spec 08-data-and-enums.md;
  typing/invalid/variant-result-owner.hd:2; probe audit/probes/claims/p95-positional-enum-payload.hd):
  `expected-token: expected ':', found ')'`;
- a parenthesized multiline closure argument (spec 07, Multiple Inline Closures;
  runtime/valid/multiple-inline-closures.hd:18): `expected-newline`;
- tuple match patterns such as `(n, n) =>` (typing/invalid/duplicate-binding-in-one-pattern.hd:5):
  `expected-token: expected a supported match pattern`.
Recommendation: implementation change in the parser; then move the cases to
test/portable/cases.tsv.
