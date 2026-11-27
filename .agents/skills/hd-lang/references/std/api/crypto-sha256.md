# `std.crypto.sha256`

Generated from `hd doc std.crypto.sha256`. Run `hd doc std.crypto.sha256.NAME` in a package for one item's full text.

## `sha256`

```text
pub fn sha256(bytes: List[u8]) -> Digest
```

The 32-byte SHA-256 digest of `bytes`, as FIPS 180-4 defines it.

## `Sha256`

```text
pub struct Sha256
```

A SHA-256 computation fed in pieces: `update` adds bytes and `finish` returns the digest.

- `pub fn new() -> *mut Sha256`: Returns a hasher that has seen no bytes, a new object that `update` changes.
- `pub fn hmac(key: List[u8]) -> *mut Sha256`: Returns a hasher whose `finish` gives HMAC-SHA256 of the message under `key`.
- `pub fn update(*mut self, bytes: List[u8]) -> *mut Sha256`: Adds `bytes` to the message and returns the hasher, so calls chain.
- `pub fn finish(self) -> Digest`: Returns the digest of every byte added so far, or the HMAC-SHA256 under the key of `hmac`. The hasher is unchanged, so it can take more bytes.
