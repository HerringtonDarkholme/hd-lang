# `std.crypto`

Generated from `hd doc std.crypto`. Run `hd doc std.crypto.NAME` in a package for one item's full text.

## `Digest`

```text
pub struct Digest
```

The result of a hash, as bytes. Compare digests with `==`, which takes the same time wherever they differ.

- `pub fn to_hex(self) -> string`: The digest as lowercase hex text, two digits per byte.
