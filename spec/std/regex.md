# Regex

Status: standard library specification draft.

This chapter defines `std.regex`, which `lib/std` writes in ordinary hd
over the language tier:

- `Regex`, a compiled pattern, and `Regex::new`, which compiles one;
- the pattern syntax, a subset of RE2's;
- `is_match`, `find`, and `find_all`, which search a text in linear time;
- `captures` and `captures_all`, which report the text of each group;
- `replace`, `replace_all`, and `split`;
- `Match`, where a match is, `Captures`, the groups of a match, and
  `RegexError`, why a pattern does not compile.

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
| r[std-regex.regex.pattern] `pattern` | `pub fn pattern(self) -> string` | the pattern as written |

1. r[std-regex.regex.decl] `std.regex` declares the data type `Regex`, with private fields, and the methods in this chapter. Code imports it, as in `use std.regex.Regex`.
2. r[std-regex.regex.plain] Every function and method in this chapter is a plain call with the empty requirement row.
3. r[std-regex.regex.immutable] A `Regex` never changes after `new`, and searching with it leaves it unchanged.
4. r[std-regex.regex.debug] `Regex` implements `Debug`, which shows the pattern.

> **Note.** Write a pattern as a raw string, `r"..."`. Then `\d` needs no
> second backslash, and a `$` before `"`, `)`, or `|` stays text
> ([`lex.interp.dollar-text`](../lang/01-lexical-structure.md#r-lex.interp.dollar-text)).
> A `$` before a letter or `{` still starts an interpolation, in a raw
> string too.

```text
use std.regex.Regex
use std.text.r

fn whole_number(line: string) -> bool:
    match Regex::new(r"^\d+$"):
        .Ok(pattern) => pattern.is_match(line)
        .Err(_) => false
```

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
3. r[std-regex.syntax.brace-literal] A `{` that does not begin a counted repetition, as in `a{` or `a{x}`, is a literal. So are `}` and `]` outside a class.

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
7. r[std-regex.syntax.class.bad-range] A range whose end is below its start is an error, at the range's first character. So is one whose start or end is `\d`, `\w`, `\s`, or their negations. Error: `BadRange`.

### Anchors, Groups, And Alternation

| Rule | Form | Matches |
| --- | --- | --- |
| r[std-regex.syntax.begin] Begin | `^` | the empty text at the start of the searched text |
| r[std-regex.syntax.end] End | `$` | the empty text at the end of the searched text |
| r[std-regex.syntax.group] Group | `(` pattern `)` | what the inner pattern matches |
| r[std-regex.syntax.group.plain] Plain group | `(?:` pattern `)` | what the inner pattern matches |
| r[std-regex.syntax.group.named] Named group | `(?P<name>` pattern `)` or `(?<name>` pattern `)` | what the inner pattern matches |
| r[std-regex.syntax.alternation] Alternation | pattern `\|` pattern | what either side matches |
| r[std-regex.syntax.empty] Empty | nothing, as in `()`, `a\|`, or the pattern `""` | the empty text |

1. r[std-regex.syntax.anchor.text] `^` and `$` see only the ends of the whole text. A `\n` inside it is no start or end.
2. r[std-regex.syntax.group.number] A `( )` group and a named group are capture groups, numbered from 1 in the order of their `(`. A `(?: )` group has no number.
3. r[std-regex.syntax.group.name] A group's name is an ASCII letter or `_`, then any number of ASCII letters, digits, and `_`.
4. r[std-regex.syntax.group.bad-name] A named group whose name breaks that rule, or that has no `>` after `<`, is an error, at the `(`. Error: `BadGroupName`.
5. r[std-regex.syntax.group.duplicate-name] A name that an earlier group already has is an error, at the later group's `(`. Error: `DuplicateGroupName`.
6. r[std-regex.syntax.group.unsupported] A `(?` that begins none of `(?:`, `(?P<`, and `(?<` is an error, at the `(`. So are `(?<=` and `(?<!`. Error: `UnsupportedGroup`.
7. r[std-regex.syntax.group.unclosed] A `(` with no matching `)` is an error, at the `(`. Error: `MissingParen`.
8. r[std-regex.syntax.group.unopened] A `)` with no matching `(` is an error, at the `)`. Error: `UnmatchedParen`.
9. r[std-regex.syntax.precedence] Repetition binds tighter than concatenation, and concatenation binds tighter than `|`. So `ab|cd*` is `(?:ab)|(?:c(?:d*))`.

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
4. r[std-regex.repeat.nothing] A quantifier with no item before it is an error, at the quantifier. That covers one at the start of the pattern or a group, and one after `|`. It also covers one right after another quantifier, as in `a**` or `a{2}{3}`. Error: `NothingToRepeat`.

```text
use std.regex.{Regex, Match}
use std.text.r

fn year(line: string) -> string?:
    match Regex::new(r"(?:19|20)\d{2}"):
        .Ok(pattern) => pattern.find(line).map(fn(found: Match) -> string: found.text)
        .Err(_) => .None
```

> **Why.** This is the syntax that RE2 and Go's `regexp` share, without
> flags, `\b`, lookaround, and Unicode classes. Each left-out form is an
> error today, not a literal, so adding it later changes no pattern's
> meaning.

## Searching

| Rule | Method | Result |
| --- | --- | --- |
| r[std-regex.is-match] `is_match` | `pub fn is_match(self, text: string) -> bool` | whether some part of `text` matches |
| r[std-regex.find] `find` | `pub fn find(self, text: string) -> Match?` | the first match in `text`, or `.None` |

1. r[std-regex.find.chars] A search steps through `text` one character, a Unicode scalar value, at a time. A match starts and ends at character boundaries.
2. r[std-regex.find.leftmost] `find` returns a match that starts at the smallest offset where any match starts.
3. r[std-regex.find.leftmost-first] Among the matches that start there, it returns the one the pattern prefers. That is the left side of `|` before the right, and each quantifier's preferred count, earlier choices first.
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

### All Matches

| Rule | Method | Result |
| --- | --- | --- |
| r[std-regex.find-all] `find_all` | `pub fn find_all(self, text: string) -> List[Match]` | every match in `text`, left to right, none overlapping |

1. r[std-regex.find-all.next] `find_all` runs a series of searches. The first starts at offset 0, and each finds the match `find` would, among those that start at or after its offset.
2. r[std-regex.find-all.empty] After a match that is not empty, the next search starts at its end. After an empty match, it starts one character later, and none follows an empty match at the end of the text.
3. r[std-regex.find-all.drop] An empty match that starts where the previous match ended is dropped.
4. r[std-regex.find-all.anchors] `^` and `$` still see only the ends of the whole text, wherever a search starts.

```text
use std.regex.Regex

fn spots() -> List[usize]:
    let starts: mut List[usize] = []
    for found in Regex::new("a*").expect("compiles").find_all("baaac"):
        starts.push(found.start)
    starts  # [0, 1, 5]
```

> **Why.** Stepping one character past an empty match keeps the loop
> finite, and dropping an empty match right after another match follows Go
> and Rust. So `a*` finds `""`, `"aaa"`, and `""` in `"baaac"`, not a
> second `""` at offset 4.

### Time Bound

1. r[std-regex.time.linear] A search takes time in O(m × n). Here n is the length of the text, and m is the size of the pattern with each counted repetition written out. No pattern makes it slower: there is no backtracking.
2. r[std-regex.time.memory] A search uses memory in O(m + n).
3. r[std-regex.time.linear.groups] A search that also reports groups, as `captures` and the replacements do, takes time in O(m × g × n). Here g is the number of groups plus one.
4. r[std-regex.time.memory.groups] Such a search uses memory in O(m × g + n).
5. r[std-regex.time.all] `find_all`, `captures_all`, `replace_all`, and `split` run at most 2k + 1 searches for k matches.
6. r[std-regex.time.no-backrefs] So the syntax has no backreference and no lookaround, which no linear-time matcher supports.
7. r[std-regex.time.size] The **written-out size** of a pattern is given by the table below, where s is the size of the repeated item.
8. r[std-regex.error.too-large] A pattern whose written-out size is above 10,000 is an error, at offset 0. Error: `TooLarge`.

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

> **Note.** `lib/std` compiles a pattern to a Pike VM. The VM runs every
> thread in step over the text and keeps at most one thread for each
> instruction, in priority order. Each thread carries the positions of the
> groups it has passed. The written-out size is the number of instructions
> that VM runs, without the final one and the two that mark each group's
> ends.

> **Why.** A script runs patterns on input it does not control. RE2's
> guarantee keeps a pattern like `(a*)*b`, which takes exponential time in
> a backtracking engine, linear. A search may read on past its match to
> the end of the text. So `find_all` can take O(m × n²), as Rust documents
> for its iterators.

## Captures

| Rule | Method | Result |
| --- | --- | --- |
| r[std-regex.captures] `captures` | `pub fn captures(self, text: string) -> Captures?` | the groups of the match `find` returns, or `.None` |
| r[std-regex.captures-all] `captures_all` | `pub fn captures_all(self, text: string) -> List[Captures]` | the groups of each match `find_all` returns, in order |

`Captures` has these methods:

| Rule | Method | Result |
| --- | --- | --- |
| r[std-regex.captures.len] `len` | `pub fn len(self) -> usize` | the number of capture groups in the pattern, plus one |
| r[std-regex.captures.get] `get` | `pub fn get(self, index: usize) -> Match?` | group `index`, where 0 is the whole match |
| r[std-regex.captures.name] `name` | `pub fn name(self, name: string) -> Match?` | the group named `name` |

1. r[std-regex.captures.decl] `std.regex` declares the data type `Captures`, with private fields.
2. r[std-regex.captures.whole] `get(0)` is the whole match, the one `find` or `find_all` returns.
3. r[std-regex.captures.absent] `get(index)` is `.None` when group `index` took no part in the match, or when `index` is not below `len()`.
4. r[std-regex.captures.name.absent] `name(name)` is `get` of the group with that name, or `.None` when no group has it.
5. r[std-regex.captures.path] A group's positions are those along the path the match takes, chosen by the preferences of [`std-regex.find.leftmost-first`](#r-std-regex.find.leftmost-first).
6. r[std-regex.captures.last] A group that the path passes more than once reports its last pass. So `(a|b)+` on `"ab"` gives group 1 the text `"b"`.
7. r[std-regex.captures.empty-loop] An optional iteration of `*`, `+`, or `{m,}` that would match the empty text is not taken. So `(a*)*` on `"b"` leaves group 1 out, and `(a*)+` does not.

```text
use std.regex.Regex
use std.text.r

fn year_and_month(line: string) -> (string, string)?:
    date := Regex::new(r"(?<year>\d{4})-(\d{2})").expect("compiles")
    found := date.captures(line)?
    .Some((found.name("year")?.text, found.get(2)?.text))  # "2026-10" gives ("2026", "10")
```

> **Why.** Group numbers are what RE2, Go, and Rust report. So are
> `.None` for a group that took no part, and the
> last pass of a repeated group. Both
> spellings of a named group are accepted, as RE2, Go 1.22, and Rust
> accept them. `(?P<name>)` is Python's spelling, and `(?<name>)` is
> JavaScript's, Java's, and .NET's.

## Replacing And Splitting

| Rule | Method | Result |
| --- | --- | --- |
| r[std-regex.replace] `replace` | `pub fn replace(self, text: string, replacement: string) -> string` | `text` with the match `find` returns replaced |
| r[std-regex.replace-all] `replace_all` | `pub fn replace_all(self, text: string, replacement: string) -> string` | `text` with each match `find_all` returns replaced |
| r[std-regex.split] `split` | `pub fn split(self, text: string) -> List[string]` | the pieces of `text` around each match `find_all` returns |

1. r[std-regex.replace.none] With no match, `replace` and `replace_all` return `text` unchanged.
2. r[std-regex.replace.expand] A match is replaced by `replacement`, with each reference in the table below replaced by the text it names in that match's groups.
3. r[std-regex.replace.once] `replacement` is read once, from left to right, for each match. A group's text is never read for references.

| Rule | Reference | Replaced by |
| --- | --- | --- |
| r[std-regex.replace.syntax.dollar] Dollar | `$$` | one `$` |
| r[std-regex.replace.syntax.number] Number | `$` and ASCII digits, as in `$1` | the text of the group with that number |
| r[std-regex.replace.syntax.braced] Braced number | `${` ASCII digits `}`, as in `${1}` | the text of the group with that number |
| r[std-regex.replace.syntax.name] Name | `${` name `}`, as in `${year}` | the text of the group with that name |

4. r[std-regex.replace.longest] A number takes every digit that follows the `$`, so `$12` is group 12. Write `${1}2` for group 1 then `2`.
5. r[std-regex.replace.absent] A reference to a group that took no part, or that the pattern does not have, is replaced by the empty text.
6. r[std-regex.replace.literal] A `$` that begins no reference in the table is itself. So `$x`, `${}`, an unclosed `${1`, and a final `$` stay as written.
7. r[std-regex.split.pieces] With k matches, `split` returns k + 1 pieces. They are the text before the first match, the text between each pair of matches, and the text after the last.
8. r[std-regex.split.ends] So a match at the start of `text` gives a first piece `""`, and one at its end a last piece `""`. Splitting `""` with no match gives `[""]`.

```text
use std.regex.Regex
use std.text.r

fn iso(dates: string) -> string:
    us := Regex::new(r"(?<month>\d\d)/(?<day>\d\d)/(\d{4})").expect("compiles")
    us.replace_all(dates, "$3-\${month}-\${day}")  # "12/31/2025" gives "2025-12-31"
```

```text
use std.regex.Regex
use std.text.r

fn fields(line: string) -> List[string]:
    Regex::new(r"\s*,\s*").expect("compiles").split(line)  # ",a , b" gives ["", "a", "b"]
```

> **Note.** A replacement is a string literal, so its `$` follows
> [Interpolation](../lang/01-lexical-structure.md#interpolation). `$1` and
> `$$` are text as written, by
> [`lex.interp.dollar-text`](../lang/01-lexical-structure.md#r-lex.interp.dollar-text).
> A braced reference needs `\$`, as in `"\${month}"`, or the string
> interpolates `month`. A raw string does not help: `r"\${month}"` keeps the
> backslash.

> **Why.** `$1`, `${name}`, and `$$` are the replacement syntax of Go and
> Rust. A bare `$name` is left out, since an hd string would interpolate
> it. So `$1a` is group 1 then `a`, where Go and Rust read a group named
> `1a`.

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
    BadGroupName
    DuplicateGroupName
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
| r[std-regex.error.kind.unsupported-group] Unsupported group | `UnsupportedGroup` | [`std-regex.syntax.group.unsupported`](#r-std-regex.syntax.group.unsupported) | `unsupported group` |
| r[std-regex.error.kind.bad-group-name] Bad group name | `BadGroupName` | [`std-regex.syntax.group.bad-name`](#r-std-regex.syntax.group.bad-name) | `invalid named capture` |
| r[std-regex.error.kind.duplicate-group-name] Duplicate group name | `DuplicateGroupName` | [`std-regex.syntax.group.duplicate-name`](#r-std-regex.syntax.group.duplicate-name) | `duplicate capture group name` |
| r[std-regex.error.kind.too-large] Too large | `TooLarge` | [`std-regex.error.too-large`](#r-std-regex.error.too-large) | `expression too large` |

1. r[std-regex.error.declared] `std.regex` declares the enum `RegexErrorKind` with the eleven kinds above, and the data type `RegexError` with the public fields above.
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
> and Rust's text for a duplicate name, so an error reads the same as there. A position lets a tool point at the
> spot in a pattern the user wrote.

## Not Yet Specified

These are not in this version of `std.regex`:

- flags, such as `(?i)`, `(?m)`, and `(?s)`, which are `UnsupportedGroup`
  errors today;
- `\b` and the Unicode classes `\p{...}`, which are `BadEscape` errors;
- a replacement computed by a function from each match's `Captures`.
