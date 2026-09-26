# F-257: `mut fn` closures are unsupported
Severity: minor
Area: coverage
Duplicates: F-352 (found independently by another worker; phase-2 evidence corroborates)
Evidence: typing/valid/mutable-closure-capture.hd fails with `expected-expression ... found 'mut'` (audit/evidence/02-coverage/unselected-classified.tsv)
Effect: The spec's only way to mutate captured state (07-functions.md#captures) is missing.
Assigning a captured `let` inside a plain closure now reports `mutable-capture-requires-mut-fn`
(formerly F-357), but no program can write the `mut fn` closure that the diagnostic asks for.
Recommendation: implementation change: support `mut fn`, or report a structured
unsupported code.
