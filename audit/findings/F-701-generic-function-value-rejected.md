# F-701: A generic function cannot be used as a value, even when its type arguments are known
Severity: minor
Area: correctness
Evidence: audit/evidence/w9/verify-findings.log (F-701 block: `hd check spec/conformance/runtime/valid/generic-function-value-instantiation.hd` reports `generic-function-value-needs-arguments` at 14:35 and 19:10, exit 1)
Effect: `let f: fn(string) -> string = identity`, `apply(identity, 42)` and `identity[i32]` are all rejected. Spec 07 (Function Types And Values) says a generic function used as a value is instantiated from the expected monomorphic function type or from a complete explicit type-argument list. The code is also outside the spec/README.md inventory.
Recommendation: implementation change: solve the generic parameters from the expected function type and accept explicit type arguments on a function reference.
