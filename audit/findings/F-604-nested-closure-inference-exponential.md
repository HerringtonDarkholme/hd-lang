# F-604: Checking nested unannotated closures doubles in time per nesting level
Severity: minor
Area: architecture
Evidence: audit/evidence/06-compiler/nested-closures.md
  (`node --experimental-strip-types audit/scripts/compiler/nested-closures.ts ...`)
Effect: With `f := fn(): ...` nested N deep, checking takes 55 ms at depth 14,
  0.4 s at depth 17, and 3.2 s at depth 20. The time ratio per level is about
  2. Deep callback nesting, such as trailing blocks inside trailing blocks,
  makes compile time grow fast.
  When a closure omits its row or result, `checkClosureExpression`
  (src/checker/checker.ts:137-181) checks the body once for discovery and
  again for real. Each check re-enters every nested closure.
Recommendation: implementation change. Infer the row and result in one pass
  and reuse the discovered HIR, or cache discovery per closure expression.
