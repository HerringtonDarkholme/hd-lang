# Text

Status: standard library specification draft.

This chapter defines the part of `std.text` that `lib/std` writes in
ordinary hd over the language tier:

- the `string` methods above the representation intrinsics: `trim`,
  `lower`, `split`, `replace`, and `starts_with`;
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
| `string` | `trim(self) -> string`; `lower(self) -> string`; `split(self, separator: string) -> List[string]`; `replace(self, old: string, replacement: string) -> string`; `starts_with(self, prefix: string) -> bool` |

1. r[std-text.string.lower] `lower` uses Unicode Default Case Conversion with full mappings.
2. r[std-text.string.trim] `trim` removes the Unicode `White_Space` property at both ends.
3. r[std-text.string.split] `split(separator)` retains empty pieces between adjacent separators and at either end.
4. r[std-text.string.split.empty-separator] An empty separator splits into one-scalar strings, with an empty input producing an empty list.
5. r[std-text.string.split.absent] With a non-empty separator, an input without that separator, including the empty string, yields one piece, so `"".split(",")` is `[""]`.
6. r[std-text.string.replace] `replace` replaces non-overlapping matches from left to right.
7. r[std-text.string.replace.empty] An empty `old` inserts the replacement at scalar boundaries.
8. r[std-text.string.starts-with] `starts_with` compares scalar sequences exactly and performs no normalization or case folding.

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
