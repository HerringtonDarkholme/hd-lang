# Full-Parser Gaps

These are minimal, specification-valid forms rejected by
`hd_syntax::parse` at the architecture foundation. Each `.hd` file keeps
one construct and cites its rule plus an affected source location.

The valid-fixture count assigns each of the 395 rejected `expectation=accept`
cases in `spec/conformance/cases.tsv` to its first parser diagnostic. The
portable suite selects 392 of those cases from the same fixture tree. Counts
therefore describe unique fixtures, not duplicate selections, and sum to 395.

| Construct | Repro | Rule | Valid fixtures blocked | `lib/std` files affected | `lib/std` diagnostics |
| --- | --- | --- | ---: | --- | ---: |
| source text containing the word `and` or `or` | [boolean-word-in-string.hd](boolean-word-in-string.hd) | `grammar.scope.syntax` | 264 | `fs.hd`, `testing.hd` | 2 |
| typed or destructuring `let` whose pattern text contains a nested comma | [typed-let-nested-comma.hd](typed-let-nested-comma.hd) | `grammar.stmt.let-pattern` | 57 | `cli.hd`, `collections.hd`, `console.hd`, `fs.hd`, `iter.hd`, `json.hd`, `serde.hd`, `testing.hd` | 19 |
| indented line with a type colon and string initializer | [string-colon-after-equals.hd](string-colon-after-equals.hd) | `grammar.scope.syntax` | 18 | `cli.hd` | 1 |
| leading-dot statement after a same-line suite | [leading-dot-statement.hd](leading-dot-statement.hd) | `lex.dot.statement-indent` | 8 | — | 0 |
| same-line conditional whose condition ends in a call | [inline-if-call-condition.hd](inline-if-call-condition.hd) | `grammar.flow.if-else` | 7 | `collections.hd`, `num.hd`, `regex.hd`, `testing.hd`, `text.hd` | 9 |
| operator-leading indented expression | [negative-literal-line.hd](negative-literal-line.hd) | `grammar.scope.syntax` | 7 | `encoding.hd`, `json.hd`, `text.hd` | 4 |
| callback function type with a requirement row before another parameter | [callback-effect-row-comma.hd](callback-effect-row-comma.hd) | `req.row.callable.same-clause` | 6 | — | 0 |
| binding expression in an `if` header | [binding-expression-if-header.hd](binding-expression-if-header.hd) | `grammar.closed.header-positions` | 4 | — | 0 |
| associated-type binding in an `impl` parameter bound | [impl-associated-binding.hd](impl-associated-binding.hd) | `grammar.generic.binding.positions-key` | 3 | — | 0 |
| comprehension over a comma-containing list literal | [comprehension-list-source.hd](comprehension-list-source.hd) | `grammar.scope.syntax` | 2 | — | 0 |
| binding chain whose right side is a same-line conditional | [binding-chain-inline-if.hd](binding-chain-inline-if.hd) | `names.bind.no-redeclare` | 2 | — | 0 |
| same-line conditional after a data-field colon | [inline-if-record-field.hd](inline-if-record-field.hd) | `grammar.scope.syntax` | 2 | — | 0 |
| assignment arm whose right side is a data expression | [assignment-arm-record.hd](assignment-arm-record.hd) | `grammar.stmt.assign-place` | 1 | `regex.hd` | 1 |
| `:` character literal in an `if` header | [colon-char-in-if-header.hd](colon-char-in-if-header.hd) | `grammar.closed.header-positions` | 1 | `regex.hd` | 1 |
| inline closure in an `if` header | [closure-in-if-header.hd](closure-in-if-header.hd) | `grammar.closed.header-positions` | 1 | — | 0 |
| two comparison filters in one comprehension | [comprehension-two-filters.hd](comprehension-two-filters.hd) | `flow.for.comprehension` | 1 | — | 0 |
| default before a final function-typed parameter | [default-before-final-function.hd](default-before-final-function.hd) | `fn.default.order-final-function` | 1 | — | 0 |
| effect-bearing function type inside a tuple parameter | [effect-row-in-tuple-parameter.hd](effect-row-in-tuple-parameter.hd) | `req.row.callable.same-clause` | 1 | — | 0 |
| exponentiation beside a named argument | [exponentiation-with-named-argument.hd](exponentiation-with-named-argument.hd) | `grammar.scope.syntax` | 1 | — | 0 |
| generic type-parameter default on a public declaration | [generic-default.hd](generic-default.hd) | `grammar.generic.default` | 1 | `annotation.hd`, `iter.hd`, `ops.hd` | 12 |
| interpolation name beginning with reserved-word text | [interpolation-reserved-prefix.hd](interpolation-reserved-prefix.hd) | `lex.raw.interpolation` | 1 | — | 0 |
| multiline closure followed by its closing delimiter | [multiline-closure-closing-delimiter.hd](multiline-closure-closing-delimiter.hd) | `lex.closure.end-token` | 1 | — | 0 |
| named arguments followed by a trailing comma | [named-pattern-trailing-comma.hd](named-pattern-trailing-comma.hd) | `fn.arg.positional-first` | 1 | — | 0 |
| pipe spelling inside a test-description string | [pipe-text-in-string.hd](pipe-text-in-string.hd) | `grammar.scope.syntax` | 1 | — | 0 |
| default on a public data field | [public-field-default.hd](public-field-default.hd) | `grammar.data.default` | 1 | `http.hd` | 4 |
| character literal containing `r` | [r-char-literal.hd](r-char-literal.hd) | `grammar.scope.syntax` | 1 | `format.hd`, `regex.hd` | 3 |
| one-payload variant whose payload is an effect-bearing function | [variant-function-effect-row.hd](variant-function-effect-row.hd) | `fn.type.parts` | 1 | — | 0 |
| comparison with an additive right operand | [comparison-additive-rhs.hd](comparison-additive-rhs.hd) | `grammar.generic.bound.and.types-only` | 0 | `text.hd`, `time.hd` | 5 |
| comparisons across a same-line `else if` | [inline-else-if-comparisons.hd](inline-else-if-comparisons.hd) | `grammar.inline.else-if` | 0 | `time.hd` | 1 |
| match arm after an arm with a same-line `if` | [match-arm-after-inline-if.hd](match-arm-after-inline-if.hd) | `grammar.flow.if-else` | 0 | `path.hd` | 1 |
| method parameter whose type contains a nested comma | [method-parameter-nested-comma.hd](method-parameter-nested-comma.hd) | `grammar.scope.syntax` | 0 | `cmp.hd`, `iter.hd` | 4 |
| explicit type arguments in an `if` header | [qualified-generic-call-if-header.hd](qualified-generic-call-if-header.hd) | `grammar.expr.type-arguments.marker` | 0 | `num.hd` | 5 |
| associated-type binding in a public supertrait bound | [supertrait-associated-binding.hd](supertrait-associated-binding.hd) | `grammar.generic.binding.positions-key` | 0 | `num.hd` | 3 |
| value-parameter default on a public function | [value-parameter-default.hd](value-parameter-default.hd) | `grammar.fn.semantic` | 0 | `testing.hd`, `text.hd` | 3 |
| **Total** | **34 repros** |  | **395** | **19 files** | **78** |
