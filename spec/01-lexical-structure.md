# Lexical Structure

Status: language specification draft.

This chapter defines how source text is divided into tokens and how
indentation produces block structure.

See also: [Grammar](02-grammar.md), which defines the syntactic use of those
tokens.

## Processing Model

An implementation processes a source file in this order:

1. r[lex.process.decode] Decode source bytes as UTF-8 and remove one optional initial byte-order mark.
2. r[lex.process.lines] Divide source text into physical lines.
3. r[lex.process.tokens] Recognize comments, whitespace, literals, identifiers, and operators.
4. r[lex.process.join] Join physical lines that continue inside `()`, `[]`, or `{}`, and leading-dot continuation lines, into logical lines, except for an indentation suite nested in that continuation.
5. r[lex.process.layout] Emit `NEWLINE`, `INDENT`, `DEDENT`, and same-line `SUITE_END` layout tokens from logical lines and nested suites.
6. r[lex.process.parse] Parse the resulting token stream.

r[lex.process.trivia] Comments and whitespace separate tokens but otherwise
do not appear in the parser token stream. Layout tokens are the exception.

### Source Encoding

1. r[lex.encoding.utf8] Source files must be valid UTF-8.
2. r[lex.encoding.invalid] Invalid UTF-8 is a compile-time lexical error.
3. r[lex.encoding.bom] If the first three bytes are `EF BB BF`, they are removed before lexical analysis.
4. r[lex.encoding.other-bom] No other byte-order mark is removed.
5. r[lex.encoding.stray-bom] A `U+FEFF` outside a comment or literal at any other position is a compile-time lexical error.
6. r[lex.encoding.scalars] Unicode scalar values are valid in identifiers, under the identifier rules below, and in comments, string literals, and character literals.

See also: [Identifiers](#identifiers).

## Physical And Logical Lines

This section defines physical lines, the logical lines that join them, and
the layout tokens emitted inside delimiters.

### Line Endings

1. r[lex.line.physical] A physical line ends at a line-feed character or at the end of the file.
2. r[lex.line.crlf] A carriage-return followed by a line-feed is treated as one line ending.
3. r[lex.line.bare-cr] A bare carriage return is a lexical error.
4. r[lex.line.no-backslash] hd-lang has no explicit backslash line-continuation syntax.

### Implicit Continuation

A physical line continues inside unmatched brackets:

```text
user := User {
    id: "user_123",
    email: "ada@example.com",
}

result := choice(
    first,
    second,
)
```

1. r[lex.line.logical] A logical line consists of one or more physical lines.
2. r[lex.continue.delimiters] A physical line is continued implicitly while the lexer is inside an unmatched `(`, `[`, or `{`.
3. r[lex.continue.no-suffix] A continued line still cannot extend the previous line's last operand with a bracketed suffix.
4. r[lex.continue.suffix-line] A call `(`, an index or type-argument `[`, a data-literal `{`, and the `!` of a suspension call must start on the same physical line as the end of their operand.
5. r[lex.continue.new-operand] Inside delimiters, a line whose first token is `(`, `[`, `{`, or `!` therefore begins a new operand, and the separator before it is required.
6. r[lex.continue.missing-separator] A list written as `first` on one line and `[1]` on the next, without a comma between them, is not `first[1]`. It is an error. Error: `syntax-error`.

```text
fn pair(first: i32, second: i32) -> (i32, i32):
    (
        first
        (second)  # error: syntax-error
    )

fn pick(first: List[i32]) -> List[List[i32]]:
    [
        first
        [1]  # error: syntax-error
    ]
```

### Leading-Dot Continuation

For example, a method chain may continue on lines that start with a member
suffix:

```text
names := users
    .filter(fn(user): user.active)
    .map(fn(user): user.name)
```

1. r[lex.dot.continue] A physical line also continues the previous logical line when all three of the following conditions hold.
2. r[lex.dot.first-token] Its first token is `.` immediately followed by an identifier, a member suffix.
3. r[lex.dot.deeper] It is indented farther than the first physical line of the logical line it continues.
4. r[lex.dot.no-opener] That logical line does not end in `:` or `=>`, the tokens that open an indented suite or match-arm body on the next line.
5. r[lex.dot.no-layout] Such a leading-dot line emits no `NEWLINE`, `INDENT`, or `DEDENT`.
6. r[lex.dot.blank-lines] Blank lines and comment-only lines before it do not matter.
7. r[lex.dot.joined] The joined text is read as if it were written on one physical line.
8. r[lex.dot.open-suite] A leading-dot line is an error when a same-line suite is still open at the end of the logical line it would continue, as after `f := fn(x): x`. Error: `syntax-error`.
9. r[lex.dot.closed-suite] A same-line suite that closed earlier on the line, such as one inside `xs.map(fn(x): x)`, does not prevent the continuation.
10. r[lex.dot.same-indent] A line starting with `.Variant` at the same indentation as the previous line, such as a match arm or an expression statement, starts a new logical line.
11. r[lex.dot.suite-line] So does the first line of an indented suite, whose header ends in `:`.
12. r[lex.dot.where] The rule applies at delimiter depth zero and on the body lines of a suite nested inside delimiters. Elsewhere inside delimiters every line already continues.
13. r[lex.continue.no-operator] A line starting with a binary operator never continues the previous line.

```text
fn trimmer() -> fn(string) -> string:
    f := fn(name: string) -> string: name
        .trim()  # error: syntax-error
    f

fn total(a: i32, b: i32) -> i32:
    sum := a
        + b  # error
    sum

fn first(pair: (i32, i32)) -> i32:
    value := pair
        .0  # error
    value
```

> **Why.** Continuing a line whose same-line suite is still open would
> silently join the chain to that suite's body.

### Suites Inside Delimiters

1. r[lex.nested.no-layout] Comments and line endings inside an implicit continuation normally do not emit `NEWLINE`, `INDENT`, or `DEDENT`.
2. r[lex.nested.suite] The exception is a suite introduced by a grammar position that expects `:` followed by `suite_body`.
3. r[lex.nested.layout] When that suite starts on the next physical line, layout processing emits its `NEWLINE`, `INDENT`, body layout, and closing `DEDENT`, even if surrounding delimiters are still open.
4. r[lex.nested.resume] After the suite closes, implicit continuation resumes.
5. r[lex.nested.reference] The indentation reference for such a nested suite is the indentation of the physical line containing its suite header.
6. r[lex.nested.body-depth] Its first body line must be indented farther than that reference, and farther than the first physical line of the logical line that contains the header. Otherwise it is an error. Error: `unexpected-indentation`.
7. r[lex.nested.body-left] A header on a continuation line therefore cannot place its body to the left of, or level with, the statement that contains it. For example, in `x := run(` followed by a less indented `fn(v):`, the body must still be deeper than `x := run(`.
8. r[lex.nested.closer] Except after a closure body, a closing delimiter at the nested suite's delimiter depth ends the last body line. Layout processing emits `NEWLINE` and all pending `DEDENT` tokens before emitting the closing delimiter.

This exception permits explicit multiline closures in calls:

```text
choice(
    (
        fn(value):
            println(value)
    ),
    fallback,
)
```

```text
fn run(callback: fn(i32) -> i32) -> i32: callback(1)

fn main() -> void:
    x := run(fn(v):
    v + 1  # error: unexpected-indentation
)
```

### Closures Inside Delimiters

1. r[lex.closure.end] A closure whose indented body is nested directly inside delimiters has a stricter end.
2. r[lex.closure.end-line] Its body ends only at a line indented no farther than the line holding the closure header.
3. r[lex.closure.end-token] That line must start with `,` or a closing delimiter at the closure's delimiter depth.
4. r[lex.closure.other-token] Any other token starting that line is an error. Error: `syntax-error`.
5. r[lex.closure.between] A line indented between the header and the body is an error. Error: `syntax-error`.
6. r[lex.closure.closer-on-body] A closing delimiter on a body line is an error. Error: `syntax-error`.
7. r[lex.closure.body-line] A line at body indentation belongs to the body.

```text
choice(fn(a):
    println(a)
, fn(b):
    println(b)
)

choice(fn(a):
    println(a)
    fallback)       # error: syntax-error
```

```text
fn choice(first: fn(i32) -> void, second: i32) -> void:
    first(second)

fn run(fallback: i32) -> void:
    choice(fn(a):
        println(a)
    fallback)  # error: syntax-error
```

> **Why.** Because a line at body indentation belongs to the body, a later
> argument written on it is caught rather than silently becoming the
> closure's result.

#### Headers After A Nested Suite

1. r[lex.closure.statement] A closure written as a statement in a nested suite's body ends like any other statement.
2. r[lex.header.resume] Inside brackets, a header may resume on the line after a nested suite that is not a closure body, such as an `if` expression.
3. r[lex.header.resume-closure] After a closure body it resumes only past the closing delimiter, as in `(if check(fn(x): ...` followed by a line that starts with `):`.
4. r[lex.header.no-indented-end] Outside brackets, and in the statements of a nested suite, a header cannot end in an indented suite: the line after that suite cannot continue it.

See also: [Statements](02-grammar.md#statements).

### Suite-Introducing Colons

1. r[lex.colon.cooperate] Layout recognition and parsing therefore cooperate at a suite-introducing colon.
2. r[lex.colon.implementation] A lexer may implement this with parser feedback or with equivalent parser-state tracking.
3. r[lex.colon.ordinary] Ordinary colons in maps, data fields, named types, and arguments do not open a suite.
4. r[lex.colon.trailing-block] Trailing-block call colons occur only at delimiter depth zero, when the call is the complete statement or the complete right-hand side of `:=`, `let ... =`, `=`, `_ :=`, `return`, or `break`.
5. r[lex.colon.trailing-block.not-headers] They are not recognized in `if`, `while`, `for`, or `match` headers or inside brackets.

See also: [Trailing Callback Blocks](07-functions.md#trailing-callback-blocks).

### Same-Line Suites

1. r[lex.suite-end.token] A same-line suite ends with the abstract token `SUITE_END`.
2. r[lex.suite-end.line] At a logical line boundary, layout closes every same-line suite opened on that logical line. It emits one `SUITE_END` per suite, from innermost to outermost.
3. r[lex.suite-end.newline] At delimiter depth zero the outermost `SUITE_END` replaces that line's `NEWLINE`; it does not precede a second terminator.
4. r[lex.suite-end.continuation] In an implicit continuation, the equivalent boundary is a comma or closing delimiter that returns control to the enclosing expression. The same innermost-first sequence is emitted before that token.
5. r[lex.suite-end.comma] A comma at the delimiter depth where a same-line suite opened always closes that suite, including at depth zero, so the suite body cannot contain such a comma.
6. r[lex.suite-end.example] For example, the body of `fn(name): name.lower()` ends immediately before that closure's closing `)`.
7. r[lex.suite-end.else] `else` is also a boundary for the immediately preceding same-line `if`, `for`, or `while` suite. Layout emits that suite's `SUITE_END` before `else` and keeps the enclosing conditional or loop open, as the conditional and loop productions require.
8. r[lex.suite-end.else-example] Thus `x := if c: 1 else: 2` is one conditional expression. The line boundary after `2` closes the `else` suite and then any enclosing same-line suite, innermost first.
9. r[lex.suite-end.no-spelling] `SUITE_END` has no source spelling. Parser-aware layout processing identifies the boundary from the expected suite and enclosing delimiter structure.

### Delimiter Matching

1. r[lex.delim.match] A closing delimiter must match the most recent unclosed delimiter.
2. r[lex.delim.unmatched] An unmatched or mismatched delimiter is a compile-time error.

## Whitespace And Indentation

1. r[lex.space.meaning] Spaces between tokens have no meaning except when they occur at the beginning of a logical line.
2. r[lex.indent.structure] Leading indentation determines block structure.

### Indentation Levels

1. r[lex.indent.compare] For each non-empty logical line outside implicit continuation, the lexer compares its indentation with a stack of active indentation levels.
2. r[lex.indent.equal] Equal indentation emits no layout token.
3. r[lex.indent.greater] Greater indentation pushes the new level and emits one `INDENT`.
4. r[lex.indent.lesser] Lesser indentation emits one or more `DEDENT` tokens until an existing level is reached.
5. r[lex.indent.unknown-column] Dedenting to a column that is not an active indentation level is a compile-time error.
6. r[lex.indent.first] The first indentation level is zero.
7. r[lex.indent.eof] At end of file, the lexer emits any remaining `DEDENT` tokens.
8. r[lex.indent.eof-newline] If the final non-empty logical line has no physical line ending, the lexer emits its terminating `NEWLINE` before those `DEDENT` tokens.
9. r[lex.indent.blank] Blank lines and comment-only lines do not affect the indentation stack and do not emit `NEWLINE` tokens.

### Block Headers

A block header is followed by an indented or a same-line suite:

```text
fn greet(name: string) -> void $ Console:
    println("hello, " + name)

fn test() -> void $ Console: println("hi")
```

1. r[lex.block.header] A block header ends in `:`.
2. r[lex.block.body] Its body may be either an indented suite beginning on the next logical line or a same-line suite.

### Tabs

1. r[lex.tab.invalid] Horizontal tab characters are invalid as source whitespace.
2. r[lex.tab.content] They may occur only as literal content represented by the `\t` escape or as raw characters inside comments.
3. r[lex.tab.spaces] Indentation therefore consists only of ASCII space characters.

> **Note.** Visual tab-width configuration therefore cannot change block
> structure.

## Comments

`#` begins a comment that runs to the end of its line:

```text
# A comment on its own line.
name := "Ada"  # A comment after code.
```

1. r[lex.comment.line] `#` begins a line comment outside a string or character literal.
2. r[lex.comment.extent] The comment continues to the end of its physical line.
3. r[lex.comment.no-block] There are no block comments.

### Documentation Comments

1. r[lex.doc.form] `##` at the start of a comment is a documentation comment.
2. r[lex.doc.attach] One or more consecutive documentation-comment lines attach to the next declaration or member at the same indentation, when no blank line or non-documentation token intervenes.
3. r[lex.doc.members] Members include data fields, embedded fields, enum variants and payload fields, trait and implementation methods, and function parameters.
4. r[lex.doc.text] The lexer removes `##` and one following space when present, then joins lines with `\n`.
5. r[lex.doc.field] The resulting string is exposed as the target shape's `doc` field. Without an attached documentation comment, `doc` is `.None`.
6. r[lex.doc.trailing] A trailing `##` comment after source code is ordinary commentary and does not attach.
7. r[lex.doc.unattached] An otherwise unattached documentation-comment line is a lexical error. Error: `doc-comment-without-target`.

```text
fn run() -> void:
    ## This cannot document an executable statement.  # error: doc-comment-without-target
    value := 1
```

### Comment Grammar

The lexical form is:

```ebnf
line_comment = "#", { comment_character } ;
doc_comment = "##", [ " " ], { comment_character } ;
comment_character = ? any supported source character other than a line ending ? ;
```

r[lex.comment.character] `comment_character` is any supported source
character other than a line ending.

## Identifiers

This section defines identifiers.

```ebnf
identifier       = identifier_start, { identifier_continue } ;
identifier_start = XID_START | "_" ;
identifier_continue = XID_CONTINUE | "_" ;
DECIMAL_DIGIT    = "0" ... "9" ;
```

1. r[lex.ident.form] An identifier begins with a Unicode `XID_Start` character or `_` and continues with Unicode `XID_Continue` characters or `_`.
2. r[lex.ident.case] Identifiers are case-sensitive.
3. r[lex.ident.nfc] Their source spelling must be in Unicode Normalization Form C (NFC).
4. r[lex.ident.non-nfc] A non-NFC identifier is a lexical error rather than being silently rewritten.
5. r[lex.ident.placeholder] The single source spelling `_` is a distinct placeholder token, not an `identifier`.
6. r[lex.ident.underscore] An identifier that begins with `_` must contain at least one additional `identifier_continue` character.
7. r[lex.ident.xid] `XID_START` and `XID_CONTINUE` denote the corresponding Unicode derived core properties.
8. r[lex.ident.unicode-version] An implementation must use one declared Unicode data version consistently for lexing, normalization, and diagnostics.

### Identifier Security

1. r[lex.ident.confusable] The compiler must diagnose identifiers that are visually confusable with another identifier visible in the same scope.
2. r[lex.ident.mixed-script] The compiler must diagnose identifiers that suspiciously mix scripts.
3. r[lex.ident.identity] These security diagnostics do not change name identity: two different NFC identifier strings remain different names.
4. r[lex.ident.ascii] Standard-library APIs, language keywords, and compiler-generated source names use ASCII.

### Reserved Words And Built-In Names

1. r[lex.ident.reserved] An identifier that exactly matches a reserved word is not an identifier token.
2. r[lex.ident.builtin-types] Built-in type names such as `i32`, `string`, `List`, and `Map` are ordinary names rather than lexically distinct tokens.

See also: [Keywords And Reserved Words](#keywords-and-reserved-words), which
lists the complete reserved-word set.

### Raw Identifiers

A raw identifier writes a name between backticks:

```text
data Token:
    `type`: string
    text: string

fn describe(`in`: Token, `match`: bool) -> string:
    if `match`: `in`.`type` else: `in`.text

label := describe(Token { `type`: "word", text: "hi" }, `match` = true)
```

```ebnf
raw_identifier = "`", identifier_start, { identifier_continue }, "`" ;
```

1. r[lex.raw.form] A raw identifier writes a name between backticks.
2. r[lex.raw.reserved] Any reserved word may be written this way, so it can name a field, a member, a named-argument label, a parameter, or a binding.
3. r[lex.raw.token] A raw identifier is one identifier token, accepted wherever the grammar accepts `identifier`.
4. r[lex.raw.name] Its name is the enclosed text without the backticks. The field above is named `type`, and `` `name` `` denotes the same identifier as `name`.
5. r[lex.raw.text] The enclosed text follows the identifier rules, including NFC and the rule for a leading `_`.
6. r[lex.raw.invalid] An empty pair of backticks, an unclosed backtick, a backtick around any other text, and a backtick anywhere else are each an error. Error: `invalid-token`.
7. r[lex.raw.not-reserved] A raw identifier is never a reserved word or a contextual word: `` `use` `` never begins a use declaration, and `` `pack`.map(xs, f) `` is an ordinary method call.
8. r[lex.raw.interpolation] `$name` interpolation takes a plain identifier; `` ${`type`} `` interpolates a raw one.

```text
fn main() -> i32:
    `` := 1  # error: invalid-token
    0

data Token:
    `type`: string

fn kind(token: Token) -> string:
    token.type  # error
```

## Keywords And Reserved Words

r[lex.keyword.reserved] The grammar uses these reserved words:

```text
Self      annotate  break     continue  data      defer
else      enum      false     fn        for       if
impl      in        is        let       match     mut
pass      pub       return    self      trait     true
type      while
```

### Contextual Words

r[lex.contextual.fixed] The following contextual words have special meaning
only in fixed positions:

| Rule | Words | Position |
| --- | --- | --- |
| r[lex.contextual.use-root] Use roots | `pkg`, `std`, `dep`; `super` | `pkg`, `std`, and `dep` in a use root position, and `super` as a use root, alone or repeated, as in `use super.shared.{Email}` |
| r[lex.contextual.as] Alias | `as` | directly after a use path or use item, before its alias |
| r[lex.contextual.use] Use | `use` | at the start of a module-level item, alone or after `pub`, when a use root (`pkg`, `std`, `dep`, `self`, or `super`) follows it; and as the operation name in the dedicated `$.use(...)` provider expression |
| r[lex.contextual.reified] Reified | `reified` | first in a generic parameter, directly before the parameter name, as in `fn pick[reified T]() -> T` |
| r[lex.contextual.test] Test | `test` | at the beginning of a module-level test block |
| r[lex.contextual.annotation] Annotation | `annotation`, `annotation_ref` | after `::` in annotation materialization |
| r[lex.contextual.context] Context | `context`, `with`, `Context` | after `$.` |
| r[lex.contextual.pack] Pack | `pack`, `map`, `map_list` | in the `pack.map(...)` and `pack.map_list(...)` forms |
| r[lex.contextual.derive] Derive | `derive` | immediately after `@` |
| r[lex.contextual.by] Delegation | `by` | after the target type of a trait implementation header, as in `impl Describe for Service by Logger` |

1. r[lex.contextual.reified.modifier] In the position the table gives for it, an unbackticked `reified` is always the modifier.
2. r[lex.contextual.reified.lone] A lone `reified`, as in `fn f[reified]()` or `[T, reified < Show]`, is therefore an error. Error: `syntax-error`.
3. r[lex.contextual.reified.raw] A parameter named reified is written `` [`reified`] ``.
4. r[lex.contextual.pack.always] The token sequences `pack . map (` and `pack . map_list (` always form the pack operation, even when a local or parameter named `pack` is in scope.
5. r[lex.contextual.pack.ordinary] Every other use of such a `pack`, as in `pack.size()`, is ordinary.
6. r[lex.contextual.elsewhere] These contextual words remain ordinary identifiers elsewhere. Declarations such as `fn test() -> void`, `fn map_list() -> void`, `fn derive() -> void`, and `fn use() -> void` are lexically valid. So are expressions such as `resource.use(f)` and `super := parent`.
7. r[lex.contextual.shadowing] The separate prelude shadowing rule still applies.

```text
trait Show:
    fn show(self) -> string

fn named[reified](value: i32) -> i32:  # error: syntax-error
    value

fn bounded[T, reified < Show](value: T) -> T:  # error: syntax-error
    value
```

## Literals

This section defines the literal forms.

### Boolean Literals

```ebnf
boolean_literal = "true" | "false" ;
```

1. r[lex.bool.literals] `true` and `false` are boolean literals.
2. r[lex.bool.no-nil] There is no literal for an absent optional: `nil` is an ordinary identifier.
3. r[lex.bool.none] Absence is written with the enum variant `.None`.

```text
let value: i32? = nil  # error
```

See also: [Optional Types](04-type-system.md#optional-types).

### Integer Literals

Integer literals use Python-style decimal, binary, octal, or hexadecimal
notation:

```ebnf
integer_literal = decimal_integer_literal
                | binary_integer_literal
                | octal_integer_literal
                | hexadecimal_integer_literal
                ;
decimal_integer_literal = decimal_digits ;
binary_integer_literal = "0", ( "b" | "B" ),
                         [ "_" ], binary_digits ;
octal_integer_literal = "0", ( "o" | "O" ),
                        [ "_" ], octal_digits ;
hexadecimal_integer_literal = "0", ( "x" | "X" ),
                              [ "_" ], hexadecimal_digits ;
BINARY_DIGIT = "0" | "1" ;
OCTAL_DIGIT = "0" ... "7" ;
binary_digits = BINARY_DIGIT, { [ "_" ], BINARY_DIGIT } ;
octal_digits = OCTAL_DIGIT, { [ "_" ], OCTAL_DIGIT } ;
hexadecimal_digits = HEX_DIGIT, { [ "_" ], HEX_DIGIT } ;
```

1. r[lex.int.forms] Integer literals use Python-style decimal, binary, octal, or hexadecimal notation.
2. r[lex.int.sign] A leading `-` is an operator, not part of the literal.
3. r[lex.int.radix-type] The radix prefix does not affect the inferred type.
4. r[lex.int.hex-case] Hexadecimal digits may use uppercase or lowercase letters.
5. r[lex.int.leading-zero] A leading zero without an explicit radix prefix remains decimal; it never selects octal implicitly.

See also: [Type System](04-type-system.md), which defines integer literal
typing and range checks.

#### Digit Separators

1. r[lex.sep.digits] An underscore may separate adjacent digits.
2. r[lex.sep.prefix] One underscore may also appear immediately after an explicit radix prefix, as in `0x_FF`.
3. r[lex.sep.placement] An underscore cannot begin or end a literal, occur twice consecutively, or touch a decimal point, exponent marker, or exponent sign.
4. r[lex.sep.value] Separators do not affect the literal's value or inferred type.
5. r[lex.sep.misplaced] A misplaced separator makes the literal form no token, so `1__0`, `1_`, `0x_`, `1_.5`, `1.5_`, `1_e5`, and `1e_5` are errors. Error: `invalid-token`.
6. r[lex.int.bare-prefix] A radix prefix followed by neither a digit of its radix nor a separator, as in a bare `0x`, is an error. Error: `syntax-error`.
7. r[lex.int.outside-radix] A radix literal followed directly by a letter or digit outside its radix, as in `0b1z` or `0b12`, is also an error. Error: `syntax-error`.

```text
count := 1__0  # error: invalid-token
value := 0x    # error: syntax-error
bits := 0b1z   # error: syntax-error
```

### Floating-Point Literals

Floating-point literals use a decimal fraction, an exponent, or both:

```ebnf
float_literal = decimal_fraction, [ decimal_exponent ]
              | decimal_digits, decimal_exponent
              ;
decimal_fraction = decimal_digits, ".", decimal_digits ;
decimal_exponent = ( "e" | "E" ), [ "+" | "-" ], decimal_digits ;
decimal_digits = DECIMAL_DIGIT, { [ "_" ], DECIMAL_DIGIT } ;
```

Thus `1e9`, `1.5e-6`, and `2E+8` are floating-point literals.

1. r[lex.float.forms] Floating-point literals use a decimal fraction, an exponent, or both.
2. r[lex.float.point] A decimal point requires digits on both sides.
3. r[lex.float.no-bare-point] `.5` and `1.` are invalid; write `0.5` and `1.0`.
4. r[lex.float.sep] Separators may occur between digits in the integer, fractional, and exponent parts.
5. r[lex.float.sep-misplaced] A separator anywhere else in a floating-point literal, as in `1_.5`, `1.5_`, or `1e+_5`, is an error. Error: `invalid-token`.
6. r[lex.float.no-hex] Hexadecimal floating-point notation is not supported.

```text
value := 1_.5  # error: invalid-token
value := 1.5_  # error: invalid-token
value := 1e_5  # error: invalid-token
```

### String And Character Literals

Double quotes delimit a `string` literal, and single quotes delimit a `char`
literal:

```text
name := "Ada"
initial := 'A'
greeting := "你好"
pattern := r"\d+\s+\w+"
template := r"""first line
second line"""
message := """hello
world"""
welcome := "Hello, $name"
summary := """User: ${user.name}
Posts: ${posts.len()}"""
```

```ebnf
string_literal = interpreted_string_literal
               | interpreted_multiline_string_literal
               | raw_string_literal
               | raw_multiline_string_literal
               ;
interpreted_string_literal = '"',
                             { string_character | escape_sequence }, '"' ;
interpreted_multiline_string_literal = '"""',
                                       { multiline_string_character
                                       | escape_sequence }, '"""' ;
raw_string_literal = 'r"', { raw_string_character }, '"' ;
raw_multiline_string_literal = 'r"""',
                               { raw_multiline_character }, '"""' ;
char_literal   = "'", (char_character | escape_sequence), "'" ;

string_text = string_character, { string_character } ;
multiline_string_text = multiline_string_character,
                        { multiline_string_character } ;

escape_sequence = "\\", ( "\\" | '"' | "'" | "n" | "r" | "t" | "0"
                       | "$" | unicode_escape ) ;
unicode_escape = "u", "{", HEX_DIGIT, { HEX_DIGIT }, "}" ;
HEX_DIGIT = DECIMAL_DIGIT | "A" ... "F" | "a" ... "f" ;

string_character = ? any Unicode scalar value other than a double quote, a backslash, a dollar sign, or a line ending ? ;
multiline_string_character = ? any Unicode scalar value other than a double quote, a backslash, or a dollar sign ? ;
raw_string_character = ? any Unicode scalar value other than an unescaped double quote or a line ending ? ;
raw_multiline_character = ? any Unicode scalar value other than the start of an unescaped """ delimiter ? ;
char_character = ? any Unicode scalar value other than a single quote, a backslash, or a line ending ? ;
```

1. r[lex.string.quotes] Double quotes delimit a `string` literal.
2. r[lex.char.quotes] Single quotes delimit a `char` literal.
3. r[lex.string.classes] The character classes of the grammar above are these sets of Unicode scalar values:

| Class | Contents |
| --- | --- |
| `string_character` | any Unicode scalar value other than `"`, `\\`, `$`, or a line ending |
| `multiline_string_character` | the same exclusions as `string_character`, except that line endings are allowed |
| `raw_string_character` | any Unicode scalar value other than an unescaped `"` or a line ending |
| `raw_multiline_character` | any Unicode scalar value other than the start of an unescaped `"""` delimiter |
| `char_character` | any Unicode scalar value other than `'`, `\\`, or a line ending |

1. r[lex.string.text-runs] `string_text` and `multiline_string_text` are maximal nonempty runs of their corresponding character class between interpolation or escape segments.
2. r[lex.char.one-scalar] A character literal must decode to exactly one Unicode scalar value.
3. r[lex.string.scalars] A string literal is a sequence of Unicode scalar values.
4. r[lex.string.single-line] A single-line literal must not contain an unescaped line ending or an unescaped copy of its own delimiter.

#### Multiline Strings

1. r[lex.multiline.form] An interpreted multiline string uses `"""..."""`.
2. r[lex.multiline.escapes] It accepts the same escape sequences as a single-line interpreted string and may contain physical line endings.
3. r[lex.multiline.verbatim] Source indentation and line endings inside the delimiters are part of the value; the compiler does not dedent or trim them.
4. r[lex.multiline.line-feed] Each source line ending contributes one line-feed scalar to the value.
5. r[lex.multiline.end] The literal continues until an unescaped `"""` delimiter.

#### Interpolation

r[lex.interp.forms] Interpreted single-line and multiline strings use
Kotlin-style interpolation:

| Rule | Form | Interpolates |
| --- | --- | --- |
| r[lex.interp.name] Name | `$name` | one identifier |
| r[lex.interp.self] Receiver | `$self` | the receiver |
| r[lex.interp.expression] Expression | `${expression}` | an arbitrary expression with balanced nested delimiters |

1. r[lex.interp.name-extent] The name after `$` extends over every identifier character.
2. r[lex.interp.stray-dollar] An unescaped `$` must begin one of those forms. A `$` followed by any other reserved word, as in `"$true"`, or by a character that cannot start an identifier, as in `"costs $5"`, is an error. Error: `syntax-error`.
3. r[lex.interp.escaped-dollar] `\$` produces a literal dollar sign.
4. r[lex.interp.braces] Braces without a leading `$` are ordinary string content.
5. r[lex.interp.scanning] The lexer switches back to normal expression tokenization inside `${...}` and resumes string scanning at the matching `}`.

```text
fn main() -> string:
    "flag: $true"  # error: syntax-error

price := "$"  # error: syntax-error
```

#### Raw Strings

1. r[lex.raw-string.form] Raw strings use Python-style `r"..."` and raw multiline strings use `r"""..."""`.
2. r[lex.raw-string.literal] Backslashes and escape-looking text are preserved literally.
3. r[lex.raw-string.backslash] A backslash may prevent the following quote from terminating the raw literal, but that backslash remains part of the resulting string.
4. r[lex.raw-string.odd-backslashes] Consequently, a raw string cannot end with an odd number of backslashes immediately before its closing delimiter.
5. r[lex.raw-string.single-line] A single-line raw string cannot contain a physical line ending.
6. r[lex.raw-string.multiline] A raw multiline string may contain line endings and continues until an unescaped `"""` delimiter.
7. r[lex.raw-string.no-hash] Hash-delimited raw strings are not part of the language.
8. r[lex.raw-string.no-interpolation] Raw strings do not interpolate, so `$` and `${...}` remain literal content in both raw forms.

#### Escape Sequences

r[lex.escape.simple] The simple escapes mean backslash, double quote, single
quote, line feed, carriage return, horizontal tab, and null respectively:

| Escape | Meaning |
| --- | --- |
| `\\` | backslash |
| `\"` | double quote |
| `\'` | single quote |
| `\n` | line feed |
| `\r` | carriage return |
| `\t` | horizontal tab |
| `\0` | null |

1. r[lex.escape.unicode] A Unicode escape has one to six hexadecimal digits and must denote a Unicode scalar value in `0..10FFFF`, excluding surrogate code points `D800..DFFF`.
2. r[lex.escape.other] Any other escape is a lexical error.
3. r[lex.bytes.none] hd-lang has no byte or bytes literal and no primitive byte or bytes type.

```text
maximum := "\u{10FFFF}"  # valid: the maximum scalar value
beyond := "\u{110000}"   # error
high := "\u{D800}"       # error
value := "bad\xescape"   # error
```

## Operators And Delimiters

r[lex.punct.tokens] The lexer recognizes these punctuation tokens:

```text
( ) [ ] { } , . : ;
```

1. r[lex.punct.semicolon] The current grammar does not use `;` as a statement separator.
2. r[lex.punct.semicolon-reserved] It is reserved for possible future use and must be diagnosed if it appears in a program.

```text
name := "Ada";  # error
```

r[lex.op.tokens] The lexer recognizes these operators and compound
punctuation tokens:

```text
+  -  *  /  %  **
&  |  ^  ~  <<  >>  &&  ||
=  ==  !=  <  <=  >  >=
:=  ->  =>  ?  !  $  @  ...  ...=  ::
```

1. r[lex.op.longest] When two tokens share a prefix, the lexer uses the longest valid token.
2. r[lex.op.longest.examples] For example, `**` is one token rather than two `*` tokens, and `...` is one token rather than three `.` tokens. Likewise `...=` is one token rather than `...` and `=`, and `&&` and `||` are single tokens.
3. r[lex.op.inequality] The sequence `!=` is always the inequality token, so `f!=g` lexes as `f`, `!=`, `g`.
4. r[lex.op.bang-call] A suspension call needs `!` immediately followed by `(`.

See also: [Expressions](05-expressions.md), which defines operator
precedence and semantics, including prefix `!` as logical not;
[Requirements and Suspension](11-requirements-and-suspension.md), which
specifies `$` and suspension-related uses of `!`.

## Lexical Token Grammar

The following EBNF summarizes the currently specified token classes. Layout
processing occurs after token recognition, as described above.

```ebnf
token = identifier
      | raw_identifier
      | keyword
      | literal_token
      | operator
      | delimiter
      ;

literal_token = boolean_literal
        | float_literal
        | integer_literal
        | string_literal
        | char_literal
        ;

delimiter = "(" | ")" | "[" | "]" | "{" | "}"
          | "," | "." | ":" | ";"
          ;

operator = "+" | "-" | "*" | "/" | "%" | "**"
         | "&" | "|" | "^" | "~" | "<<" | ">>" | "&&" | "||"
         | "=" | "==" | "!=" | "<" | "<=" | ">" | ">="
         | ":=" | "->" | "=>" | "?" | "!" | "$" | "@"
         | "..." | "...=" | "::"
         ;

keyword = ? a reserved word listed in Keywords And Reserved Words ? ;
```

1. r[lex.grammar.keyword] `keyword` expands to the reserved words listed above.
2. r[lex.grammar.abstract-tokens] The abstract tokens `NEWLINE`, `INDENT`, `DEDENT`, `SUITE_END`, and end-of-file are produced by layout processing rather than matched directly from source characters.

## Unsupported Lexical Extensions

1. r[lex.unsupported.hex-float] Hexadecimal floating-point notation is not part of the language.
2. r[lex.unsupported.diagnose] Implementations must diagnose it rather than assign implementation-defined behavior.

> **Note.** A future extension may add hexadecimal floating-point notation
> with new grammar.
