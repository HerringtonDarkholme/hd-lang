# F-303: Reference parser accepts an unescaped `$` that does not start an interpolation
Severity: minor
Area: spec
Duplicates: F-206 (lone `$` part), found independently; this file adds the missing inventory code
Evidence: audit/evidence/03-fuzz/replay.txt (F-303 block); fuzz signature parse-agreement|ref-accept/impl-reject|invalid-string-interpolation, round 1 (6 cases)
Effect: `price := "$"` passes `parseSource`. Chapter 01 says "An unescaped `$` must begin one of those forms", and hd rejects it with `invalid-string-interpolation`. That code is not in the spec inventory, and the spec names no code for this error. Two implementations can disagree here and both look conformant.
Recommendation: OPEN_ISSUES question: which stable code diagnoses a stray `$` in an interpreted string? Then an implementation change so the reference parser reports it.
