# Digest

Status: standard library specification draft.

This chapter defines `std.digest`, which `lib/std` writes in ordinary hd
over the language tier:

- `Digest`, the result of a hash, with constant-time equality;
- `sha256`, the SHA-256 digest of bytes.

Nothing in it is language tier. The 32-bit word arithmetic uses the
wrapping methods of [Num](num.md#wrapping-arithmetic) and the shifts of
[Shifts](../lang/05-expressions.md#shifts).

1. r[std-digest.import] `std.digest` declares `Digest` and `sha256`. Neither is a prelude name; code imports them, as in `use std.digest.sha256`.

```text
use std.digest.sha256

fn digest_of(text: string) -> string:
    sha256(text.bytes().collect()).to_hex()
```

## Digest

A `Digest` holds the bytes of a hash result. It compares in constant time,
as Go's `hmac.Equal` does, so checking a received signature does not
reveal how many leading bytes were right.

```text
pub data Digest:
    pub bytes: List[u8]

impl Digest:
    pub fn to_hex(self) -> string

impl Eq for Digest
```

1. r[std-digest.digest] `Digest` is a data type with one public field, `bytes: List[u8]`. Code may build one from bytes it received, as `Digest { bytes: hex_decode(text)? }`.
2. r[std-digest.digest.to-hex] `to_hex` returns the bytes as lowercase hex text, as [`hex_encode`](encoding.md#r-std-encoding.hex.encode) does.
3. r[std-digest.digest.eq] Two digests are equal when their byte lists have the same length and the same bytes.
4. r[std-digest.digest.eq.constant-time] When the lengths are equal, `==` reads every byte pair, and neither its control flow nor its memory accesses depend on the byte values.
5. r[std-digest.digest.eq.length] Digests of different lengths are unequal, and the comparison may return before it reads any byte.
6. r[std-digest.digest.eq.scope] The property covers `==` on two `Digest` values only. Comparing their `bytes` lists or `to_hex` texts is not constant time.

```text
use std.digest.{Digest, sha256}
use std.encoding.hex_decode

fn signature_matches(body: List[u8], sent_hex: string) -> bool:
    match hex_decode(sent_hex):
        .Ok(sent) => sha256(body) == Digest { bytes: sent }
        .Err(_) => false
```

> **Why.** A `List[u8]` comparison stops at the first differing byte. An
> attacker who times many forged signatures learns the right bytes one at
> a time. The length is not secret: a SHA-256 digest is always 32 bytes.

## SHA-256

`sha256` hashes bytes with SHA-256:

```text
use std.digest.sha256

fn checksum(text: string) -> string:
    sha256(text.bytes().collect()).to_hex()

fn same_content(left: List[u8], right: List[u8]) -> bool:
    sha256(left) == sha256(right)
```

| Rule | Function | Result |
| --- | --- | --- |
| r[std-digest.sha256] `sha256` | `pub fn sha256(bytes: List[u8]) -> Digest` | the SHA-256 digest of `bytes` |

1. r[std-digest.sha256.fips] The digest is SHA-256 as FIPS 180-4 defines it, of the message that `bytes` holds in order.
2. r[std-digest.sha256.length] The result's `bytes` holds 32 bytes: the eight words of the final hash value in order, each written high byte first.
3. r[std-digest.sha256.hex] So `sha256(bytes).to_hex()` returns 64 lowercase hex digits.
4. r[std-digest.no-panic] `sha256`, `to_hex` and `==` on digests do not panic.

| Message | `to_hex` gives |
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
