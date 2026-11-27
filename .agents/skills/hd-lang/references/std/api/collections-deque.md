# `std.collections.deque`

Generated from `hd doc std.collections.deque`. Run `hd doc std.collections.deque.NAME` in a package for one item's full text.

## `Deque`

```text
pub struct Deque[T]
```

A double-ended queue: pushes and pops at both ends, in amortized constant time.

- `pub fn new() -> Deque[T]`: Returns an empty deque.
- `pub fn len(&self) -> usize`: Returns the number of elements.
- `pub fn is_empty(&self) -> bool`: True exactly when the deque has no elements.
- `pub fn push_back(&mut self, value: T) -> void`: Adds `value` after the back.
- `pub fn push_front(&mut self, value: T) -> void`: Adds `value` before the front.
- `pub fn pop_front(&mut self) -> T?`: Removes the front element and returns it in `.Some`, or `.None` when the deque is empty.
- `pub fn pop_back(&mut self) -> T?`: Removes the back element and returns it in `.Some`, or `.None` when the deque is empty.
- `pub fn front(&self) -> T?`: Returns a copy of the front element without removing it, or `.None` when the deque is empty.
- `pub fn back(&self) -> T?`: Returns a copy of the back element without removing it, or `.None` when the deque is empty.
- `pub fn get(&self, index: usize) -> T?`: Returns a copy of the element `index` places from the front in `.Some`, so `get(0)` is the front. An `index` that is not less than `len` gives `.None`.

## `DequeIter`

```text
pub struct DequeIter[T]
```

The cursor of a deque's `iter`: its elements front to back.

- `pub fn rev(self) -> Rev[Self, T]`: Reverses the remaining sequence without advancing or buffering it.
