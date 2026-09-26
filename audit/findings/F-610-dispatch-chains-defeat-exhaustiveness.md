# F-610: Expression dispatch is split into `??` chains, so missing kinds fail only at runtime
Severity: note
Area: architecture
Evidence: src/checker/checker.ts:14-27 (10 partial handlers),
  src/emitter/function-body.ts:149-157 (4 partial handlers);
  audit/evidence/06-compiler/function-lengths.md (emitValueExpression 299 lines,
  1 under the 300-line cap)
Effect: When a new HIR or AST kind is added and one handler is forgotten,
  `tsc` stays silent. The compiler throws `unsupported expression` at run time,
  or skips the kind silently in walkers without a `default` (see F-600). The
  split appears to be shaped by the 300-line function cap. 13 functions sit
  at 250-299 lines.
Recommendation: implementation change. Keep one exhaustive `switch` per kind
  that delegates to per-kind helpers, and end it with an `assertNever(kind)`
  default.
