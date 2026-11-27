# `std.text`

Generated from `hd doc std.text`. Run `hd doc std.text.NAME` in a package for one item's full text.

## `r`

```text
pub fn r(t: Template[*dyn Display]) -> string
```

The standard string prefix: raw text with the values' `Display` text interpolated, as in `r"cost $5"`.

## `interpolate`

```text
pub fn interpolate[T < Display](t: Template[T]) -> string
```

Joins a template's pieces with the `Display` text of each value, which every literal function uses.

## `EscapeError`

```text
pub struct EscapeError
```

A template expression that is not well formed, at a byte position.

- `pub position: usize`: The byte offset of the first character that makes the expression malformed.

## `process_escapes`

```text
pub fn process_escapes(text: string) -> Result[string, EscapeError]
```

Reads one template expression's escapes: `\n`, `\t`, `\r`, `\u{...}`, `\"`, `\$`, `\\`, `\{`, and `\}`.

## `Utf8Error`

```text
pub enum Utf8Error
```

A malformed byte sequence given to `string::from_utf8`.

## `Parse`

```text
pub trait Parse
```

A type that reads itself from text: `parse` builds a `Self` from the whole text, or an `Error`.

## `ParseBoolError`

```text
pub enum ParseBoolError
```

Why `bool` text was refused: it was neither `true` nor `false`.

## `ParseCharError`

```text
pub enum ParseCharError
```

Why `char` text was refused: it was empty or held more than one character.

## `StringBuilder`

```text
pub struct StringBuilder
```

Joins many pieces of text by pushing them, cheaper than repeated concatenation.

- `pub fn new() -> *mut StringBuilder`: Returns an empty builder, a new object that `push` changes.
- `pub fn push(&mut self, text: string) -> void`: Appends `text` to the builder.
- `pub fn push_char(&mut self, c: char) -> void`: Appends the character `c` to the builder.
- `pub fn push_ascii(&mut self, byte: u8) -> void`: Appends the ASCII byte `byte`. A byte of 128 or more panics.
- `pub fn reserve(&mut self, count: usize) -> void`: Makes room for at least `count` more bytes and appends nothing.
- `pub fn build(&mut self) -> string`: Returns every text pushed since the builder was made or last built, in push order, joined with nothing between, and empties the builder.

## `Chars`

```text
pub struct Chars
```

The cursor of a string's `chars`: its characters in order.

- `pub fn rev(self) -> Rev[Self, char]`: Reverses the remaining sequence without advancing or buffering it.

## `CharIndices`

```text
pub struct CharIndices
```

The cursor of a string's `char_indices`: `(byte offset, character)` pairs in order.

- `pub fn rev(self) -> Rev[Self, (usize, char)]`: Reverses the remaining sequence without advancing or buffering it.

## `Bytes`

```text
pub struct Bytes
```

The cursor of a string's `bytes`: its bytes in order.

- `pub fn rev(self) -> Rev[Self, u8]`: Reverses the remaining sequence without advancing or buffering it.

## `string`

- `pub fn to_utf8(self) -> List[u8]`: Returns the string's bytes as a new list.
- `pub fn len(&self) -> usize`: Returns the number of bytes in the string.
- `pub fn parse[T < Parse](self) -> Result[T, T::Error]`: Reads the whole string as a `T` through `T::parse`.
- `pub fn chars(self) -> Chars`: Iterates the string's characters, one Unicode scalar value at a time.
- `pub fn char_indices(self) -> CharIndices`: Iterates `(byte offset, character)` pairs, one per character.
- `pub fn bytes(self) -> Bytes`: Iterates the string's bytes, one `u8` at a time.
- `pub fn slice(self, start: usize, end: usize) -> string`: Returns the characters from byte offset `start` up to `end`, sharing the string's bytes.
- `pub fn from_utf8(bytes: List[u8]) -> Result[string, Utf8Error]`: Decodes well-formed UTF-8 bytes into a string, or the first malformed sequence.
- `pub fn is_empty(&self) -> bool`: True exactly when the string has no characters.
- `pub fn starts_with(&self, prefix: string) -> bool`: True when the string begins with `prefix`.
- `pub fn ends_with(&self, suffix: string) -> bool`: True when the string ends with `suffix`.
- `pub fn contains(&self, needle: string) -> bool`: True when `needle` occurs in the string.
- `pub fn find(&self, needle: string) -> usize?`: Returns the byte offset of the first occurrence of `needle`, in `.Some`, or `.None`.
- `pub fn rfind(&self, needle: string) -> usize?`: Returns the byte offset of the last occurrence of `needle`, in `.Some`, or `.None`.
- `pub fn splitn(self, separator: string, n: usize) -> List[string]`: Splits at the first `n - 1` occurrences of `separator`; the last piece holds the rest.
- `pub fn split(self, separator: string) -> List[string]`: Splits the string around each occurrence of `separator`, in order.
- `pub fn replace(self, old: string, replacement: string) -> string`: Returns the string with each occurrence of `old` replaced by `replacement`.
- `pub fn trim(self) -> string`: Returns the string without whitespace at either end.
- `pub fn trim_start(self) -> string`: Returns the string without whitespace at its start.
- `pub fn trim_end(self) -> string`: Returns the string without whitespace at its end.
- `pub fn lower(self) -> string`: Returns the string in lowercase.
- `pub fn upper(self) -> string`: Returns the string in uppercase.
- `pub fn strip_prefix(self, prefix: string) -> string?`: Returns the string without its `prefix` when it starts with it, in `.Some`, or `.None`.
- `pub fn strip_suffix(self, suffix: string) -> string?`: Returns the string without its `suffix` when it ends with it, in `.Some`, or `.None`.
- `pub fn lines(self) -> List[string]`: Splits the string into its lines, without their endings.
- `pub fn repeat(self, count: usize) -> string`: Joins `count` copies of the string.
- `pub fn split_once(self, separator: string) -> (string, string)?`: Splits around the first occurrence of `separator`, in `.Some`, or `.None` when there is none.
- `pub fn split_whitespace(self) -> List[string]`: Splits on runs of whitespace, with no empty pieces.
- `pub fn pad_start(self, width: usize, fill: char = ' ') -> string`: Pads the string on the left with `fill` until it is `width` characters long.
- `pub fn pad_end(self, width: usize, fill: char = ' ') -> string`: Pads the string on the right with `fill` until it is `width` characters long.
- `pub fn count(&self, needle: string) -> usize`: Returns the number of occurrences of `needle` in the string.

## `char`

- `pub fn is_ascii_digit(self) -> bool`: True when the character is an ASCII digit `0` to `9`.
- `pub fn is_alphabetic(self) -> bool`: True when the character has the Unicode Alphabetic property.
- `pub fn is_uppercase(self) -> bool`: True when the character has the Unicode Uppercase property.
- `pub fn is_alphanumeric(self) -> bool`: True when the character is alphabetic or a digit.
- `pub fn is_whitespace(self) -> bool`: True when the character has the Unicode White_Space property.
- `pub fn to_digit(self, radix: u32) -> u32?`: Reads the character as a digit of `radix` (2 to 36), or `.None` when it is not one.
- `pub fn to_u32(self) -> u32`: Returns the character's Unicode scalar value.
