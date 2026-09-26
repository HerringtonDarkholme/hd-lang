# F-204: Three panic fixtures mark a line that does not cause the panic
Severity: minor
Area: test-integrity
Evidence: audit/evidence/01-test-quality/oracle.tsv (verdict `code-persists` for compiler/46, runtime/panic/competing-suspension-drivers.hd, runtime/panic/reentrant-suspension-poll.hd); audit/evidence/01-test-quality/probes.log (the neutralized copies still panic with the marked code)
Effect: compiler/46 puts `# panic: explicit-panic` on the defer body that must not run. The panic really comes from `panic("boom")` on line 6. An implementation that wrongly runs the defer still passes. The two scenario fixtures panic even when `main!` is only `pass`, so their marked line tests nothing. Because panic lines are unchecked (F-155), nothing reports this.
Recommendation: fixture change: move the compiler/46 marker to line 6. OPEN_ISSUES question: should a scenario-driven panic carry a line marker, or a file-level expectation?
