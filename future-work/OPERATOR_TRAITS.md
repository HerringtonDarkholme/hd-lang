# Operator Traits: Open Points

Status: open. Nothing here is accepted behavior. Owner decisions OP1-OP13
(2026-09-29) are applied, and the specification is authoritative for them:
[Operator Traits](../spec/05-expressions.md#operator-traits),
[Primitive Implementations](../spec/05-expressions.md#primitive-implementations),
[Compound Assignment](../spec/05-expressions.md#compound-assignment),
[Index Traits](../spec/05-expressions.md#index-traits),
[Supertrait Bindings](../spec/09-traits.md#supertrait-bindings), and
[Numeric Traits](../spec/09-traits.md#numeric-traits). Since OP13, there
are no assign traits: `a op= b` always means `a = a op b`. The survey, the
options, and the decision log are in git history.

## Still Open

None. The owner answered points 3, 19, and 21 on 2026-09-29 as the
[evening follow-ups](OPEN_ISSUES.md#follow-ups-decided-2026-09-29-evening)
7, 5, and 6, and they are applied:

| # | Answer | Rules |
| --- | --- | --- |
| 3 | The ten tokens and the right side's forms are confirmed | [`grammar.stmt.compound-assign`](../spec/02-grammar.md#r-grammar.stmt.compound-assign) |
| 19 | `m[k] op= v` on a `Map` reads the entry as `V` and panics when the key is missing | [`expr.assign.compound.map-present`](../spec/05-expressions.md#r-expr.assign.compound.map-present), [`expr.assign.compound.map-missing`](../spec/05-expressions.md#r-expr.assign.compound.map-missing) |
| 21 | Unwrapping a newtype carries its permission | [`types.newtype.unwrap-permission`](../spec/04-type-system.md#r-types.newtype.unwrap-permission) |
