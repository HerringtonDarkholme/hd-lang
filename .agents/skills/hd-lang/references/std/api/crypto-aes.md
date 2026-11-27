# `std.crypto.aes`

Generated from `hd doc std.crypto.aes`. Run `hd doc std.crypto.aes.NAME` in a package for one item's full text.

## `AesError`

```text
pub enum AesError
```

Why an AES key was rejected.

## `Aes`

```text
pub struct Aes
```

An immutable expanded AES key for encryption and decryption.

- `pub fn new(key: List[u8]) -> Result[Aes, AesError]`: Expands a 16-, 24-, or 32-byte AES key.
- `pub fn block_size(&self) -> usize`: AES always processes 16-byte blocks.
- `pub fn encrypt_block(&self, block: List[u8]) -> List[u8]`: Encrypts one block, returning a new 16-byte list.
- `pub fn encrypt_block_into(&self, block: &mut List[u8], offset: usize) -> void`: Encrypts sixteen bytes in a caller buffer, starting at offset.
- `pub fn decrypt_block(&self, block: List[u8]) -> List[u8]`: Decrypts one block, returning a new 16-byte list.
