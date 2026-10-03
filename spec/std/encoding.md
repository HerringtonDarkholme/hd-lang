# Encoding

Status: standard library specification draft.

This chapter defines `std.encoding`, which `lib/std` writes in ordinary hd
over the language tier:

- hex text for bytes: `hex_encode` and `hex_decode`;
- base64 text for bytes, with the standard alphabet of
  [RFC 4648](https://www.rfc-editor.org/rfc/rfc4648): `base64_encode` and
  `base64_decode`;
- `DecodeError`, the error that both decoders return.

Nothing in it is language tier. Bytes are a `List[u8]`. The bytes of a
string come from the language-tier `bytes` method
([`module.string.bytes`](../lang/10-modules.md#r-module.string.bytes)), as
in `text.bytes().collect()`.

1. r[std-encoding.import] `std.encoding` declares `DecodeError`, `hex_encode`, `hex_decode`, `base64_encode`, and `base64_decode`. None is a prelude name; code imports them, as in `use std.encoding.hex_encode`.
2. r[std-encoding.no-panic] None of these functions panics.

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
3. r[std-encoding.hex.decode.length] Text whose length in characters is odd gives `.Err(DecodeError.InvalidLength(position))`, where `position` is that length.
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

Base64 text writes each group of three bytes as four symbols:

```text
use std.encoding.{base64_decode, base64_encode, DecodeError}

fn token(bytes: List[u8]) -> string:
    base64_encode(bytes)  # "Zm9vYg==" for the bytes of "foob"

fn payload(text: string) -> Result[List[u8], DecodeError]:
    base64_decode(text)
```

| Rule | Function | Result |
| --- | --- | --- |
| r[std-encoding.base64.encode] `base64_encode` | `pub fn base64_encode(bytes: List[u8]) -> string` | the base64 text of `bytes` |
| r[std-encoding.base64.decode] `base64_decode` | `pub fn base64_decode(text: string) -> Result[List[u8], DecodeError]` | `.Ok` of the bytes that `text` writes, or `.Err` of the first error |

The base64 alphabet is the standard one of RFC 4648, section 4:

| Values | Symbols |
| --- | --- |
| 0 to 25 | `A` to `Z` |
| 26 to 51 | `a` to `z` |
| 52 to 61 | `0` to `9` |
| 62 | `+` |
| 63 | `/` |

### Encoding

1. r[std-encoding.base64.encode.groups] `base64_encode` writes each group of three bytes, in order, as four symbols of six bits each, the high bits first.
2. r[std-encoding.base64.encode.padding] A final group of one byte gives two symbols and `==`. A final group of two bytes gives three symbols and `=`.
3. r[std-encoding.base64.encode.pad-bits] The low bits of the last symbol that encode no byte, the **pad bits**, are zero.

### Decoding

`base64_decode` accepts exactly the texts that `base64_encode` returns.

1. r[std-encoding.base64.decode.value] Accepted text gives `.Ok` of the bytes that `base64_encode` turns into that text.
2. r[std-encoding.base64.decode.length] Text whose length in characters is not a multiple of four gives `.Err(DecodeError.InvalidLength(position))`, where `position` is that length.
3. r[std-encoding.base64.decode.padding] The **padding** is the last one or two characters of the text, when they are `=`. Any other `=` gives `.Err(DecodeError.InvalidPadding(position))`.
4. r[std-encoding.base64.decode.character] A character that is neither in the alphabet nor `=` gives `.Err(DecodeError.InvalidCharacter(position))`.
5. r[std-encoding.base64.decode.pad-bits] When the text has padding and the character before it is in the alphabet, that symbol's pad bits must be zero. Otherwise the result is `.Err(DecodeError.InvalidPadding(position))` at that symbol.
6. r[std-encoding.base64.decode.order] The length is checked first. Among the other errors, the one at the smallest position is the result.

| Text | `base64_decode` gives |
| --- | --- |
| `""` | `.Ok([])` |
| `"Zm9vYg=="` | `.Ok` of the bytes of `"foob"` |
| `"Zg="`, `"Zm9v\n"` | `.Err(InvalidLength(3))`, `.Err(InvalidLength(5))` |
| `"Zm-_"` | `.Err(InvalidCharacter(2))` |
| `"Zg=a"`, `"Z==="` | `.Err(InvalidPadding(2))`, `.Err(InvalidPadding(1))` |
| `"Zh=="` | `.Err(InvalidPadding(1))`: the pad bits of `h` are not zero |

> **Note.** Decoding skips nothing, so a line break or a space in the text
> is an error. The URL-safe alphabet's `-` and `_` are not in the alphabet.

> **Why.** Canonical decoding gives each list of bytes exactly one text,
> so two texts that decode to the same bytes are equal. Go's
> `Encoding.Strict` and Rust's `base64` crate also reject nonzero pad bits.

## Decode Errors

A failed decode reports one of three errors, each at a position:

```text
pub enum DecodeError:
    InvalidCharacter(position: usize)
    InvalidLength(position: usize)
    InvalidPadding(position: usize)
```

1. r[std-encoding.error.declared] `std.encoding` declares the enum `DecodeError` with the variants `InvalidCharacter`, `InvalidLength`, and `InvalidPadding`, each with one field `position: usize`.
2. r[std-encoding.error.position] A `position` is an index into the text, counted in characters from 0.
3. r[std-encoding.error.traits] `DecodeError` implements `Eq`, `Debug`, and `Display`.

> **Note.** Every character before an `InvalidCharacter` or
> `InvalidPadding` position is ASCII, so that `position` is also the
> character's byte offset.

See also: [Digest](digest.md), [Num](num.md#integer-parsing), [Strings](../lang/04-type-system.md#strings).
