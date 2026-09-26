# F-262: Any `f64` `<`, `<=`, `>`, or `>=` crashes code generation
Severity: blocker
Area: correctness
Duplicates: F-160 (found independently by another worker; phase-2 evidence corroborates)
Evidence: audit/evidence/02-claims/repros.log (F-262); audit/evidence/02-claims/f64-relational-wat.txt (the emitted body has 31 `(` and 30 `)`; Binaryen: `685:1: error: expected end of module`); probes audit/probes/claims/p91-f64-relational.hd, p42-ordering-nan.hd, wat/f64-gt-minimal.hd
Effect: `fn gt(a: f64, b: f64) -> bool: a > b` passes `hd check`, then `hd build`, `hd run`,
and `hd test` die with an uncaught `WasmValidationError` stack trace. The same happens for
tuples, lists, and optionals with `f64` components. The claim "unordered floating components
make all four relational operators false" (MVP plan line 235) cannot be exercised. No
fixture compares two `f64` values relationally, so the suite stays green.
Recommendation: implementation change: close the missing parenthesis in the f64
`partial_cmp` lowering; add a runtime conformance case covering ordered, equal, and NaN operands.
