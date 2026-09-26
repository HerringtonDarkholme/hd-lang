# F-703: `pub` on a top-level binding is accepted
Severity: minor
Area: correctness
Evidence: audit/evidence/w9/verify-findings.log (F-703 blocks: `hd parse` and `hd check` on spec/conformance/parse/invalid/pub-top-level-binding.hd both print `ok`, exit 0)
Effect: `pub answer := 42` compiles. Spec 10 (Name Resolution Across Packages) says top-level bindings cannot be public, because a public signature must be fully annotated and a binding's type is inferred.
Recommendation: implementation change: report `syntax-error` for `pub` before a top-level binding.
