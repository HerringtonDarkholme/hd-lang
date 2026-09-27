# F-163: Comparing a readonly list binding with a list literal is rejected
Severity: major
Area: correctness
Evidence: `hd check` on `xs := [1, 2]` then `xs == [1, 2]` reports `type-mismatch: operator operands have types List[i32] and mut:List[i32]`
Effect: `xs := [1, 2]` followed by `xs == [1, 2]` or `xs < [2]` fails with `type-mismatch: operator operands have types List[i32] and mut:List[i32]`. Comparing two bindings works. A fresh list expression's mutable access "may be weakened immediately by an expected readonly type" (04-type-system.md#bindings-and-fresh-values), so the literal should unify with `List[i32]`.
Recommendation: implementation change: weaken fresh operand access before unifying the operands of `==`, `!=`, and the relational operators. Add a typing/valid fixture.
