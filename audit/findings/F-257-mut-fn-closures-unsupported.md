# F-257: `mut fn` closures are unsupported, and assigning a captured `let` reports `unknown-name`
Severity: minor
Area: coverage
Duplicates: F-352, F-357 (found independently by another worker; phase-2 evidence corroborates)
Evidence: typing/valid/mutable-closure-capture.hd fails with `expected-expression ... found 'mut'` (audit/evidence/02-coverage/unselected-classified.tsv); audit/probes/claims/c2-plain-closure-assigns-let.hd gives `unknown-name: unknown binding 'count'` (audit/evidence/02-claims/repros.log)
Effect: The spec's only way to mutate captured state (07-functions.md#captures) is missing.
Assigning a captured `let` inside a closure tells the user the variable does not exist,
which is false and hides the real rule.
Recommendation: implementation change: support `mut fn`, or report a structured
unsupported code; report `mutable-capture-requires-mut-fn` for captured assignment.
