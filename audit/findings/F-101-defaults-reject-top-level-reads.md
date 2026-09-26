# F-101: Parameter and data-field defaults reject any read of a top-level binding
Severity: minor
Area: correctness
Evidence: audit/evidence/01-spec-edits/probe-runs.log (probes audit/probes/spec-edits/h1-default-reads-short-binding.hd, h1-default-reads-constant-global.hd, h1-default-reads-mutable-global.hd, h1-default-calls-global-reader.hd, h1-data-default-reads-short-binding.hd)
Effect: `limit := 10` followed by `fn current(value: i32 = limit) -> i32` fails with `impure-parameter-default` at 7:25. A data field `size: i32 = limit` fails with `impure-data-default`. The spec allows both. Chapter 07 forbids writes, provider access, and suspension in defaults, not reads. The window edit to 07-functions.md adds that purity "does not claim referential transparency." No conformance fixture covers a default that reads a top-level binding, so the portable suite does not catch this.
Recommendation: implementation change: treat reads of top-level bindings as pure in defaults, and in the functions they call. Add a `typing/valid` fixture whose default reads a top-level `:=` binding.
