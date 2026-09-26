# F-602: Module-initialization check takes exponential time on call DAGs
Severity: major
Area: architecture
Evidence: audit/evidence/06-compiler/init-dag.md
  (`node --experimental-strip-types audit/scripts/compiler/init-dag.ts ...`)
Effect: A top-level binding that calls a chain where each function calls the
  next twice takes 1.8 s to check at depth 20 and 8.6 s at depth 22. The same
  program without the top-level binding checks in 1 ms. `hd check` time doubles
  with each level. Any script whose initializer reaches a helper through
  several paths pays for every path.
  `checkModuleInitialization` (src/checker/module-initialization.ts) removes
  each function from `activeFunctions` after visiting it. It never memoizes
  per-function read sets, so it walks every call path.
Recommendation: implementation change. Compute each function's transitive
  global read set once (SCC or memoized DFS) and reuse it per statement.
