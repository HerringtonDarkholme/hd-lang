# F-256: Row-kinded generic data arguments such as `Job[$()]` fail to parse
Severity: minor
Area: coverage
Evidence: audit/evidence/02-coverage/unselected-classified.tsv rows typing/valid/row-kinded-arguments.hd, typing/invalid/row-kind-mismatch.hd
Effect: `fn pure(job: Job[$()]) -> void` fails with `expected-token: expected '.', found '('`.
Generic requirement-row parameters are claimed for functions (S2), but a data type
parameterised by a row cannot be instantiated. `generic-kind-mismatch` is never reachable.
Recommendation: implementation change in type-argument parsing and kind checking.
