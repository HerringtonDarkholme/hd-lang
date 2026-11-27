# `std.sync`

Generated from `hd doc std.sync`. Run `hd doc std.sync.NAME` in a package for one item's full text.

## `Arc`

```text
pub struct Arc[T < ?Copy]
```

An owner of a `T` that several threads may hold at once. A clone shares the value and adds to its count; the drop of the last one drops the value.

- `pub fn new(value: T) -> Arc[T]`: A new `Arc` that owns `value`, with a strong count of one.
- `pub fn clone(&self) -> Arc[T]`: Another `Arc` that owns the same value.
- `pub fn get(&self) -> &T`: The value, lent for as long as this `Arc` is.
- `pub fn strong_count(this: &Arc[T]) -> usize`: How many `Arc`s own the value; another thread may change it at any time.
- `pub fn weak_count(this: &Arc[T]) -> usize`: How many `Weak`s refer to the value; another thread may change it at any time.
- `pub fn ptr_eq(a: &Arc[T], b: &Arc[T]) -> bool`: Whether `a` and `b` own the same value.
- `pub fn downgrade(this: &Arc[T]) -> Weak[T]`: A `Weak` to the value.
- `pub fn get_mut(this: &mut Arc[T]) -> (&mut T)?`: The value to change, when no other `Arc` or `Weak` holds it.
- `pub fn try_unwrap(this: Arc[T]) -> Result[T, Arc[T]]`: The value, when this is the only `Arc` that owns it; else `.Err` gives the `Arc` back.

## `Weak`

```text
pub struct Weak[T < ?Copy]
```

A reference to an `Arc`'s value that does not keep it alive.

- `pub fn upgrade(&self) -> Arc[T]?`: A new `Arc` to the value while an `Arc` still owns it, and `.None` after the value was dropped.
- `pub fn clone(&self) -> Weak[T]`: Another `Weak` to the same value.
