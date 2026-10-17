# Digest

Status: standard library specification draft.

This chapter defines `std.digest`, which `lib/std` writes in ordinary hd
over the language tier:

- `Digest`, the result of a hash, with constant-time equality;
- `Sha256`, a SHA-256 computation that takes its input in pieces;
- `Sha256::hmac`, the same hasher keyed for HMAC-SHA256;
- `sha256`, the SHA-256 digest of bytes in one call.

Nothing in it is language tier. The 32-bit word arithmetic uses the
wrapping methods of [Num](num.md#wrapping-arithmetic) and the shifts of
[Shifts](../lang/05-expressions.md#shifts).

1. r[std-digest.import] `std.digest` declares `Digest`, `Sha256` and `sha256`. None is a prelude name; code imports them, as in `use std.digest.sha256`.

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
4. r[std-digest.sha256.oneshot] `sha256(bytes)` returns the same digest as `Sha256::new().update(bytes).finish()`, and does not copy `bytes`.
5. r[std-digest.no-panic] `sha256`, `Sha256::new`, `Sha256::hmac`, `update`, `finish`, `to_hex` and `==` on digests do not panic.

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

## Streaming

`Sha256` hashes input that arrives in pieces, as Go's `hash.Hash` and
Rust's `Digest` do: `new`, then `update` for each piece, then `finish`.
A reader of a large file or a socket feeds each chunk to `update` and
never holds the whole message.

```text
pub data Sha256: ...

impl Sha256:
    pub fn new() -> mut Sha256
    pub fn update(mut self, bytes: List[u8]) -> mut Sha256
    pub fn finish(self) -> Digest
```

```text
use std.digest.{Digest, Sha256}

fn digest_of_chunks(chunks: List[List[u8]]) -> Digest:
    let hasher: mut Sha256 = Sha256::new()
    for chunk in chunks:
        hasher.update(chunk)
    hasher.finish()
```

1. r[std-digest.sha256.decl] `Sha256` is a data type with private fields. Code cannot build one except with `Sha256::new`.
2. r[std-digest.sha256.new] `Sha256::new() -> mut Sha256` returns a hasher that has seen no bytes.
3. r[std-digest.sha256.update] `h.update(bytes: List[u8]) -> mut Sha256` adds `bytes` to the end of the message and returns `h`, so calls chain; it takes `mut self`. The message is the bytes of all calls in call order, so how it is split across calls does not change the digest.
4. r[std-digest.sha256.finish] `h.finish() -> Digest` returns the digest of the message added so far, as [`sha256`](#r-std-digest.sha256) defines it. It takes `self`, so it leaves `h` as it was: `h` takes more `update` calls, and a second `finish` returns the same digest.
5. r[std-digest.sha256.memory] `Sha256` keeps at most one partial block of the message, fewer than 64 bytes. `update` hashes each full block as it arrives, and `finish` pads only the last block.

> **Why.** `sha256` needs the whole message in memory. Streaming lets a
> script hash a file larger than memory. Keeping the hasher usable after
> `finish` is what Go's `Sum` does.

## HMAC

`Sha256::hmac` keys the hasher, as BLAKE3's `Hasher::new_keyed` does.
Webhooks and signed tokens carry an HMAC-SHA256 of the body; the receiver
computes the same MAC under the shared secret and compares.

```text
impl Sha256:
    pub fn hmac(key: List[u8]) -> mut Sha256
```

```text
use std.digest.{Digest, Sha256}
use std.encoding.hex_decode

fn signature_ok(secret: string, body: string, sent_hex: string) -> bool:
    match hex_decode(sent_hex):
        .Ok(sent) =>
            expected := Sha256::hmac(secret.bytes().collect()).update(body.bytes().collect()).finish()
            expected == Digest { bytes: sent }
        .Err(_) => false
```

1. r[std-digest.hmac] `Sha256::hmac(key: List[u8]) -> mut Sha256` returns a hasher that has seen no message bytes. Its `finish` returns HMAC-SHA256 of the message under `key`, as RFC 2104 defines it with SHA-256 as the hash.
2. r[std-digest.hmac.key] A key longer than 64 bytes is replaced by its SHA-256 digest first; a key of up to 64 bytes is used as given. The result is padded with zero bytes to 64 bytes. An empty key is allowed.
3. r[std-digest.hmac.finish] `update`, and `finish` leaving the hasher unchanged, behave as for a plain hasher ([`update`](#r-std-digest.sha256.update), [`finish`](#r-std-digest.sha256.finish)): the split of the message across calls does not change the MAC.
4. r[std-digest.hmac.result] The result is a `Digest` of 32 bytes. Checking a received MAC is `expected == sent`, constant time by [`==`](#r-std-digest.digest.eq.constant-time).
5. r[std-digest.hmac.vectors] The MACs of RFC 4231 test cases 1 to 7 are the values that RFC lists for HMAC-SHA-256 (case 5 in full, not truncated).

> **Cost.** A keyed hasher compresses three more 64-byte blocks than a
> plain one: the inner key block, the outer key block, and the block that
> holds the inner digest.

> **Why.** Hashing `key || message` is not a MAC: an attacker who sees
> the digest can extend it. HMAC's two nested hashes avoid that.

See also: [Encoding](encoding.md), [Hash](hash.md).
