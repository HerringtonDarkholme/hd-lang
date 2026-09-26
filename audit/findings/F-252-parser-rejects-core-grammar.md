# F-252: The parser rejects core grammar forms
Severity: major
Area: correctness
Duplicates: F-309 (same-line if/else, found independently by the fuzzer)
Evidence: the 7 cases tagged F-252 in test/portable/KNOWN_FAILURES.tsv; `hd parse spec/conformance/parse/valid/nested-layout.hd`
Effect: Five core, non-deferred forms fail to parse:
- same-line `if c: a else: b`, which chapter 01 uses as an example
  (`x := if c: 1 else: 2` "is one conditional expression"): `expected-newline:
  expected a line ending, found 'else'` (parse/valid/nested-layout.hd:2,
  grammar-disambiguation.hd:10, binding-same-line-if-else.hd:3);
- relative `use self.local.{Config}` / `use super.shared.{Email}` (parse/valid/uses.hd:6):
  `expected-token: expected a module path after use`;
- unnamed positional enum payloads such as `Circle(f64)` (spec 08-data-and-enums.md;
  typing/invalid/variant-result-owner.hd:2; probe audit/probes/claims/p95-positional-enum-payload.hd):
  `expected-token: expected ':', found ')'`;
- a parenthesized multiline closure argument (spec 07, Multiple Inline Closures;
  runtime/valid/multiple-inline-closures.hd:18): `expected-newline`;
- tuple match patterns such as `(n, n) =>` (typing/invalid/duplicate-binding-in-one-pattern.hd:5):
  `expected-token: expected a supported match pattern`.
Recommendation: implementation change in the parser (for same-line suites, chapter 01:
"`else` is also a boundary"); then move the cases to test/portable/cases.tsv.
