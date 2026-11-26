# Cipher modes

Status: standard library specification draft.

`std.crypto.cipher` supplies GCM authenticated encryption and CTR streaming
encryption over an immutable block cipher.

```text
use std.crypto.aes.Aes
use std.crypto.cipher.{CipherError, Gcm}

fn round_trip(key: Aes, nonce: List[u8], plain: List[u8]) -> Result[List[u8], CipherError]:
    mode := Gcm::[Aes].new(key)?
    mode.open(nonce, mode.seal(nonce, plain, []), [])
```

1. r[std-crypto-cipher.import] `std.crypto.cipher` declares `Block`, `CipherError`, `Gcm`, and `Ctr`, none a prelude name.
2. r[std-crypto-cipher.block] `Block < ?Copy` declares `block_size(&self) -> usize` and `encrypt_block(&self, bytes: &mut List[u8], offset: usize) -> void`.
3. r[std-crypto-cipher.block-contract] A block implementation encrypts exactly `block_size()` bytes at `offset`, preserving its key, list length, and other bytes.
4. r[std-crypto-cipher.aes] `Aes` implements `Block` using its 16-byte block size and `encrypt_block_into` operation.
5. r[std-crypto-cipher.error] `CipherError` implements `Error`, `Debug`, and `Eq`, with variants `InvalidBlockSize(size: usize)`, `InvalidNonceSize(size: usize)`, `InvalidTagSize(size: usize)`, and `AuthenticationFailed`.
6. r[std-crypto-cipher.gcm] `Gcm[B < ?Copy & Block]` owns its block cipher and implements Galois/Counter Mode from NIST SP 800-38D.
7. r[std-crypto-cipher.gcm.new] `Gcm::[B].new(block: B) -> Result[Gcm[B], CipherError]` selects 12-byte nonces and 16-byte authentication tags.
8. r[std-crypto-cipher.gcm.nonce] `with_nonce_size(block: B, size: usize) -> Result[Gcm[B], CipherError]` selects a positive nonce size and 16-byte tags.
9. r[std-crypto-cipher.gcm.tag] `with_tag_size(block: B, size: usize) -> Result[Gcm[B], CipherError]` selects 12-byte nonces and tags of 12 through 16 bytes.
10. r[std-crypto-cipher.gcm.configuration] Constructors reject block sizes other than 16, zero nonce sizes, and invalid tag sizes with the corresponding `CipherError` carrying the rejected size.
11. r[std-crypto-cipher.gcm.nonce-size] `nonce_size(&self) -> usize` returns the configured nonce length.
12. r[std-crypto-cipher.gcm.overhead] `overhead(&self) -> usize` returns the configured tag length.
13. r[std-crypto-cipher.gcm.seal] `seal(&self, nonce: List[u8], plaintext: List[u8], associated: List[u8]) -> List[u8]` returns ciphertext followed by its authentication tag.
14. r[std-crypto-cipher.gcm.open] `open(&self, nonce: List[u8], sealed: List[u8], associated: List[u8]) -> Result[List[u8], CipherError]` authenticates before releasing plaintext.
15. r[std-crypto-cipher.gcm.rejected] A short message, incorrect tag, or excessive ciphertext length returns `.Err(.AuthenticationFailed)` without decrypting plaintext.
16. r[std-crypto-cipher.gcm.nonce-contract] Both message operations require the configured nonce length; a mismatch panics. Panic: `explicit-panic`.
17. r[std-crypto-cipher.gcm.length] `seal` rejects plaintext exceeding 2^36 − 32 bytes, or an unrepresentable output length, by panicking. Panic: `explicit-panic`.
18. r[std-crypto-cipher.gcm.tag-comparison] Authentication compares every tag byte without a byte-dependent branch or memory address before deciding success.
19. r[std-crypto-cipher.ctr] `Ctr[B < ?Copy & Block]` owns a block cipher and retains counter position and unused keystream bytes across calls.
20. r[std-crypto-cipher.ctr.new] `Ctr::[B].new(block: B, iv: List[u8]) -> *mut Ctr[B]` starts with a private copy of `iv`.
21. r[std-crypto-cipher.ctr.iv] CTR requires a positive block size and an IV of that size; a mismatch panics. Panic: `explicit-panic`.
22. r[std-crypto-cipher.ctr.xor] `xor_key_stream(*mut self, bytes: List[u8]) -> List[u8]` returns input XOR the next keystream bytes, advancing the stream.
23. r[std-crypto-cipher.ctr.counter] CTR encrypts the full-width big-endian counter, then increments it modulo its width after each block.
24. r[std-crypto-cipher.ctr.chunks] Splitting input into calls, including empty calls, produces the same concatenated output as one call.
25. r[std-crypto-cipher.inputs] Mode operations preserve caller input lists, and GCM operations preserve the mode's key and configuration.

> **Why.** The interface follows Go's block, GCM, and CTR contracts,
> using an in-place block operation to reuse storage across blocks.
> GCM accepts non-12-byte nonces through a separate constructor.

> **Note.** Callers must use a distinct nonce for each message under a GCM
> key and a distinct counter sequence under a CTR key.
> CTR supplies encryption only; it supplies no authentication.

See also: [AES](crypto-aes.md), [Crypto](crypto.md),
[NIST SP 800-38D](https://csrc.nist.gov/pubs/sp/800/38/d/final),
[Go cipher](https://pkg.go.dev/crypto/cipher).
