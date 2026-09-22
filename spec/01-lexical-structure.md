# Lexical Structure

Status: core specification draft.

This chapter defines how source text is divided into tokens and how indentation
produces block structure. Syntactic use of those tokens is defined in
[Grammar](02-grammar.md).

## Processing Model

An implementation processes a source file in this order:

1. Divide source text into physical lines.
2. Recognize comments, whitespace, literals, identifiers, and operators.
3. Join physical lines that continue inside `()`, `[]`, or `{}` into logical
   lines, except for an indentation suite nested in that continuation.
4. Emit `NEWLINE`, `INDENT`, `DEDENT`, and same-line `SUITE_END` layout tokens
   from logical lines and nested suites.
5. Parse the resulting token stream.

Comments and whitespace separate tokens but otherwise do not appear in the
parser token stream. Layout tokens are the exception.

Source files must be valid UTF-8. A byte-order mark is not permitted. Invalid
UTF-8 is a compile-time lexical error. Identifiers are restricted to ASCII in
v1; Unicode scalar values remain valid in comments, string literals, and
character literals.

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
context tracking. Ordinary colons in maps, struct fields, named types, and
arguments do not open a suite.

A same-line suite ends with the abstract token `SUITE_END`. At delimiter depth
zero, `SUITE_END` replaces the logical `NEWLINE` that terminates the suite. In
an implicit continuation, it is emitted before the comma or closing delimiter
that returns control to the enclosing expression. For example, the body of
`fn(name): name.lower()` ends immediately before that closure's closing `)`.
`SUITE_END` has no source spelling; parser-aware layout processing identifies
the boundary from the expected suite and enclosing delimiter structure.

A closing delimiter must match the most recent unclosed delimiter. An unmatched
or mismatched delimiter is a compile-time error.

hd-lang has no explicit backslash line-continuation syntax in the current core.

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
fn greet(name: string) -> void:
    println("hello, " + name)

fn test() -> void: println("hi")
```

Horizontal tab characters are not permitted as source whitespace in v1. They
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

There are no block comments in the current core.

The lexical form is:

```ebnf
line_comment = "#", { comment_character } ;
```

`comment_character` is any supported source character other than a line
ending.

## Identifiers

An identifier begins with an ASCII letter or `_` and continues with ASCII
letters, decimal digits, or `_`. Identifiers are case-sensitive. Any non-ASCII
character in an identifier position is a lexical error.

```ebnf
identifier       = identifier_start, { identifier_continue } ;
identifier_start = ASCII_LETTER | "_" ;
identifier_continue = ASCII_LETTER | DECIMAL_DIGIT | "_" ;
ASCII_LETTER     = "A" ... "Z" | "a" ... "z" ;
DECIMAL_DIGIT    = "0" ... "9" ;
```

An identifier that exactly matches a reserved word is not an identifier token.
The complete reserved-word set is defined by the consolidated grammar. Built-in
type names such as `i32`, `string`, `list`, and `map` are ordinary names rather
than lexically distinct tokens.

## Keywords And Reserved Words

The current core grammar uses these reserved words:

```text
and       as        break     continue   else      enum
export    false     fn        for        if        impl
import    in        let       match      mut       nil
not       or        pass      pub        reified   return
self      struct    super     trait      true      type
while
```

`pkg`, `std`, and `dep` have special meaning only in an import root position.
`test` has special meaning only at the beginning of a module-level test block.
These contextual words remain ordinary identifiers elsewhere, so declarations
such as `fn test() -> void` are valid. Keywords used only by provisional
chapters are reserved only if the consolidated grammar explicitly lists them.

## Literals

### Boolean And Nil Literals

`true` and `false` are boolean literals. `nil` is the empty optional literal.

```ebnf
boolean_literal = "true" | "false" ;
nil_literal     = "nil" ;
```

### Integer Literals

The current core requires decimal integer literals:

```ebnf
integer_literal = DECIMAL_DIGIT, { DECIMAL_DIGIT } ;
```

A leading `-` is an operator, not part of the literal. Unary `+` is not
supported in v1. Integer literal typing and range checks are defined in
[Type System](04-type-system.md).

Radix prefixes and digit separators are not supported in v1.

### Floating-Point Literals

The required floating-point form contains digits on both sides of a decimal
point:

```ebnf
float_literal = DECIMAL_DIGIT, { DECIMAL_DIGIT }, ".",
                DECIMAL_DIGIT, { DECIMAL_DIGIT } ;
```

Exponent notation, hexadecimal floating-point notation, and digit separators
are not supported in v1.

### String And Character Literals

Double quotes delimit a `string` literal. Single quotes delimit a `char`
literal:

```text
name := "Ada"
initial := 'A'
greeting := "你好"
```

```ebnf
string_literal = '"', { string_character | escape_sequence }, '"' ;
char_literal   = "'", (char_character | escape_sequence), "'" ;

escape_sequence = "\\", ( "\\" | '"' | "'" | "n" | "r" | "t" | "0"
                       | unicode_escape ) ;
unicode_escape = "u", "{", HEX_DIGIT, { HEX_DIGIT }, "}" ;
HEX_DIGIT = DECIMAL_DIGIT | "A" ... "F" | "a" ... "f" ;
```

A character literal must decode to exactly one Unicode scalar value. A string
literal is a sequence of Unicode scalar values. A literal may not contain an
unescaped line ending or an unescaped copy of its own delimiter.

The simple escapes mean backslash, double quote, single quote, line feed,
carriage return, horizontal tab, and null respectively. A Unicode escape has
one to six hexadecimal digits and must denote a Unicode scalar value in
`0..10FFFF`, excluding surrogate code points `D800..DFFF`. Any other escape is
a lexical error.

Raw strings, multiline strings, and string interpolation are not supported in
v1.

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
:=  ->  =>  ?  !  $  ...  ::
```

When two tokens share a prefix, the lexer uses the longest valid token. For
example, `**` is one token rather than two `*` tokens, and `...` is one token
rather than three `.` tokens.

Operator precedence and semantics are defined in
[Expressions](05-expressions.md). `$` and suspension-related uses of `!` are
specified provisionally in
[Requirements and Suspension](provisional/requirements-and-suspension.md).

## Lexical Token Grammar

The following EBNF summarizes the currently specified token classes. Layout
processing occurs after token recognition as described above.

```ebnf
token = identifier
      | keyword
      | literal
      | operator
      | delimiter
      ;

literal = boolean_literal
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
         | ":=" | "->" | "=>" | "?" | "!" | "$"
         | "..." | "::"
         ;
```

`keyword` expands to the reserved words listed above. The abstract tokens
`NEWLINE`, `INDENT`, `DEDENT`, `SUITE_END`, and end-of-file are produced by
layout processing rather than matched directly from source characters.

## Unsupported Lexical Extensions

Unicode identifiers, raw or multiline strings, interpolation, numeric radix
prefixes, exponent notation, and digit separators are not part of v1. A future
edition may add them with new grammar; v1 implementations must diagnose them
rather than assign implementation-defined behavior.
