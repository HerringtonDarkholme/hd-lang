# Regex

Status: standard library specification draft.

This chapter defines `std.regex`, which `lib/std` writes in ordinary hd
over the language tier:

- `Regex`, a compiled pattern, and `Regex::new`, which compiles one;
- the pattern syntax, a subset of RE2's;
- `is_match` and `find`, which search a text in linear time;
- `Match`, where a match is, and `RegexError`, why a pattern does not
  compile.

A pattern is ordinary text, so a backslash in it is written `\\` in a
plain string. The prefix `r` of `std.text` keeps backslashes as written
([Raw Text Prefix](text.md#raw-text-prefix)), and `\$` keeps a dollar sign
from starting an interpolation
([`lex.interp.escaped-dollar`](../lang/01-lexical-structure.md#r-lex.interp.escaped-dollar)).

## Compiling A Pattern

```text
use std.regex.{Regex, RegexError}
use std.text.r

fn digits() -> Result[Regex, RegexError]:
    Regex::new(r"\d+")
```

| Rule | Item | Meaning |
| --- | --- | --- |
| r[std-regex.regex.new] `new` | `pub fn new(pattern: string) -> Result[Regex, RegexError]` | the compiled `pattern`, or the first error in it |
| r[std-regex.regex.as-str] `as_str` | `pub fn as_str(self) -> string` | the pattern as written |

1. r[std-regex.regex.decl] `std.regex` declares the data type `Regex`, with private fields, and the methods in this chapter. Code imports it, as in `use std.regex.Regex`.
2. r[std-regex.regex.plain] Every function and method in this chapter is a plain call with the empty requirement row.
3. r[std-regex.regex.immutable] A `Regex` never changes after `new`, and searching with it leaves it unchanged.
4. r[std-regex.regex.debug] `Regex` implements `Debug`, which shows the pattern.

## Pattern Syntax

A pattern is read as a sequence of characters, each a Unicode scalar
value. The forms below are the whole syntax.

### Literals And Escapes

| Rule | Form | Matches |
| --- | --- | --- |
| r[std-regex.syntax.literal] Literal | a character with no meaning below | that character |
| r[std-regex.syntax.dot] Any character | `.` | any one character except `\n` |
| r[std-regex.syntax.escape.meta] Escaped punctuation | `\` and an ASCII punctuation character, as in `\.` or `\\` | that character |
| r[std-regex.syntax.escape.control] Control escape | `\n`, `\t`, `\r` | a line feed, a tab, a carriage return |
| r[std-regex.syntax.escape.digit] Digit | `\d`; `\D` | an ASCII digit `0` to `9`; any other character |
| r[std-regex.syntax.escape.word] Word | `\w`; `\W` | an ASCII letter, digit, or `_`; any other character |
| r[std-regex.syntax.escape.space] Space | `\s`; `\S` | a character with the Unicode White_Space property; any other character |

1. r[std-regex.syntax.punctuation] The ASCII punctuation characters are those from `!` to `/`, from `:` to `@`, from `[` to `` ` ``, and from `{` to `~`.
2. r[std-regex.syntax.escape.other] Any other escape, such as `\b`, `\q`, or `\1`, or a `\` that ends the pattern, is an error. Error: `BadEscape`.
3. r[std-regex.syntax.brace-literal] A `{` that does not begin a counted repetition, as in `a{` or `a{x}`, is a literal, and so are `}` and `]` outside a class.

### Classes

| Rule | Form | Matches |
| --- | --- | --- |
| r[std-regex.syntax.class] Class | `[` items `]` | a character that some item matches |
| r[std-regex.syntax.class.negated] Negated class | `[^` items `]` | a character that no item matches, `\n` included |

1. r[std-regex.syntax.class.item] An item is a character, a range `a-z` of two characters, or one of `\d`, `\D`, `\w`, `\W`, `\s`, and `\S`.
2. r[std-regex.syntax.class.char] An item's character is any character but `\`, or an escape of [Literals And Escapes](#literals-and-escapes) that stands for one character.
3. r[std-regex.syntax.class.range] A range `a-z` matches every character from `a` to `z`, compared by scalar value.
4. r[std-regex.syntax.class.close-first] A `]` right after `[` or `[^` is an item, not the end, so `[]a]` matches `]` or `a`.
5. r[std-regex.syntax.class.dash] A `-` first, last, or right after a range is an item, as in `[-a]` or `[a-]`.
6. r[std-regex.syntax.class.unclosed] A class with no closing `]` is an error, at its `[`. Error: `MissingBracket`.
7. r[std-regex.syntax.class.bad-range] A range whose end is below its start, or whose start or end is `\d`, `\w`, `\s`, or their negations, is an error, at the range's first character. Error: `BadRange`.

### Anchors, Groups, And Alternation

| Rule | Form | Matches |
| --- | --- | --- |
| r[std-regex.syntax.begin] Begin | `^` | the empty text at the start of the searched text |
| r[std-regex.syntax.end] End | `$` | the empty text at the end of the searched text |
| r[std-regex.syntax.group] Group | `(` pattern `)` | what the inner pattern matches |
| r[std-regex.syntax.group.plain] Plain group | `(?:` pattern `)` | what the inner pattern matches |
| r[std-regex.syntax.alternation] Alternation | pattern `\|` pattern | what either side matches |
| r[std-regex.syntax.empty] Empty | nothing, as in `()`, `a\|`, or the pattern `""` | the empty text |

1. r[std-regex.syntax.anchor.text] `^` and `$` see only the ends of the whole text. A `\n` inside it is no start or end.
2. r[std-regex.syntax.group.capture] A `( )` group is a capture group, and a `(?: )` group is not. This version reports no capture's text, so the two match alike.
3. r[std-regex.syntax.group.other] A `(?` followed by anything but `:` is an error, at the `(`. So flags, as in `(?i)`, and named groups, as in `(?P<name>a)`, are not supported. Error: `UnsupportedGroup`.
4. r[std-regex.syntax.group.unclosed] A `(` with no matching `)` is an error, at the `(`. Error: `MissingParen`.
5. r[std-regex.syntax.group.unopened] A `)` with no matching `(` is an error, at the `)`. Error: `UnmatchedParen`.
6. r[std-regex.syntax.precedence] Repetition binds tighter than concatenation, and concatenation binds tighter than `|`. So `ab|cd*` is `(?:ab)|(?:c(?:d*))`.

### Repetition

A quantifier follows the item it repeats: a character, `.`, an escape, a
class, an anchor, or a group.

| Rule | Quantifier | Repeats the item | Lazy form |
| --- | --- | --- | --- |
| r[std-regex.repeat.star] Star | `*` | zero or more times | `*?` |
| r[std-regex.repeat.plus] Plus | `+` | one or more times | `+?` |
| r[std-regex.repeat.optional] Optional | `?` | zero times or once | `??` |
| r[std-regex.repeat.exact] Exact | `{m}` | `m` times | `{m}?` |
| r[std-regex.repeat.at-least] At least | `{m,}` | `m` or more times | `{m,}?` |
| r[std-regex.repeat.between] Between | `{m,n}` | from `m` to `n` times | `{m,n}?` |

1. r[std-regex.repeat.counts] `m` and `n` are runs of ASCII digits.
2. r[std-regex.repeat.greedy] A quantifier prefers more repetitions, and its lazy form prefers fewer. The preference picks among matches that start at one position; see [Searching](#searching).
3. r[std-regex.repeat.bad-count] A count above 1000, or an `n` below `m`, is an error, at the `{`. Error: `BadRepeat`.
4. r[std-regex.repeat.nothing] A quantifier with no item before it is an error, at the quantifier. That covers one at the start of the pattern or a group, one after `|`, and one right after another quantifier, as in `a**` or `a{2}{3}`. Error: `NothingToRepeat`.

```text
use std.regex.{Regex, Match}
use std.text.r

fn year(line: string) -> string?:
    match Regex::new(r"(?:19|20)\d{2}"):
        .Ok(pattern) => pattern.find(line).map(fn(found: Match) -> string: found.text)
        .Err(_) => .None
```

> **Why.** This is the syntax that RE2 and Go's `regexp` share, without
> flags, captures by name, `\b`, and Unicode classes. Each left-out form is
> an error today, not a literal, so adding it later changes no pattern's
> meaning.

## Searching

| Rule | Method | Result |
| --- | --- | --- |
| r[std-regex.is-match] `is_match` | `pub fn is_match(self, text: string) -> bool` | whether some part of `text` matches |
| r[std-regex.find] `find` | `pub fn find(self, text: string) -> Match?` | the first match in `text`, or `.None` |

1. r[std-regex.find.chars] A search steps through `text` one character, a Unicode scalar value, at a time. A match starts and ends at character boundaries.
2. r[std-regex.find.leftmost] `find` returns a match that starts at the smallest offset where any match starts.
3. r[std-regex.find.leftmost-first] Among the matches that start there, it returns the one the pattern prefers: the left side of `|` before the right, and each quantifier's preferred count, earlier choices first.
4. r[std-regex.find.empty] A match may be empty. So `x*` finds the empty match at offset 0 of `"abc"`.
5. r[std-regex.is-match.find] `is_match(text)` is `find(text).is_some()`.

```text
use std.regex.{Regex, Match}

fn first() -> string?:
    match Regex::new("a|ab"):
        .Ok(pattern) => pattern.find("ab").map(fn(found: Match) -> string: found.text)  # "a"
        .Err(_) => .None
```

> **Why.** Leftmost-first is what RE2, Go, Rust, Perl, and JavaScript
> return, so a pattern copied from one of them finds the same text. POSIX
> leftmost-longest would return `"ab"` here.

### Matches

```text
pub data Match:
    pub start: usize
    pub end: usize
    pub text: string
```

1. r[std-regex.match.decl] `std.regex` declares the data type `Match` with the public fields above.
2. r[std-regex.match.offsets] `start` and `end` are byte offsets into the searched text, with `start <= end`, as `string.slice` takes them.
3. r[std-regex.match.text] `text` is `text.slice(start, end)` of the searched text.
4. r[std-regex.match.traits] `Match` implements `Eq` and `Debug`.

### Time Bound

1. r[std-regex.time.linear] A search takes time in O(m × n), where n is the length of the text and m is the size of the pattern with each counted repetition written out. No pattern makes it slower: there is no backtracking.
2. r[std-regex.time.memory] A search uses memory in O(m + n).
3. r[std-regex.time.no-backrefs] So the syntax has no backreference and no lookaround, which no linear-time matcher supports.
4. r[std-regex.time.size] The **written-out size** of a pattern is given by the table below, where s is the size of the repeated item.
5. r[std-regex.error.too-large] A pattern whose written-out size is above 10,000 is an error, at offset 0. Error: `TooLarge`.

| Form | Written-out size |
| --- | --- |
| a character, `.`, an escape, a class, or an anchor | 1 |
| the empty pattern | 0 |
| a group | the size of its inner pattern |
| a concatenation | the sum of its parts |
| an alternation of k branches | the sum of the branches, plus 2 × (k − 1) |
| `e*`, `e{0,}`, and their lazy forms | s + 2 |
| `e+`, and `e{m,}` with m at least 1, and their lazy forms | m × s + 1, with m = 1 for `e+` |
| `e?`, `e{m}`, `e{m,n}`, and their lazy forms | m × s + (n − m) × (s + 1), with m = 0 and n = 1 for `e?`, and n = m for `e{m}` |

```text
use std.regex.Regex

fn quick(text: string) -> bool:
    match Regex::new("(a*)*b"):
        .Ok(pattern) => pattern.is_match(text)  # linear, even on a long run of "a"
        .Err(_) => false
```

> **Note.** `lib/std` compiles a pattern to a Pike VM: it runs every
> thread in step over the text and keeps at most one thread for each
> instruction, in priority order. The written-out size is the number of
> instructions that VM runs, without the final one.

> **Why.** A script runs patterns on input it does not control. RE2's
> guarantee keeps a pattern like `(a*)*b`, which takes exponential time in
> a backtracking engine, linear.

## Pattern Errors

```text
pub enum RegexErrorKind:
    MissingParen
    UnmatchedParen
    MissingBracket
    BadEscape
    BadRange
    NothingToRepeat
    BadRepeat
    UnsupportedGroup
    TooLarge

pub data RegexError:
    pub kind: RegexErrorKind
    pub position: usize
```

| Rule | Kind | When | `Display` text |
| --- | --- | --- | --- |
| r[std-regex.error.kind.missing-paren] Missing paren | `MissingParen` | [`std-regex.syntax.group.unclosed`](#r-std-regex.syntax.group.unclosed) | `missing closing )` |
| r[std-regex.error.kind.unmatched-paren] Unmatched paren | `UnmatchedParen` | [`std-regex.syntax.group.unopened`](#r-std-regex.syntax.group.unopened) | `unexpected )` |
| r[std-regex.error.kind.missing-bracket] Missing bracket | `MissingBracket` | [`std-regex.syntax.class.unclosed`](#r-std-regex.syntax.class.unclosed) | `missing closing ]` |
| r[std-regex.error.kind.bad-escape] Bad escape | `BadEscape` | [`std-regex.syntax.escape.other`](#r-std-regex.syntax.escape.other) | `invalid escape` |
| r[std-regex.error.kind.bad-range] Bad range | `BadRange` | [`std-regex.syntax.class.bad-range`](#r-std-regex.syntax.class.bad-range) | `invalid character class range` |
| r[std-regex.error.kind.nothing-to-repeat] Nothing to repeat | `NothingToRepeat` | [`std-regex.repeat.nothing`](#r-std-regex.repeat.nothing) | `missing argument to repetition operator` |
| r[std-regex.error.kind.bad-repeat] Bad repeat | `BadRepeat` | [`std-regex.repeat.bad-count`](#r-std-regex.repeat.bad-count) | `invalid repeat count` |
| r[std-regex.error.kind.unsupported-group] Unsupported group | `UnsupportedGroup` | [`std-regex.syntax.group.other`](#r-std-regex.syntax.group.other) | `unsupported group` |
| r[std-regex.error.kind.too-large] Too large | `TooLarge` | [`std-regex.error.too-large`](#r-std-regex.error.too-large) | `expression too large` |

1. r[std-regex.error.decl] `std.regex` declares the enum `RegexErrorKind` with the nine kinds above, and the data type `RegexError` with the public fields above.
2. r[std-regex.error.position] `position` is the byte offset in the pattern where the rule for the kind puts the error.
3. r[std-regex.error.first] `new` reads the pattern from left to right and returns the first error it meets. A group or class is checked for its closing character after its contents.
4. r[std-regex.error.size-last] `TooLarge` is checked only once the whole pattern has no other error.
5. r[std-regex.error.traits] `RegexErrorKind` and `RegexError` implement `Eq`, `Debug`, and `Display`.
6. r[std-regex.error.display] A kind's `Display` text is the text in the table. A `RegexError`'s is the kind's text, then ` at byte `, then `position`, as in `invalid escape at byte 3`.

```text
use std.regex.{Regex, RegexError}

fn broken() -> RegexError?:
    Regex::new("a(b").err()  # .Some(RegexError { kind: .MissingParen, position: 1 })
```

> **Why.** The kinds and texts follow Go's `regexp/syntax` error codes,
> so an error reads the same as there. A position lets a tool point at the
> spot in a pattern the user wrote.

## Not Yet Specified

These are part 2 of `std.regex` and are not in this version:

- `captures`, the text of each capture group;
- `find_all`, every match that does not overlap the one before;
- `replace` and `replace_all`, with `$1` references to captures;
- flags, such as `(?i)`, `(?m)`, and `(?s)`.
