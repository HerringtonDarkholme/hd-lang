# Text

Status: standard library specification draft.

This chapter defines the part of `std.text` that `lib/std` writes in
ordinary hd over the language tier:

- the `string` methods above the representation intrinsics: `trim`,
  `lower`, `split`, `replace`, `starts_with`, `lines`, and `repeat`;
- the splitting and padding methods `split_once`, `split_whitespace`,
  `pad_start`, `pad_end`, and `count`;
- the `char` classification methods and `to_digit`;
- the UTF-8 conversions `to_utf8` and `string::from_utf8`, and
  `Utf8Error`;
- the `Parse` trait, `string.parse::[T]()`, and the `Parse` impls of
  `bool`, `char`, and `string`;
- the raw-text string prefix `r`.

The language tier keeps the representation of a string and the methods
that expose it
([Strings](../lang/04-type-system.md#strings),
[String Indexing](../lang/05-expressions.md#string-indexing),
[String Methods](../lang/10-modules.md#string-methods)):

| Item | Why it stays in the language tier |
| --- | --- |
| `len`, `s[i]` | they read the byte representation that the compiler lays out |
| `bytes`, `slice` | intrinsics over that representation; `slice` shares bytes in constant time |
| `chars`, `char_indices` | [`flow.for.string-explicit`](../lang/06-control-flow.md#r-flow.for.string-explicit), a language rule, names them as the way to loop over a string |
| UTF-8 and byte offsets | [`module.string.utf8`](../lang/10-modules.md#r-module.string.utf8) and [`module.string.byte-offsets`](../lang/10-modules.md#r-module.string.byte-offsets) hold for every string method, in both tiers |

The prefix mechanism, `@str_prefix` and `std.ops.Template`, is language
tier too ([Prefixed Strings](../lang/05-expressions.md#prefixed-strings)).

## String Methods

`std` gives `string` these methods, beside the language-tier ones:

| Receiver | Methods |
| --- | --- |
| `string` | `trim(self) -> string`; `lower(self) -> string`; `split(self, separator: string) -> List[string]`; `replace(self, old: string, replacement: string) -> string`; `starts_with(self, prefix: string) -> bool`; `lines(self) -> List[string]`; `repeat(self, count: usize) -> string` |

1. r[std-text.string.lower] `lower` uses Unicode Default Case Conversion with full mappings.
2. r[std-text.string.trim] `trim` removes the Unicode `White_Space` property at both ends.
3. r[std-text.string.split] `split(separator)` retains empty pieces between adjacent separators and at either end.
4. r[std-text.string.split.empty-separator] An empty separator splits into one-scalar strings, with an empty input producing an empty list.
5. r[std-text.string.split.absent] With a non-empty separator, an input without that separator, including the empty string, yields one piece, so `"".split(",")` is `[""]`.
6. r[std-text.string.replace] `replace` replaces non-overlapping matches from left to right.
7. r[std-text.string.replace.empty] An empty `old` inserts the replacement at scalar boundaries.
8. r[std-text.string.starts-with] `starts_with` compares scalar sequences exactly and performs no normalization or case folding.
9. r[std-text.string.lines] `lines` returns the pieces between `\n` separators, and removes a `\r` directly before each `\n`.
10. r[std-text.string.lines.final] A final `\n` ends the last line and starts no empty one, so `"a\nb\n".lines()` is `["a", "b"]` and `"".lines()` is `[]`.
11. r[std-text.string.repeat] `repeat(count)` joins `count` copies of the string, so a `count` of 0 gives `""`.

```text
fn rows(text: string) -> usize:
    text.lines().len()  # 2 for "a\r\nb\n", as Rust's str::lines

fn rule(width: usize) -> string:
    "-".repeat(width)
```

## Splitting And Padding

`std` also gives `string` methods that split text a bounded number of
times, split it into words, pad it to a width, and count matches:

```text
fn setting(line: string) -> string:
    match line.split_once("="):
        .Some((key, value)) => "$key is $value"  # "port is 80=x" for "port=80=x"
        .None => "no setting"

fn first_two(path: string) -> List[string]:
    path.splitn("/", 2)  # ["a", "b/c"] for "a/b/c"

fn cell(text: string) -> string:
    text.pad_start(6)  # "    42" for "42"

fn word_count(text: string) -> usize:
    text.split_whitespace().len()  # 2 for " a\tb "
```

| Receiver | Methods |
| --- | --- |
| `string` | `split_once(self, separator: string) -> (string, string)?`; `splitn(self, separator: string, n: usize) -> List[string]`; `split_whitespace(self) -> List[string]`; `pad_start(self, width: usize, fill: char = ' ') -> string`; `pad_end(self, width: usize, fill: char = ' ') -> string`; `count(self, needle: string) -> usize` |

1. r[std-text.split-once] `split_once(separator)` returns `.Some((before, after))`, with the text before and after the first occurrence of `separator`.
2. r[std-text.split-once.absent] It returns `.None` when `separator` does not occur, so `"".split_once("=")` is `.None`.
3. r[std-text.split-once.empty] An empty separator occurs at offset 0, so `"ab".split_once("")` is `.Some(("", "ab"))`.
4. r[std-text.splitn] `splitn(separator, n)` returns the first `n - 1` pieces that `split(separator)` returns, followed by the rest of the string after the last of those separators as the final piece.
5. r[std-text.splitn.few] A string with at most `n - 1` separators gives the pieces of `split(separator)`, so `"a=b".splitn("=", 5)` is `["a", "b"]`.
6. r[std-text.splitn.small] An `n` of 1 gives the whole string as one piece, and an `n` of 0 gives `[]`.
7. r[std-text.splitn.empty-separator] An empty separator takes one scalar value per piece, as `split` does, so `"héy".splitn("", 2)` is `["h", "éy"]` and `"".splitn("", 2)` is `[]`.
8. r[std-text.split-whitespace] `split_whitespace` returns, in order, each maximal run of scalar values that lack the Unicode `White_Space` property.
9. r[std-text.split-whitespace.no-empty] It returns no empty piece, so an empty string, or one of whitespace only, gives `[]`.
10. r[std-text.pad.length] `pad_start` and `pad_end` measure a string's length in scalar values, not in bytes or display columns.
11. r[std-text.pad.start] `pad_start(width, fill)` puts copies of `fill` before the string until its length is `width`.
12. r[std-text.pad.end] `pad_end(width, fill)` puts the copies after the string instead.
13. r[std-text.pad.width-at-most-length] A `width` at or below the string's length gives the string unchanged.
14. r[std-text.pad.fill-default] `fill` defaults to the space U+0020, so `"7".pad_end(3)` is `"7  "`.
15. r[std-text.count] `count(needle)` returns the number of non-overlapping matches of `needle`, found from left to right, so `"aaaa".count("aa")` is 2.
16. r[std-text.count.empty] An empty needle matches at every scalar boundary, so it gives the number of scalar values plus one, and `"".count("")` is 1.

| Call | Result |
| --- | --- |
| `"a=b=c".split_once("=")` | `.Some(("a", "b=c"))` |
| `"abc".split_once(",")` | `.None` |
| `"a=b=c".splitn("=", 2)` | `["a", "b=c"]` |
| `"a=b".splitn("=", 0)` | `[]` |
| `" a  b ".split_whitespace()` | `["a", "b"]` |
| `"é".pad_start(3, '.')` | `"..é"`, since `é` is one scalar value |
| `"abc".pad_end(2)` | `"abc"` |
| `"héllo".count("")` | 6 |

> **Why.** The names and results are Rust's `splitn`, `split_once` and
> `split_whitespace`, Python's `str.count`, and JavaScript's `padStart`
> and `padEnd`. A width in scalar values is the unit `chars` yields;
> display columns would need the East Asian Width tables. An empty needle
> counts the boundaries that `replace` with an empty `old` fills, as in
> Python.

## Character Classification

`std` gives `char` four classification methods and `to_digit`, as Rust
does:

```text
fn word_start(c: char) -> bool:
    c.is_alphabetic() || c == '_'

fn hex_value(c: char) -> u32?:
    c.to_digit(16)  # .Some(15) for 'f' and for 'F'
```

| Rule | Method | Result |
| --- | --- | --- |
| r[std-text.char.ascii-digit] ASCII digit | `is_ascii_digit(self) -> bool` | `true` for `'0'` to `'9'` only |
| r[std-text.char.alphabetic] Alphabetic | `is_alphabetic(self) -> bool` | `true` for a scalar value with the Unicode `Alphabetic` property |
| r[std-text.char.alphanumeric] Alphanumeric | `is_alphanumeric(self) -> bool` | `true` for an `Alphabetic` scalar value, or one whose general category is `Nd`, `Nl`, or `No` |
| r[std-text.char.whitespace] Whitespace | `is_whitespace(self) -> bool` | `true` for a scalar value with the Unicode `White_Space` property, the set that `trim` removes |
| r[std-text.char.to-digit] To digit | `to_digit(self, radix: u32) -> u32?` | the digit's value in `radix`, by the rules below |

1. r[std-text.char.unicode-version] The Unicode properties and general categories that this section names are those of Unicode 17.0.
2. r[std-text.char.to-digit.value] `to_digit` gives `'0'` to `'9'` the values 0 to 9, and gives `'a'` to `'z'` and `'A'` to `'Z'` the values 10 to 35.
3. r[std-text.char.to-digit.result] It returns `.Some` of that value when the value is less than `radix`, and `.None` otherwise.
4. r[std-text.char.to-digit.ascii-only] Every other `char` gives `.None`, so `'٣'.to_digit(10)` is `.None`, though `'٣'.is_alphanumeric()` is `true`.
5. r[std-text.char.to-digit.radix] A `radix` below 2 or above 36 panics. Panic: `explicit-panic`.

> **Why.** The names and results are Rust's, so agents and readers
> already know them. `to_digit` reads ASCII only, so a number parser
> built on it accepts no digit of another script by accident.

## UTF-8 Conversion

`to_utf8` copies a string's bytes, and `string::from_utf8` checks bytes
and makes a string of them, as
[`types.string.from-bytes`](../lang/04-type-system.md#r-types.string.from-bytes)
requires:

```text
use std.text.Utf8Error

fn decode(bytes: List[u8]) -> string:
    match string::from_utf8(bytes):
        .Ok(text) => text
        .Err(Utf8Error.InvalidSequence(position)) => "bad byte at $position"
        .Err(Utf8Error.Truncated(position)) => "cut short at $position"
```

| Receiver | Methods |
| --- | --- |
| `string` | `to_utf8(self) -> List[u8]`; the associated function `string::from_utf8(bytes: List[u8]) -> Result[string, Utf8Error]` |

1. r[std-text.utf8.to-utf8] `to_utf8` returns a new list of the string's bytes, in order.
2. r[std-text.utf8.from-utf8] `string::from_utf8(bytes)` returns `.Ok` of the string whose bytes are `bytes` when they are well-formed UTF-8, and `.Err` otherwise.
3. r[std-text.utf8.well-formed] Well-formed UTF-8 is that of the Unicode Standard, so an overlong encoding, a surrogate code point, and a value above U+10FFFF are not well formed.
4. r[std-text.utf8.round-trip] For every string `s`, `string::from_utf8(s.to_utf8())` is `.Ok(s)`.
5. r[std-text.utf8.error.declared-positions] `std.text` declares the enum `Utf8Error`, with the variants `InvalidSequence(position: usize)` and `Truncated(position: usize)`. Code imports it, as in `use std.text.Utf8Error`.
6. r[std-text.utf8.error.first] The error describes the first sequence, from the start of `bytes`, that is not well formed.
7. r[std-text.utf8.error.truncated-position] It is `Truncated(position)` when the bytes end before that sequence has the length its first byte gives, where `position` is the byte offset at which that sequence starts. That holds only when each byte of it after the first is a continuation byte, `0x80` to `0xBF`.
8. r[std-text.utf8.error.invalid] Otherwise it is `InvalidSequence(position)`, where `position` is the byte offset at which that sequence starts.
9. r[std-text.utf8.error.traits] `Utf8Error` implements `Eq`, `Debug`, and `Display`. Two errors are equal when they are the same variant with the same position.

| Rule | Error | Display text |
| --- | --- | --- |
| r[std-text.utf8.error.display.invalid] Invalid sequence | `InvalidSequence(position)` | `invalid UTF-8 sequence at byte ` followed by `position` in decimal |
| r[std-text.utf8.error.display.truncated-position] Truncated | `Truncated(position)` | `incomplete UTF-8 sequence at byte ` followed by `position` in decimal |

> **Note.** So `[0xE2, 0x82]` is `Truncated(0)`, `[0x68, 0xE2, 0x82]` is `Truncated(1)`, `[0xC3, 0x28]` is
> `InvalidSequence(0)`, and the overlong `[0xC0, 0xAF]` is
> `InvalidSequence(0)`.

## Parsing Text

`Parse` is the trait of a type that reads itself from text, as Rust's
`FromStr`. `string.parse::[T]()` calls it:

```text
use std.text.Parse

fn port(text: string) -> i32:
    match text.parse::[i32]():
        .Ok(value) => value
        .Err(_) => 8080

fn read[T < Parse](text: string) -> Result[T, T::Error]:
    T::parse(text)

fn flag(text: string) -> bool:
    text.parse::[bool]().unwrap_or(false)
```

The trait and the `bool`, `char`, and `string` impls are declared here.
The number impls are in [Integer Parsing](num.md#integer-parsing).

```text
pub trait Parse:
    type Error
    fn parse(text: string) -> Result[Self, Self::Error]

pub enum ParseBoolError:
    Invalid

pub enum ParseCharError:
    Empty
    TooMany
```

1. r[std-text.parse.trait] `std.text` declares the trait `Parse` as above. It is not a prelude name; code imports it, as in `use std.text.Parse`.
2. r[std-text.parse.whole] `Self::parse(text)` reads the whole of `text`. An impl never trims, skips, or ignores any part of it.
3. r[std-text.parse.method] `std` gives `string` the method `parse[T < Parse](self) -> Result[T, T::Error]`, which returns `T::parse(self)`. Calling it needs no import, and the call names the type, as in `text.parse::[i32]()`.
4. r[std-text.parse.string] `string` implements `Parse` with `Error` set to `never`, and `parse` returns `.Ok` of the same text.
5. r[std-text.parse.bool] `bool` implements `Parse` with `Error` set to `ParseBoolError`. The text `"true"` gives `.Ok(true)`, and `"false"` gives `.Ok(false)`.
6. r[std-text.parse.bool-other] Any other text gives `.Err(ParseBoolError.Invalid)`. That includes `""`, `"True"`, `"1"`, and text with a space.
7. r[std-text.parse.bool-error] `std.text` declares `ParseBoolError` as above. It implements `Eq`, `Debug`, `Display`, and `Error`. Its `Display` text is ``provided string was not `true` or `false` ``.
8. r[std-text.parse.char] `char` implements `Parse` with `Error` set to `ParseCharError`. Text of exactly one Unicode scalar value gives `.Ok` of it.
9. r[std-text.parse.char-empty] Empty text gives `.Err(ParseCharError.Empty)`.
10. r[std-text.parse.char-many] Text of two or more scalar values gives `.Err(ParseCharError.TooMany)`.
11. r[std-text.parse.char-error] `std.text` declares `ParseCharError` as above. It implements `Eq`, `Debug`, `Display`, and `Error`. Its `Display` texts are `cannot parse a character from empty text` for `Empty` and `text has more than one character` for `TooMany`.
12. r[std-text.parse.never-traits] `never` implements `Eq`, `Debug`, `Display`, and `Error`, so `Result[string, never]` works wherever `Result[T, ParseNumberError]` does, and a generic reader over `T < Parse[Error = E]` and `E < Display` accepts `string`. No value of `never` exists, so none of their methods runs.

| Call | Result |
| --- | --- |
| `"true".parse::[bool]()` | `.Ok(true)` |
| `"yes".parse::[bool]()` | `.Err(ParseBoolError.Invalid)` |
| `"é".parse::[char]()` | `.Ok('é')` |
| `"ab".parse::[char]()` | `.Err(ParseCharError.TooMany)` |
| `"ab".parse::[string]()` | `.Ok("ab")` |

> **Why.** `Parse` is Rust's `FromStr`: one trait gives `std.cli` and
> `std.host` a typed read without a function per type, and a user type
> can join it. An impl reads the whole text so that the same text means the
> same value everywhere, as `parse_i32` does.

## Raw Text Prefix

The standard library declares one prefix, in `std.text`:

| Rule | Declaration | Meaning |
| --- | --- | --- |
| r[std-text.prefix.std.r] Raw text | `@str_prefix pub fn r(t: Template[dyn Display]) -> string` | the pieces joined with the values' `Display` text, with no escape processed |

1. r[std-text.prefix.std.r-meaning] So `r"\d+ $n"` is the text `\d+ ` followed by the `Display` text of `n`, and `r"a\"b"` keeps its backslash.
2. r[std-text.prefix.std.only-r] `r` is the only standard prefix. `std` declares no `b`, so `b"..."` names nothing until a bytes type exists.
3. r[std-text.prefix.std.import-text] `std.text` declares the string prefix `r`. It is not a prelude name; code imports it, as in `use std.text.r`.

```text
use std.text.r

fn digits(count: i32) -> string:
    r"\d{$count}"  # the text \d{ then count, then }
```

See also: [Prefixed Strings](../lang/05-expressions.md#prefixed-strings), which
specifies how a prefixed string calls its prefix function.

## More String Methods

`std` also gives `string` methods that test, search, and trim text:

```text
fn extension(name: string) -> string?:
    match name.rfind("."):
        .Some(at) => .Some(name.slice(at + 1, name.len()))  # "gz" for "a.tar.gz"
        .None => .None

fn header(line: string) -> string?:
    if line.is_empty() || !line.contains(":"):
        return .None
    match line.strip_prefix("# "):
        .Some(rest) => .Some(rest.trim_end().upper())
        .None => .None
```

| Receiver | Methods |
| --- | --- |
| `string` | `is_empty(self) -> bool`; `ends_with(self, suffix: string) -> bool`; `contains(self, needle: string) -> bool`; `find(self, needle: string) -> usize?`; `rfind(self, needle: string) -> usize?`; `trim_start(self) -> string`; `trim_end(self) -> string`; `upper(self) -> string`; `strip_prefix(self, prefix: string) -> string?`; `strip_suffix(self, suffix: string) -> string?` |

1. r[std-text.more.is-empty] `is_empty` is true exactly when the string has no bytes.
2. r[std-text.more.ends-with] `ends_with` compares scalar sequences exactly, as `starts_with` does.
3. r[std-text.more.contains] `contains(needle)` is true when `needle` occurs in the string. An empty `needle` occurs in every string.
4. r[std-text.more.find] `find(needle)` returns the byte offset of the first occurrence of `needle` in `.Some`, or `.None`. An empty `needle` is found at 0.
5. r[std-text.rfind] `rfind(needle)` returns the byte offset of the last occurrence of `needle` in `.Some`, or `.None`. Occurrences may overlap, so `"aaaa".rfind("aa")` is `.Some(2)`.
6. r[std-text.rfind.empty] An empty `needle` is found at the length of the string in bytes, as in Rust, so `"abc".rfind("")` is `.Some(3)`.
7. r[std-text.rfind.linear] `rfind` takes time linear in the lengths of the string and the needle together.
8. r[std-text.more.trim-ends] `trim_start` removes the Unicode `White_Space` property at the start only, and `trim_end` at the end only.
9. r[std-text.more.upper] `upper` uses Unicode Default Case Conversion with full mappings, as `lower` does.
10. r[std-text.more.strip] `strip_prefix(prefix)` returns the rest of the string after `prefix` in `.Some` when the string starts with it, and `.None` otherwise. `strip_suffix` does the same at the end.

| Call | Result |
| --- | --- |
| `"héllo".find("llo")` | `.Some(3)` |
| `"abc".contains("")` | `true` |
| `"  a ".trim_start()` | `"a "` |
| `"straße".upper()` | `"STRASSE"` |
| `"key=1".strip_prefix("key=")` | `.Some("1")` |

> **Why.** The names are Rust's `str` methods. `find` gives a byte
> offset, so its result is a valid `slice` bound.

## Joining And Building

The `join` method of `List[string]` and `StringBuilder` join many
pieces of text:

```text
use std.text.StringBuilder

fn csv(cells: List[string]) -> string:
    cells.join(",")

fn report(names: List[string]) -> string:
    out := StringBuilder::new()
    for name in names:
        out.push("- ").push(name).push_char('\n')
    out.build()
```

1. r[std-text.join.list-method] `std.text` gives `List[string]` the method `join(self, separator: string) -> string`. Calling it needs no import.
2. r[std-text.join.list-result] `join` returns the parts in order with `separator` between each two, so one part gives that part and no part gives `""`.
3. r[std-text.builder.decl-struct] `std.text` declares the struct `StringBuilder`, with private fields, which implements `Debug`. Code imports it, as in `use std.text.StringBuilder`.
4. r[std-text.builder.new-ref] `StringBuilder::new() -> &mut StringBuilder` returns an empty builder, a new object. Every copy of the reference pushes to that one builder.
5. r[std-text.builder.push-ref] `b.push(text: string) -> &mut StringBuilder` appends `text` and returns `b`, so calls chain; it takes `&mut self`. A statement may discard the result, since `StringBuilder` is not a must-use type ([Must-Use Values](../lang/06-control-flow.md#must-use-values)).
6. r[std-text.builder.push-char-ref] `b.push_char(c: char) -> &mut StringBuilder` appends the UTF-8 encoding of `c`, one to four bytes, as `push` of the one-character string would, and returns `b`. It takes `&mut self`.
7. r[std-text.builder.build] `b.build() -> string` returns every pushed text in push order, joined with nothing between. It leaves the builder unchanged.

> **Why.** The method is Rust's `[String].join(sep)`, and agents wrote it
> in usability probes. There is no free `join(parts, separator)`, since it
> was a second spelling of the method.
> `trait.inherent.unique-unifying.disjoint` already allows an inherent
> implementation for `List[string]` alone.

## Prefix Helpers

`std.text` gives a custom string prefix the helpers that `r` uses:

```text
use std.ops.{Template, str_prefix}
use std.text.{interpolate, process_escapes}

@str_prefix
fn cooked(t: Template[i32]) -> string:
    let mut pieces: List[string] = []
    for piece in t.raw_parts:
        match process_escapes(piece):
            .Ok(text) => pieces.push(text)
            .Err(error) => pieces.push("?${error.position}")
    interpolate(Template { raw_parts: pieces, values: t.values })
```

1. r[std-text.prefix-helper.interpolate] `std.text` declares `pub fn interpolate[T < Display](t: Template[T]) -> string`, which joins the raw pieces and the values' `Display` text, alternating, starting with the first piece.
2. r[std-text.prefix-helper.escapes] `std.text` declares `pub fn process_escapes(text: string) -> Result[string, EscapeError]`, which replaces each escape sequence of [Escape Sequences](../lang/01-lexical-structure.md#escape-sequences) with its meaning.
3. r[std-text.prefix-helper.escape-error-value] `std.text` declares `pub struct EscapeError`, a value with one public field, `position: usize`, the byte offset of the first invalid escape's backslash. It implements `Eq` and `Debug`.
4. r[std-text.prefix-helper.final-backslash] A lone final backslash is an invalid escape.

| Call | Result |
| --- | --- |
| `process_escapes("a\\tb")` | `.Ok` of `a`, a tab, and `b` |
| `process_escapes("é\\q")` | `.Err(EscapeError { position: 2 })` |

> **Why.** The field is `position`, as in every other std error. Its
> byte unit is the one `slice` takes.
