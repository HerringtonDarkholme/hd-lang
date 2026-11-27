# `std.crypto.sha512`

Generated from `hd doc std.crypto.sha512`. Run `hd doc std.crypto.sha512.NAME` in a package for one item's full text.

## `sha512`

```text
pub fn sha512(bytes: List[u8]) -> Digest
```

The 64-byte SHA-512 digest of bytes, as FIPS 180-4 defines it.

## `Sha512`

```text
pub struct Sha512
```

A SHA-512 computation that accepts its message in chunks.

- `pub fn new() -> *mut Sha512`: Returns a fresh SHA-512 computation.
- `pub fn hmac(key: List[u8]) -> *mut Sha512`: Returns a fresh HMAC-SHA512 computation under key.
- `pub fn update(*mut self, bytes: List[u8]) -> *mut Sha512`: Adds bytes to the message and returns the same hasher.
- `pub fn finish(self) -> Digest`: Returns the digest so far, leaving the hasher available for more input.
