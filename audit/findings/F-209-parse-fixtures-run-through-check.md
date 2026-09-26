# F-209: test/fixtures has no phase, so lexer and parser rejections run through `check`
Severity: note
Area: test-integrity
Evidence: audit/evidence/01-test-quality/stub-runner.log; reference-parser comparison in SUMMARY.md (5 frontend reject fixtures get the same code and line from the reference parser, yet fail under the stub because the runner calls `check`)
Effect: a parser-only implementation or the reference parser cannot pass lexer and parser rejection fixtures in test/fixtures. spec/conformance avoids this with the `parse` phase in cases.tsv.
Recommendation: fixture change: promote the parse-level fixtures into spec/conformance/parse/invalid, where the phase is explicit.
