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
4. r[lex.process.join] Join physical lines that continue inside `()`, `[]`, or `{}`, and leading-dot continuation lines, into logical lines. An indentation suite nested in that continuation is the exception.
5. r[lex.process.layout] Emit `NEWLINE`, `INDENT`, `DEDENT`, and same-line `SUITE_END` layout tokens from logical lines and nested suites.
6. r[lex.process.parse] Parse the resulting token stream.

r[lex.process.trivia] Comments and whitespace separate tokens but otherwise
do not appear in the parser token stream. Layout tokens are the exception.

```hd
# a comment, gone before parsing
fn demo() -> i32:
    +1
```

### Source Encoding

1. r[lex.encoding.utf8] Source files must be valid UTF-8.
2. r[lex.encoding.invalid] Invalid UTF-8 is a compile-time lexical error.
3. r[lex.encoding.bom] If the first three bytes are `EF BB BF`, they are removed before lexical analysis.
4. r[lex.encoding.other-bom] No other byte-order mark is removed.
5. r[lex.encoding.stray-bom] A `U+FEFF` outside a comment or literal at any other position is a compile-time lexical error. Error: `unexpected-bom`.
6. r[lex.encoding.scalars] Unicode scalar values are valid in identifiers, under the identifier rules below, and in comments, string literals, and character literals.
7. r[lex.limit.file-size] An implementation may limit the size of a source file in bytes. The limit is implementation-defined.
8. r[lex.limit.file-size.error] A source file over that limit is an error. Error: `file-too-large`.

```hd
fn café() -> string:
    "with accents"
```

> **Why.** A limit bounds the time and memory that checking any input
> can take. Each limit has its own code, so a report names the limit.

See also: [Identifiers](#identifiers).

## Physical And Logical Lines

This section defines physical lines, the logical lines that join them, and
the layout tokens emitted inside delimiters.

### Line Endings

1. r[lex.line.physical] A physical line ends at a line-feed character or at the end of the file.
2. r[lex.line.crlf] A carriage-return followed by a line-feed is treated as one line ending.
3. r[lex.line.bare-cr] A bare carriage return is a lexical error. Error: `invalid-token`.
4. r[lex.line.no-backslash] hd-lang has no explicit backslash line-continuation syntax. A backslash outside a literal is an error. Error: `invalid-token`.

```hd
fn demo() -> i32:
    x := +1
    x
```

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
4. r[lex.continue.suffix-line] Five tokens must start on the same physical line as the end of their operand. They are a call `(`, an index `[`, the `::` of a type-argument list, a data-literal `{`, and the `!` of a suspension call.
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
word := line
    .slice(0, 8)
    .slice(2, 5)
```

1. r[lex.dot.continue] A physical line also continues the previous logical line when all three of the following conditions hold.
2. r[lex.dot.first-token] Its first token is `.` immediately followed by an identifier, a member suffix.
3. r[lex.dot.deeper] It is indented farther than the first physical line of the logical line it continues.
4. r[lex.dot.no-opener] That logical line does not end in `:` or `=>`, the tokens that open an indented suite or match-arm body on the next line.
5. r[lex.dot.no-layout] Such a leading-dot line emits no `NEWLINE`, `INDENT`, or `DEDENT`.
6. r[lex.dot.blank-lines] Blank lines and comment-only lines before it do not matter.
7. r[lex.dot.joined] The joined text is read as if it were written on one physical line.
8. r[lex.dot.open-suite] A leading-dot line is an error when a same-line suite is still open at the end of the logical line it would continue. One case is the line after `f := fn(x): x`. Error: `syntax-error`.
9. r[lex.dot.closed-suite] A same-line suite that closed earlier on the line, such as one inside `apply(xs, fn(x): x)`, does not prevent the continuation.
10. r[lex.dot.statement-indent] A leading-dot line indented no deeper than the first line of the statement it would continue starts a new statement. This holds whatever the indentation of the physical line before it.
11. r[lex.dot.contextual-statement] So a `.Variant` line at statement indentation, such as `.Ok(Step { ... })` after a `:=` line, is a contextual-variant expression statement or tail value. A match arm starts the same way.
12. r[lex.dot.suite-line] So does the first line of an indented suite, whose header ends in `:`.
13. r[lex.dot.where] The rule applies at delimiter depth zero and on the body lines of a suite nested inside delimiters. Elsewhere inside delimiters every line already continues.
14. r[lex.continue.no-other-operator] A line starting with a binary operator other than `|>` never continues the previous line.
15. r[lex.continue.paren-line] A line whose first token is `(` never continues the previous line either. Outside delimiters it starts a new logical line, so `f` on one line and `(a, b)` on the next are two statements, never the call `f(a, b)`.

Only a deeper line continues. Below, `.grow()` and `.size` continue
`start`, and `.Ok(...)` at statement indentation is the tail value:

```text
data Step:
    size: i32

impl Step:
    fn grow(self) -> Step: Step { size: self.size + 1 }

fn measure(start: Step) -> Result[Step, string]:
    size := start
        .grow()
        .size
    .Ok(Step { size: size })
```

```text
fn sizer() -> fn(string) -> usize:
    f := fn(name: string) -> usize: name
        .len()  # error: syntax-error
    f

fn total(a: i32, b: i32) -> i32:
    sum := a
        + b  # error: syntax-error
    sum

fn first(pair: (i32, i32)) -> i32:
    value := pair
        .0  # error: syntax-error
    value
```

> **Why.** Continuing a line whose same-line suite is still open would
> silently join the chain to that suite's body.

### Leading-Pipe Continuation

A [pipe](05-expressions.md#pipe-expressions) chain may continue on lines
that start with `|>`:

```text
fn label(raw: string) -> usize:
    raw
        |> _.slice(0, 8)
        |> _.len()
```

1. r[lex.pipe.continue] A physical line whose first token is `|>` continues the previous logical line under the conditions of a leading-dot line.
2. r[lex.pipe.conditions] It must be indented farther than the first physical line of the logical line it continues. That logical line must not end in `:` or `=>`.
3. r[lex.pipe.layout] Like a leading-dot line, it emits no layout tokens and ignores blank and comment-only lines before it. It is read as if joined to the previous line.
4. r[lex.pipe.open-suite] A leading-`|>` line is an error when a same-line suite is still open at the end of the logical line it would continue. Error: `syntax-error`.
5. r[lex.pipe.no-dot-line] A leading-dot line is an error when the logical line it would continue contains `|>` at that line's own delimiter depth. Error: `syntax-error`.
6. r[lex.pipe.dot-before] A leading-dot line before the first `|>` of its logical line is valid. It continues the value that the chain pipes, as `.len()` does in `values`, `.len()`, `|> twice` on three lines.

```text
fn size(raw: string) -> usize:
    count := raw
        |> _.slice(0, 8)
        .len()  # error: syntax-error
    count
```

> **Why.** After `|> f`, a leading `.len()` would attach to the step `f`
> rather than to the pipe's result. Write the call as a step,
> `|> _.len()`, or bind a name first.

### Suites Inside Delimiters

1. r[lex.nested.no-layout] Comments and line endings inside an implicit continuation normally do not emit `NEWLINE`, `INDENT`, or `DEDENT`.
2. r[lex.nested.suite] The exception is a suite introduced by a grammar position that expects `:` followed by `suite_body`.
3. r[lex.nested.layout] When that suite starts on the next physical line, layout processing emits its `NEWLINE`, `INDENT`, body layout, and closing `DEDENT`. This holds even if surrounding delimiters are still open.
4. r[lex.nested.resume] After the suite closes, implicit continuation resumes.
5. r[lex.nested.reference] The indentation reference for such a nested suite is the indentation of the physical line containing its suite header.
6. r[lex.nested.body-depth] Its first body line must be indented farther than that reference. It must also be indented farther than the first physical line of the logical line that contains the header. Otherwise it is an error. Error: `unexpected-indentation`.
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

> **Note.** A line between the header and the body may also dedent to a
> column that no enclosing suite uses, which
> [`lex.indent.unknown-column`](#r-lex.indent.unknown-column) reports as
> `invalid-dedent`. [`lex.closure.between`](#r-lex.closure.between) is
> the more specific rule, so the error is `syntax-error`.

> **Why.** A line at body indentation belongs to the body. So a later
> argument written on it is caught rather than silently becoming the
> closure's result.

#### Headers After A Nested Suite

1. r[lex.closure.statement] A closure written as a statement in a nested suite's body ends like any other statement.
2. r[lex.header.resume] Inside brackets, a header may resume on the line after a nested suite that is not a closure body, such as an `if` expression.
3. r[lex.header.resume-closure] After a closure body it resumes only past the closing delimiter, as in `(if check(fn(x): ...` followed by a line that starts with `):`.
4. r[lex.header.no-indented-end] Outside brackets, and in the statements of a nested suite, a header cannot end in an indented suite. The line after that suite cannot continue it.

```hd
fn demo(flag: bool) -> i32:
    (if flag: +1
     else: +2)
```

See also: [Statements](02-grammar.md#statements).

### Suite-Introducing Colons

1. r[lex.colon.cooperate] Layout recognition and parsing therefore cooperate at a suite-introducing colon.
2. r[lex.colon.implementation] A lexer may implement this with parser feedback or with equivalent parser-state tracking.
3. r[lex.colon.ordinary] Ordinary colons in maps, data fields, named types, and arguments do not open a suite.
4. r[lex.colon.trailing-block] Trailing-block call colons occur only at delimiter depth zero. Even there, the call must be the complete statement or the complete right-hand side of `:=`, `let ... =`, `=`, `_ :=`, `return`, or `break`.
5. r[lex.colon.trailing-block.not-headers] They are not recognized in `if`, `while`, `for`, or `match` headers or inside brackets.

```hd
fn demo() -> void:
    _ := {"a": +1}   # an ordinary colon, no suite
```

See also: [Trailing Callback Blocks](07-functions.md#trailing-callback-blocks).

### Same-Line Suites

1. r[lex.suite-end.token] A same-line suite ends with the abstract token `SUITE_END`.
2. r[lex.suite-end.line] At a logical line boundary, layout closes every same-line suite opened on that logical line. It emits one `SUITE_END` per suite, from innermost to outermost.
3. r[lex.suite-end.newline] At delimiter depth zero the outermost `SUITE_END` replaces that line's `NEWLINE`; it does not precede a second terminator.
4. r[lex.suite-end.continuation] In an implicit continuation, the equivalent boundary is a comma or closing delimiter that returns control to the enclosing expression. The same innermost-first sequence is emitted before that token.
5. r[lex.suite-end.comma] A comma at the delimiter depth where a same-line suite opened always closes that suite, including at depth zero. So the suite body cannot contain such a comma.
6. r[lex.suite-end.example] For example, the body of `fn(name): name.len()` ends immediately before that closure's closing `)`.
7. r[lex.suite-end.else] `else` is also a boundary for the immediately preceding same-line `if`, `for`, or `while` suite. Layout emits that suite's `SUITE_END` before `else` and keeps the enclosing conditional or loop open, as the conditional and loop productions require.
8. r[lex.suite-end.else-next-line] At delimiter depth zero, `else` may follow a same-line `if` suite on the next physical line at the containing statement's indentation.
9. r[lex.suite-end.else-example] Thus `x := if c: 1 else: 2` is one conditional expression. The line boundary after `2` closes the `else` suite and then any enclosing same-line suite, innermost first.
10. r[lex.suite-end.no-spelling] `SUITE_END` has no source spelling. Parser-aware layout processing identifies the boundary from the expected suite and enclosing delimiter structure.

```hd
fn demo(flag: bool) -> i32:
    x := if flag: +1 else: +2
    y := if flag: +1
    else: +2
    x + y
```

### Delimiter Matching

1. r[lex.delim.match] A closing delimiter must match the most recent unclosed delimiter.
2. r[lex.delim.unmatched] A closing delimiter with no open delimiter, or one that closes a different kind of delimiter, is an error. Error: `unmatched-delimiter`.
3. r[lex.delim.unclosed] An opening delimiter that is still unclosed at the end of the file is an error. Error: `unclosed-delimiter`.

```text
fn total() -> i32:
    +1
]   # error: unmatched-delimiter
```

## Whitespace And Indentation

1. r[lex.space.meaning] Spaces between tokens have no meaning except when they occur at the beginning of a logical line.
2. r[lex.indent.structure] Leading indentation determines block structure.

```hd
fn demo() -> i32:
    x := +1
    y := +2
    x + y
```

### Indentation Levels

1. r[lex.indent.compare] For each non-empty logical line outside implicit continuation, the lexer compares its indentation with a stack of active indentation levels.
2. r[lex.indent.equal] Equal indentation emits no layout token.
3. r[lex.indent.greater] Greater indentation pushes the new level and emits one `INDENT`.
4. r[lex.indent.lesser] Lesser indentation emits one or more `DEDENT` tokens until an existing level is reached.
5. r[lex.indent.unknown-column] Dedenting to a column that is not an active indentation level is a compile-time error. Error: `invalid-dedent`.
6. r[lex.indent.first] The first indentation level is zero.
7. r[lex.indent.eof] At end of file, the lexer emits any remaining `DEDENT` tokens.
8. r[lex.indent.eof-newline] If the final non-empty logical line has no physical line ending, the lexer emits its terminating `NEWLINE` before those `DEDENT` tokens.
9. r[lex.indent.blank] Blank lines and comment-only lines do not affect the indentation stack and do not emit `NEWLINE` tokens.

```text
fn f() -> i32:
    x := +1
   y := +2   # error: invalid-dedent
```

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

1. r[lex.tab.invalid] A horizontal tab character used as source whitespace is an error. Error: `tab-whitespace`.
2. r[lex.tab.content] They may occur only as literal content represented by the `\t` escape or as raw characters inside comments. Error: `tab-whitespace`.
3. r[lex.tab.spaces] Indentation therefore consists only of ASCII space characters.

```hd
fn demo() -> string:
    "a\tb"   # a tab as literal content, through its escape
```

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
5. r[lex.doc.field] For a field or variant, the resulting string is the `doc` field of its `Member` or `VariantInfo`; for any other target it is for tools. Without an attached documentation comment, `doc` is `.None`.
6. r[lex.doc.trailing] A trailing `##` comment after source code is ordinary commentary and does not attach.
7. r[lex.doc.module] The first documentation-comment block of a file documents the file's module when no token precedes it and a blank line follows it.
8. r[lex.doc.module.once] A file has at most one module documentation block. A later block follows `lex.doc.attach`.
9. r[lex.doc.module.text] The text of module documentation is formed as `lex.doc.text` forms it, and is for tools.
10. r[lex.doc.unattached] An otherwise unattached documentation-comment line is a lexical error. Error: `doc-comment-without-target`.

```text
fn run() -> void:
    ## This cannot document an executable statement.  # error: doc-comment-without-target
    value := 1
```

A module documentation block sits at the top of the file, apart from the
first declaration:

```text
## Text helpers for URLs and titles.

## Turns a title into a URL slug.
pub fn slugify(title: string) -> string: title
```

> **Note.** A fenced `hd` block inside a documentation comment, module
> documentation included, is a [doc test](10-modules.md#doc-tests), which
> `hd test` runs.

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
4. r[lex.ident.non-nfc] A non-NFC identifier is a lexical error rather than being silently rewritten. Error: `invalid-token`.
5. r[lex.ident.placeholder] The single source spelling `_` is a distinct placeholder token, not an `identifier`.
6. r[lex.ident.underscore] An identifier that begins with `_` must contain at least one additional `identifier_continue` character.
7. r[lex.ident.xid] `XID_START` and `XID_CONTINUE` denote the corresponding Unicode derived core properties.
8. r[lex.ident.unicode-version] An implementation must use one declared Unicode data version consistently for lexing, normalization, and diagnostics.

> **Note.** Tuple elements and unnamed shared enum parameters are selected
> through identifiers such as `_0` and `_1`. No lexing rule treats digits
> after `.` specially, so `t.0.1` lexes as `t`, `.`, and the floating-point
> literal `0.1`.

See also: [Bang And Dot Tokens](02-grammar.md#bang-and-dot-tokens).

### Identifier Security

1. r[lex.ident.mixed-script.warning] The compiler must warn about an identifier that suspiciously mixes scripts. Warning: `mixed-script-identifier`.
2. r[lex.ident.identity] This security diagnostic does not change name identity: two different NFC identifier strings remain different names.
3. r[lex.ident.ascii] Standard-library APIs, language keywords, and compiler-generated source names use ASCII.

```hd
fn naïve() -> i32:
    +1
```

### Reserved Words And Built-In Names

1. r[lex.ident.reserved] An identifier that exactly matches a reserved word is not an identifier token.
2. r[lex.ident.builtin-types] Built-in type names such as `i32`, `string`, `List`, and `Map` are ordinary names rather than lexically distinct tokens.

```text
fn done() -> void:
    let match = +1   # error: syntax-error
```

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
7. r[lex.raw.not-reserved] A raw identifier is never a reserved word or a contextual word: `` `use` `` never begins a use declaration.
8. r[lex.raw.interpolation] `$name` interpolation takes a plain identifier; `` ${`type`} `` interpolates a raw one.

```text
fn main() -> i32:
    `` := 1  # error: invalid-token
    0

data Token:
    `type`: string

fn kind(token: Token) -> string:
    token.type  # error: syntax-error
```

## Keywords And Reserved Words

r[lex.keyword.reserved-words] The grammar uses these reserved words:

```text
Self      break     continue  data      defer     dyn
else      enum      false     fn        for       if
impl      in        is        let       match     mut
pass      pub       return    self      tests     trait
true      type      while
```

1. r[lex.keyword.tests] `tests` begins a module's `tests:` block ([Test Blocks](02-grammar.md#test-blocks)).
2. r[lex.keyword.dyn] `dyn` begins a trait value type, as in `dyn Display` ([Trait Value Types](04-type-system.md#trait-value-types)).

### Contextual Words

r[lex.contextual.fixed] The following contextual words have special meaning
only in fixed positions:

| Rule | Words | Position |
| --- | --- | --- |
| r[lex.contextual.use-root] Use roots | `pkg`, `std`, `dep`; `super` | `pkg`, `std`, and `dep` in a use root position, and `super` as a use root, alone or repeated, as in `use super.shared.{Email}` |
| r[lex.contextual.as] Alias | `as` | directly after a use path or use item, before its alias |
| r[lex.contextual.use] Use | `use` | at the start of a module-level item, alone or after `pub`, when a use root (`pkg`, `std`, `dep`, `self`, or `super`) follows it; and as the operation name in the dedicated `$.use(...)` provider expression |
| r[lex.contextual.context] Context | `context`, `with`, `Context` | after `$.` |
| r[lex.contextual.derive] Derive | `derive` | immediately after `@` |
| r[lex.contextual.by-header] Delegation and derivation | `by` | after the target type of an implementation header, as in `impl Describe for Service by Logger` or `impl User by Structure`, without a trait |

1. r[lex.contextual.elsewhere] These contextual words remain ordinary identifiers elsewhere. Declarations such as `fn with() -> void`, `fn derive() -> void`, and `fn use() -> void` are lexically valid. So are expressions such as `resource.use(f)` and `super := parent`.
2. r[lex.contextual.shadowing] The separate prelude shadowing rule still applies.

```text
fn use(value: i32) -> i32:
    value + 1

data Resource:
    as: i32
```

## Literals

This section defines the literal forms.

### Boolean Literals

```ebnf
boolean_literal = "true" | "false" ;
```

1. r[lex.bool.literals] `true` and `false` are boolean literals.
2. r[lex.bool.no-nil] There is no literal for an absent optional: `nil` is an ordinary identifier, so an undeclared `nil` is an error. Error: `unknown-name`.
3. r[lex.bool.none] Absence is written with the enum variant `.None`.

```text
let value: i32? = nil  # error: unknown-name
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
3. r[lex.float.no-bare-point] `.5` and `1.` are errors; write `0.5` and `1.0`. Error: `invalid-token`.
4. r[lex.float.sep] Separators may occur between digits in the integer, fractional, and exponent parts.
5. r[lex.float.sep-misplaced] A separator anywhere else in a floating-point literal, as in `1_.5`, `1.5_`, or `1e+_5`, is an error. Error: `invalid-token`.
6. r[lex.float.no-hex] Hexadecimal floating-point notation is not supported.

```text
value := 1_.5  # error: invalid-token
value := 1.5_  # error: invalid-token
value := 1e_5  # error: invalid-token
```

### Literal Suffixes

A numeric literal may end in a **literal suffix**, a name written directly
after its digits:

```text
use std.ops.num_suffix

@num_suffix
fn ms(count: i64) -> i64: count

@num_suffix
fn s(count: i64) -> i64: count * 1000

timeout := 5s
delay := 1_500ms
```

```ebnf
suffixed_literal = ( decimal_integer_literal | float_literal ), literal_suffix ;
literal_suffix = XID_START, { identifier_continue } ;
```

1. r[lex.suffix.form] A **suffixed literal** is a numeric literal followed directly by a literal suffix, with nothing between them.
2. r[lex.suffix.name] A literal suffix begins with a Unicode `XID_Start` character and takes every following `identifier_continue` character.
3. r[lex.suffix.decimal] Only a decimal integer or floating-point literal takes a suffix, as in `5s`, `1.5kb`, and `5_000ms`. A letter after a radix literal follows the integer rules, so `0xffB` is `0xffb` and `0x1fs` is an error. Error: `syntax-error`.
4. r[lex.suffix.exponent] An `e` or `E` after the digits begins an exponent when digits follow it, after an optional sign, and otherwise a suffix. So `1e3ms` is `1e3` with the suffix `ms`, and `5em` is `5` with the suffix `em`.
5. r[lex.literal-fn.reserved-glued] A reserved word written directly after a numeric literal or directly before a string literal is an error. That is the position where a suffix or prefix would go, as in `5else` and `return"done"`. Error: `syntax-error`.
6. r[lex.literal-fn.meaning] A literal suffix or string prefix is resolved as a name, and the literal is a call, as [Literal Suffixes](05-expressions.md#literal-suffixes) specifies.

```text
wait := 5_ms   # error: invalid-token
mask := 0b101s  # error: syntax-error
flag := 5else   # error: syntax-error
name := "abc"u  # error: syntax-error
fn done() -> string: return"done"  # error: syntax-error
mask := 0xff'B  # error: unterminated-string
```

> **Note.** The other near misses need no rule of their own. `5_ms` ends
> its digits in a separator ([`lex.sep.misplaced`](#r-lex.sep.misplaced)),
> and `"abc"u` is a string followed by a name. A `'` after digits begins a
> character literal, so `5'ms` is unterminated.

> **Why.** Hexadecimal digits include letters such as `B`, so a letter
> after a radix literal cannot begin a suffix. Only decimal and
> floating-point literals take one, so radix literals need no separate
> spelling. A reserved word glued to a literal reads like a suffix or a
> prefix, so it is rejected, and a space fixes it.

See also: [Suffixed Literals](04-type-system.md#suffixed-literals),
[Literal Suffix Names](03-names-and-scopes.md#literal-suffix-names).

### String And Character Literals

Double quotes delimit a `string` literal, and single quotes delimit a `char`
literal:

```text
name := "Ada"
initial := 'A'
greeting := "你好"
message := """hello
world"""
welcome := "Hello, $name"
summary := """User: ${user.name}
Posts: ${posts.len()}"""
```

```ebnf
string_literal = interpreted_string_literal
               | interpreted_multiline_string_literal
               ;
interpreted_string_literal = '"',
                             { string_character | escape_sequence
                             | dollar_text }, '"' ;
interpreted_multiline_string_literal = '"""',
                                       { multiline_string_character
                                       | escape_sequence | dollar_text },
                                       '"""' ;
char_literal   = "'", (char_character | escape_sequence), "'" ;
prefixed_string_literal = string_prefix, '"',
                          { prefixed_string_character }, '"'
                        | string_prefix, '"""',
                          { prefixed_multiline_character }, '"""' ;
string_prefix = identifier ;

string_text = string_character, { string_character } ;
multiline_string_text = multiline_string_character,
                        { multiline_string_character } ;

escape_sequence = "\\", ( "\\" | '"' | "'" | "n" | "r" | "t" | "0"
                       | "$" | unicode_escape ) ;
unicode_escape = "u", "{", HEX_DIGIT, { HEX_DIGIT }, "}" ;
HEX_DIGIT = DECIMAL_DIGIT | "A" ... "F" | "a" ... "f" ;

string_character = ? any Unicode scalar value other than a double quote, a backslash, a dollar sign, or a line ending ? ;
multiline_string_character = ? any Unicode scalar value other than a backslash, a dollar sign, or the start of an unescaped """ delimiter ? ;
prefixed_string_character = ? any Unicode scalar value other than an unescaped double quote or a line ending ? ;
prefixed_multiline_character = ? any Unicode scalar value other than the start of an unescaped """ delimiter ? ;
char_character = ? any Unicode scalar value other than a single quote, a backslash, or a line ending ? ;
dollar_text = ? a dollar sign followed by neither "{" nor a character that can start an identifier ? ;
```

1. r[lex.string.quotes] Double quotes delimit a `string` literal.
2. r[lex.char.quotes] Single quotes delimit a `char` literal.
3. r[lex.string.classes] The character classes of the grammar above are these sets of Unicode scalar values:

| Class | Contents |
| --- | --- |
| `string_character` | any Unicode scalar value other than `"`, `\\`, `$`, or a line ending |
| `multiline_string_character` | any Unicode scalar value other than `\\`, `$`, or the start of an unescaped `"""` delimiter, so line endings and a lone `"` are allowed |
| `prefixed_string_character` | any Unicode scalar value other than an unescaped `"` or a line ending |
| `prefixed_multiline_character` | any Unicode scalar value other than the start of an unescaped `"""` delimiter |
| `char_character` | any Unicode scalar value other than `'`, `\\`, or a line ending |
| `dollar_text` | a `$` followed by neither `{` nor a character that can start an identifier, as [`lex.interp.dollar-text`](#r-lex.interp.dollar-text) states |

1. r[lex.string.text-runs] `string_text` and `multiline_string_text` are maximal nonempty runs of their corresponding character class between interpolation or escape segments.
2. r[lex.char.one-scalar] A character literal that does not decode to exactly one Unicode scalar value is an error. Error: `invalid-token`.
3. r[lex.string.scalars] A string literal is a sequence of Unicode scalar values.
4. r[lex.string.single-line] A single-line literal must not contain an unescaped line ending or an unescaped copy of its own delimiter.

#### Multiline Strings

1. r[lex.multiline.form] An interpreted multiline string uses `"""..."""`.
2. r[lex.multiline.escapes] It accepts the same escape sequences as a single-line interpreted string and may contain physical line endings.
3. r[lex.multiline.verbatim] Source indentation and line endings inside the delimiters are part of the value; the compiler does not dedent or trim them.
4. r[lex.multiline.line-feed] Each source line ending contributes one line-feed scalar to the value.
5. r[lex.multiline.end] The literal continues until an unescaped `"""` delimiter.

```hd
fn demo() -> string:
    """
    line one
    line two
    """
```

#### Interpolation

r[lex.interp.forms] Interpreted single-line and multiline strings use
Kotlin-style interpolation:

| Rule | Form | Interpolates |
| --- | --- | --- |
| r[lex.interp.name] Name | `$name` | one identifier |
| r[lex.interp.self] Receiver | `$self` | the receiver |
| r[lex.interp.expression] Expression | `${expression}` | an arbitrary expression with balanced nested delimiters |

1. r[lex.interp.name-extent] The name after `$` extends over every identifier character.
2. r[lex.interp.dollar-text] In every string, plain or prefixed, an unescaped `$` followed by neither `{` nor a character that can start an identifier is text. So `"costs $5"` keeps `$5` as text, and `"$"` is a one-character string.
3. r[lex.interp.reserved-dollar] In every string, a `$` followed by a reserved word other than `self`, as in `"$true"` or `r"$true"`, is an error. Error: `syntax-error`.
4. r[lex.interp.escaped-dollar] `\$` produces a literal dollar sign.
5. r[lex.interp.braces] Braces without a leading `$` are ordinary string content.
6. r[lex.interp.scanning] The lexer switches back to normal expression tokenization inside `${...}` and resumes string scanning at the matching `}`.

```text
fn main() -> string:
    "flag: $true"  # error: syntax-error

price := "costs $5"  # valid: `$5` is text
```

> **Why.** A `$` before a digit, a space, or the closing quote can start no
> interpolation, so it has one reading. Kotlin, whose templates hd follows,
> keeps it as text. A prefixed and a plain string follow the same rule.

#### Raw Strings

> **Note.** hd has no built-in raw string literal. `r"..."` is an
> ordinary [prefixed string](#prefixed-strings), and its prefix `r`
> resolves as any prefix name does.

See also: [Raw Text Prefix](../std/text.md#raw-text-prefix) for the standard
library's `r`.

#### Prefixed Strings

A prefixed string is a name written directly before a string's opening
quote. Its text is raw, and it may interpolate:

```text
pattern := r"\d+\s+\w+"
prompt := r"""first line
second line"""
query := sql"select * from users where id = $id"
anchored := r"^\d+$"
quoted := r"say \"hi\""
```

1. r[lex.prefix.form] A **prefixed string** is an identifier followed directly by `"` or `"""`, as in `sql"..."` and `r"""..."""`. A space, as in `sql "..."`, or a single quote, as in `x'a'`, leaves the name a separate token.
2. r[lex.prefix.name] The prefix is an identifier, and a contextual word may be a prefix.
3. r[lex.prefix.raw-text] The text is raw: backslashes and escape-looking text stay as written, and no escape sequence is processed.
4. r[lex.prefix.backslash] A backslash keeps the following quote from ending the literal, and keeps a following `$` from beginning an interpolation. The backslash stays in the text, so a prefixed string cannot end in an odd number of backslashes.
5. r[lex.prefix.lines] A prefixed string follows the line rules of an unprefixed one. A single-line form holds no line ending, and a multiline form keeps its line endings and indentation as written.
6. r[lex.prefix.interpolation] A prefixed string interpolates with the forms of [Interpolation](#interpolation): `$name`, `$self`, and `${expression}`.
7. r[lex.prefix.bare-only] A prefix is one bare identifier and is never module-qualified, as a literal suffix is not.
8. r[lex.suffix.bare] A literal suffix is always a bare identifier and is never module-qualified.

The reserved-word and meaning rules of [Literal Suffixes](#literal-suffixes)
cover a prefix too.

A `$` that begins no interpolation follows the rules of
[Interpolation](#interpolation), as in every string. So `r"^\d+$"` ends
in a dollar sign, and a reserved word after `$` is an error:

```text
flag := r"$true"  # error: syntax-error
```

> **Why.** A prefix is an ordinary library function, so `sql"..."` needs
> no new syntax and `r` needs no built-in form. The text stays raw, as
> Scala's interpolators keep it, so each prefix decides what a backslash
> means.

See also: [Prefixed Strings](05-expressions.md#prefixed-strings),
[String Prefix Names](03-names-and-scopes.md#string-prefix-names),
[`grammar.primary.prefix-after-dot`](02-grammar.md#r-grammar.primary.prefix-after-dot)
for a path before a prefix.

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

1. r[lex.escape.unicode] A Unicode escape has one to six hexadecimal digits and must denote a Unicode scalar value in `0..10FFFF`, excluding surrogate code points `D800..DFFF`. Error: `invalid-escape`.
2. r[lex.escape.other] Any other escape is a lexical error. Error: `invalid-escape`.
3. r[lex.bytes.none] hd-lang has no byte or bytes literal and no primitive byte or bytes type.

```text
maximum := "\u{10FFFF}"  # valid: the maximum scalar value
beyond := "\u{110000}"   # error: invalid-escape
high := "\u{D800}"       # error: invalid-escape
value := "bad\xescape"   # error: invalid-escape
```

## Operators And Delimiters

r[lex.punct.tokens] The lexer recognizes these punctuation tokens:

```text
( ) [ ] { } , . : ;
```

1. r[lex.punct.semicolon] The current grammar does not use `;` as a statement separator.
2. r[lex.punct.semicolon-reserved] It is reserved for possible future use, and a `;` in a program is an error. Error: `reserved-semicolon`.

```text
name := "Ada";  # error: reserved-semicolon
```

r[lex.op.token-list-assign] The lexer recognizes these operators and compound
punctuation tokens:

```text
+  -  *  /  %  **
&  |  ^  ~  <<  >>  &&  ||  |>
=  ==  !=  <  <=  >  >=
:=  ->  =>  ?  !  $  @  ...  ...=  ::  +=
-=  *=  /=  %=  &=  |=  ^=  <<=  >>=
..  ..=
```

1. r[lex.op.longest] When two tokens share a prefix, the lexer uses the longest valid token.
2. r[lex.op.longest.examples] For example, `**` is one token rather than two `*` tokens, and `...` is one token rather than three `.` tokens. Likewise `...=` is one token rather than `...` and `=`, and `&&` and `||` are single tokens.
3. r[lex.op.pipe] `|>` is one token, the [pipe operator](05-expressions.md#pipe-expressions).
4. r[lex.op.inequality] The sequence `!=` is always the inequality token, so `f!=g` lexes as `f`, `!=`, `g`.
5. r[lex.op.bang-call] A suspension call needs `!` immediately followed by `(`.
6. r[lex.op.compound-assign] `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`, `<<=`, and `>>=` are single tokens. A statement uses them for [compound assignment](05-expressions.md#compound-assignment), and a member line also uses `+=`, as [Member Lines](14-annotations.md#member-lines) defines.
7. r[lex.op.compound-assign.examples] By longest match, `a-=b` lexes as `a`, `-=`, `b`, and `x<<=1` as `x`, `<<=`, `1`.
8. r[lex.op.no-power-assign] `**=` is not a token: it lexes as `**` and `=`, which no grammar rule accepts.
9. r[lex.op.range] `..` and `..=` are single tokens, the [range operators](05-expressions.md#range-expressions).
10. r[lex.op.range.longest] By longest match, `...` and `...=` win over `..` and `..=`, so `a...b` lexes as `a`, `...`, `b` and is never a range.
11. r[lex.op.range.integer] A number directly before `..` ends at the first `.`, since a decimal point needs a digit after it, by [`lex.float.point`](#r-lex.float.point).
12. r[lex.op.range.examples] So `0..3` lexes as `0`, `..`, `3`, and `1.5..2` as `1.5`, `..`, `2`.
13. r[lex.op.range.not-dot] `..` is not the member-access `.`, so a line that starts with `..` is no [leading-dot continuation](#leading-dot-continuation).

See also: [Expressions](05-expressions.md), which defines operator
precedence and semantics, including prefix `!` as logical not.
[Requirements and Suspension](11-requirements-and-suspension.md)
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
        | suffixed_literal
        | float_literal
        | integer_literal
        | string_literal
        | prefixed_string_literal
        | char_literal
        ;

delimiter = "(" | ")" | "[" | "]" | "{" | "}"
          | "," | "." | ":" | ";"
          ;

operator = "+" | "-" | "*" | "/" | "%" | "**"
         | "&" | "|" | "^" | "~" | "<<" | ">>" | "&&" | "||" | "|>"
         | "=" | "==" | "!=" | "<" | "<=" | ">" | ">="
         | ":=" | "->" | "=>" | "?" | "!" | "$" | "@"
         | "..." | "...=" | "::"
         | ".." | "..="
         | "+=" | "-=" | "*=" | "/=" | "%="
         | "&=" | "|=" | "^=" | "<<=" | ">>="
         ;

keyword = ? a reserved word listed in Keywords And Reserved Words ? ;
```

1. r[lex.grammar.keyword] `keyword` expands to the reserved words listed above.
2. r[lex.grammar.abstract-tokens] The abstract tokens `NEWLINE`, `INDENT`, `DEDENT`, `SUITE_END`, and end-of-file are produced by layout processing rather than matched directly from source characters.

## Unsupported Lexical Extensions

1. r[lex.unsupported.hex-float] Hexadecimal floating-point notation is not part of the language.
2. r[lex.unsupported.diagnose] Implementations must diagnose it rather than assign implementation-defined behavior.

```text
value := 0x1.8p3   # error: syntax-error
```

> **Note.** A future extension may add hexadecimal floating-point notation
> with new grammar.
