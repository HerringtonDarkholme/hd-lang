# F-263: Primitive types do not satisfy `PartialEq`, `PartialOrd`, or `Display` bounds
Severity: major
Area: correctness
Duplicates: F-350, F-500 (found independently by another worker; phase-2 evidence corroborates)
Evidence: audit/evidence/02-claims/repros.log (F-263); audit/probes/claims/p92-primitive-bounds.hd; scratch runs of `fn f[T: PartialOrd](a: T, b: T) -> bool: a < b` called with `1, 2`
Effect: `same(1, 1)` with `T: PartialEq`, `less("a", "b")` with `T: PartialOrd`, and
`show(5)` with `T: Display` all fail with `missing-trait-implementation: type 'i32' does not
implement PartialEq` (same for string, f64). 05-expressions.md line 346 says the standard
library implements these traits for primitives. Generic code works only for user types.
Recommendation: implementation change: register the standard primitive
implementations as dictionaries for bound satisfaction, not only for direct operators.
