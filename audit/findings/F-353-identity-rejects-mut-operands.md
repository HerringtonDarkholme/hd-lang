# F-353: `is` rejects operands whose static type is `mut T`
Severity: minor
Area: correctness
Evidence: audit/evidence/03-blind-run/probes.log (p-is-mut-both.hd: `identity-requires-references: identity comparison does not accept 'mut:User'`; p-is-mut-vs-readonly.hd and p-is-mut-list-literal.hd: `type-mismatch: identity operands have types mut:User and User`); audit/evidence/03-blind-run/run.log (a5-composite-identity-and-views, a8-copy-update-shallow); audit/evidence/03-blind-run/variants.log (a8-copy-update-shallow.weakened.hd passes once operands are weakened)
Effect: `first is alias` with `first, alias: mut User` fails to compile, and so does `values is [1, 2]` because a list literal is `mut`. spec/05-expressions.md#unary-and-binary-operators: data values and collections have allocation identity and "access permission (`mut`) does not change it". Users must copy into readonly bindings before comparing.
Recommendation: implementation change: strip `mut` from both operand types before the identity-type and compatibility checks. The exact compatibility rule (for example `list[User]` against `mut list[mut User]`) is an OPEN_ISSUES question (AMB-24).
