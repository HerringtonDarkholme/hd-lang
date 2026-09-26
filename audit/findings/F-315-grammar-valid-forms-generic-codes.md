# F-315: Grammar-valid forms are rejected at parse with generic codes, not unsupported diagnostics
Severity: note
Area: correctness
Related: F-250, F-252 (positional payloads), and F-254 (local declarations); empty pattern clauses, variant generics, and bodyless inherent impls are new here
Evidence: audit/evidence/03-fuzz/round1/parse/signatures.tsv (expected-expression 431, expected-token 362, missing-impl-body 20); replay of audit/fuzz/findings/F-315-impl-local-data.hd
Effect: `hd parse` rejects these grammar-valid inputs with `expected-expression`, `expected-token`, or `missing-impl-body`, none of them spec codes:
- local `data` and `type` declarations in bodies and trailing blocks;
- empty pattern argument clauses (`A() => ...`);
- positional variant payloads (`A(i32)`);
- variant generic parameters (`If[T]`);
- bodyless inherent `impl Point`.
The README requires unsupported syntax to be diagnosed, and it is. But a generic parse error reads as the user's mistake. The MVP's structured `unsupported-*` codes cover generic bounds but not these forms.
Recommendation: implementation change: extend the `unsupported-*` diagnostics to these forms. OPEN_ISSUES question: should the spec reserve an `unsupported-*` code family?
