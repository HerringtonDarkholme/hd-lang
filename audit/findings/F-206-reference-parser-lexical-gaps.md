# F-206: The reference parser disagrees with spec/01 on four lexical rules
Severity: minor
Area: spec
Evidence: audit/evidence/01-test-quality/probes.log (stub `parse` runs of audit/probes/oracle/refparser/*.hd, next to `hd parse`); spec/reference-parser/lexer.ts line 48 `new Set(\`0nrt\\"'$ {}\`)`; spec/01-lexical-structure.md (`unicode_escape`, integer separators, interpolation)
Effect: the grammar oracle gives the wrong answer for four lexical forms.
- It rejects valid `"\u{1F600}"` and `'\u{41}'` with `invalid-escape`.
- It accepts `"\ "`, `"\{"` and `"\}"`, which spec/01 calls lexical errors.
- It accepts `1__0` although "an underscore cannot ... occur twice consecutively".
- It accepts `"price: $"` although "an unescaped `$` must begin one of those forms".
hd agrees with spec/01 on all four. frontend/14-...-6 is the only parse case that fails under the stub runner, for the first reason. No spec/conformance fixture covers any of the four, so spec/check.sh never notices.
Recommendation: spec change: fix the reference lexer, and add parse/valid and parse/invalid fixtures for each rule.
