# F-160: Any f64 `<`, `<=`, `>`, or `>=` crashes code generation
Severity: major
Area: correctness
Evidence: audit/evidence/01-gap-probes/results.tsv (G06a, G06b, G06c); audit/evidence/01-special-casing/crash-triggers.log (k1)
Effect: `fn lt(a: f64, b: f64) -> bool: a < b` passes `hd check`, but `hd run`, `hd test`, and `hd build` crash with `WasmValidationError: Binaryen could not parse generated WAT: [object Object]`. The raw Binaryen error is `expected end of module`. This covers even `1.0 < 2.0`, so the NaN ordering rule (05-expressions.md#unary-and-binary-operators) cannot run at all. `src/README.md` claims "scalar and string comparisons". No selected fixture orders an `f64` at runtime, so `npm run check` passes.
Recommendation: implementation change: the f64 template in `emitValueOrdering` (src/emitter/value-comparison.ts) has 19 `(` and 18 `)`. Add the missing `)`, then add a runtime fixture covering all four operators on ordered values and on NaN. Also report the Binaryen message instead of `[object Object]`.
