# `std.iter`

Generated from `hd doc std.iter`. Run `hd doc std.iter.NAME` in a package for one item's full text.

## `Iterator`

```text
pub trait Iterator[T < ?Copy] < ?Copy
```

A cursor that yields its next item, or None when exhausted.

- `fn filter_map[U < ?Copy, F < ?Copy & FnMut(T) -> U?]( self, transform: F, ) -> FilterMap[Self, T, U, F]`: Lazily yields each present result of `transform`, in source order.
- `fn any[F < ?Copy & FnMut(&T) -> bool](self, mut test: F) -> bool`: True when `test` accepts an item; it stops at the first one.
- `fn all[F < ?Copy & FnMut(&T) -> bool](self, mut test: F) -> bool`: True when `test` accepts every item; it stops at the first one it rejects.
- `fn find[F < ?Copy & FnMut(&T) -> bool](self, mut test: F) -> T?`: The first item that `test` accepts, in `.Some`, or `.None`.
- `fn last(self) -> T?`: The final remaining item, or `.None` when there is none.
- `fn position[F < ?Copy & FnMut(T) -> bool $ R, $ R](&mut self, mut test: F) -> usize? $ R`: The first matching item's remaining index; keeps this cursor after that item.
- `fn for_each[F < ?Copy & FnMut(T) -> void $ R, $ R](self, mut visit: F) -> void $ R`: Calls `visit` once for each remaining item, in order.
- `fn count(self) -> usize`: The number of remaining items; it drains the iterator.
- `fn min(self) -> T? where T < Ord`: The smallest remaining item, the first of equal ones, or `.None`; it drains the iterator.
- `fn max(self) -> T? where T < Ord`: The largest remaining item, the last of equal ones, or `.None`; it drains the iterator.
- `fn fold[A, F < ?Copy & FnMut(A, T) -> A $ R, $ R](self, initial: A, mut step: F) -> A $ R`: Folds the remaining items into `initial`: `step(accumulator, item)` per item, in order.
- `fn collect[C < ?Copy & FromIterator[T]](self) -> C`: A `C` built from the remaining items of `self`; the target is named by the expected type or the type argument, as `List[_]`.

## `ByRef`

```text
pub struct ByRef[I < ?Copy]
```

A cursor that borrows another cursor and shares its progress.

- `pub fn rev[T < ?Copy](self) -> Rev[Self, T] where I < DoubleEndedIterator[T]`: Reverses the remaining sequence without advancing or buffering it.

## `Sum`

```text
pub trait Sum[T < ?Copy]
```

A total built from the items of a cursor.

## `Product`

```text
pub trait Product[T < ?Copy]
```

A product built from the items of a cursor.

## `DynIterator`

```text
pub struct DynIterator[T < ?Copy]
```

A concrete cursor forwarding to an erased iterator reference.

## `FromFn`

```text
pub struct FromFn[T < ?Copy, F < FnMut() -> T? = fn() -> T?]
```

## `from_fn`

```text
pub fn from_fn[T < ?Copy, F < FnMut() -> T?](step: F) -> FromFn[T, F]
```

## `MapIter`

```text
pub struct MapIter[I < ?Copy & Iterator[T], T < ?Copy, U < ?Copy, F < FnMut(T) -> U = fn(T) -> U]
```

- `pub fn rev(self) -> Rev[Self, U]`: Reverses the remaining sequence without advancing or buffering it.

## `FilterMap`

```text
pub struct FilterMap[
```

A lazy cursor yielding the present results of a value-consuming callback.

- `pub fn rev(self) -> Rev[Self, U]`: Reverses the remaining sequence without advancing or buffering it.

## `Filter`

```text
pub struct Filter[I < ?Copy & Iterator[T], T < ?Copy, F < FnMut(&T) -> bool = fn(&T) -> bool]
```

- `pub fn rev(self) -> Rev[Self, T]`: Reverses the remaining sequence without advancing or buffering it.

## `Take`

```text
pub struct Take[I < ?Copy & Iterator[T], T < ?Copy]
```

- `pub fn rev(self) -> Rev[Self, T]`: Reverses the remaining sequence without advancing or buffering it.

## `Enumerate`

```text
pub struct Enumerate[I < ?Copy & Iterator[T], T < ?Copy]
```

- `pub fn rev(self) -> Rev[Self, (usize, T)]`: Reverses the remaining sequence without advancing or buffering it.

## `Skip`

```text
pub struct Skip[I < ?Copy & Iterator[T], T < ?Copy]
```

- `pub fn rev(self) -> Rev[Self, T]`: Reverses the remaining sequence without advancing or buffering it.

## `TakeWhile`

```text
pub struct TakeWhile[I < ?Copy & Iterator[T], T < ?Copy, F < FnMut(&T) -> bool = fn(&T) -> bool]
```

## `Zip`

```text
pub struct Zip[I < ?Copy & Iterator[T], T < ?Copy, U < ?Copy, J < ?Copy & Iterable[U]]
```

- `pub fn rev(self) -> Rev[Self, (T, U)]`: Reverses the remaining sequence without advancing or buffering it.

## `Chain`

```text
pub struct Chain[I < ?Copy & Iterator[T], T < ?Copy, J < ?Copy & Iterable[T]]
```

- `pub fn rev(self) -> Rev[Self, T]`: Reverses the remaining sequence without advancing or buffering it.

## `FlatMap`

```text
pub struct FlatMap[
```

- `pub fn rev(self) -> Rev[Self, U]`: Reverses the remaining sequence without advancing or buffering it.

## `FromIterator`

```text
pub trait FromIterator[T < ?Copy] < ?Copy
```

What `collect` builds: a target that gathers items of `T`.

## `Iterable`

```text
pub trait Iterable[T < ?Copy] < ?Copy
```

What `for` iterates: a type whose `iter` returns its cursor type `IntoIter`, an iterator of its items.

## `ListIter`

```text
pub struct ListIter[T]
```

The cursor of a list's `iter`: its elements in order.

- `pub fn rev(self) -> Rev[Self, T]`: Reverses the remaining sequence without advancing or buffering it.

## `MapEntries`

```text
pub struct MapEntries[K < Eq & Hash, V]
```

The cursor of a map's `iter`: its `(key, value)` entries in iteration order.

## `DoubleEndedIterator`

```text
pub trait DoubleEndedIterator[T < ?Copy] < ?Copy & Iterator[T]
```

A cursor whose front and back share the same remaining sequence.

- `fn rev(self) -> Rev[Self, T]`: Reverses the remaining sequence without advancing or buffering it.

## `ExactSizeIterator`

```text
pub trait ExactSizeIterator[T < ?Copy] < ?Copy & Iterator[T]
```

A cursor with a constant-time exact remaining item count.

## `Rev`

```text
pub struct Rev[I < ?Copy, T < ?Copy]
```

A lazy reversal of a double-ended cursor.

- `pub fn rev(self) -> Rev[Self, T]`: Reverses the remaining sequence without advancing or buffering it.

## `DynDoubleEndedIterator`

```text
pub struct DynDoubleEndedIterator[T < ?Copy]
```

- `pub fn rev(self) -> Rev[Self, T]`: Reverses the remaining sequence without advancing or buffering it.

## `OptionIter`

```text
pub struct OptionIter[T]
```

A cursor yielding an optional's payload once, if present.

- `pub fn rev(self) -> Rev[Self, T]`: Reverses the remaining sequence without advancing or buffering it.
