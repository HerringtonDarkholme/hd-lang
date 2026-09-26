# F-304: `hd check` does not check `Ok`/`Err` payloads against the expected Result type
Severity: major
Area: correctness
Evidence: audit/evidence/03-fuzz/replay.txt (F-304 block); round 1 contract and wasm signatures "struct.new operand N must have proper type" (seed typing/valid/option-result.hd with `Ok(1e308)`); fixture audit/fuzz/findings/F-304-impl-result-payload-unchecked.hd
Effect: `hd check` accepts `Ok("text")` where `Result[i32, string]` is expected, and `Err(42)` where the error type is `string`. This holds in return position, `let` annotations, and arguments. `run`, `test`, and `build` then crash with an uncaught `WasmValidationError` and no located code. Binaryen catches it today. A backend without validation would run an ill-typed program.
Recommendation: implementation change. Check constructor payloads against the contextual Result type, as is already done for list elements and optionals (`[1.5]` for `list[i32]` gets type-mismatch).
