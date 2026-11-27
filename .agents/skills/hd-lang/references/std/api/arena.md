# `std.arena`

Generated from `hd doc std.arena`. Run `hd doc std.arena.NAME` in a package for one item's full text.

## `Arena`

```text
pub struct Arena
```

Values of any types that live and die together: each `alloc` result

- `pub fn new() -> Arena`: An empty arena; its first chunk is taken by the first `alloc`.
- `pub fn alloc[T < ?Copy](&self, val: T) -> &mut T`: Moves `val` into the arena and lends it writable while the arena is
- `pub fn reset(&mut self) -> void`: Drops every value, newest first, and frees every chunk but the
