# F-353: `is` rejects operands whose static type is `mut T`
Severity: major
Area: correctness
Duplicates: F-261 (merged)
Evidence: `hd test audit/probes/claims/p93-identity-mut-operands.hd` reports `identity-requires-references: identity comparison does not accept 'mut:Box'`; the cases tagged F-353 and L7 in test/portable/KNOWN_FAILURES.tsv
Effect: `first is alias` with `first, alias: mut User` fails with
`identity-requires-references`, and readonly-against-`mut` operands fail with
`type-mismatch: identity operands have types mut:User and User`. A list literal is
`mut`, so `values is [1, 2]` fails too. spec/05-expressions.md says access permission
does not change identity, and rule L7 settles operand compatibility: types are
compared with `mut` removed at every level. Users must copy into readonly bindings
before comparing.
Recommendation: implementation change: apply rule L7 (strip `mut` at every level
before the identity-type and compatibility checks).
