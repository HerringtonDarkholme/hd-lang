# F-261: `is` rejects operands with `mut` access
Severity: major
Area: correctness
Duplicates: F-353 (found independently by another worker; phase-2 evidence corroborates)
Evidence: audit/evidence/02-claims/repros.log (F-261); audit/probes/claims/p93-identity-mut-operands.hd
Effect: With `let original: mut Box = ...`, `original is original` fails with
`identity-requires-references: identity comparison does not accept 'mut:Box'`, and
`view is original` (readonly vs mut) fails with `type-mismatch: identity operands have types
Box and mut:Box`. 05-expressions.md lines 356-371 says access permission does not change
identity. Any identity check on a mutable value is impossible.
Recommendation: implementation change: strip access permission before the identity
operand check and the compatibility check.
