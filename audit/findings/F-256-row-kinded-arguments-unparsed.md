# F-256: Row-kinded generic data arguments such as `Job[$()]` fail to parse
Severity: minor
Area: coverage
Evidence: the 2 cases tagged F-256 in test/portable/KNOWN_FAILURES.tsv (typing/valid/row-kinded-arguments.hd, typing/invalid/row-kind-mismatch.hd; re-checked 2026-09-26)
Effect: `fn pure(job: Job[$()]) -> void` fails with `syntax-error: expected '.', found '('`.
Generic requirement-row parameters are claimed for functions (S2), but a data type
parameterised by a row cannot be instantiated. `generic-kind-mismatch` is never reachable.
Recommendation: implementation change in type-argument parsing and kind checking.
