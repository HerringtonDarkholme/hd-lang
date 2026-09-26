# F-257: `mut fn` closure literals do not parse
Severity: minor
Area: coverage
Duplicates: F-352 (found independently by the blind run)
Evidence: the 3 cases tagged F-257 in test/portable/KNOWN_FAILURES.tsv; `hd check spec/conformance/typing/valid/mutable-closure-capture.hd` fails with `expected-expression: expected an expression, found 'mut'`
Effect: The spec's only way to mutate captured state (07-functions.md#captures) is missing.
The grammar allows `closure_expression = [ "mut" ], "fn", ...` and the `mut fn(...)` type
parses, so a program can name the type but never construct a value of it. Assigning a
captured `let` inside a plain closure reports `mutable-capture-requires-mut-fn`, but no
program can write the `mut fn` closure that the diagnostic asks for.
Recommendation: implementation change: parse and check `mut fn` literals, or report a
structured unsupported code until they land.
