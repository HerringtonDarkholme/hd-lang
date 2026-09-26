# F-600: explain-requirements skips 23 HIR kinds and reports real uses as "declared"
Severity: minor
Area: correctness
Evidence: audit/evidence/06-compiler/explain-requirements.md
  (`hd explain-requirements audit/probes/compiler/explain-paths.hd`)
Effect: A requirement reached only through a `for` body, a tuple, an
  interpolation, or a comprehension prints as `in_for -> declared`. A user
  reading the explanation concludes the row entry is unused and can be removed.
  `src/requirements.ts` `visitExpression` has no `default` and no exhaustiveness
  check. 23 of 80 HIR expression kinds fall through silently, including `for`,
  `tuple`, `string-build`, `list-comprehension`, `binding-expression`,
  `field-set`, `list-append`, `map-set`, and `suspension-drive`.
Recommendation: implementation change. Walk HIR generically, as
  `checker/module-initialization.ts` does, or add an exhaustive `never` check.
