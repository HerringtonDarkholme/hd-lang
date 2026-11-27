# `std.encoding.base64`

Generated from `hd doc std.encoding.base64`. Run `hd doc std.encoding.base64.NAME` in a package for one item's full text.

## `Base64`

```text
pub enum Base64
```

A base64 encoding: the standard or URL-safe alphabet, with or without `=` padding.

- `pub fn encode(self, bytes: List[u8]) -> string`: The bytes as text in this encoding.
- `pub fn decode(self, text: string) -> Result[List[u8], DecodeError]`: Reads text in this encoding into bytes, or returns the first error.
