# `lib/std` Full-Parser Gaps

These are minimal, specification-valid forms rejected by
`hd_syntax::parse` at the architecture foundation. Each `.hd` file keeps
one construct and cites its rule plus the `lib/std` source locations.

| Construct | Repro | `02-grammar.md` rule | `lib/std` files affected | Diagnostics |
| --- | --- | --- | --- | ---: |
| generic type-parameter default on a public declaration | [generic-default.hd](generic-default.hd) | `grammar.generic.default` | `annotation.hd`, `iter.hd`, `ops.hd` | 12 |
| value-parameter default on a public function | [value-parameter-default.hd](value-parameter-default.hd) | `grammar.fn.semantic` | `testing.hd`, `text.hd` | 3 |
| default on a public data field | [public-field-default.hd](public-field-default.hd) | `grammar.data.default` | `http.hd` | 4 |
| associated-type binding in a public supertrait bound | [supertrait-associated-binding.hd](supertrait-associated-binding.hd) | `grammar.generic.binding.positions-key` | `num.hd` | 3 |
| typed `let` whose type contains a nested comma | [typed-let-nested-comma.hd](typed-let-nested-comma.hd) | `grammar.stmt.let-pattern` | `cli.hd`, `collections.hd`, `console.hd`, `fs.hd`, `iter.hd`, `json.hd`, `serde.hd`, `testing.hd` | 19 |
| method parameter whose type contains a nested comma | [method-parameter-nested-comma.hd](method-parameter-nested-comma.hd) | `grammar.scope.syntax` | `cmp.hd`, `iter.hd` | 4 |
| `let` initializer string containing `:` | [string-colon-after-equals.hd](string-colon-after-equals.hd) | `grammar.scope.syntax` | `cli.hd` | 1 |
| same-line `if` whose condition ends in a call | [inline-if-call-condition.hd](inline-if-call-condition.hd) | `grammar.flow.if-else` | `collections.hd`, `num.hd`, `regex.hd`, `testing.hd`, `text.hd` | 9 |
| negative literal as an indented expression | [negative-literal-line.hd](negative-literal-line.hd) | `grammar.scope.syntax` | `encoding.hd`, `json.hd`, `text.hd` | 4 |
| character literal containing `r` | [r-char-literal.hd](r-char-literal.hd) | `grammar.scope.syntax` | `format.hd`, `regex.hd` | 3 |
| string text containing the word `or` | [boolean-word-in-string.hd](boolean-word-in-string.hd) | `grammar.scope.syntax` | `fs.hd`, `testing.hd` | 2 |
| `:` character literal in an `if` header | [colon-char-in-if-header.hd](colon-char-in-if-header.hd) | `grammar.closed.header-positions` | `regex.hd` | 1 |
| match arm after an arm with a same-line `if` | [match-arm-after-inline-if.hd](match-arm-after-inline-if.hd) | `grammar.flow.if-else` | `path.hd` | 1 |
| assignment arm whose right side is a data expression | [assignment-arm-record.hd](assignment-arm-record.hd) | `grammar.stmt.assign-place` | `regex.hd` | 1 |
| comparisons across a same-line `else if` | [inline-else-if-comparisons.hd](inline-else-if-comparisons.hd) | `grammar.inline.else-if` | `time.hd` | 1 |
| explicit type arguments in an `if` header | [qualified-generic-call-if-header.hd](qualified-generic-call-if-header.hd) | `grammar.expr.type-arguments.marker` | `num.hd` | 5 |
| comparison with an additive right operand | [comparison-additive-rhs.hd](comparison-additive-rhs.hd) | `grammar.generic.bound.and.types-only` | `text.hd`, `time.hd` | 5 |
| **Total** | **17 repros** |  | **19 files** | **78** |
