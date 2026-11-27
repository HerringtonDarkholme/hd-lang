# `std.crypto.cipher`

Generated from `hd doc std.crypto.cipher`. Run `hd doc std.crypto.cipher.NAME` in a package for one item's full text.

## `Block`

```text
pub trait Block < ?Copy
```

An immutable block cipher writing into caller-provided storage.

## `CipherError`

```text
pub enum CipherError
```

A rejected GCM configuration, or a message that did not authenticate.

## `Gcm`

```text
pub struct Gcm[B < ?Copy & Block]
```

Galois/Counter Mode with a fixed nonce size and authentication tag size.

- `pub fn new(block: B) -> Result[Gcm[B], CipherError]`: Uses 12-byte nonces and 16-byte tags, as Go's NewGCM does.
- `pub fn with_nonce_size(block: B, size: usize) -> Result[Gcm[B], CipherError]`: Uses the given positive nonce size and 16-byte tags.
- `pub fn with_tag_size(block: B, size: usize) -> Result[Gcm[B], CipherError]`: Uses 12-byte nonces and tags between 12 and 16 bytes.
- `pub fn nonce_size(&self) -> usize`: The nonce size selected at construction.
- `pub fn overhead(&self) -> usize`: The number of authentication bytes appended by seal.
- `pub fn seal(&self, nonce: List[u8], plaintext: List[u8], associated: List[u8]) -> List[u8]`: Encrypts plaintext and appends its authentication tag.
- `pub fn open( &self, nonce: List[u8], sealed: List[u8], associated: List[u8], ) -> Result[List[u8], CipherError]`: Authenticates before allocating or decrypting any plaintext.

## `Ctr`

```text
pub struct Ctr[B < ?Copy & Block]
```

Counter mode, retaining unused keystream bytes across calls.

- `pub fn new(block: B, iv: List[u8]) -> *mut Ctr[B]`: Starts with the given full-width big-endian counter.
- `pub fn xor_key_stream(*mut self, bytes: List[u8]) -> List[u8]`: XORs the next keystream bytes with input and advances the stream.
