# `std.collections.set`

Generated from `hd doc std.collections.set`. Run `hd doc std.collections.set.NAME` in a package for one item's full text.

## `Set`

```text
pub struct Set[T < Eq & Hash]
```

A set holds distinct elements, in the order they were inserted.

- `pub fn new() -> Set[T]`: Returns an empty set.
- `pub fn union(&self, other: &Set[T]) -> Union[T]`: A lazy snapshot of the distinct elements in either set.
- `pub fn intersection(&self, other: &Set[T]) -> Intersection[T]`: A lazy snapshot of the elements common to both sets.
- `pub fn len(&self) -> usize`: Returns the number of elements.
- `pub fn is_empty(&self) -> bool`: True exactly when the set has no elements.
- `pub fn contains(&self, value: &T) -> bool`: True exactly when the set has an element equal to `value`.
- `pub fn insert(&mut self, value: T) -> bool`: Adds `value` and returns `true` when the set has no element equal to it. Otherwise it changes nothing and returns `false`.
- `pub fn remove(&mut self, value: T) -> bool`: Removes the element equal to `value` and returns `true`. When there is none, it changes nothing and returns `false`.

## `SetIter`

```text
pub struct SetIter[T < Eq & Hash]
```

The cursor of a set's `iter`: its elements in the set's order.

## `Union`

```text
pub struct Union[T < Eq & Hash]
```

The lazy union of two set snapshots.

## `Intersection`

```text
pub struct Intersection[T < Eq & Hash]
```

The lazy intersection of two set snapshots.
