# F-406: Four stable panic categories have no producer, and one panic has no category
Severity: note
Area: spec
Evidence: audit/evidence/04-runtime/panics.tsv; src/runtime-panic.ts (11 names, UNVERIFIED beyond the runs)
Effect: Spec 06 lists 15 categories. Fixtures produce 11 exactly. `stack-exhausted` surfaces as a host `RangeError` (F-161). `failed-checked-cast` has no construct in the spec that produces it. `annotation-reference-unresolved` and `annotation-resolution-reentry` need annotations, which the MVP does not implement. A negative integer exponent must panic (spec 05), but no category is named, and the MVP traps with `RuntimeError: unreachable` (F-153).
Recommendation: OPEN_ISSUES questions: which construct produces `failed-checked-cast`, or should it leave the list? Which category does a negative exponent report?
