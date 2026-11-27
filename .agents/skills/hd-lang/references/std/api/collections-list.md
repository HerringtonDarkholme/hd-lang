# `std.collections.list`

Generated from `hd doc std.collections.list`. Run `hd doc std.collections.list.NAME` in a package for one item's full text.

## `before`

```text
pub fn before[T < Ord](left: T, right: T) -> bool
```

## `counts`

```text
pub fn counts[T < Eq & Hash](items: List[T]) -> Map[T, usize]
```

How many times each element occurs.

## `ListDrain`

```text
pub struct ListDrain[T, 'a]
```

A borrowed range drain that restores the unselected tail on drop.

- `pub fn rev(self) -> Rev[Self, T]`: Reverses the remaining sequence without advancing or buffering it.

## `list_reserve`

```text
pub fn list_reserve[T](items: &mut List[T], additional: usize) -> void
```

## `List`

- `pub fn with_capacity(capacity: usize) -> List[T]`: Returns an empty list with room for at least `capacity` elements.
- `pub fn reserve(&mut self, additional: usize) -> void`: Reserves room for `additional` elements without changing the length.
- `pub fn truncate(&mut self, len: usize) -> void`: Removes elements after `len`; a greater length has no effect.
- `pub fn split_off(&mut self, at: usize) -> List[T]`: Splits off the suffix starting at `at` into a new list.
- `pub fn append(&mut self, other: &mut List[T]) -> void`: Appends every element of `other`, leaving `other` empty.
- `pub fn drain['a](&'a mut self, range: Range[usize]) -> ListDrain[T, 'a]`: Borrows this list and yields the selected range, restoring the tail on drop.
- `pub fn resize(&mut self, len: usize, value: T) -> void`: Changes the length to `len`, copying `value` into new elements.
- `pub fn fill(&mut self, value: T) -> void`: Replaces every element with a copy of `value`.
- `pub fn reverse(&mut self) -> void`: Reverses the elements in place.
- `pub fn slice(&self, start: usize, end: usize) -> List[T]`: Returns a new list containing elements from `start` up to `end`.
- `pub fn pop(&mut self) -> T?`: Removes and returns the last item, or `.None` for an empty list.
- `pub fn insert(&mut self, index: usize, value: T) -> void`: Puts `value` at `index` and moves the later items up one place; `index == len()` appends.
- `pub fn remove(&mut self, index: usize) -> T`: Removes and returns the item at `index`, moving the later items down one place.
- `pub fn clear(&mut self) -> void`: Removes every item, so `len()` is 0 after it.
- `pub fn map[U, F < ?Copy & FnMut(&T) -> U](&self, mut transform: F) -> List[U]`: Returns a new list with `transform` applied to each element, in order.
- `pub fn filter[F < ?Copy & FnMut(&T) -> bool](&self, mut keep: F) -> List[T]`: Returns a new list of the elements for which `keep` is true, in list order.
- `pub fn get(&self, index: usize) -> T?`: Returns `.Some` of a copy of the element at `index`, or `.None` when `index` is not less than `len()`. It never panics.
- `pub fn is_empty(&self) -> bool`: True exactly when the list has no elements.
- `pub fn take(&self, count: usize) -> List[T]`: Returns a new list of the first `count` elements, or of every element when `count` is at least `len()`.
- `pub fn extend(&mut self, other: List[T]) -> void`: Appends each element of `other` to the list, in order, as `push` does.
- `pub fn first(&self) -> T?`: Returns a copy of the first element in `.Some`, or `.None` for an empty list.
- `pub fn last(&self) -> T?`: Returns a copy of the last element in `.Some`, or `.None` for an empty list.
- `pub fn reversed(&self) -> List[T]`: Returns a new list of the elements in reverse order.
- `pub fn sort_by[F < ?Copy & FnMut(&T, &T) -> Ordering](&mut self, compare: F) -> void`: Sorts the list in ascending order by `compare`, in place. The sort is stable.
- `pub fn sort_by_key[K < Ord, F < ?Copy & FnMut(&T) -> K](&mut self, key: F) -> void`: Sorts the list in ascending order of `key(element)` by `Ord`, in place, calling `key` once per element. The sort is stable.
- `pub fn retain[F < ?Copy & FnMut(&T) -> bool](&mut self, mut keep: F) -> void`: Keeps the elements for which `keep` is true, in list order, in place; `keep` runs once per element, in order.
- `pub fn swap(&mut self, a: usize, b: usize) -> void`: Exchanges the elements at `a` and `b`; either index out of range panics with `index-out-of-bounds`.
- `pub fn chunks(&self, size: usize) -> List[ListView[T]]`: Returns read-only views in consecutive pieces of `size` elements; only the last may be shorter.
- `pub fn zip[U](&self, other: List[U]) -> List[(T, U)]`: Returns a new list of pairs `(self[i], other[i])` for each index `i` of both lists, stopping at the shorter one.
- `pub fn sorted_by_key[K < Ord, F < ?Copy & FnMut(&T) -> K](&self, mut key: F) -> List[T]`: Returns the elements in ascending order of `key(element)` by `Ord`, calling `key` once per element.
- `pub fn group_by[K < Eq & Hash, F < ?Copy & FnMut(&T) -> K]( &self, mut key: F, ) -> Map[K, List[T]]`: Groups the elements by `key(element)`, each group in list order.
- `pub fn partition[F < ?Copy & FnMut(&T) -> bool](&self, mut keep: F) -> (List[T], List[T])`: Splits the list into the elements for which `keep` is true and the rest, each in list order.
- `pub fn any[F < ?Copy & FnMut(&T) -> bool](&self, mut test: F) -> bool`: True when `test` is true for at least one element, stopping at the first.
- `pub fn all[F < ?Copy & FnMut(&T) -> bool](&self, mut test: F) -> bool`: True when `test` is true for every element, stopping at the first false.
- `pub fn find[F < ?Copy & FnMut(&T) -> bool](&self, mut test: F) -> T?`: Returns a copy of the first element for which `test` is true, in `.Some`, or `.None`.
- `pub fn flat_map[U, I < Iterable[U], F < ?Copy & FnMut(&T) -> I]( &self, mut transform: F, ) -> List[U]`: Applies `transform` to each element and concatenates the results, in order.
- `pub fn windows(&self, size: usize) -> List[ListView[T]]`: Returns read-only views of every run of `size` consecutive elements, each starting one index later.
- `pub fn enumerate(&self) -> Enumerate[ListIter[T], T]`: A lazy snapshot of `(index, item)` pairs, starting at zero.
- `pub fn view(&self, start: usize, end: usize) -> ListView[T]`: A read-only window on the elements from `start` up to `end`, with no copy.
- `pub fn span['a](&'a self) -> Span[T, 'a]`: Returns a read-only live view of this list. The span cannot outlive the list.
- `pub fn span_mut['a](&'a mut self) -> SpanMut[T, 'a]`: Returns a mutable live view of this list. The span cannot outlive the list.
- `pub fn sorted_by[F < ?Copy & FnMut(&T, &T) -> Ordering](&self, compare: F) -> List[T]`: Returns a new list in ascending order by `compare`. The sort is stable.
- `pub fn contains(&self, value: &T) -> bool`: True exactly when the list has an element equal to `value`.
- `pub fn index_of(&self, value: T) -> usize?`: Returns the index of the first element equal to `value` by `Eq`, in `.Some`, or `.None`.
- `pub fn dedup(&mut self) -> void`: Removes consecutive equal elements in place, keeping the first in each run.
- `pub fn sum(&self) -> T`: Returns the sum of the elements, left to right.
- `pub fn binary_search(&self, value: &T) -> Result[usize, usize]`: Returns a matching index, or the insertion position when absent.
- `pub fn sort(&mut self) -> void`: Sorts the list in ascending order by `Ord`, in place. The sort is stable.
- `pub fn min(&self) -> T?`: Returns a copy of the first smallest element by `Ord` in `.Some`, or `.None` for an empty list.
- `pub fn max(&self) -> T?`: Returns a copy of the last largest element by `Ord` in `.Some`, or `.None` for an empty list.
- `pub fn sorted(&self) -> List[T]`: Returns the elements in ascending order by `Ord`.
- `pub fn unique(&self) -> List[T]`: Returns the list with each element after its first occurrence dropped, in list order.
- `pub fn min_by_key[K < Ord, F < ?Copy & FnMut(&T) -> K](&self, mut key: F) -> T?`: Returns a copy of the first element with the smallest `key` by `Ord` in `.Some`, or `.None` for an empty list.
- `pub fn max_by_key[K < Ord, F < ?Copy & FnMut(&T) -> K](&self, mut key: F) -> T?`: Returns a copy of the last element with the largest `key` by `Ord` in `.Some`, or `.None` for an empty list.
- `pub fn join(&self, separator: string) -> string`: Joins each element's Display text with `separator` between elements.
