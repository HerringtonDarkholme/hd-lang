# F-254: `type` declarations and local type/impl declarations are rejected as syntax errors
Severity: minor
Area: coverage
Evidence: audit/evidence/02-coverage/unselected-classified.tsv rows parse/valid/declarations.hd, typing/valid/local-types.hd, typing/invalid/local-impl-nonlocal-pair.hd, typing/invalid/nominal-map-key.hd
Effect: `type UserId(string)` and `type UserName = string` fail with
`expected-expression: expected an expression, found 'type'`; declarations inside a function
body (`type`, `data`, `enum`, `trait`, `impl`) fail the same way. Neither feature is in the
deferred list. The user sees a parse error for valid code.
Recommendation: implementation change (support or emit a structured unsupported
code); OPEN_ISSUES question on whether newtypes and local declarations belong in the MVP.
