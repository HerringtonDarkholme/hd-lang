# F-202: `expect-result` compares JavaScript formatting of Wasm return values, not a specified rendering
Severity: major
Area: test-integrity
Evidence: audit/evidence/01-test-quality/probes.log (`hd run` of compiler/06-floating-power... prints `3`; compiler/42-character-literals... prints `128512`); src/cli.ts `console.log(result)`; spec/04-type-system.md "Display ... Fixed notation always contains a decimal point" (so 3.0 renders `3.0`)
Effect: an implementation that prints results with the language's own `Display` fails compiler/06 (`main = 3` for an f64) and compiler/42 (`main = 128512` for a char). Any implementation that returns i64, bool, or string results must guess the MVP's JS encoding. 140 fixtures (169 directives) depend on this unspecified rendering.
Recommendation: OPEN_ISSUES question: should `expect-result` values be the `Display` rendering of the returned value, or should the directive be replaced by in-source `assert_equal` checks run through `test`? The fixture-format proposal drafts the Display option.
