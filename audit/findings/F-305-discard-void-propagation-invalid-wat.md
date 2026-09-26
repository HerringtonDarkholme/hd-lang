# F-305: `_ := f()?` on `Result[void, E]` emits WAT that Binaryen cannot parse
Severity: major
Area: correctness
Duplicates: F-559 (resource-disposed-result part); this file adds the minimal trigger `_ := f()?` on `Result[void, E]`
Evidence: audit/evidence/03-fuzz/replay.txt (F-305 block); round 1 wasm signature 01 and contract signatures 06/07 (12 cases each, seed spec/conformance/typing/valid/resource-disposed-result.hd)
Effect: `hd check` accepts `_ := close()?` where `close` returns `Result[void, string]`. `build`, `run`, and `test` then crash with `WasmValidationError: Binaryen could not parse generated WAT: [object Object]`, without a located code. The selected type-phase conformance fixture `typing/valid/resource-disposed-result.hd` has the same pattern and cannot be built. The portable suite runs only `check` on it, so the suite stays green.
Recommendation: implementation change: lower a discarded `void` success value from `?`. Also run type-phase `valid` fixtures through `build` in the portable suite, or record why not.
