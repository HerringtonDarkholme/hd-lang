# F-252: The parser rejects core grammar forms
Severity: major
Area: correctness
Duplicates: F-309 (same-line if/else, found independently by the fuzzer); F-315 (merged: grammar-valid forms rejected with generic parse codes)
Evidence: the 5 cases tagged F-252 in test/portable/KNOWN_FAILURES.tsv; audit/probes/claims/p95-positional-enum-payload.hd; `hd parse` on the last two one-line forms below (re-checked 2026-09-26)
Effect: These core, non-deferred forms fail to parse:
- unnamed positional enum payloads such as `Circle(f64)` (spec 08-data-and-enums.md;
  typing/invalid/variant-result-owner.hd:2): `syntax-error: expected ':', found ')'`;
- tuple match patterns such as `(n, n) =>` (typing/invalid/duplicate-binding-in-one-pattern.hd:5):
  `syntax-error: expected a supported match pattern`;
- a closing delimiter ending the last body line of a nested `match` or `if`
  suite, as in `x := (match c:` ... `false => 0)` (spec 01, Physical And
  Logical Lines; parse/valid/closing-delimiter-ends-nested-suite.hd:6):
  `syntax-error: expected a line ending after a match header`;
- a bracketed `for`, `if`, or closure header ending its line, as in
  `[for x in xs:` (parse/valid/bracketed-suite-header-line.hd:5):
  `expected-comprehension-clause`;
- `else` after a same-line `for` or `while` suite
  (parse/valid/same-line-loop-else.hd:6): `expected-expression`;
- an empty pattern argument clause on a bare variant name, `A() => ...`:
  `syntax-error: expected '=>', found '('`;
- a bodyless inherent implementation, `impl Point` (02-grammar.md
  `impl_decl`): `missing-impl-body`, a code outside the spec inventory.
Recommendation: implementation change in the parser; then move the cases to
test/portable/cases.tsv. Multi-statement closure bodies inside brackets are
tracked as GQ2, and local declarations as F-254.
