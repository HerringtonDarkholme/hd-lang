# F-358: The reference parser rejects a string literal inside `${...}`
Severity: minor
Area: spec
Evidence: audit/evidence/03-blind-run/refparser.log (amb01-string-in-interpolation.hd: `parseSource` gives `syntax-error@7`; `hd parse` exits 0; control amb01-control-local-in-interpolation.hd accepted by both)
Effect: `"x${f("y")}z"` is valid by spec/01-lexical-structure.md#string-and-character-literals: `${expression}` takes "an arbitrary expression with balanced nested delimiters", and "the lexer switches back to normal expression tokenization inside `${...}`". The grammar oracle rejects it, so fixture authors avoid the construct (blind AMB-01) and the parse-agreement fuzzer reports false disagreements.
Recommendation: spec change: make the reference lexer re-enter expression tokenization inside `${...}`, and add a parse/valid fixture with a nested string literal.
