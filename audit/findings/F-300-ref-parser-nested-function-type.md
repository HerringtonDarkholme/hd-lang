# F-300: Reference parser rejects `fn() -> fn() -> T` unless a requirement clause is present
Severity: major
Area: spec
Evidence: audit/evidence/03-fuzz/replay.txt (F-300 block); fixture audit/fuzz/findings/F-300-ref-nested-function-type.hd; audit/evidence/03-fuzz/grammar-check.txt
Effect: `parseSource` reports `syntax-error` for a valid curried function type in any type position: parameter, `let` annotation, alias, or type argument. Examples: `let x: fn() -> fn() -> i32 = g` and `fn f(g: fn() -> fn() -> i32) -> i32: 1`. Adding `$ Clock` to the inner type makes it parse. `fn f() -> fn() -> i32: g` also parses, because the outer arrow belongs to a declaration, not a second `function_type`. hd accepts all of these. The spec oracle therefore rejects valid programs, and a conformance case using a curried type would fail `spec/check.sh`.
Recommendation: implementation change (reference parser). In the Earley predictor, when the predicted symbol is nullable and already completed at the current position, also advance the predicting item (the Aycock-Horspool fix).
Mechanism (read from spec/reference-parser/grammar.ts): `[requirement_clause]` compiles to one shared nullable helper. The inner `function_type` completes that helper empty at the end position. The outer item predicts the same helper later, `addItem` returns false, and the item never advances.
