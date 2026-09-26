# F-252: The parser rejects core chapter-02 forms that the reference parser accepts
Severity: major
Area: correctness
Evidence: audit/evidence/02-coverage/unselected-classified.tsv; `node --experimental-strip-types audit/scripts/coverage/refparse.ts spec/conformance/parse/valid/{nested-layout,grammar-disambiguation,uses,declarations}.hd` prints `[]` (accepted) for each, while `hd parse` rejects them
Effect: Three core, non-deferred forms fail to parse:
- single-line `if c: a else: b` (nested-layout.hd:2, grammar-disambiguation.hd:10): `expected-newline`;
- relative `use self.local.{Config}` / `use super.shared.{Email}` (uses.hd:6): `expected-token`;
- unnamed positional enum payloads such as `Circle(f64)` (spec 08-data-and-enums.md line 200;
  variant-result-owner.hd:2; probe audit/probes/claims/p95-positional-enum-payload.hd, which the
  reference parser accepts): `expected-token: expected ':', found ')'`.
Four `parse/valid` fixtures (plus decorators.hd, which is deferred) fail although the S0 gate reports green, because the gate only
checks the selected subset.
Recommendation: implementation change in the parser; then add the fixtures to
test/portable/cases.tsv. The S0 gate should also compare every parse fixture with the
reference parser, not only the selected ones.
