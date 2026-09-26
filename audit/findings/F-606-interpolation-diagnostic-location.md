# F-606: Diagnostics inside `${...}` interpolation are reported at 1:1
Severity: minor
Area: correctness
Evidence: audit/evidence/06-compiler/interpolation-span.md
  (`hd check audit/probes/compiler/interp-span.hd`, `interp-span2.hd`)
Effect: `_ := "value ${missing_name}"` on line 3 reports
  `interp-span.hd:1:1: unknown-name`. A type error inside an interpolation is
  also reported at 1:1. Editors and users are sent to the wrong line.
  The parser re-lexes each interpolation fragment as a new source
  (src/parser/parser.ts:136-140 `parseExpressionSource`) and never offsets its
  spans to the enclosing string token.
Recommendation: implementation change. Rebase fragment spans onto the
  string token's start position.
