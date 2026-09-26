# Lexical Structure

Status: language specification draft.

This chapter defines how source text is divided into tokens and how indentation
produces block structure. Syntactic use of those tokens is defined in
[Grammar](02-grammar.md).

## Processing Model

An implementation processes a source file in this order:

1. Decode source bytes as UTF-8 and remove one optional initial byte-order mark.
2. Divide source text into physical lines.
3. Recognize comments, whitespace, literals, identifiers, and operators.
4. Join physical lines that continue inside `()`, `[]`, or `{}` into logical
   lines, except for an indentation suite nested in that continuation.
5. Emit `NEWLINE`, `INDENT`, `DEDENT`, and same-line `SUITE_END` layout tokens
   from logical lines and nested suites.
6. Parse the resulting token stream.

Comments and whitespace separate tokens but otherwise do not appear in the
parser token stream. Layout tokens are the exception.

Source files must be valid UTF-8. If the first three bytes are `EF BB BF`, they
are removed before lexical analysis. No other byte-order mark is removed; a
`U+FEFF` outside a comment or literal at any other position is a compile-time
lexical error. Invalid UTF-8 is also a compile-time lexical error. Unicode
scalar values are valid in identifiers under the identifier rules below and in
comments, string literals, and character literals.

## Physical And Logical Lines

A physical line ends at a line-feed character or at the end of the file. A
carriage-return followed by a line-feed is treated as one line ending. A bare
carriage return is a lexical error.

A logical line consists of one or more physical lines. A physical line is
continued implicitly while the lexer is inside an unmatched `(`, `[`, or `{`:

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

Comments and line endings inside an implicit continuation normally do not emit
`NEWLINE`, `INDENT`, or `DEDENT`. The exception is a suite introduced by a
grammar position that expects `:` followed by `suite_body`. When that suite
starts on the next physical line, layout processing emits its `NEWLINE`,
`INDENT`, body layout, and closing `DEDENT` even if surrounding delimiters are
still open. After the suite closes, implicit continuation resumes.

The indentation reference for such a nested suite is the indentation of the
physical line containing its suite header. Its first body line must be indented
farther than that reference. A closing delimiter at the nested suite's
delimiter depth ends the last body line: layout processing emits `NEWLINE` and
all pending `DEDENT` tokens before emitting the closing delimiter.

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

Layout recognition and parsing therefore cooperate at a suite-introducing
colon; a lexer may implement this with parser feedback or with equivalent
parser-state tracking. Ordinary colons in maps, data fields, named types, and
arguments do not open a suite. Trailing-block call colons occur only at
delimiter depth zero when the call is the complete statement or the outermost
right-hand side of a binding. They are not recognized in `if`, `while`, `for`,
or `match` headers or inside brackets.

A same-line suite ends with the abstract token `SUITE_END`. At a logical line
boundary, layout closes every same-line suite opened on that logical line,
emitting one `SUITE_END` per suite from innermost to outermost. At delimiter
depth zero the outermost `SUITE_END` replaces that line's `NEWLINE`; it does
not precede a second terminator. In an implicit continuation, the equivalent
boundary is a comma or closing delimiter that returns control to the enclosing
expression, and the same innermost-first sequence is emitted before that token.
A comma at the delimiter depth where a same-line suite opened always closes
that suite, including at depth zero, so the suite body cannot contain such a
comma.
For example, the body of `fn(name): name.lower()` ends immediately before that
closure's closing `)`.

`else` is also a boundary for the immediately preceding same-line `if` suite:
layout emits that suite's `SUITE_END` before `else` and keeps the enclosing
conditional open. Thus `x := if c: 1 else: 2` is one conditional expression;
the line boundary after `2` closes the `else` suite and then any enclosing
same-line suite, innermost first.
`SUITE_END` has no source spelling; parser-aware layout processing identifies
the boundary from the expected suite and enclosing delimiter structure.

A closing delimiter must match the most recent unclosed delimiter. An unmatched
or mismatched delimiter is a compile-time error.

hd-lang has no explicit backslash line-continuation syntax.

## Whitespace And Indentation

Spaces between tokens have no meaning except when they occur at the beginning
of a logical line. Leading indentation determines block structure.

For each non-empty logical line outside implicit continuation, the lexer
compares its indentation with a stack of active indentation levels:

1. Equal indentation emits no layout token.
2. Greater indentation pushes the new level and emits one `INDENT`.
3. Lesser indentation emits one or more `DEDENT` tokens until an existing
   level is reached.
4. Dedenting to a column that is not an active indentation level is a
   compile-time error.

The first indentation level is zero. At end of file, the lexer emits any
remaining `DEDENT` tokens. If the final non-empty logical line has no physical
line ending, the lexer emits its terminating `NEWLINE` before those `DEDENT`
tokens. Blank lines and comment-only lines do not affect the indentation stack
and do not emit `NEWLINE` tokens.

A block header ends in `:`. Its body may be either an indented suite beginning
on the next logical line or a same-line suite:

```text
fn greet(name: string) -> void $ Console:
    println("hello, " + name)

fn test() -> void $ Console: println("hi")
```

Horizontal tab characters are not permitted as source whitespace. They
may occur only as literal content represented by the `\t` escape or as raw
characters inside comments. Indentation therefore consists only of ASCII space
characters, and visual tab-width configuration cannot change block structure.

## Comments

`#` begins a line comment outside a string or character literal. The comment
continues to the end of its physical line:

```text
# A comment on its own line.
name := "Ada"  # A comment after code.
```

There are no block comments.

`##` at the start of a comment is a documentation comment. One or more
consecutive documentation-comment lines attach to the next declaration or
member at the same indentation when no blank line or non-documentation token
intervenes. Members include data fields, embedded fields, enum variants and
payload fields, trait and implementation methods, and function parameters.
The lexer removes `##` and one following space when present, then joins lines
with `\n`. The resulting string is exposed as the target shape's `doc` field;
without an attached documentation comment, `doc` is `nil`. A trailing `##`
comment after source code is ordinary commentary and does not attach. An
otherwise unattached documentation-comment line is a
`doc-comment-without-target` lexical error.

The lexical form is:

```ebnf
line_comment = "#", { comment_character } ;
doc_comment = "##", [ " " ], { comment_character } ;
```

`comment_character` is any supported source character other than a line
ending.

## Identifiers

An identifier begins with a Unicode `XID_Start` character or `_` and continues
with Unicode `XID_Continue` characters or `_`. Identifiers are case-sensitive.
Their source spelling must be in Unicode Normalization Form C (NFC); a
non-NFC identifier is a lexical error rather than being silently rewritten.

```ebnf
identifier       = identifier_start, { identifier_continue } ;
identifier_start = XID_START | "_" ;
identifier_continue = XID_CONTINUE | "_" ;
DECIMAL_DIGIT    = "0" ... "9" ;
```

The single source spelling `_` is a distinct placeholder token, not an
`identifier`; an identifier that begins with `_` must contain at least one
additional `identifier_continue` character. `XID_START` and `XID_CONTINUE`
denote the corresponding Unicode derived core properties. An implementation
must use one declared Unicode data version consistently for lexing,
normalization, and diagnostics.

The compiler must diagnose identifiers that are visually confusable with
another identifier visible in the same scope and identifiers that suspiciously
mix scripts. These security diagnostics do not change name identity: two
different NFC identifier strings remain different names. Standard-library
APIs, language keywords, and compiler-generated source names use ASCII.

An identifier that exactly matches a reserved word is not an identifier token.
The complete reserved-word set is listed below. Built-in
type names such as `i32`, `string`, `list`, and `map` are ordinary names rather
than lexically distinct tokens.

## Keywords And Reserved Words

The grammar uses these reserved words:

```text
Self      and       annotate  as        break     continue
data      defer     else      enum      false     fn        for
if        impl      in        is        let       match
mut       nil       not       or        pass      pub
reified   return    self      shape     super     trait
true      type      use       where     while
```

`pkg`, `std`, and `dep` have special meaning only in a use root position.
`test` has special meaning only at the beginning of a module-level test block.
`annotation` and `annotation_ref` are contextual after `::` in annotation
materialization;
`context`, `with`, and `Context` are contextual after `$.`; `pack`, `map`, and
`map_list` are contextual in the `pack.map(...)` and `pack.map_list(...)`
forms; and `derive` is contextual immediately after `@`. These contextual words
remain ordinary identifiers elsewhere, so declarations such as `fn test() ->
void`, `fn map_list() -> void`, and `fn derive() -> void` are lexically valid;
the separate prelude shadowing rule still applies. The reserved
word `use` is also accepted in the dedicated `$.use(...)` provider expression.

## Literals

### Boolean And Nil Literals

`true` and `false` are boolean literals. `nil` is the empty optional literal.

```ebnf
boolean_literal = "true" | "false" ;
nil_literal     = "nil" ;
```

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

A leading `-` is an operator, not part of the literal. Integer literal typing
and range checks are defined in
[Type System](04-type-system.md).

The radix prefix does not affect the inferred type. Hexadecimal digits may use
uppercase or lowercase letters. A leading zero without an explicit radix
prefix remains decimal; it never selects octal implicitly.

An underscore may separate adjacent digits. One underscore may also appear
immediately after an explicit radix prefix, as in `0x_FF`. An underscore cannot
begin or end a literal, occur twice consecutively, or touch a decimal point,
exponent marker, or exponent sign. Separators do not affect the literal's value
or inferred type.

### Floating-Point Literals

Floating-point literals use a decimal fraction, an exponent, or both. A
decimal point requires digits on both sides:

```ebnf
float_literal = decimal_fraction, [ decimal_exponent ]
              | decimal_digits, decimal_exponent
              ;
decimal_fraction = decimal_digits, ".", decimal_digits ;
decimal_exponent = ( "e" | "E" ), [ "+" | "-" ], decimal_digits ;
decimal_digits = DECIMAL_DIGIT, { [ "_" ], DECIMAL_DIGIT } ;
```

Thus `1e9`, `1.5e-6`, and `2E+8` are floating-point literals. Separators may
occur between digits in the integer, fractional, and exponent parts. `.5` and
`1.` are invalid; write `0.5` and `1.0`. Hexadecimal floating-point notation is
not supported.

### String And Character Literals

Double quotes delimit a `string` literal. Single quotes delimit a `char`
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
```

`string_character` is any Unicode scalar value other than `"`, `\\`, `$`, or
a line ending. `multiline_string_character` has the same exclusions except that
line endings are allowed. `raw_string_character` is any Unicode scalar value
other than an unescaped `"` or a line ending; `raw_multiline_character` is any
Unicode scalar value other than the start of an unescaped `"""` delimiter.
`char_character` is any Unicode scalar value other than `'`, `\\`, or a line
ending. `string_text` and `multiline_string_text` are maximal nonempty runs of
their corresponding character class between interpolation or escape segments.

A character literal must decode to exactly one Unicode scalar value. A string
literal is a sequence of Unicode scalar values. A single-line literal may not
contain an unescaped line ending or an unescaped copy of its own delimiter.

An interpreted multiline string uses `"""..."""`. It accepts the same escape
sequences as a single-line interpreted string and may contain physical line
endings. Source indentation and line endings inside the delimiters are part of
the value; the compiler does not dedent or trim them. Each source line ending
contributes one line-feed scalar to the value. The literal continues until an
unescaped `"""` delimiter.

Interpreted single-line and multiline strings use Kotlin-style interpolation.
`$name` interpolates one identifier, and `${expression}` interpolates an
arbitrary expression with balanced nested delimiters. An unescaped `$` must
begin one of those forms; `\$` produces a literal dollar sign. Braces without a
leading `$` are ordinary string content. The lexer switches back to normal
expression tokenization inside `${...}` and resumes string scanning at the
matching `}`.

Raw strings use Python-style `r"..."` and raw multiline strings use
`r"""..."""`. Backslashes and escape-looking text are preserved literally.
A backslash may prevent the following quote from terminating the raw literal,
but that backslash remains part of the resulting string. Consequently, a raw
string cannot end with an odd number of backslashes immediately before its
closing delimiter. A single-line raw string cannot contain a physical line
ending. A raw multiline string may contain line endings and continues until an
unescaped `"""` delimiter. Hash-delimited raw strings are not part of the
language. Raw strings do not interpolate, so `$` and `${...}` remain literal
content in both raw forms.

The simple escapes mean backslash, double quote, single quote, line feed,
carriage return, horizontal tab, and null respectively. A Unicode escape has
one to six hexadecimal digits and must denote a Unicode scalar value in
`0..10FFFF`, excluding surrogate code points `D800..DFFF`. Any other escape is
a lexical error.

hd-lang has no byte or bytes literal and no primitive byte or bytes type.

## Operators And Delimiters

The lexer recognizes these punctuation tokens:

```text
( ) [ ] { } , . : ;
```

The current grammar does not use `;` as a statement separator. It is reserved
for possible future use and must be diagnosed if it appears in a program.

The lexer recognizes these operators and compound punctuation tokens:

```text
+  -  *  /  %  **
&  |  ^  ~  <<  >>
=  ==  !=  <  <=  >  >=
:=  ->  =>  ?  !  $  @  ...  ::
```

When two tokens share a prefix, the lexer uses the longest valid token. For
example, `**` is one token rather than two `*` tokens, and `...` is one token
rather than three `.` tokens.

Operator precedence and semantics are defined in
[Expressions](05-expressions.md). `$` and suspension-related uses of `!` are
specified in [Requirements and Suspension](11-requirements-and-suspension.md).

## Lexical Token Grammar

The following EBNF summarizes the currently specified token classes. Layout
processing occurs after token recognition as described above.

```ebnf
token = identifier
      | keyword
      | literal_token
      | operator
      | delimiter
      ;

literal_token = boolean_literal
        | nil_literal
        | float_literal
        | integer_literal
        | string_literal
        | char_literal
        ;

delimiter = "(" | ")" | "[" | "]" | "{" | "}"
          | "," | "." | ":" | ";"
          ;

operator = "+" | "-" | "*" | "/" | "%" | "**"
         | "&" | "|" | "^" | "~" | "<<" | ">>"
         | "=" | "==" | "!=" | "<" | "<=" | ">" | ">="
         | ":=" | "->" | "=>" | "?" | "!" | "$" | "@"
         | "..." | "::"
         ;
```

`keyword` expands to the reserved words listed above. The abstract tokens
`NEWLINE`, `INDENT`, `DEDENT`, `SUITE_END`, and end-of-file are produced by
layout processing rather than matched directly from source characters.

## Unsupported Lexical Extensions

Hexadecimal floating-point notation is not part of the language.
A future extension may add them with new grammar; implementations must diagnose them
rather than assign implementation-defined behavior.
