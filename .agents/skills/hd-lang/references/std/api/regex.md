# `std.regex`

Generated from `hd doc std.regex`. Run `hd doc std.regex.NAME` in a package for one item's full text.

## `Regex`

```text
pub struct Regex
```

A compiled regular expression: immutable, and every match runs in time linear in the text.

- `pub fn new(pattern: string) -> Result[Regex, RegexError]`: Compiles `pattern`, or returns the first error it holds.
- `pub fn pattern(&self) -> string`: The pattern the regex was compiled from.
- `pub fn is_match(&self, text: string) -> bool`: True when the regex matches anywhere in `text`.
- `pub fn find(&self, text: string) -> Match?`: The leftmost match in `text`; of several starting there, the first the pattern's alternation order prefers.
- `pub fn find_all(&self, text: string) -> List[Match]`: Every non-overlapping match in `text`, from left to right.
- `pub fn captures(&self, text: string) -> Captures?`: The capture groups of the leftmost match, or `.None`.
- `pub fn decode[T < Deserialize](self, text: string) -> Result[T, RegexError]`: Decodes the first match as a record through its Deserialize implementation.
- `pub fn captures_all(&self, text: string) -> List[Captures]`: The capture groups of every non-overlapping match, from left to right.
- `pub fn replace(&self, text: string, replacement: string) -> string`: The text with its leftmost match replaced by `replacement`, whose `$` forms expand groups.
- `pub fn replace_all(&self, text: string, replacement: string) -> string`: The text with every non-overlapping match replaced by `replacement`, whose `$` forms expand groups.
- `pub fn split(&self, text: string) -> List[string]`: The text's pieces between the matches, in order.

## `Match`

```text
pub struct Match
```

One match of a regex in a text.

- `pub start: usize`: The byte offset where the match starts.
- `pub end: usize`: The byte offset one past the match's end.
- `pub text: string`: The matched text.

## `Captures`

```text
pub struct Captures
```

The capture groups of one match: group 0 is the whole match.

- `pub fn len(&self) -> usize`: The number of capture groups, the whole match included.
- `pub fn get(&self, index: usize) -> Match?`: The match of group `index`, or `.None` when it took no part or is out of range.
- `pub fn name(&self, name: string) -> Match?`: The match of the group named `name`, or `.None` when there is no such group or it took no part.
- `pub fn decode[T < Deserialize](self) -> Result[T, RegexError]`: Decodes this capture set as a record through its Deserialize implementation.

## `RegexError`

```text
pub enum RegexError
```

Why a pattern failed to compile, a match was absent, or a group could not be decoded.
