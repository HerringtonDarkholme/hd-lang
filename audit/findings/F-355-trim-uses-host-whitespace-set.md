# F-355: `string.trim()` removes U+FEFF and keeps U+0085
Severity: minor
Area: correctness
Evidence: audit/probes/blind-triage/p-trim-{feff,nel,zs,zwsp}.hd (feff returns 1, expected 2; nel returns 2, expected 1; zs and zwsp pass); spec/conformance/runtime/valid/trim-unicode-white-space.hd fails with `assertion-failed`
Effect: `"\u{FEFF}x".trim()` yields `"x"` and `"\u{85}x".trim()` keeps the NEL. spec/10-modules.md#prelude: "`trim` removes the Unicode `White_Space` property at both ends". U+FEFF is not White_Space; U+0085 is. The host bridge calls JavaScript `String.prototype.trim` (src/compiler.ts line 469), whose set differs from White_Space.
Recommendation: implementation change: trim with an explicit White_Space table (in WAT or the host), and add these two scalars to a portable case.
