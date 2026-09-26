# F-301: Reference parser rejects `\u{...}` escapes that chapter 01 defines
Severity: major
Area: spec
Duplicates: F-206 (unicode-escape part), found independently by the fuzzer
Evidence: audit/evidence/03-fuzz/replay.txt (F-301 block); fuzz signature parse-agreement|ref-reject/impl-accept|invalid-escape, round 1 (7 cases)
Effect: `face := "\u{1F600}"` gets `invalid-escape` from `parseSource`. hd parses, checks, and runs it. Chapter 01 defines `escape_sequence = "\\", ( ... | unicode_escape )` with `unicode_escape = "u", "{", HEX_DIGIT, { HEX_DIGIT }, "}"`. No conformance fixture can use a unicode escape while `spec/check.sh` gates on this parser.
Recommendation: implementation change (reference parser). Accept `\u{HEX+}` in `validEscapes` handling (spec/reference-parser/lexer.ts). Add a conformance fixture for it.
