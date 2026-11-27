# `std.hash`

Generated from `hd doc std.hash`. Run `hd doc std.hash.NAME` in a package for one item's full text.

## `Fnv1a`

```text
pub struct Fnv1a
```

A hasher computing 64-bit FNV-1a over the bytes written to it. Its

- `pub fn new() -> Fnv1a`: Returns a fresh hasher value whose state is the FNV offset basis.
- `pub fn finish(self) -> u64`: The FNV-1a hash of the bytes written so far.

## `hash_of`

```text
pub fn hash_of[T < Hash](value: T) -> u64
```

`value`'s hash under a fresh `DefaultHasher`.

## `DefaultHasher`

```text
pub struct DefaultHasher
```

The standard hasher. For one hd toolchain it gives the same digest for the

- `pub fn new() -> DefaultHasher`: Returns a fresh hasher value, with no bytes written.
- `pub fn finish(self) -> u64`: The digest of the bytes written so far.
