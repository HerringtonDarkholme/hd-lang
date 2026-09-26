# F-314: The EBNF derives same-line suite forms that layout processing cannot produce
Severity: note
Area: spec
Evidence: audit/evidence/03-fuzz/grammar-check.txt (768 of 4000 derivations rejected, smallest samples listed); audit/evidence/03-fuzz/replay.txt (F-314 block); fixture audit/fuzz/findings/F-314-spec-inline-suite-comma.hd
Effect: EBNF derivations that both tools reject:
(a) `if flag: a, b := 1, 2` and `test "t": a, b := 1`. The grammar derives these, and chapter 01 says only a comma inside brackets ends a same-line suite. The reference lexer ends the suite at any comma.
(b) `_ := y := if c: 1 else: 2` and `return y := if c: 1 else: 2`. The grammar derives them through `simple_statement, NEWLINE`, but layout replaces that NEWLINE with SUITE_END. `x := y := if c: 1 else: 2` is covered by a `suite_statement` alternative and parses.
Recommendation: OPEN_ISSUES question: are these forms meant to be valid? If so, add the missing `suite_statement` alternatives and fix the comma rule in the reference lexer. If not, state the restriction in chapter 01 or 02.
