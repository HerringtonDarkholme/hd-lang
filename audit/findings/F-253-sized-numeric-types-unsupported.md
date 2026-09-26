# F-253: Sized numeric types (i8-i64, u8-u64, f32) are unimplemented and not listed as deferred
Severity: minor
Area: coverage
Evidence: audit/evidence/02-coverage/unselected-classified.tsv rows typing/valid/numeric-types.hd, numeric-corners.hd, numeric-widening-and-display-list.hd, newtypes-and-widening.hd, typing/invalid/integer-literal-range.hd, integer-narrowing.hd, mixed-signedness.hd, unsigned-negation.hd
Effect: 8 conformance cases fail with `unknown-type: unknown or unsupported type 'u8'` (and `i16`,
`i64`, `u32`, `f32`). The MVP "Deferred Work" list does not name these types, so readers
of src/MVP_IMPLEMENTATION_PLAN.md cannot tell whether they are planned. The code
`unknown-type` is not a structured unsupported diagnostic (MVP goal 5).
Recommendation: OPEN_ISSUES question: are sized numeric types part of the MVP, or
deferred? If deferred, list them in "Deferred Work" and report a dedicated
unsupported code instead of `unknown-type`.
