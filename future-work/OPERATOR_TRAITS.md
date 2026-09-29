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

Points the apply passes met (2026-09-29). The specification states the
reading in the Applied column, so each can change without breaking a
decision. Each waits for the owner.

| # | Point | Applied | **Recommendation** |
| --- | --- | --- | --- |
| 3 | OP5 syntax: which tokens, and where may the right side be a suite? | Ten tokens (`**=` is not one), and the right side takes the forms of `=`, suites included ([`grammar.stmt.compound-assign`](../spec/02-grammar.md#r-grammar.stmt.compound-assign)) | Keep. |
| 19 | On a built-in `Map`, `counts[w] += 1` reads `counts[w]` as `i32?`, so it is `type-mismatch`, while stress decision 11 says `m[k] += v` works | The read behaves as any read of `m[k]` ([`expr.assign.compound.map-read`](../spec/05-expressions.md#r-expr.assign.compound.map-read)); a `List` element and a user index place work | Confirm, since both decisions say the read is `m[k]`. The word count then writes `counts[w] = counts[w].unwrap_or(0) + 1`, or a STDLIB update method. |
| 21 | OP12 names construction. Does unwrapping, `Draft(order)` with `order: mut Order`, give `mut Draft`? | No rule; the prototype gives a readonly `Draft`, as for a field declared `Draft` | Unwrap with the newtype's permission, so the two directions match. |
