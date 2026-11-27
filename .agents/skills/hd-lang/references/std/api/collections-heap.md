# `std.collections.heap`

Generated from `hd doc std.collections.heap`. Run `hd doc std.collections.heap.NAME` in a package for one item's full text.

## `Heap`

```text
pub struct Heap[T < Ord]
```

A max-heap: `pop` and `peek` see a largest element.

- `pub fn new() -> Heap[T]`: Returns an empty heap.
- `pub fn len(&self) -> usize`: Returns the number of elements.
- `pub fn is_empty(&self) -> bool`: True exactly when the heap has no elements.
- `pub fn push(&mut self, value: T) -> void`: Adds `value` to the heap.
- `pub fn pop(&mut self) -> T?`: Removes a largest element by `Ord` and returns it in `.Some`. On an empty heap, it returns `.None`.
- `pub fn peek(&self) -> T?`: Returns a copy of a largest element without removing it, or `.None` when the heap is empty.
- `pub fn to_sorted_list(&self) -> List[T]`: Returns a new list of every element in ascending order by `Ord`, and leaves the heap unchanged.
