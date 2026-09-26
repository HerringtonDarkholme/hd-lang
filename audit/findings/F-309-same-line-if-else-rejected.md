# F-309: hd rejects same-line `if c: a else: b`, a form chapter 01 uses as an example
Severity: major
Area: correctness
Duplicates: F-252 (same-line if/else part), found independently by the fuzzer
Evidence: audit/evidence/03-fuzz/replay.txt (F-309 block); round 1 parse signature expected-newline (15 cases); `hd parse spec/conformance/parse/valid/nested-layout.hd`
Effect: `x := if c: 1 else: 2` and `if c: return 1 else: return 2` fail with `expected-newline: expected a line ending, found 'else'`. Chapter 01 says `x := if c: 1 else: 2` "is one conditional expression". Three conformance fixtures use this form: parse/valid/nested-layout.hd, parse/valid/grammar-disambiguation.hd, and typing/valid/gadts.hd. All three are unselected, so the gap is invisible in `npm test`.
Recommendation: implementation change: emit the `else` boundary for same-line suites (chapter 01, "`else` is also a boundary").
