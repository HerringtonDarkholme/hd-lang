# F-710: A generic bound written with `:` is accepted
Severity: minor
Area: correctness
Evidence: `hd parse` and `hd check` on spec/conformance/parse/invalid/colon-generic-bound.hd both print `ok`, exit 0 (2026-09-26)
Effect: `fn render[T: Show](value: T) -> string` compiles. Decision S1 made `<` the only bound token (02-grammar.md, Generic Parameters And Bounds), so a conforming parser reports `syntax-error`. Code written in the old spelling passes the prototype and fails elsewhere.
Recommendation: implementation change: report `syntax-error` for `:` after a generic parameter name; then move the case to test/portable/cases.tsv.
