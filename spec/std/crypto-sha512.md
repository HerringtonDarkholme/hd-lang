# SHA-512

Status: standard library specification draft.

`std.crypto.sha512` computes SHA-512 in ordinary hd, as
[FIPS 180-4](https://csrc.nist.gov/pubs/fips/180-4/upd1/final) defines it.

```text
use std.crypto.sha512.{sha512, Sha512}

fn checksum(message: List[u8]) -> string:
    sha512(message).to_hex()

fn chunks(first: List[u8], second: List[u8]) -> string:
    Sha512::new().update(first).update(second).finish().to_hex()
```

1. r[std-crypto-sha512.import] `std.crypto.sha512` declares `sha512` and `Sha512`, neither a prelude name.
2. r[std-crypto-sha512.hash] `sha512(bytes: List[u8]) -> Digest` returns the SHA-512 digest of `bytes` in order, for messages shorter than 2¹²⁸ bits.
3. r[std-crypto-sha512.result] The result is a [`Digest`](crypto.md#digest) containing 64 bytes, with each final state word written most significant byte first.
4. r[std-crypto-sha512.streaming] `Sha512` is a struct with private fields, representing a streaming SHA-512 computation behind `*mut`.
5. r[std-crypto-sha512.new] `Sha512::new() -> *mut Sha512` returns a fresh computation that has received no message bytes.
6. r[std-crypto-sha512.update] `update(*mut self, bytes: List[u8]) -> *mut Sha512` appends `bytes` to the message and returns the same hasher.
7. r[std-crypto-sha512.chunks] Splitting the message across `update` calls does not change the result.
8. r[std-crypto-sha512.finish] `finish(self) -> Digest` returns the digest so far without changing the hasher, which remains available for further input.
9. r[std-crypto-sha512.oneshot] `sha512(bytes)` returns the same result as `Sha512::new().update(bytes).finish()`.
10. r[std-crypto-sha512.memory] Between calls, the hasher retains fewer than 128 message bytes and fixed-size state independent of total message length.
11. r[std-crypto-sha512.constant-time] Control flow and memory addresses depend only on public message lengths and counters, never on message byte values.

> **Why.** The streaming interface follows SHA-256 and Go's non-destructive
> `Sum`; it hashes large inputs without retaining the complete message.

See also: [Crypto](crypto.md), [Encoding](encoding.md).

## HMAC

`Sha512::hmac` starts a keyed SHA-512 computation, following
[`Sha256::hmac`](crypto.md#hmac).

```text
use std.crypto.sha512.Sha512

fn signature(key: List[u8], message: List[u8]) -> string:
    Sha512::hmac(key).update(message).finish().to_hex()
```

1. r[std-crypto-sha512.hmac] `Sha512::hmac(key: List[u8]) -> *mut Sha512` returns a fresh computation whose `finish` returns HMAC-SHA512 under `key`, following RFC 2104.
2. r[std-crypto-sha512.hmac.key] Keys longer than 128 bytes are first hashed with SHA-512; shorter keys, including empty keys, are zero-padded to 128 bytes.
3. r[std-crypto-sha512.hmac.streaming] Keyed hashers support the same `update` and non-destructive `finish` operations as plain hashers.
4. r[std-crypto-sha512.hmac.result] Each result contains 64 bytes, matching RFC 4231's HMAC-SHA-512 vectors, with case 5 returned in full.

> **Why.** Keeping keyed construction on each hasher avoids a second
> generic API for the same HMAC operation.
