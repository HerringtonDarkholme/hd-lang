# F-359: The reference parser flags `g(f(a=1, b=2), 3)` as `argument-order`
Severity: minor
Area: spec
Evidence: audit/evidence/03-blind-run/refparser.log (amb02-nested-named-arguments.hd: `argument-order@10`; `hd parse` exits 0; control amb02-control-positional-inner.hd accepted by both)
Effect: a positional argument that is itself a call with named arguments makes the next positional argument look out of order. spec/02-grammar.md `argument_list` accepts the program: the outer call has two positional arguments. spec/reference-parser/contextual.ts `argumentOrderDiagnostics` treats any part matching `/^[^=,:]+=(?!=)/` as named, which matches `f(a=1`.
Recommendation: spec change: classify a part as named only when it starts with `identifier =`; add a parse/valid fixture.
