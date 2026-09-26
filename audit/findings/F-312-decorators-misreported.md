# F-312: Every decorator is rejected as `decorator-not-top-level`, even at top level
Severity: minor
Area: correctness
Duplicates: F-405 and F-250 (decorator part), found independently by the fuzzer
Evidence: audit/evidence/03-fuzz/replay.txt (F-312 block); round 1 parse signature decorator-not-top-level (105 cases); `hd parse spec/conformance/parse/valid/decorators.hd`
Effect: `@derive(Eq)` on a top-level `data` fails with "decorators are only valid on top-level declarations". Unsupported decorators look like a user error with a false explanation. The code is a stable spec code with a different meaning, so a portable fixture expecting `decorator-not-top-level` passes for the wrong reason.
Recommendation: implementation change: report an unsupported-feature diagnostic for decorators the MVP does not implement. Keep `decorator-not-top-level` for local decorators.
