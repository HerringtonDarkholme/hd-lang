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

`std` also gives `string` methods that split text once, split it into
words, pad it to a width, and count matches:

```text
fn setting(line: string) -> string:
    match line.split_once("="):
        .Some((key, value)) => "$key is $value"  # "port is 80=x" for "port=80=x"
        .None => "no setting"

fn cell(text: string) -> string:
    text.pad_start(6)  # "    42" for "42"

fn word_count(text: string) -> usize:
    text.split_whitespace().len()  # 2 for " a\tb "
```

| Receiver | Methods |
| --- | --- |
| `string` | `split_once(self, separator: string) -> (string, string)?`; `split_whitespace(self) -> List[string]`; `pad_start(self, width: usize, fill: char = ' ') -> string`; `pad_end(self, width: usize, fill: char = ' ') -> string`; `count(self, needle: string) -> usize` |

1. r[std-text.split-once] `split_once(separator)` returns `.Some((before, after))`, with the text before and after the first occurrence of `separator`.
2. r[std-text.split-once.absent] It returns `.None` when `separator` does not occur, so `"".split_once("=")` is `.None`.
3. r[std-text.split-once.empty] An empty separator occurs at offset 0, so `"ab".split_once("")` is `.Some(("", "ab"))`.
4. r[std-text.split-whitespace] `split_whitespace` returns, in order, each maximal run of scalar values that lack the Unicode `White_Space` property.
5. r[std-text.split-whitespace.no-empty] It returns no empty piece, so an empty string, or one of whitespace only, gives `[]`.
6. r[std-text.pad.length] `pad_start` and `pad_end` measure a string's length in scalar values, not in bytes or display columns.
7. r[std-text.pad.start] `pad_start(width, fill)` puts copies of `fill` before the string until its length is `width`.
8. r[std-text.pad.end] `pad_end(width, fill)` puts the copies after the string instead.
9. r[std-text.pad.width-at-most-length] A `width` at or below the string's length gives the string unchanged.
10. r[std-text.pad.fill-default] `fill` defaults to the space U+0020, so `"7".pad_end(3)` is `"7  "`.
11. r[std-text.count] `count(needle)` returns the number of non-overlapping matches of `needle`, found from left to right, so `"aaaa".count("aa")` is 2.
12. r[std-text.count.empty] An empty needle matches at every scalar boundary, so it gives the number of scalar values plus one, and `"".count("")` is 1.

| Call | Result |
| --- | --- |
| `"a=b=c".split_once("=")` | `.Some(("a", "b=c"))` |
| `"abc".split_once(",")` | `.None` |
| `" a  b ".split_whitespace()` | `["a", "b"]` |
| `"é".pad_start(3, '.')` | `"..é"`, since `é` is one scalar value |
| `"abc".pad_end(2)` | `"abc"` |
| `"héllo".count("")` | 6 |

> **Why.** The names and results are Rust's `split_once` and
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
        .Err(Utf8Error.Truncated) => "cut short"
```

| Receiver | Methods |
| --- | --- |
| `string` | `to_utf8(self) -> List[u8]`; the associated function `string::from_utf8(bytes: List[u8]) -> Result[string, Utf8Error]` |

1. r[std-text.utf8.to-utf8] `to_utf8` returns a new list of the string's bytes, in order.
2. r[std-text.utf8.from-utf8] `string::from_utf8(bytes)` returns `.Ok` of the string whose bytes are `bytes` when they are well-formed UTF-8, and `.Err` otherwise.
3. r[std-text.utf8.well-formed] Well-formed UTF-8 is that of the Unicode Standard, so an overlong encoding, a surrogate code point, and a value above U+10FFFF are not well formed.
4. r[std-text.utf8.round-trip] For every string `s`, `string::from_utf8(s.to_utf8())` is `.Ok(s)`.
5. r[std-text.utf8.error.declared] `std.text` declares the enum `Utf8Error`, with the variants `InvalidSequence(position: usize)` and `Truncated`. Code imports it, as in `use std.text.Utf8Error`.
6. r[std-text.utf8.error.first] The error describes the first sequence, from the start of `bytes`, that is not well formed.
7. r[std-text.utf8.error.truncated] It is `Truncated` when the bytes end before that sequence has the length its first byte gives, and each byte of it after the first is a continuation byte, `0x80` to `0xBF`.
8. r[std-text.utf8.error.invalid] Otherwise it is `InvalidSequence(position)`, where `position` is the byte offset at which that sequence starts.
9. r[std-text.utf8.error.traits] `Utf8Error` implements `Eq`, `Debug`, and `Display`. Two errors are equal when they are the same variant with the same position.

| Rule | Error | Display text |
| --- | --- | --- |
| r[std-text.utf8.error.display.invalid] Invalid sequence | `InvalidSequence(position)` | `invalid UTF-8 sequence at byte ` followed by `position` in decimal |
| r[std-text.utf8.error.display.truncated] Truncated | `Truncated` | `incomplete UTF-8 sequence at the end of the bytes` |

> **Note.** So `[0xE2, 0x82]` is `Truncated`, `[0xC3, 0x28]` is
> `InvalidSequence(0)`, and the overlong `[0xC0, 0xAF]` is
> `InvalidSequence(0)`.

## Raw Text Prefix

The standard library declares one prefix, in `std.text`:

| Rule | Declaration | Meaning |
| --- | --- | --- |
| r[std-text.prefix.std.r] Raw text | `@str_prefix pub fn r(t: Template[Display]) -> string` | the pieces joined with the values' `Display` text, with no escape processed |

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
