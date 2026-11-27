# `std.encoding.hex`

Generated from `hd doc std.encoding.hex`. Run `hd doc std.encoding.hex.NAME` in a package for one item's full text.

## `hex_encode`

```text
pub fn hex_encode(bytes: List[u8]) -> string
```

The bytes as lowercase hex text, two digits per byte.

## `hex_decode`

```text
pub fn hex_decode(text: string) -> Result[List[u8], DecodeError]
```

Reads hex text into bytes, or returns the first error.
