# F-601: explain-requirements shows closure-routed requirements as a direct `$.use`
Severity: minor
Area: correctness
Evidence: audit/evidence/06-compiler/explain-requirements.md (row-local.hd),
  audit/evidence/06-compiler/row-ripple.md (explain-requirements diff)
Effect: `outer` has no `$.use`. It reaches `Logger` through a local function
  and a closure, but the tool prints `Logger: outer -> $.use(Logger)` twice.
  In the row-ripple probe, `f0 -> $.use(Cache)` is printed, but the real path
  is f0 -> closure -> f2 -> ... -> f30. The provider forwarded into a closure
  call is a checker-synthesized `provider-use` node, and the walker cannot tell
  it apart from a source `$.use`. Closures are never listed.
Recommendation: implementation change. Mark synthesized provider forwarding
  in HIR, or follow `closure-call` into `program.closures`.
