# F-268: `typing/valid/explicit-generic-method.hd` panics when executed
Severity: note
Area: test-integrity
Evidence: audit/evidence/02-coverage/examples/examples-fixtures.tsv (test_run `fail(panic:explicit-panic)`); `hd test spec/conformance/typing/valid/explicit-generic-method.hd` prints `explicit-panic: runtime panic`
Effect: The fixture's module body calls `convert` and `parser.parse`, whose bodies are
`panic(...)`. The type-phase expectation passes, but running the spec example through
the full pipeline always panics, so the chapter-07 example cannot serve as a runtime check.
Recommendation: implementation change to the fixture: move the panicking calls into
functions that are never called, or document it as type-only in examples.tsv.
