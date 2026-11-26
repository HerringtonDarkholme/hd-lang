# AES

Status: standard library specification draft.

`std.crypto.aes` supplies the AES block cipher, as
[FIPS 197](https://csrc.nist.gov/pubs/fips/197/final) defines it.

```text
use std.crypto.aes.{Aes, AesError}

fn round_trip(key: List[u8], block: List[u8]) -> Result[List[u8], AesError]:
    cipher := Aes::new(key)?
    .Ok(cipher.decrypt_block(cipher.encrypt_block(block)))
```

1. r[std-crypto-aes.import] `std.crypto.aes` declares `Aes` and `AesError`, neither a prelude name.
2. r[std-crypto-aes.type] `Aes` is a struct with private fields containing an immutable expanded key.
3. r[std-crypto-aes.new] `Aes::new(key: List[u8]) -> Result[Aes, AesError]` accepts exactly 16, 24, or 32 key bytes.
4. r[std-crypto-aes.key-error] `AesError` implements `Error`, `Debug`, and `Eq`, with variant `InvalidKeyLength(length: usize)`.
5. r[std-crypto-aes.key-rejected] Any other key length returns `.Err(.InvalidKeyLength(key.len()))`.
6. r[std-crypto-aes.block-size] `block_size(&self) -> usize` returns 16.
7. r[std-crypto-aes.encrypt] `encrypt_block(&self, block: List[u8]) -> List[u8]` returns the AES encryption of one 16-byte block.
8. r[std-crypto-aes.decrypt] `decrypt_block(&self, block: List[u8]) -> List[u8]` returns the AES decryption of one 16-byte block.
9. r[std-crypto-aes.block-length] `encrypt_block` and `decrypt_block` require exactly 16 input bytes; any other length panics. Panic: `explicit-panic`.
10. r[std-crypto-aes.algorithm] Key lengths select AES-128, AES-192, or AES-256, using FIPS 197's key expansion and round transformations.
11. r[std-crypto-aes.immutable] `encrypt_block` and `decrypt_block` leave the key and input list unchanged.
12. r[std-crypto-aes.constant-time] Branches and memory addresses must not depend on key or block byte values; public key lengths, block lengths, and round counters may determine them.

13. r[std-crypto-aes.encrypt-into] `encrypt_block_into(&self, block: &mut List[u8], offset: usize) -> void` encrypts the 16 bytes beginning at `offset` in place.
14. r[std-crypto-aes.into-bounds] An offset beyond the list, or fewer than 16 remaining bytes, panics. Panic: `explicit-panic`.
15. r[std-crypto-aes.into-preserves] In-place encryption preserves the expanded key, list length, and bytes outside that 16-byte range.

> **Why.** Key construction returns an error, as Go's `NewCipher` does.
> Block operations validate a caller contract; exact 16-byte values match
> RustCrypto's block shape, represented here by runtime-checked lists.

> **Note.** A block cipher processes blocks only; authenticated encryption
> is a separate mode over it.

See also: [Crypto](crypto.md), [SHA-512](crypto-sha512.md).
