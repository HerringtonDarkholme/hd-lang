# Escape and outlive checks miss local references stored through a std List

Rule: `ref.local.escape`, `flow.for.cursor.outlives`: a local reference
stored into a list (or yielded by its cursor) may not outlive its local.

Today (lane 299 WIP): no diagnostic for
`lifetime-store-element-escapes`, the three
`local-reference-escapes-function-value-written-lifetime-*list-element*`
cases, `join-cold-list-holds-local-argument`,
`suspension-forward-list-holds-local-argument`,
`for-cursor-item-outlives-local`, `for-mut-cursor-item-outlives-local`.
Each stores or yields the reference through `FromListLiteral`'s builder,
`IndexSet` or `ListIter`, where the builtin list's `NewList`/element
place carried it.

Where: `crates/hd_check/src/dep_results` (what a builder `push`, an
`index_set` value argument and a cursor's `next` result hold).

Fix: the value argument of `push`/`index_set` flows into the receiver's
held set (a store into `*self`), so `List[&'a T]`'s lifetime is the
union; then the existing escape check applies.

Done: the eight cases reject with their expected codes in three modes.
