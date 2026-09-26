# F-355: `string.trim()` removes U+FEFF and keeps U+0085
Severity: minor
Area: correctness
Evidence: audit/evidence/03-blind-run/probes.log (p-trim-feff.hd returns 1, expected 2; p-trim-nel.hd returns 2, expected 1; p-trim-zs.hd and p-trim-zwsp.hd pass); audit/evidence/03-blind-run/run.log (a9-trim-white-space: `assertion-failed`)
Effect: `"\u{FEFF}x".trim()` yields `"x"` and `"\u{85}x".trim()` keeps the NEL. spec/10-modules.md#prelude: "`trim` removes the Unicode `White_Space` property at both ends". U+FEFF is not White_Space; U+0085 is. The host bridge calls JavaScript `String.prototype.trim` (src/compiler.ts line 469, UNVERIFIED as the sole cause), whose set differs from White_Space.
Recommendation: implementation change: trim with an explicit White_Space table (in WAT or the host), and add these two scalars to a portable case.
