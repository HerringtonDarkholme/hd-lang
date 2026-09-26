# F-608: Program-wide numbering makes one inserted declaration rewrite half the WAT
Severity: note
Area: architecture
Evidence: audit/evidence/06-compiler/wat-stability.md
  (`node --experimental-strip-types audit/scripts/compiler/wat-stability.ts audit/probes/compiler/scale-100.hd`)
Effect: In a 628-function module, changing one literal alters 1 function body.
  Inserting one function before the others alters 308 bodies. Inserting one
  closure in the first function alters 204 bodies. A per-function cache of HIR
  or WAT would miss on almost every structural edit.
  Function, closure (`closures.push`, src/checker/checker.ts:120-181),
  signature, adapter, and provider-key indices are all assigned in visit order.
  They are baked into names such as `$f12`, `$c3`, and `$sig4`.
Recommendation: implementation change, if incremental compilation is a goal.
  Key declarations by stable path-based IDs, and number them only at final
  module assembly.
