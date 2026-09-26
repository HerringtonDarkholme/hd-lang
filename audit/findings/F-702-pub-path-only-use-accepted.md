# F-702: `pub` on a path-only `use` is accepted
Severity: minor
Area: correctness
Evidence: audit/evidence/w9/verify-findings.log (F-702 blocks: `hd parse` and `hd check` on spec/conformance/parse/invalid/pub-path-only-use.hd both print `ok`, exit 0)
Effect: `pub use std.testing.assert_equal` compiles. Spec 10 (Use Forms) says only the grouped form accepts a `pub` prefix, so a library can re-export a module path alias that other implementations reject.
Recommendation: implementation change: report `syntax-error` for `pub` on a use without a braced group.
