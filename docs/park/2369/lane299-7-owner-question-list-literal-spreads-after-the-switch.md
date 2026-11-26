# Owner question: what may a spread in a `List` literal be, once `List` builds through `FromListLiteral`?

Rule today: `expr.list.spread.type` (a literal that produces a `List`:
the spread operand must be a `List[U]`, `type-mismatch`) and
`expr.list.spread.iterable` (a literal that builds through
`FromListLiteral`: any `for` source, `unsatisfied-trait-bound`).

After the switch, `List` is the only std type implementing
`FromListLiteral`, so every literal that produces a `List` also builds
through it, and the two rules give different answers for `[0, 1..3...]`.

Options:
1. Keep `.type` for `List` (no program changes); `.iterable` then has
   no carrier in std and its two cases
   (`list-literal-spread-of-any-iterable-into-a-list.hd`,
   `...-of-a-non-iterable-into-a-list.hd`, converted from PlainList by
   lane 299) are deleted or moved to a user type.
2. One rule: any `for` source spreads into any built literal, `List`
   included (`.type` deleted; `[xs...]` of a `List` still drains the
   list, so no accepted program changes meaning; a non-iterable reports
   `unsatisfied-trait-bound` instead of `type-mismatch`).

Recommendation: 2 (fewest rules; the builder path already drains any
iterable, and the `List`-only check is the one extra line in
`list_expr`). Lane 299 left `.type` in place.
