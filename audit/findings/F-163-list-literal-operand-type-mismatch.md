# F-163: Comparing a readonly list binding with a list literal is rejected
Severity: major
Area: correctness
Evidence: audit/evidence/01-special-casing/crash-triggers.log (k6, k7, control k7b)
Effect: `xs := [1, 2]` followed by `xs == [1, 2]` or `xs < [2]` fails with `type-mismatch: operator operands have types list[i32] and mut:list[i32]`. Comparing two bindings works (k7b). A fresh list expression's mutable access "may be weakened immediately by an expected readonly type" (04-type-system.md#bindings-and-fresh-values), so the literal should unify with `list[i32]`.
Recommendation: implementation change: weaken fresh operand access before unifying the operands of `==`, `!=`, and the relational operators. Add a typing/valid fixture.
