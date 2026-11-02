# Collections

Status: standard library specification draft.

This chapter defines the part of `std.collections` that `lib/std` writes
in ordinary hd over the language tier:

- the `List` methods `view` and `chunks`, and the access and building
  methods such as `get`, `take`, and `extend`;
- the `ListView[T]` type returned by `view`, `chunks`, and `windows`;
- the borrowed `Span[T, 'a]` type returned by `span` and `slice`;
- the uniquely owned `UniqList[T]`, which may hold non-copyable elements;
- the uniquely owned `UniqMap[K, V]`, which may hold non-copyable values;
- the `List` helpers, such as `sorted_by_key`, `group_by`, and `windows`,
  and the function `counts`;
- the `Map` methods `get_or_insert`, `contains_key`, `keys`,
  and `values`;
- the set `Set[T]`, the double-ended queue `Deque[T]`, and the max-heap
  `Heap[T]`, which are values as a list is.

The language tier keeps what the compiler knows about a list
([List Indexing](../lang/05-expressions.md#list-indexing),
[Built-In Methods](../lang/10-modules.md#built-in-methods),
[Iterator Snapshots](../lang/06-control-flow.md#iterator-snapshots)):

| Item | Why it stays in the language tier |
| --- | --- |
| `items[i]`, `len`, `iter`, `push`, `pop`, `insert`, `remove_at`, `clear` | built-in indexing and intrinsics over the list's representation |

## UniqList

`UniqList[T]` is the uniquely owned counterpart of `List[T]`. It accepts
any element type and moves its elements when it grows or shifts them:

```text
use std.collections.UniqList

struct Socket:
    id: usize

impl !Copy for Socket

fn pool() -> UniqList[Socket]:
    let mut sockets = UniqList::[Socket].new()
    sockets.push(Socket { id: 1 })
    sockets
```

Its surface is:

| Item | Signature |
| --- | --- |
| construction | `new() -> UniqList[T]`; `with_capacity(capacity: usize) -> UniqList[T]` |
| size | `len(&self) -> usize`; `is_empty(&self) -> bool` |
| append/remove | `push(&mut self, value: T) -> void`; `pop(&mut self) -> T?` |
| indexed change | `insert(&mut self, index: usize, value: T) -> void`; `remove_at(&mut self, index: usize) -> T`; `swap(&mut self, left: usize, right: usize) -> void` |
| shortening | `truncate(&mut self, len: usize) -> void`; `clear(&mut self) -> void` |
| copyable access, `T < Copy` | `get(&self, index: usize) -> T?`; `first(&self) -> T?`; `last(&self) -> T?`; `Index[usize]` with `Out = T` |
| shared access | `get_ref['a](&'a self, index: usize) -> (&'a T)?`; `first_ref['a](&'a self) -> (&'a T)?`; `last_ref['a](&'a self) -> (&'a T)?` |
| mutable access | `get_mut['a](&'a mut self, index: usize) -> (&'a mut T)?`; `first_mut['a](&'a mut self) -> (&'a mut T)?`; `last_mut['a](&'a mut self) -> (&'a mut T)?` |
| borrowing cursor | `iter['a](&'a self) -> UniqListIter[T, 'a]`, whose item is `&'a T` |
| consuming cursor | `Iterable[T]` with `UniqListIntoIter[T]`, whose item is `T`; `FromIterator[T]` |
| copyable conversion, `T < Copy` | `from_list(items: List[T]) -> UniqList[T]`; `to_list(&self) -> List[T]` |
| looking | `find_ref['a](&'a self, test: fn(&T) -> bool) -> (&'a T)?`; `any(&self, test: fn(&T) -> bool) -> bool`; `all(&self, test: fn(&T) -> bool) -> bool` |
| changing | `retain(&mut self, keep: fn(&T) -> bool) -> void`; `sort_by(&mut self, compare: fn(&T, &T) -> Ordering) -> void`; `sort_by_key[K < Ord](&mut self, key: fn(&T) -> K) -> void` |
| ordered, `T < ?Copy & Ord` | `sort(&mut self) -> void` |
| value-looking traits | `Eq` for `T < ?Copy & Eq`; `Debug` for `T < ?Copy & Debug`; `Hash` for `T < ?Copy & Hash` |

1. r[std-collections.uniq-list.type] `std.collections` declares `UniqList[T < ?Copy]` with a private `Buffer[T]` field and `impl[T < ?Copy] !Copy for UniqList[T]`. It is not a prelude name, so code that names it imports it.
2. r[std-collections.uniq-list.new] `new()` returns an empty list. `with_capacity(capacity)` returns an empty list whose first `capacity` pushes need no growth.
3. r[std-collections.uniq-list.len] `len()` returns the number of elements, and `is_empty()` is true exactly when that number is zero.
4. r[std-collections.uniq-list.push] `push(value)` moves `value` after the former last element. `pop()` moves the last element out and returns it in `.Some`, or returns `.None` when the list is empty.
5. r[std-collections.uniq-list.insert] `insert(index, value)` moves `value` to `index` and moves each element formerly at or after `index` one place toward the end. `index` may equal `len()`; a greater index panics. Panic: `index-out-of-bounds`.
6. r[std-collections.uniq-list.remove-at] `remove_at(index)` moves the indexed element out, moves each later element one place toward the start, and returns the removed element. An index outside the list panics. Panic: `index-out-of-bounds`.
7. r[std-collections.uniq-list.swap] `swap(left, right)` exchanges the two elements without copying either one. An index outside the list panics. Panic: `index-out-of-bounds`.
8. r[std-collections.uniq-list.truncate] `truncate(len)` drops every element at `len` and after. A `len` at least the current length has no effect. `clear()` is `truncate(0)`.
9. r[std-collections.uniq-list.get] For `T < Copy`, `get(index)` returns `.Some` of a copy of the indexed element, or `.None` outside the list.
10. r[std-collections.uniq-list.first-last] For `T < Copy`, `first()` and `last()` return `.Some` of a copy of the first or last element, or `.None` when the list is empty.
11. r[std-collections.uniq-list.movers] A change that shifts elements moves each one through `Ptr[T].read` and `Ptr[T].write`. Growth moves every element into a larger `Buffer[T]`, sets the old buffer's length to zero, and releases it holding no elements. No operation tests whether the buffer is uniquely referenced or makes a copy-on-write clone.
12. r[std-collections.uniq-list.get-ref] `get_ref(index)` returns a shared reference to the indexed element in `.Some`, or `.None` outside the list. `first_ref()` and `last_ref()` return the first or last element by shared reference, or `.None` when the list is empty. Each reference lives for the receiver borrow.
13. r[std-collections.uniq-list.get-mut] `get_mut(index)` returns a mutable reference to the indexed element in `.Some`, or `.None` outside the list. `first_mut()` and `last_mut()` return the first or last element by mutable reference, or `.None` when the list is empty. Each reference lives for the mutable receiver borrow.
14. r[std-collections.uniq-list.index] For `T < Copy`, `items[index]` returns a copy and panics when `index` is outside the list. Panic: `index-out-of-bounds`.
15. r[std-collections.uniq-list.iter] `iter()` borrows the list and yields shared references to its elements in order. Its cursor is `UniqListIter[T, 'a]`, with private fields. The cursor and each yielded reference live for the list borrow.
16. r[std-collections.uniq-list.find-ref] `find_ref(test)` returns a shared reference to the first element for which `test` is true in `.Some`, or `.None` when there is none.
17. r[std-collections.uniq-list.any-all] `any(test)` and `all(test)` have the behavior of [`std-collections.helper.any-all`](#r-std-collections.helper.any-all) and [`std-collections.helper.short-circuit`](#r-std-collections.helper.short-circuit), with `fn(&T)` callbacks.
18. r[std-collections.uniq-list.retain] `retain(keep)` drops each rejected element in place and preserves the order of those accepted.
19. r[std-collections.uniq-list.sort] `sort`, `sort_by`, and `sort_by_key` sort in place and are stable. They order as [`std-collections.helper.sort`](#r-std-collections.helper.sort), [`std-collections.helper.sort-by`](#r-std-collections.helper.sort-by), and [`std-collections.helper.sort-by-key`](#r-std-collections.helper.sort-by-key) do.
20. r[std-collections.uniq-list.into-iter] `UniqList[T]` implements `Iterable[T]`. Its private `UniqListIntoIter[T]` cursor owns the list and moves its elements out in order. A `for` loop over a `UniqList` therefore consumes the list and each item.
21. r[std-collections.uniq-list.collect] `UniqList[T]` implements `FromIterator[T]` for every `T < ?Copy`, preserving item order. An explicitly targeted `collect`, including `let items: UniqList[_] = cursor.collect()`, may therefore collect non-copyable items.
22. r[std-collections.uniq-list.conversion] For `T < Copy`, `from_list(items)` copies the list's elements in order into a new `UniqList`, and `to_list()` copies a `UniqList`'s elements in order into a new `List`.
23. r[std-collections.uniq-list.eq] For `T < ?Copy & Eq`, two `UniqList[T]` values are equal exactly when they have the same length and equal elements at every position.
24. r[std-collections.uniq-list.debug] For `T < ?Copy & Debug`, `UniqList[T]` implements `Debug` with the same bracketed, comma-separated form as a `List[T]`.
25. r[std-collections.uniq-list.hash] For `T < ?Copy & Hash`, `UniqList[T]` hashes its length as a `u64`, then each element in order, as `List[T]` does.

This surface has no literal construction.

## UniqMap

`UniqMap[K, V]` is the uniquely owned counterpart of `Map[K, V]`. Keys
stay copyable so lookup can take them by value, while values may be
non-copyable and move into and out of the table.

| Item | Signature |
| --- | --- |
| construction | `new() -> UniqMap[K, V]` |
| size | `len(&self) -> usize`; `is_empty(&self) -> bool` |
| lookup | `contains_key(&self, key: K) -> bool` |
| change | `insert(&mut self, key: K, value: V) -> V?`; `remove(&mut self, key: K) -> V?`; `clear(&mut self) -> void` |
| copyable access, `V < Copy` | `get(&self, key: K) -> V?` |
| reference access | `get_ref(&self, key: K) -> (&V)?`; `get_mut(&mut self, key: K) -> (&mut V)?`; `get_or_insert(&mut self, key: K, default: V) -> &mut V` |
| borrowed iteration | `iter(&self) -> UniqMapIter[K, V, '_]`; `keys(&self) -> UniqMapKeys[K, V, '_]` |

1. r[std-collections.uniq-map.type] `std.collections` declares `UniqMap[K < Eq & Hash, V < ?Copy]` with private fields and `impl[K < Eq & Hash, V < ?Copy] !Copy for UniqMap[K, V]`. It is not a prelude name, so code that names it imports it.
2. r[std-collections.uniq-map.new] `new()` returns an empty map.
3. r[std-collections.uniq-map.len] `len()` returns the number of entries, and `is_empty()` is true exactly when that number is zero.
4. r[std-collections.uniq-map.contains-key] `contains_key(key)` is true exactly when the map has an entry whose key is equal to `key`.
5. r[std-collections.uniq-map.insert] `insert(key, value)` moves `value` into the entry for `key`. It returns the former value in `.Some` when an equal key was present, or `.None` when the entry is new. Replacing a value does not change `len()`.
6. r[std-collections.uniq-map.remove] `remove(key)` removes the entry whose key equals `key` and moves its value out in `.Some`, or returns `.None` when there is no such entry.
7. r[std-collections.uniq-map.clear] `clear()` removes every entry and drops every value.
8. r[std-collections.uniq-map.get] For `V < Copy`, `get(key)` returns `.Some` of a copy of the value whose key equals `key`, or `.None` when absent.
9. r[std-collections.uniq-map.storage] The implementation is a hash table over `Buffer`. It moves a slot out while probing and writes it back before advancing, and moves entries when it grows. No operation tests whether the buffer is uniquely referenced or makes a copy-on-write clone.
10. r[std-collections.uniq-map.get-ref] `get_ref(key)` returns a shared reference to the value whose key equals `key` in `.Some`, or `.None` when absent. The reference lives for the receiver borrow.
11. r[std-collections.uniq-map.get-mut] `get_mut(key)` returns a mutable reference to the value whose key equals `key` in `.Some`, or `.None` when absent. The reference lives for the mutable receiver borrow.
12. r[std-collections.uniq-map.get-or-insert] `get_or_insert(key, default)` returns a mutable reference to the value whose key equals `key`. When the key is absent, it first inserts `default`; when the key is present, it drops `default` without replacing the value. The reference lives for the mutable receiver borrow.
13. r[std-collections.uniq-map.iter] `iter()` borrows the map and yields `(K, &V)` pairs in table order. Its cursor is `UniqMapIter[K, V, 'a]`, with private fields. The cursor and each yielded value reference live for the map borrow.
14. r[std-collections.uniq-map.keys] `keys()` borrows the map and yields copies of its keys in the same order as `iter()`. Its cursor is `UniqMapKeys[K, V, 'a]`, with private fields, and lives for the map borrow.
15. r[std-collections.uniq-map.into-iter] `UniqMap[K, V]` implements `Iterable[(K, V)]`. Its private `UniqMapIntoIter[K, V]` cursor owns the map and moves each key and value out in table order. A `for` loop over a `UniqMap` therefore consumes the map and each value.
16. r[std-collections.uniq-map.collect] `UniqMap[K, V]` implements `FromIterator[(K, V)]`. When equal keys occur more than once, the last value replaces and drops the former value. An explicitly targeted `collect`, including `let items: UniqMap[_, _] = cursor.collect()`, may therefore collect non-copyable values.

This surface has no literal construction.

## List And Map Values

A list or a map is a value ([Collection Values](../lang/04-type-system.md#collection-values)),
and the `std` methods treat it as one.

```text
fn joined(first: List[i32], second: List[i32]) -> List[i32]:
    let mut all = first
    all.extend(second)  # first is unchanged
    all
```

1. r[std-collections.value.receiver] A `std` method that changes a list or a map, as `extend` does, needs its receiver in a writable place, as a language-tier changing method does ([`types.coll.writable`](../lang/04-type-system.md#r-types.coll.writable)).
2. r[std-collections.value.own-copy] Such a change reaches only the receiver's own value. Another copy of the list or the map is unchanged.
3. r[std-collections.value.results] A method that returns a list or a map, as `keys`, `filter`, and `chunks` do, returns a new value, which the caller may bind with `let mut` and change.
4. r[std-collections.value.pinned-length] A `std` method that changes the length of a pinned list or map, as `extend` with a non-empty list does, is a checked runtime panic, as a language-tier change is ([`ref.elem.pinned`](../lang/13-values-and-references.md#r-ref.elem.pinned), [`ref.elem.pinned.map`](../lang/13-values-and-references.md#r-ref.elem.pinned.map)). Panic: `collection-pinned`.
5. r[std-collections.value.ref-receiver] Each such method takes `&mut self`. On a map value or a list element, as in `groups[key].extend(more)`, it changes the collection where it is stored, as [`types.coll.nested`](../lang/04-type-system.md#r-types.coll.nested) states for every change through a path.

## List Methods

`std` gives `List[T]` these methods, beside the language-tier ones:

| Receiver | Methods |
| --- | --- |
| `List[T]` | `view(&self, start: usize, end: usize) -> ListView[T]`; `chunks(&self, size: usize) -> List[ListView[T]]` |

1. r[std-collections.list.chunks.views] `chunks(size)` returns consecutive read-only `ListView[T]` values in order; only the last view may be shorter than `size`.
2. r[std-collections.list.chunks.size] A `size` below 1 panics. Panic: `explicit-panic`.
3. r[std-collections.list.window-snapshot] `chunks` takes `&self`, and `windows` takes `self`; every returned view holds a copy of that list value.
4. r[std-collections.list.window-cost] Both methods create one result list of views and should copy no elements.

```text
use std.collections.ListView

fn pairs(items: List[i32]) -> List[ListView[i32]]:
    items.chunks(2)  # views of [1, 2] and [3] for [1, 2, 3]
```

> **Note.** `piece.to_list()` copies a returned view into a list.
> A copy of a run of elements is also a slice,
> `items[start..end]`, a new list
> ([Slicing](../lang/05-expressions.md#slicing)).

### Views

`view` gives a read-only window on a run of a list's elements. It takes
`&self` and stores a copy of that list value, as Swift's `ArraySlice` does.
The view shares its storage, so taking it copies no element:

```text
use std.collections.ListView

fn window(items: List[i32]) -> ListView[i32]:
    items.view(1, 3)

fn total(view: ListView[i32]) -> i32:
    let sum = 0
    for item in view:
        sum = sum + item
    sum + view[view.len() - 1]
```

`ListView[T]` has this surface:

| Item | Signature |
| --- | --- |
| `len` | `len(&self) -> usize` |
| `to_list` | `to_list(&self) -> List[T]` |
| index read | `Index[usize]` with `Out = T` |
| iteration | `Iterable[T]` |

1. r[std-collections.view] `view(start, end)` returns a `ListView[T]` of the list's elements at indices `start` up to, not including, `end`.
2. r[std-collections.view.type] `std.collections` declares `ListView[T]` with private fields and the surface in the table above, and no other method or trait implementation.
3. r[std-collections.view.snapshot] `view` takes `&self` and stores a copy of that list value in the returned view.
4. r[std-collections.view.value-copy] A `ListView[T]` is a struct value. A copy of a view is an equal view.
5. r[std-collections.view.import] `ListView` is not a prelude name, so code that names it imports it, as in `use std.collections.ListView`.
6. r[std-collections.view.no-copy] Creating a view copies no element.
7. r[std-collections.view.range] A `start` or `end` that is greater than the list's length is a checked runtime panic. Panic: `index-out-of-bounds`.
8. r[std-collections.view.reversed] A `start` greater than `end` is a checked runtime panic. Panic: `index-out-of-bounds`.
9. r[std-collections.view.len] `len` returns the number of elements in the view, `end - start`.
10. r[std-collections.view.index] `view[i]` reads the list's element at index `start + i`.
11. r[std-collections.view.index.range] A view index that is not less than the view's `len` is a checked runtime panic, even when the list has an element there. Panic: `index-out-of-bounds`.
12. r[std-collections.view.iter] Iterating a view yields its elements in order, each as `T`.
13. r[std-collections.view.cursor] A view's cursor type is `ListViewIter[T]`, which `std.collections` declares with private fields.
14. r[std-collections.view.to-list-value] `to_list` returns a new list value that holds the view's elements in order, as the slice `items[start..end]` does, so `let mut copy = view.to_list()` may grow the copy.
15. r[std-collections.view.read-only] A view has no `IndexSet` implementation, so assigning to `view[i]` is an error. Error: `invalid-assignment-target`.
16. r[std-collections.view.snapshot-writes] An element write or a length change to the list after the view was taken, as `items[i] = v` or `items.push(v)`, does not show through the view.

```text
use std.collections.ListView

fn invalid(mut view: ListView[i32]) -> void:
    view[0] = 7  # error: invalid-assignment-target
```

```text
fn snapshot() -> i32:
    let mut items = [+1, +2, +3]
    window := items.view(0, 2)
    items[0] = +9
    items.push(+4)
    window[0]  # 1: the view kept the list it was taken from
```

> **Note.** A view keeps the whole list's storage, not only its run.
> Copy a run you keep for long with `to_list()`.

> **Why.** A view is a value, as Swift's `ArraySlice` is, so it can never
> go stale and no use of it needs a runtime check. A view that referred to
> the list's place would need a reference that outlives the call.

See also: [List Indexing](../lang/05-expressions.md#list-indexing),
[List And Optional Map](iter.md#list-and-optional-map).

### Spans

`span` gives a read-only live view of a list. Unlike `ListView`, a span
borrows the caller's list instead of holding a snapshot value:

```text
use std.collections.Span

fn middle['a](items: &'a List[i32]) -> Span[i32, 'a]:
    items.span().slice(1..3)
```

`Span[T, 'a]` has this surface:

| Item | Signature |
| --- | --- |
| construction | `span['a](&'a self) -> Span[T, 'a]` on `List[T]` |
| `len` | `len(&self) -> usize` |
| `get` | `get(self, index: usize) -> (&'a T)?` |
| `slice` | `slice(self, range: Range[usize]) -> Span[T, 'a]` |
| index read | `Index[usize]` with `Out = &'a T` |
| `iter` | `iter(self) -> SpanIter[T, 'a]` |

1. r[std-collections.span.type] `std.collections` declares `Span[T, 'a]` with private fields and the surface above.
2. r[std-collections.span.list] `list.span()` borrows the list for `'a` and returns a span covering every element.
3. r[std-collections.span.no-copy] Creating a span copies no element.
4. r[std-collections.span.import] `Span` is not a prelude name, so code that names it imports `std.collections.Span`.
5. r[std-collections.span.len] `len` returns the number of elements in the span.
6. r[std-collections.span.get] `get(index)` returns the indexed element as `&'a T` in `.Some`, or `.None` when the index is outside the span.
7. r[std-collections.span.index] `span[index]` returns the indexed element as `&'a T`.
8. r[std-collections.span.index-range] An index outside the span is a checked runtime panic, even when the list has an element there. Panic: `index-out-of-bounds`.
9. r[std-collections.span.slice] `slice(start..end)` returns the elements at relative indices `start` up to, not including, `end`.
10. r[std-collections.span.slice-inclusive] `slice(start..=end)` includes the element at relative index `end`.
11. r[std-collections.span.slice-range] A slice bound outside the span, or a start after its exclusive end, is a checked runtime panic. Panic: `index-out-of-bounds`.
12. r[std-collections.span.slice-borrow] Slicing copies no element and returns another span that borrows the same list for `'a`.
13. r[std-collections.span.lifetime] A span cannot outlive its list; returning a span of a local list is an error. Error: `local-reference-escapes`.
14. r[std-collections.span.read-only] A span has no `IndexSet` implementation, so assigning through `span[index]` is an error. Error: `invalid-assignment-target`.
15. r[std-collections.span.iter] `iter()` returns a cursor that yields each element in span order as `&'a T`.
16. r[std-collections.span.cursor] A span's cursor type is `SpanIter[T, 'a]`, which `std.collections` declares with private fields.
17. r[std-collections.span.iter-lifetime] A span cursor retains the span's borrow for `'a`, so it cannot outlive the borrowed list. Error: `local-reference-outlives-local`.

> **Why.** `Span` supports borrowed reads without a snapshot allocation.
> Its explicit lifetime makes retention beyond the source list a compile-time error.

#### Mutable Spans

`span_mut` gives an exclusive mutable view of a list. Its indices remain
relative to the view, and every mutation preserves the list's length.

| Operation | Signature or trait |
| --- | --- |
| whole-list view | `span_mut['a](&'a mut self) -> SpanMut[T, 'a]` |
| `len` | `len(&self) -> usize` |
| `get` | `get['b](&'b self, index: usize) -> (&'b T)?` |
| `get_mut` | `get_mut['b](&'b mut self, index: usize) -> (&'b mut T)?` |
| `slice_mut` | `slice_mut['b](&'b mut self, range: Range[usize]) -> SpanMut[T, 'b]` |
| index read | `Index[usize]` with `Out = T` |
| index write | `IndexSet[usize, T]` |
| `swap` | `swap(&mut self, left: usize, right: usize) -> void` |
| `fill` | `fill(&mut self, value: T) -> void` |

1. r[std-collections.span-mut.type] `std.collections` declares `SpanMut[T, 'a]` with private fields and the surface above.
2. r[std-collections.span-mut.list] `list.span_mut()` borrows the list mutably for `'a` and returns a mutable span covering every element.
3. r[std-collections.span-mut.no-copy] Creating or slicing a mutable span copies no element.
4. r[std-collections.span-mut.import] `SpanMut` is not a prelude name, so code that names it imports `std.collections.SpanMut`.
5. r[std-collections.span-mut.len] `len` returns the number of elements in the mutable span.
6. r[std-collections.span-mut.get] `get(index)` returns the indexed element as `&'b T` in `.Some`, or `.None` outside the span. It borrows the span for `'b`, so the span stays usable after the last use of the result.
7. r[std-collections.span-mut.get-mut] `get_mut(index)` returns the indexed element as `&'b mut T` in `.Some`, or `.None` outside the span. It borrows the span mutably for `'b`.
8. r[std-collections.span-mut.index] `span[index]` returns a copy of the indexed element, as a list's `items[i]` does. The read borrows the span only for the call, so the span stays usable; `get` gives a reference.
9. r[std-collections.span-mut.index-set] `span[index] = value` replaces the indexed element without changing the list's length.
10. r[std-collections.span-mut.index-range] An index read or write outside the span is a checked runtime panic. Panic: `index-out-of-bounds`.
11. r[std-collections.span-mut.slice] `slice_mut(start..end)` returns the elements at relative indices `start` up to, not including, `end`.
12. r[std-collections.span-mut.slice-inclusive] `slice_mut(start..=end)` includes the element at relative index `end`.
13. r[std-collections.span-mut.slice-range] An invalid mutable slice range is a checked runtime panic. Panic: `index-out-of-bounds`.
14. r[std-collections.span-mut.slice-borrow] Mutable slicing reborrows the span for `'b` and returns another span over the same list, so the original span is unusable while the slice is live. Error: `conflicting-access`.
15. r[std-collections.span-mut.swap] `swap(left, right)` exchanges two indexed elements without changing the list's length.
16. r[std-collections.span-mut.swap-range] An out-of-range `swap` index is a checked runtime panic. Panic: `index-out-of-bounds`.
17. r[std-collections.span-mut.fill] `fill(value)` replaces every element in the mutable span with `value`.
18. r[std-collections.span-mut.lifetime] A mutable span cannot outlive its list. Error: `local-reference-escapes`.
19. r[std-collections.span-mut.receivers] `len`, `get`, `get_mut`, `slice_mut`, `swap` and `fill` borrow the span, with `&self` or `&mut self` as the table shows, so one span serves any number of calls in a row.
20. r[std-collections.span-mut.exclusive] While a mutable span is live, its source list may be used only through that span. Error: `conflicting-access`.

> **Why.** `SpanMut` exposes in-place element updates while the existing
> exclusive-borrow rules prevent changing the source list behind the view.

## List Helpers

`std` also gives `List[T]` helpers that sort, group, split, and search a
list without a hand-written loop:

```text
use std.collections.ListView

struct Task:
    name: string
    owner: string
    hours: i32

fn plan(tasks: List[Task]) -> List[Task]:
    tasks.sorted_by_key(fn(task: &Task) -> i32: task.hours)

fn by_owner(tasks: List[Task]) -> Map[string, List[Task]]:
    tasks.group_by(fn(task: &Task) -> string: task.owner)

fn has_long(tasks: List[Task]) -> bool:
    tasks.any(fn(task: &Task) -> bool: task.hours > 8)

fn steps(readings: List[i32]) -> List[ListView[i32]]:
    readings.windows(2)  # views of [1, 4] and [4, 9] for [1, 4, 9]

fn shortest(tasks: List[Task]) -> Task?:
    tasks.min_by_key(fn(task: &Task) -> i32: task.hours)  # the first of equal hours

fn owners(tasks: List[Task]) -> List[string]:
    tasks.map(fn(task: &Task) -> string: task.owner).unique()  # ["bo", "al"] for bo, al, bo
```

| Receiver | Methods |
| --- | --- |
| `List[T]` | `sorted_by_key[K < Ord](&self, key: fn(&T) -> K) -> List[T]`; `group_by[K < Eq & Hash](&self, key: fn(&T) -> K) -> Map[K, List[T]]`; `partition(&self, keep: fn(&T) -> bool) -> (List[T], List[T])`; `any(&self, test: fn(&T) -> bool) -> bool`; `all(&self, test: fn(&T) -> bool) -> bool`; `find(&self, test: fn(&T) -> bool) -> T?`; `flat_map[U, I < Iterable[U]](self, transform: fn(T) -> I) -> List[U]`; `windows(self, size: usize) -> List[ListView[T]]` |
| `List[T]` | `min_by_key[K < Ord](&self, key: fn(&T) -> K) -> T?`; `max_by_key[K < Ord](&self, key: fn(&T) -> K) -> T?` |
| `List[T]`, when `T < Eq` | `contains(&self, value: T) -> bool`; `index_of(&self, value: T) -> usize?`; `dedup(self) -> List[T]` |
| `List[T]`, when `T < Eq & Hash` | `unique(self) -> List[T]` |
| `List[T]`, when `T < Ord` | `sorted(&self) -> List[T]`; `sort(&mut self) -> void`; `min(&self) -> T?`; `max(&self) -> T?` |
| `List[T]`, when `T < Num` | `sum(self) -> T` |

1. r[std-collections.helper.unchanged] Each helper but `retain`, `sort`, `sort_by`, `sort_by_key`, and `swap` leaves its receiver unchanged, and each one that returns a list returns a new list.
2. r[std-collections.helper.callback-order] A helper calls its callback on the elements in list order, at most once each.
3. r[std-collections.helper.callback-row] Each callback has the empty row, as `Iterator.filter`'s `keep` does. A function value whose row lists a requirement key does not fit it. Error: `type-mismatch`.
4. r[std-collections.helper.sorted-by-key] `sorted_by_key(key)` returns the elements in ascending order of `key(element)` by `Ord`, and calls `key` once per element.
5. r[std-collections.helper.sorted] `sorted` returns the elements in ascending order by `Ord`.
6. r[std-collections.helper.stable] Both sorts are stable: elements that compare equal keep their order.
7. r[std-collections.helper.group-by] `group_by(key)` maps each key that `key` returns to the list of the elements that gave it, in list order.
8. r[std-collections.helper.partition] `partition(keep)` returns the elements for which `keep` is true, then the rest, each in list order.
9. r[std-collections.helper.any-all] `any(test)` is true when `test` is true for some element, and `all(test)` when it is true for every element. So on an empty list `any` is false and `all` is true.
10. r[std-collections.helper.find-copy] `find(test)` returns a copy of the first element for which `test` is true in `.Some`, or `.None` when there is none.
11. r[std-collections.helper.short-circuit] `any`, `all`, and `find` call `test` on no element after the first one that decides the result.
12. r[std-collections.helper.flat-map.iterable-owned] `flat_map(transform)` appends every item from each returned `Iterable` into one result list, in source order.
13. r[std-collections.helper.windows.views] `windows(size)` returns overlapping `ListView[T]` values of `size` elements, in starting-index order.
14. r[std-collections.helper.windows.short] A list with fewer than `size` elements gives `[]`.
15. r[std-collections.helper.windows.size] A `size` below 1 panics, as it does for `chunks`. Panic: `explicit-panic`.
16. r[std-collections.helper.contains] `contains(value)` is true when an element equals `value` by `Eq`. `index_of(value)` returns the index of the first such element in `.Some`, or `.None`.
17. r[std-collections.helper.min-max.copy-first] `min` returns a copy of the first smallest element and `max` one to the first largest by `Ord`, each in `.Some`. An empty list gives `.None`.
18. r[std-collections.helper.sum] `sum` returns the total of the elements, added with `+` in list order from 0 of type `T`, so an empty list gives 0.
19. r[std-collections.helper.sum.overflow] An integer total that leaves the range of `T` panics, as `+` does. Panic: `integer-overflow`.
20. r[std-collections.helper.sum.iterator] `items.sum()` returns what `items.iter().sum()` returns.
21. r[std-collections.helper.by-key.copy-first] `min_by_key(key)` returns a copy of the first element whose `key` is smallest by `Ord`. `max_by_key(key)` returns a copy of the first whose `key` is largest. Each returns `.Some`, so equal keys give the earlier element, as `min` and `max` do. An empty list gives `.None`.
22. r[std-collections.helper.dedup] `dedup` returns the list without each element that equals the element just before it in the receiver, as Rust's `Vec::dedup` does, so only adjacent duplicates go and `[1, 2, 1]` is unchanged.
23. r[std-collections.helper.unique] `unique` returns the list without each element that equals an earlier element, so every distinct element stays once, at its first position.
24. r[std-collections.helper.callback-ref] Each callback looks at an element by reference: `fn(&T)`, and `fn(&T, &T)` for `sorted_by` and `sort_by`. Each receiver is `&self`, or `&mut self` for a method that changes the list.
25. r[std-collections.helper.retain] `retain(keep)` keeps the elements for which `keep` is true, in list order, and drops the rest, in place.
26. r[std-collections.helper.sort] `sort` puts the elements in ascending order by `Ord`, in place.
27. r[std-collections.helper.sort-by] `sort_by(compare)` puts the elements in ascending order by `compare`, in place.
28. r[std-collections.helper.sort-by-key] `sort_by_key(key)` puts the elements in ascending order of `key(element)` by `Ord`, in place, and calls `key` once per element.
29. r[std-collections.helper.sort.stable] `sort`, `sort_by`, and `sort_by_key` are stable, as `sorted_by` is: elements that compare equal keep their order.
30. r[std-collections.helper.swap] `swap(a, b)` exchanges the elements at `a` and `b`. An index that is not less than `len()` panics. Panic: `index-out-of-bounds`.

| Call | Result |
| --- | --- |
| `[3, 1, 2].partition(fn(n: &i32) -> bool: n > 1)` | `([3, 2], [1])` |
| `[1, 2].flat_map(fn(n: i32) -> List[i32]: [n, n])` | `[1, 1, 2, 2]` |
| `[1, 2, 3].windows(2)` | views of `[1, 2]` and `[2, 3]` |
| `[1, 2].windows(3)` | `[]` |
| `[5, 7, 5].index_of(5)` | `.Some(0)` |
| `[2, 9, 4].max()` | `.Some(9)` |
| `["bb", "a", "cc"].max_by_key(fn(w: &string) -> usize: w.len())` | `.Some("bb")` |
| `[1, 1, 2, 1].dedup()` | `[1, 2, 1]` |
| `[1, 1, 2, 1].unique()` | `[1, 2]` |
| `[1, 2, 3].sum()` | `6` |
| `[0.5, 0.25].sum()` | `0.75` |
| `[]` of `i64`, `.sum()` | `0` |

```text
fn paged(items: List[i32]) -> List[List[i32]]:
    items.windows(0)  # panics with explicit-panic
```

> **Why.** The names are Rust's (`sort_by_key`, `partition`, `windows`,
> `any`, `all`, `find`, `flat_map`, `contains`, `min`, `max`), Kotlin's
> `groupBy`, and Python's `list.index`. A sort copies, as `sorted_by`
> does, since hd has no consuming methods. `min` and `max` both pick the
> first of equal elements, as Python's `min` and `max` do, so one rule
> covers both. `sum` is Python's `sum` and Rust's `Iterator::sum`; a list
> has it as a method beside `min` and `max`, since a list is not an
> iterator.
>
> `dedup` is `Vec::dedup`, which removes every duplicate only
> from a sorted list; `unique` is what Python's `dict.fromkeys` gives for
> a list that is not sorted. The names differ because the results differ.

### Counts

`counts` tallies how often each element occurs:

```text
use std.collections.counts

fn tally(words: List[string]) -> Map[string, usize]:
    counts(words)  # {"a": 2, "b": 1} for ["a", "b", "a"]
```

1. r[std-collections.counts.decl] `std.collections` declares `pub fn counts[T < Eq & Hash](items: List[T]) -> Map[T, usize]`. Code imports it, as in `use std.collections.counts`.
2. r[std-collections.counts.value] The map has one entry per distinct element of `items`, and its value is the number of elements equal to that one.
3. r[std-collections.counts.empty] An empty list gives an empty map.

> **Why.** `counts` is Python's `collections.Counter` as a plain map, so
> the `Map` methods read it.

## Map Methods

`std` gives `Map[K, V]` these methods, beside the language-tier ones:

| Receiver | Methods |
| --- | --- |
| `Map[K, V]` | `get_or_insert(&mut self, key: K, default: V) -> &mut V`; `contains_key(&self, key: K) -> bool`; `keys(&self) -> List[K]`; `values(&self) -> List[V]`; `is_empty(&self) -> bool`; `entries(&self) -> List[(K, V)]` |
| `Map[K, usize]` | `most_common(&self, n: usize) -> List[(K, usize)]` |

```text
fn shelf_count(stock: &Map[string, List[string]], item: string) -> usize:
    match stock.get(item):  # List[string]?: one count of the list buffer
        .Some(shelves) => shelves.len()
        .None => 0

fn summary(stock: Map[string, i32]) -> string:
    if stock.contains_key("tea"):
        return "${stock.keys().len()} items"
    "no tea"

fn top_words(words: List[string]) -> List[(string, usize)]:
    counts(words).most_common(2)  # [("a", 2), ("b", 1)] for ["a", "b", "a", "c"]

fn pairs(stock: Map[string, i32]) -> List[(string, i32)]:
    stock.entries()  # [("tea", 3), ("milk", 1)] for {"tea": 3, "milk": 1}

fn tally(words: List[string]) -> Map[string, usize]:
    let mut found: Map[string, usize] = {}
    for word in words:
        *found.get_or_insert(word, 0) += 1
    found
```

1. r[std-collections.map.get] `get(key)` is the built-in copying lookup ([`expr.index.map.get`](../lang/05-expressions.md#r-expr.index.map.get)): it returns `.Some` of a copy of the stored value, or `.None` when the map has no equal key.
2. r[std-collections.map.get-or-insert] `get_or_insert(key, default)` returns a mutable reference to the stored value when the map has an equal key.
3. r[std-collections.map.get-or-insert.missing] When the map has no equal key, `get_or_insert` inserts `(key, default)` and returns a mutable reference to the inserted value.
4. r[std-collections.map.get-or-insert.reference] The reference that `get_or_insert` returns is dependent on its receiver by [`ref.dep.elide`](../lang/13-values-and-references.md#r-ref.dep.elide).
5. r[std-collections.map.get-or-insert.pinned] An insertion by `get_or_insert` while that map is pinned panics `collection-pinned`, as every map length change does.
6. r[std-collections.map.get-or-insert.probe-once] Each `get_or_insert` call invokes the key's `Hash` implementation once and performs one map search, reusing a missing search position for insertion.
7. r[std-collections.map.contains-key] `contains_key(key)` is true exactly when the map has an entry whose key equals `key`.
8. r[std-collections.map.keys.order] `keys` returns the map's keys in its [iteration order](../lang/04-type-system.md#r-types.map.order.deterministic).
9. r[std-collections.map.values] `values` returns the map's values in the same order.
10. r[std-collections.map.snapshot] Each returned list is a snapshot: a later change to the map does not change it.
11. r[std-collections.map.is-empty] `is_empty` is true exactly when `len()` is 0.
12. r[std-collections.map.entries] `entries` returns the map's `(key, value)` pairs in its [iteration order](../lang/04-type-system.md#r-types.map.order.deterministic), which is the order of `keys` and `values`, as a snapshot like theirs.
13. r[std-collections.map.most-common] `most_common(n)` returns the `n` pairs of the map with the largest values, largest first, or every pair when `n` is at least `len()`. An `n` of 0 gives `[]`.
14. r[std-collections.map.most-common.ties] Pairs with equal values keep the map's iteration order, so on a `counts` result the element that occurred first comes first, as in Python's `Counter.most_common`.

> **Why.** `entries` is a list snapshot like `keys` and `values`, since a
> loop that sorts or indexes the pairs needs a list. `most_common` is
> Python's `Counter.most_common`; a stable sort over the map's order gives
> its tie rule with no extra bookkeeping. `get_or_insert` is Rust's
> `HashMap::entry(key).or_insert(default)` operation without exposing an
> entry object that holds a reference.

See also: [Map Key Types](../lang/04-type-system.md#map-key-types).

## Set, Deque, And Heap Values

`Set[T]`, `Deque[T]`, and `Heap[T]` are structs over a list or a map, so
each one is a value, as a list is:

```text
use std.collections.Deque

fn with_last(queue: Deque[string]) -> Deque[string]:
    let mut own = queue
    own.push_back("last")  # the caller's deque is unchanged
    own

fn enqueue(queue: &mut Deque[string], name: string) -> void:
    queue.push_back(name)  # changes the caller's deque
```

1. r[std-collections.struct.value] `Set[T]`, `Deque[T]`, and `Heap[T]` are structs, so each is a value. Binding, assigning, passing, returning, and storing one copies it, and a change to one copy leaves every other copy unchanged.
2. r[std-collections.struct.cost] A copy shares its source's elements until one of the two changes, as a list does ([`types.coll.copy-on-write`](../lang/04-type-system.md#r-types.coll.copy-on-write)).
3. r[std-collections.struct.receiver-mut] Methods that change a collection, such as `push_back`, `insert`, and `pop`, take `&mut self`.
4. r[std-collections.struct.lend-or-box] A function that changes the caller's set, deque, or heap takes `&mut Set[T]`, `&mut Deque[T]`, or `&mut Heap[T]`, and a change through it changes the place it refers to. A collection that two holders keep is boxed, as `box(queue)`, which gives a `*mut Deque[T]`.

> **Why.** These types were shared objects. As values they follow the
> list they wrap: a copy is cheap until it changes, and sharing is
> written where it happens.

See also: [Reference Receivers](../lang/07-functions.md#reference-receivers)
for temporary calls and `not-writable` on read-only places.

## Set

A `Set[T]` holds distinct elements, in a deterministic order:

```text
use std.collections.Set

fn unique(words: List[string]) -> List[string]:
    let mut seen: Set[string] = Set::new()
    let mut kept: List[string] = []
    for word in words:
        if seen.insert(word):
            kept.push(word)
    kept  # ["b", "a"] for ["b", "a", "b"]

fn distinct(words: List[string]) -> Set[string]:
    words.iter().collect()
```

`Set[T < Eq & Hash]` has this surface:

| Item | Signature |
| --- | --- |
| `new` | `Set::new() -> Set[T]` |
| `len`, `is_empty` | `len(&self) -> usize`; `is_empty(&self) -> bool` |
| `contains` | `contains(&self, value: T) -> bool` |
| `insert`, `remove` | `insert(&mut self, value: T) -> bool`, and the same for `remove` |
| iteration | `Iterable[T]` |
| `collect` | `FromIterator[T]` |
| equality | `Eq` |
| `Debug` | when `T < Debug` |

1. r[std-collections.set.decl] `std.collections` declares `Set[T < Eq & Hash]` with private fields, the surface in the table above, and no other method or trait implementation. Code imports it, as in `use std.collections.Set`.
2. r[std-collections.set.distinct] A set holds elements no two of which are equal by `Eq`. `new` returns an empty set.
3. r[std-collections.set.len] `len` returns the number of elements, and `is_empty` is true exactly when that number is 0.
4. r[std-collections.set.contains] `contains(value)` is true exactly when the set has an element equal to `value`.
5. r[std-collections.set.insert] `insert(value)` adds `value` and returns `true` when the set has no element equal to it. Otherwise it changes nothing and returns `false`.
6. r[std-collections.set.remove] `remove(value)` removes the element equal to `value` and returns `true`. When there is none, it changes nothing and returns `false`.
7. r[std-collections.set.iter] Iterating a set yields its elements in the order that a map's keys would have after the same inserts and removes, by [`types.map.order.deterministic`](../lang/04-type-system.md#r-types.map.order.deterministic).
8. r[std-collections.set.cursor] A set's cursor type is `SetIter[T]`, which `std.collections` declares with private fields.
9. r[std-collections.set.snapshot] Iterating a set walks the set as it was when `iter` was called. An `insert` or a `remove` while the loop runs changes the set but not the loop, and never panics, as [`flow.for.snapshot`](../lang/06-control-flow.md#r-flow.for.snapshot) states for a map.
10. r[std-collections.set.eq] Two sets are equal when each element of one equals an element of the other, in any order.
11. r[std-collections.set.collect] A set implements `FromIterator[T]`. Collecting inserts each item in order, so the set holds each distinct item once, in the order of its first occurrence, and an empty iterator gives an empty set.

> **Note.** `lib/std` builds a set over a `Map` whose values carry no
> meaning, so `insert`, `remove`, and `contains` take expected constant
> time, by [`module.map.complexity`](../lang/10-modules.md#r-module.map.complexity).

> **Why.** `insert` and `remove` report whether they changed the set, as
> Rust's `HashSet` methods do, so a loop that skips duplicates needs no
> second lookup. A set iterates in a map's deterministic order, so its
> iteration and `Debug` text are the same in every run. A set is a value,
> so a loop walks a snapshot of it, as a loop over a map does.

## Deque

A `Deque[T]` is a double-ended queue: a sequence that grows and shrinks
at both ends.

```text
use std.collections.Deque

fn rotate(items: List[i32]) -> List[i32]:
    let mut queue: Deque[i32] = Deque::new()
    for item in items:
        queue.push_back(item)
    match queue.pop_front():
        .Some(first) => queue.push_back(first)
        .None => pass
    let mut rotated: List[i32] = []
    for item in queue:
        rotated.push(item)
    rotated  # [2, 3, 1] for [1, 2, 3]
```

`Deque[T]` has this surface:

| Item | Signature |
| --- | --- |
| `new` | `Deque::new() -> Deque[T]` |
| `len`, `is_empty` | `len(&self) -> usize`; `is_empty(&self) -> bool` |
| `push_front`, `push_back` | `push_front(&mut self, value: T) -> void`, and the same for `push_back` |
| `pop_front`, `pop_back` | `pop_front(&mut self) -> T?`, and the same for `pop_back` |
| `front`, `back` | `front(&self) -> T?`, and the same for `back` |
| `get` | `get(&self, index: usize) -> T?` |
| iteration | `Iterable[T]` |
| equality | `Eq` when `T < Eq` |
| `Debug` | when `T < Debug` |
| `collect` | `FromIterator[T]` |

1. r[std-collections.deque.collect-surface] `std.collections` declares `Deque[T]` with private fields, the surface in the table above, and no other method or trait implementation. Code imports it, as in `use std.collections.Deque`.
2. r[std-collections.deque.order] A deque holds a sequence of elements from its front to its back. `new` returns an empty deque.
3. r[std-collections.deque.len] `len` returns the number of elements, and `is_empty` is true exactly when that number is 0.
4. r[std-collections.deque.push] `push_front(value)` adds `value` before the front, and `push_back(value)` adds it after the back.
5. r[std-collections.deque.pop] `pop_front` removes the front element and returns it in `.Some`, and `pop_back` does the same at the back. On an empty deque, each returns `.None` and changes nothing.
6. r[std-collections.deque.ends-copy] `front` and `back` return a copy of the front and the back element without removing it, or `.None` when the deque is empty.
7. r[std-collections.deque.get-copy] `get(i)` returns a copy of the element `i` places from the front in `.Some`, so `get(0)` is the front. An `i` that is not less than `len` gives `.None`.
8. r[std-collections.deque.iter] Iterating a deque yields its elements from front to back, each as `T`.
9. r[std-collections.deque.cursor] A deque's cursor type is `DequeIter[T]`, which `std.collections` declares with private fields.
10. r[std-collections.deque.snapshot] Iterating a deque walks the deque as it was when `iter` was called. A push or a pop while the loop runs changes the deque but not the loop, and never panics, as [`flow.for.snapshot`](../lang/06-control-flow.md#r-flow.for.snapshot) states for a list.
11. r[std-collections.deque.eq] Two deques are equal when they have the same length and equal elements in the same order from the front, as two lists are.
12. r[std-collections.deque.collect] A deque implements `FromIterator[T]`, collecting every element in source order.

```text
use std.collections.Deque

fn grow(queue: &mut Deque[i32]) -> void:
    for item in *queue:  # the deque as it was here
        queue.push_back(item)  # doubles the deque; the loop never panics
```

> **Note.** `lib/std` builds a deque as a ring buffer over a `List[T?]`.
> A push into a full buffer first copies the elements into a buffer
> twice its size. So each push and pop takes amortized constant time,
> and `get` takes constant time.

> **Why.** `get` returns an optional, as Rust's `VecDeque::get` does: a
> queue's length changes often, so a missing element is an expected
> case. A deque is a value, so a loop walks a snapshot of it, as a loop
> over a list does.

## Heap

A `Heap[T]` is a binary max-heap, as Rust's `BinaryHeap` is: `pop`
removes its largest element.

```text
use std.cmp.Reverse
use std.collections.Heap

fn largest(items: List[i32]) -> i32?:
    let mut heap: Heap[i32] = Heap::new()
    for item in items:
        heap.push(item)
    heap.pop()  # .Some(9) for [5, 9, 2]

fn smallest(items: List[i32]) -> i32?:
    let mut heap: Heap[Reverse[i32]] = Heap::new()
    for item in items:
        heap.push(Reverse { value: item })
    heap.pop().map(fn(top: Reverse[i32]) -> i32: top.value)  # .Some(2) for [5, 9, 2]
```

`Heap[T < Ord]` has this surface:

| Item | Signature |
| --- | --- |
| `new` | `Heap::new() -> Heap[T]` |
| `len`, `is_empty` | `len(&self) -> usize`; `is_empty(&self) -> bool` |
| `push` | `push(&mut self, value: T) -> void` |
| `pop` | `pop(&mut self) -> T?` |
| `peek` | `peek(&self) -> T?` |
| `to_sorted_list` | `to_sorted_list(&self) -> List[T]` |
| `Debug` | when `T < Debug` |
| `collect` | `FromIterator[T]` |

1. r[std-collections.heap.collect-surface] `std.collections` declares `Heap[T < Ord]` with private fields, the surface in the table above, and no other method or trait implementation. Code imports it, as in `use std.collections.Heap`.
2. r[std-collections.heap.len] `new` returns an empty heap. `len` returns the number of elements, and `is_empty` is true exactly when that number is 0.
3. r[std-collections.heap.push] `push(value)` adds `value` to the heap.
4. r[std-collections.heap.pop] `pop` removes a largest element by `Ord` and returns it in `.Some`. On an empty heap, it returns `.None`.
5. r[std-collections.heap.peek-copy] `peek` returns a copy of a largest element without removing it, or `.None` when the heap is empty.
6. r[std-collections.heap.ties] When several elements are equal by `cmp`, which of them `pop` and `peek` return is not specified.
7. r[std-collections.heap.sorted] `to_sorted_list` returns a new list of every element in ascending order by `Ord`, and leaves the heap unchanged.
8. r[std-collections.heap.min] `std` has no separate min-heap: a `Heap[Reverse[T]]` pops the smallest `T` first, by [`std-cmp.reverse.order`](cmp.md#r-std-cmp.reverse.order).
9. r[std-collections.heap.collect] A heap implements `FromIterator[T]`, collecting every element into a max-heap.
10. r[std-collections.heap.collect.linear] Heapification of `n` collected elements performs O(n) comparisons, by sifting non-leaves from the bottom up.

> **Note.** `lib/std` keeps the elements in a `List[T?]` in heap order,
> so `push` and `pop` take logarithmic time and `peek` constant time.

> **Why.** `to_sorted_list` copies, as `sorted_by` does, because hd has no
> consuming methods; its ascending order is that of Rust's
> `into_sorted_vec`. A heap has no `Eq` and no iteration, as in Rust:
> two heaps with the same elements may hold them in different orders.

See also: [Reverse](cmp.md#reverse), [Debug For Standard Types](format.md#debug-for-standard-types).

## List Access And Building

`std` gives `List[T]` methods that read one element safely, copy part of
a list, and grow one list by another:

```text
fn second(items: List[string]) -> string?:
    items.get(1)

fn preview(lines: List[string]) -> List[string]:
    if lines.is_empty():
        return ["(empty)"]
    lines.take(3)

fn merged(head: List[i32], tail: List[i32]) -> List[i32]:
    let mut all: List[i32] = []
    all.extend(head.filter(fn(n: &i32) -> bool: n > 0))
    all.extend(tail)
    all.sorted_by(fn(a: &i32, b: &i32) -> Ordering: b.cmp(a))
```

| Receiver | Methods |
| --- | --- |
| `List[T]` | `get(&self, index: usize) -> T?`; `is_empty(&self) -> bool`; `first(&self) -> T?`; `last(&self) -> T?`; `take(self, count: usize) -> List[T]`; `filter(&self, keep: fn(&T) -> bool) -> List[T]`; `reversed(self) -> List[T]`; `sorted_by(&self, compare: fn(&T, &T) -> Ordering) -> List[T]`; `zip[U](self, other: List[U]) -> List[(T, U)]` |
| `List[T]`, changing | `extend(&mut self, other: List[T]) -> void`; `retain(&mut self, keep: fn(&T) -> bool) -> void`; `sort_by(&mut self, compare: fn(&T, &T) -> Ordering) -> void`; `sort_by_key[K < Ord](&mut self, key: fn(&T) -> K) -> void`; `swap(&mut self, a: usize, b: usize) -> void` |

1. r[std-collections.access.get-copy] `get(index)` returns `.Some` of a copy of the element at `index`, or `.None` when `index` is not less than `len()`. It never panics.
2. r[std-collections.access.is-empty] `is_empty` is true exactly when `len()` is 0.
3. r[std-collections.access.first-last-copy] `first` returns a copy of the element at index 0 and `last` one of the element at index `len() - 1`, each in `.Some`. An empty list gives `.None`.
4. r[std-collections.access.take] `take(count)` returns a new list of the first `count` elements, or of every element when `count` is at least `len()`.
5. r[std-collections.access.filter] `filter(keep)` returns a new list of the elements for which `keep` is true, in list order.
6. r[std-collections.access.reversed] `reversed` returns a new list of the elements in reverse order.
7. r[std-collections.access.sorted-by] `sorted_by(compare)` returns a new list in ascending order by `compare`. The sort is stable.
8. r[std-collections.access.zip] `zip(other)` returns a new list of pairs `(self[i], other[i])` for each index `i` of both lists, so it stops at the shorter one.
9. r[std-collections.access.extend] `extend(other)` appends each element of `other` to the receiver, in order, as `push` does.
10. r[std-collections.access.extend.self] `items.extend(items)` appends a copy of the elements `items` held before the call, so it doubles the list.
11. r[std-collections.access.helper-rules] The callbacks of `filter` and `sorted_by` follow [`std-collections.helper.callback-row`](#r-std-collections.helper.callback-row): each has the empty row.

```text
fn adjust(items: &mut List[i32], stock: &mut Map[string, i32]) -> void:
    items[1] += 10
    *stock.get_or_insert("tea", 0) += 1
```

| Call | Result |
| --- | --- |
| `[4, 5].get(1)` | `.Some(5)` |
| `[4, 5].get(2)` | `.None` |
| `[1, 2, 3].take(2)` | `[1, 2]` |
| `[1, 2].take(5)` | `[1, 2]` |
| `[1, 2, 3].zip(["a", "b"])` | `[(1, "a"), (2, "b")]` |

> **Why.** `get` is the checked, copying read beside `items[i]`.
> `take` is Kotlin's `List.take`: like `List.map`, the list
> method is eager and returns a list, while `Iterator.take` stays lazy.
> `extend` is Rust's `Vec::extend`, and replaces `items = items + more`.

See also: [Built-In Methods](../lang/10-modules.md#built-in-methods),
[Iterator Adapters](iter.md#iterator-adapters).
