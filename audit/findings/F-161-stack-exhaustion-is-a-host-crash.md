# F-161: Unbounded recursion crashes the host instead of panicking with `stack-exhausted`
Severity: minor
Area: runtime
Evidence: audit/evidence/01-special-casing/crash-triggers.log (k2); audit/evidence/01-harness/plant-run.log (C14b)
Effect: `fn deep(n: i32) -> i32: deep(n + 1) + 1` ends with a Node `RangeError: Maximum call stack size exceeded` stack trace and exit 1. `stack-exhausted` is a stable panic category in 06-control-flow.md#runtime-panics. No fixture covers it.
Recommendation: implementation change: map the host stack-overflow error to `stack-exhausted: runtime panic`, and add a runtime/panic fixture.
