# F-352: `mut fn` closure literals do not parse
Severity: minor
Area: coverage
Evidence: audit/evidence/03-blind-run/refparser.log section "mut fn closure literal" (p-mut-fn-closure.hd: `parseSource` ACCEPT; `hd parse` exit 1, `expected-expression: expected an expression, found 'mut'`); audit/evidence/03-blind-run/run.log (a6-closure-capture-permission, a6-closure-capture-runtime)
Effect: the spec 07#captures example `let next: mut fn() -> i32 = mut fn() -> i32: ...` is rejected by `hd parse` with a generic syntax error. The `mut fn(...)` type parses, so a program can name the type but never construct a value of it. The grammar (spec/02-grammar.md `closure_expression = [ "mut" ], "fn", ...`) allows it, and the reference parser accepts it. src/README.md does not list it as deferred, and there is no stable unsupported-feature code.
Recommendation: implementation change: parse and check `mut fn` literals, or reject them with a stable unsupported-feature diagnostic listed in src/README.md until they land.
