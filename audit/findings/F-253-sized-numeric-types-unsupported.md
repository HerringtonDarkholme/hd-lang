# F-253: Sized numeric types (i8-i64, u8-u64, f32) are unimplemented and not listed as deferred
Severity: minor
Area: coverage
Evidence: the 10 cases tagged F-253 in test/portable/KNOWN_FAILURES.tsv (for example typing/valid/numeric-types.hd, typing/invalid/integer-literal-range.hd, runtime/valid/i64-u64-precision-through-generics.hd)
Effect: 10 conformance cases fail with `unknown-type: unknown or unsupported type 'u8'` (and `i16`,
`i64`, `u32`, `f32`). Decision L2 also needs `u32`. The "Deferred Work" list in
src/MVP_IMPLEMENTATION_PLAN.md does not name these types, so readers cannot tell
whether they are planned. The code `unknown-type` is not a structured unsupported
diagnostic (MVP goal 5).
Recommendation: implementation change: add the sized types, or list them in
"Deferred Work" and report a dedicated unsupported code instead of `unknown-type`.
