# F-700: Optional `?` inside a closure emits WAT that Binaryen cannot parse
Severity: major
Area: correctness
Evidence: audit/evidence/w9/verify-findings.log (F-700 block: `hd test spec/conformance/runtime/valid/propagation-in-closure-targets-closure.hd` passes `check`, then throws `WasmValidationError: Binaryen could not parse generated WAT` with a stack trace, exit 1); audit/evidence/w9/mvp-conformance.log
Effect: A closure with an optional result that uses postfix `?` type-checks but cannot run. The user sees a JavaScript stack trace from `src/wasm.ts`, not a diagnostic. Spec 05 says `?` returns `nil` from the nearest function or closure, so the program is valid.
Recommendation: implementation change: lower `?` inside a closure against the closure's own return, and treat Binaryen parse failures as internal errors with a located report.
