# Encoding

Status: standard library specification draft.

This chapter defines `std.encoding`, which `lib/std` writes in ordinary hd
over the language tier:

- hex text for bytes: `hex_encode` and `hex_decode`;
- base64 text for bytes, in the four encodings of
  [RFC 4648](https://www.rfc-editor.org/rfc/rfc4648) sections 4 and 5,
  as the enum `Base64` with `encode` and `decode`;
- `DecodeError`, the error that both decoders return.

Nothing in it is language tier. Bytes are a `List[u8]`. The bytes of a
string come from the language-tier `bytes` method
([`module.string.bytes`](../lang/10-modules.md#r-module.string.bytes)), as
in `text.bytes().collect()`.

1. r[std-encoding.import] `std.encoding` declares `DecodeError`, `Base64`, `hex_encode`, and `hex_decode`. None is a prelude name; code imports them, as in `use std.encoding.hex_encode`.
2. r[std-encoding.no-panic] None of these functions panics.

```text
use std.encoding.hex_encode

fn hex_of(text: string) -> string:
    hex_encode(text.bytes().collect())
```

## Hex

Hex text writes each byte as two digits:

```text
use std.encoding.{hex_decode, hex_encode, DecodeError}

fn fingerprint(text: string) -> string:
    hex_encode(text.bytes().collect())  # "666f6f" for "foo"

fn key(text: string) -> List[u8]:
    match hex_decode(text):
        .Ok(bytes) => bytes
        .Err(_) => []
```

| Rule | Function | Result |
| --- | --- | --- |
| r[std-encoding.hex.encode] `hex_encode` | `pub fn hex_encode(bytes: List[u8]) -> string` | the hex text of `bytes` |
| r[std-encoding.hex.decode] `hex_decode` | `pub fn hex_decode(text: string) -> Result[List[u8], DecodeError]` | `.Ok` of the bytes that `text` writes, or `.Err` of the first error |

1. r[std-encoding.hex.encode.digits] `hex_encode` writes each byte in order as two digits, its high four bits first. The digits are `0` to `9` and the lowercase `a` to `f`.
2. r[std-encoding.hex.decode.case] `hex_decode` accepts the digits `0` to `9`, `a` to `f`, and `A` to `F`. It reads each pair of digits as one byte, the high digit first.
3. r[std-encoding.hex.decode.length-bytes] Text whose length in bytes is odd gives `.Err(DecodeError.InvalidLength(position))`, where `position` is that length.
4. r[std-encoding.hex.decode.character] Otherwise, the first character that is not a hex digit gives `.Err(DecodeError.InvalidCharacter(position))`.
5. r[std-encoding.hex.decode.order] The length is checked before any character.

| Text | `hex_decode` gives |
| --- | --- |
| `""` | `.Ok([])` |
| `"666f6F"` | `.Ok` of the bytes of `"foo"` |
| `"abc"`, `"zz1"` | `.Err(InvalidLength(3))` |
| `"0g"` | `.Err(InvalidCharacter(1))` |

> **Note.** So `hex_decode(hex_encode(bytes))` is `.Ok(bytes)` for every
> list of bytes.

## Base64

Base64 text writes each group of three bytes as four symbols. `Base64` is a
closed set of four encodings, after Go's `base64.Encoding` values:

```text
pub enum Base64:
    Standard
    StandardNoPad
    Url
    UrlNoPad
```

```text
use std.encoding.{Base64, DecodeError}

fn token(bytes: List[u8]) -> string:
    Base64.Standard.encode(bytes)  # "Zm9vYg==" for the bytes of "foob"

fn jwt_part(bytes: List[u8]) -> string:
    Base64.UrlNoPad.encode(bytes)  # "Zm9vYg" for the bytes of "foob"

fn payload(text: string) -> Result[List[u8], DecodeError]:
    Base64.UrlNoPad.decode(text)
```

1. r[std-encoding.base64.variants] `std.encoding` declares the enum `Base64` with the variants `Standard`, `StandardNoPad`, `Url`, and `UrlNoPad`, none with a field. `Base64` implements `Eq` and `Debug`.

| Variant | Alphabet | Padding | Used by |
| --- | --- | --- | --- |
| `Standard` | standard, RFC 4648 section 4 | `=` | MIME, HTTP Basic |
| `StandardNoPad` | standard | none | |
| `Url` | URL-safe, RFC 4648 section 5 | `=` | |
| `UrlNoPad` | URL-safe | none | JWT, URL tokens |

| Rule | Method | Result |
| --- | --- | --- |
| r[std-encoding.base64.encode] `encode` | `pub fn encode(self, bytes: List[u8]) -> string` | the base64 text of `bytes` in the encoding `self` |
| r[std-encoding.base64.decode] `decode` | `pub fn decode(self, text: string) -> Result[List[u8], DecodeError]` | `.Ok` of the bytes that `text` writes in the encoding `self`, or `.Err` of the first error |

1. r[std-encoding.base64.alphabet] The alphabets differ in the symbols for the values 62 and 63 only.

| Values | Standard | URL-safe |
| --- | --- | --- |
| 0 to 25 | `A` to `Z` | `A` to `Z` |
| 26 to 51 | `a` to `z` | `a` to `z` |
| 52 to 61 | `0` to `9` | `0` to `9` |
| 62 | `+` | `-` |
| 63 | `/` | `_` |

### Encoding

1. r[std-encoding.base64.encode.groups] `encode` writes each group of three bytes, in order, as four symbols of six bits each, the high bits first, from the alphabet of `self`.
2. r[std-encoding.base64.encode.padding] In a padded encoding, a final group of one byte gives two symbols and `==`. A final group of two bytes gives three symbols and `=`.
3. r[std-encoding.base64.encode.no-pad] In a `NoPad` encoding, those final groups give two symbols and three symbols, and no `=`.
4. r[std-encoding.base64.encode.pad-bits] The low bits of the last symbol that encode no byte, the **pad bits**, are zero.

```text
use std.encoding.Base64

fn token(bytes: List[u8]) -> string:
    Base64.Standard.encode(bytes)

fn padded() -> List[string]:
    [token("f".bytes().collect()), token("fo".bytes().collect()), token("foo".bytes().collect())]
    # ["Zg==", "Zm8=", "Zm9v"]: one, two, and three bytes
```

| Bytes | `Standard` | `StandardNoPad` | `Url` | `UrlNoPad` |
| --- | --- | --- | --- | --- |
| `"fo"` | `"Zm8="` | `"Zm8"` | `"Zm8="` | `"Zm8"` |
| `[0xfb, 0xff]` | `"+/8="` | `"+/8"` | `"-_8="` | `"-_8"` |

### Decoding

`decode` accepts exactly the texts that `encode` returns for the same variant.

1. r[std-encoding.base64.decode.value] Accepted text gives `.Ok` of the bytes that `encode` turns into that text.
2. r[std-encoding.base64.decode.length-bytes] In a padded encoding, text whose length in bytes is not a multiple of four gives `.Err(DecodeError.InvalidLength(position))`, where `position` is that length. In a `NoPad` encoding, text whose length in bytes leaves remainder one when divided by four gives the same error.
3. r[std-encoding.base64.decode.padding] In a padded encoding, the **padding** is the last one or two characters of the text, when they are `=`. Any other `=` gives `.Err(DecodeError.InvalidPadding(position))`.
4. r[std-encoding.base64.decode.no-pad] In a `NoPad` encoding, every `=` gives `.Err(DecodeError.InvalidPadding(position))`, the first one at its position.
5. r[std-encoding.base64.decode.character] A character that is neither in the alphabet of `self` nor `=` gives `.Err(DecodeError.InvalidCharacter(position))`. The symbols of the other alphabet are such characters.
6. r[std-encoding.base64.decode.pad-bits] When the text ends in a symbol that has pad bits, and the character before any padding is in the alphabet, that symbol's pad bits must be zero. Otherwise the result is `.Err(DecodeError.InvalidPadding(position))` at that symbol.
7. r[std-encoding.base64.decode.order] The length is checked first. Among the other errors, the one at the smallest position is the result.

```text
use std.encoding.{Base64, DecodeError}

fn problem(text: string) -> DecodeError?:
    match Base64.Standard.decode(text):
        .Ok(_) => .None
        .Err(error) => .Some(error)

fn at(text: string) -> string:
    match problem(text):
        .Some(DecodeError.InvalidLength(n)) => "length $n"
        .Some(DecodeError.InvalidCharacter(n)) => "character $n"
        .Some(DecodeError.InvalidPadding(n)) => "padding $n"
        .None => "ok"
```

| Variant | Text | `decode` gives |
| --- | --- | --- |
| `Standard` | `""` | `.Ok([])` |
| `Standard` | `"Zm9vYg=="` | `.Ok` of the bytes of `"foob"` |
| `Standard` | `"Zg="`, `"Zm9v\n"` | `.Err(InvalidLength(3))`, `.Err(InvalidLength(5))` |
| `Url` | `"Zg"` | `.Err(InvalidLength(2))`: the padding is required |
| `Standard` | `"Zm-_"` | `.Err(InvalidCharacter(2))` |
| `UrlNoPad` | `"Zm+/"` | `.Err(InvalidCharacter(2))` |
| `Standard` | `"Zg=a"`, `"Z==="` | `.Err(InvalidPadding(2))`, `.Err(InvalidPadding(1))` |
| `Standard` | `"Zh=="` | `.Err(InvalidPadding(1))`: the pad bits of `h` are not zero |
| `UrlNoPad` | `"Zg=="` | `.Err(InvalidPadding(2))`: no `=` is allowed |
| `UrlNoPad` | `"Zm9vY"` | `.Err(InvalidLength(5))` |
| `UrlNoPad` | `"Zm9"` | `.Err(InvalidPadding(2))`: the pad bits of `9` are not zero |

> **Note.** Decoding skips nothing, so a line break or a space in the text
> is an error.

> **Why.** Canonical decoding gives each list of bytes exactly one text
> per variant, so two texts that decode to the same bytes are equal. Go's
> `Encoding.Strict` and Rust's `base64` crate also reject nonzero pad bits.
> A `NoPad` variant rejects `=` and a padded one requires it, as Go's
> `RawStdEncoding` and `StdEncoding` do.

### Cost

1. r[std-encoding.base64.cost] `encode` computes the length of its text from the length of `bytes` and fills one buffer of exactly that size, and its text is not checked for UTF-8 again. `decode` reads the text in place by byte offset and fills one buffer of exactly the decoded size.

> **Note.** This rule fixes the allocation count, not the representation.

## Decode Errors

A failed decode reports one of three errors, each at a position:

```text
pub enum DecodeError:
    InvalidCharacter(position: usize)
    InvalidLength(position: usize)
    InvalidPadding(position: usize)
```

1. r[std-encoding.error.declared] `std.encoding` declares the enum `DecodeError` with the variants `InvalidCharacter`, `InvalidLength`, and `InvalidPadding`, each with one field `position: usize`.
2. r[std-encoding.error.position-bytes] A `position` is a byte offset into the text, counted in bytes of its UTF-8 encoding from 0, as [`slice`](../lang/10-modules.md#r-module.string.byte-offsets) counts.
3. r[std-encoding.error.traits] `DecodeError` implements `Eq`, `Debug`, and `Display`.

> **Note.** Every character before an `InvalidCharacter` or
> `InvalidPadding` position is ASCII, so that `position` is also the
> character's index.

See also: [Digest](digest.md), [Num](num.md#integer-parsing), [Strings](../lang/04-type-system.md#strings).
