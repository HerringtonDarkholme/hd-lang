# Digest

Status: standard library specification draft.

This chapter defines `std.digest`, which `lib/std` writes in ordinary hd
over the language tier:

- `sha256`, the SHA-256 digest of bytes;
- `sha256_hex`, the same digest as hex text.

Nothing in it is language tier. The 32-bit word arithmetic uses the
wrapping methods of [Num](num.md#wrapping-arithmetic) and the shifts of
[Shifts](../lang/05-expressions.md#shifts).

1. r[std-digest.import] `std.digest` declares `sha256` and `sha256_hex`. Neither is a prelude name; code imports them, as in `use std.digest.sha256`.

```text
use std.digest.sha256_hex

fn digest_of(text: string) -> string:
    sha256_hex(text.bytes().collect())
```

## SHA-256

`sha256` hashes bytes with SHA-256:

```text
use std.digest.{sha256, sha256_hex}

fn checksum(text: string) -> string:
    sha256_hex(text.bytes().collect())

fn same_content(left: List[u8], right: List[u8]) -> bool:
    sha256(left) == sha256(right)
```

| Rule | Function | Result |
| --- | --- | --- |
| r[std-digest.sha256] `sha256` | `pub fn sha256(bytes: List[u8]) -> List[u8]` | the SHA-256 digest of `bytes` |
| r[std-digest.sha256-hex] `sha256_hex` | `pub fn sha256_hex(bytes: List[u8]) -> string` | `hex_encode(sha256(bytes))`, by [`std-encoding.hex.encode`](encoding.md#r-std-encoding.hex.encode) |

1. r[std-digest.sha256.fips] The digest is SHA-256 as FIPS 180-4 defines it, of the message that `bytes` holds in order.
2. r[std-digest.sha256.length] The result holds 32 bytes: the eight words of the final hash value in order, each written high byte first.
3. r[std-digest.sha256-hex.text] So `sha256_hex` returns 64 lowercase hex digits.
4. r[std-digest.no-panic] Neither function panics.

| Message | `sha256_hex` gives |
| --- | --- |
| `""` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `"abc"` | `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad` |

> **Note.** A string's message is its UTF-8 bytes, from the language-tier
> `bytes` method
> ([`module.string.bytes`](../lang/10-modules.md#r-module.string.bytes)).
> So the digest of a text matches the `sha256sum` of a file that holds
> exactly that text.

> **Why.** SHA-256 covers what a script needs: checksums, content
> addresses, and comparison with other tools. MD5 and SHA-1 are broken for
> collisions and are left out.

See also: [Encoding](encoding.md), [Hash](hash.md).
