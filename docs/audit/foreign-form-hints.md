Status: diagnostics audit of current main `ec12f358b` for #2334. Nothing
in this report adds accepted language behavior. The inputs are the writing
log, [lexical rules](../../spec/lang/01-lexical-structure.md),
[grammar](../../spec/lang/02-grammar.md),
[types](../../spec/lang/04-type-system.md), and requirement rules.

## Ranking method

Read the **written form** column of all 1,347 dated mistake rows, preserving
escaped pipes and backtick spans. Count each row once per form family;
repeated generated occurrences within a row still count once. Group grammar
variants of one mistake, including where parentheses belong in a function
type. Sort by row count, then the first source row for ties.

The source is [the immutable log](https://github.com/HerringtonDarkholme/hd-lang/blob/ec12f358b/docs/audit/hd-writing-log.md),
blob `59a1b46c9e9d00990e784855286f031eeed808d0`. Line numbers below name that snapshot.

The ranking concerns foreign spelling and grouping conventions, including
foreign primitive/API spellings. Exclude compiler bugs, CLI invocations,
internal/private library implementation mistakes and ordinary hd type,
ownership or requirement-rule mistakes. These do not have a foreign form
with an equivalent hd spelling. For example, repeated private `Rev` field
construction and invariant generic row inference remain governed by their
existing rules.

## Highest missing or partial hints

A family with a repair for only some of its variants still needs coverage.
The first ten eligible families follow. The last row is the ticket's
explicit closure example, covered in addition to the ten.

| Rank | Written form                                       | Rows | Source lines                                                                       |
| ---- | -------------------------------------------------- | ---: | ---------------------------------------------------------------------------------- |
| 1    | Function-type grouping                             |   17 | 46, 488, 489, 490, 491, 493, 495, 496, 497, 498, 499, 500, 501, 502, 503, 504, 505 |
| 2    | Reserved names                                     |   15 | 295, 365, 380, 625, 626, 627, 628, 629, 630, 631, 638, 639, 730, 1112, 1293        |
| 3    | Unparenthesized multiline expressions              |   11 | 238, 700, 712, 783, 930, 934, 948, 1127, 1219, 1220, 1276                          |
| 4    | Type spelling used to construct a reference        |    9 | 611, 612, 613, 614, 615, 616, 617, 618, 619                                        |
| 5    | Inline empty trait/impl bodies                     |    8 | 739, 1231, 1232, 1310, 1311, 1324, 1325, 1398                                      |
| 6    | Foreign numeric primitive names                    |    5 | 1316, 1317, 1318, 1319, 1320                                                       |
| 7    | Foreign empty-map literals                         |    4 | 61, 1132, 1133, 1134                                                               |
| 8    | Numeric suffixes on radix literals                 |    4 | 399, 944, 986, 1284                                                                |
| 9    | Associated bindings in an impl trait argument list |    4 | 1328, 1329, 1330, 1331                                                             |
| 10   | `as` casts                                         |    3 | 68, 820, 1189                                                                      |
| 11   | Rust pipe closures (explicit ticket example)       |    1 | 1184                                                                               |

The existing parenthesized-name numeric `as` repair covers line 68 only;
other operands and collection annotations still lacked a pointer. The
existing `{:}` empty-map repair covers line 61, while `[:]` lacked one.
These are incomplete families, rather than separate ranked spellings.

## Existing pointers and accepted forms

Current-main probes and source review remove these from the missing list:

- Result `unwrap()` is accepted. Option `unwrap()` already points to
  `expect(message)` or `unwrap_or(fallback)`.
- `const`, `not`, `and`, `elif`, tuple `.0`, typed bindings missing `let`,
  prefix spreads, `///`, `newtype`, colon casts, generic colon bounds,
  single-name public re-exports, and postfix bang calls already have hints
  or edits.
- An `if` or `match` used as an operand already asks for parentheses.
  A multiline closure's closing delimiter already asks for a line ending.
- `true`, decimal `1u8`, associated `Type.make()`, and `struct Empty: pass`
  are accepted. A pipeline continuation beginning `|>` is accepted too.

Lower-frequency missing families include bracketed generic calls,
associated-type dots, `+` between trait bounds, Rust raw identifiers,
bodyless struct headers and colon-separated named arguments (two rows
each), followed by one-row closure arrows, import grouping, range and
postfix-dereference spellings. They do not displace the ten above.

The completed change is integrated onto `31c816ce4`. Its newer typo
repairs also suggest `usize` for unresolved `isize`; those edits remain
available alongside the explanation of fixed-width numeric alternatives.
The ranking records the initial current-main audit above.

## Implementation and focused fixtures

The parser reports these at the existing failing token. It does not accept
new syntax. Exact edits use existing spans; ambiguous scope or numeric
width choices receive a hint. In particular, radix conversion edits avoid
moving a possible unary sign across a checked conversion.

The focused fixtures live in
[foreign-forms](../../crates/hd_check/tests/cases/foreign-forms/).
They include the valid neighboring forms, both trait/impl headers,
function-row/optional grouping, declaration/member name positions, radix
variants, and signed/call/grouped/binary `as` operands. Their generated
`diagnostics` and `fixed` views record the pointer and the repaired program.

The reference is rustc's
[parser diagnostics](https://github.com/rust-lang/rust/blob/master/compiler/rustc_parse/src/parser/diagnostics.rs),
particularly keyword escaping and span-based suggestions after a failed
parse. hd keeps its existing token/layout parser and exact/suggestion
classification. There is no speculative reparse or new compiler pass.

Cost: constant token lookahead at existing failure sites; output work is
proportional to diagnostic/edit text. Radix repair scans only its own
literal; closure repair scans a bare parameter prefix once, stopping at the
body or line boundary. Cast repair uses operand endpoints without copying or rescanning
the operand. No retry, new input limit, fixpoint or native recursion.
