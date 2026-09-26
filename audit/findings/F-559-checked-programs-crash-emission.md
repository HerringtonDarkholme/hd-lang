# F-559: two type-checked fixtures crash Wasm emission with an internal error
Severity: minor
Area: correctness
Evidence: audit/evidence/05-requirements/emit-crash.md
Effect: `hd check` accepts both files, but `hd test` and `hd build` exit 1 with a JS stack trace and no diagnostic. The files are `test/fixtures/frontend/30-parser-lowers-multi-provider-use-to-an-ordered-tuple.hd` (opaque externref providers stored unboxed in an anyref `$hd.list`) and `spec/conformance/typing/valid/resource-disposed-result.hd` (Binaryen cannot parse the WAT; the message is `[object Object]`). The suites only parse or check them, so this goes unnoticed.
Update: the `_ := f()?` trigger for `Result[void, E]` (formerly F-305) is fixed. `hd build --wat` on `resource-disposed-result.hd` now fails later, with a Binaryen validator error in `poll0`: "return value should be a subtype of the function result type".
Recommendation: implementation change. Box `externref` providers (`$hd.box-extern`) before putting them in tuple lists. Report emitter failures as a structured internal diagnostic that includes Binaryen's message.
