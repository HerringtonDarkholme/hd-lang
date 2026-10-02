# Collections

Status: standard library specification draft.

This chapter defines the part of `std.collections` that `lib/std` writes
in ordinary hd over the language tier:

- the `List` methods `view` and `chunks`;
- the `ListView[T]` type that `view` returns;
- the `Map` methods `contains_key`, `keys`, and `values`.

The language tier keeps what the compiler knows about a list
([List Indexing](../lang/05-expressions.md#list-indexing),
[Built-In Methods](../lang/10-modules.md#built-in-methods),
[Iterator Invalidation](../lang/06-control-flow.md#iterator-invalidation)):

| Item | Why it stays in the language tier |
| --- | --- |
| `items[i]`, `len`, `iter`, `append` | built-in indexing and intrinsics over the list's representation |
| the structural-version counter | [`flow.for.version`](../lang/06-control-flow.md#r-flow.for.version), a language rule, defines when a change invalidates an iterator |

## List Methods

`std` gives `List[T]` these methods, beside the language-tier ones:

| Receiver | Methods |
| --- | --- |
| `List[T]` | `view(self, start: i32, end: i32) -> ListView[T]`; `chunks(self, size: i32) -> List[List[T]]` |

1. r[std-collections.list.no-negative] `view` never counts from the end when an argument is negative. The rules below state the panic.
2. r[std-collections.list.chunks] `chunks(size)` returns the list's elements in order, in consecutive pieces of `size` elements. Only the last piece may be shorter.
3. r[std-collections.list.chunks.size] A `size` below 1 panics. Panic: `explicit-panic`.

```text
fn pairs(items: List[i32]) -> List[List[i32]]:
    items.chunks(2)  # [[1, 2], [3]] for [1, 2, 3]
```

> **Why.** With negative indices counting from the end, as in Python,
> `items[i - 1]` with `i == 0` would read the last element instead of
> failing.

> **Note.** A copy of a run of elements is a slice,
> `items[start..end]`, a new `mut List[T]`
> ([Slicing](../lang/05-expressions.md#slicing)).

### Views

`view` gives a read-only window on a run of elements, with no copy:

```text
use std.collections.ListView

fn window(items: List[i32]) -> ListView[i32]:
    items.view(1, 3)

fn total(view: ListView[i32]) -> i32:
    let sum = 0
    for item in view:
        sum = sum + item
    sum + view[0] + view.len()
```

`ListView[T]` has this surface:

| Item | Signature |
| --- | --- |
| `len` | `len(self) -> i32` |
| `to_list` | `to_list(self) -> mut List[T]` |
| index read | `Index[i32]` with `Out = T` |
| iteration | `Iterable[T]` |

1. r[std-collections.view] `view(start, end)` returns a `ListView[T]` of the list's elements at indices `start` up to, not including, `end`.
2. r[std-collections.view.type] `std.collections` declares `ListView[T]` with private fields and the surface in the table above, and no other method or trait implementation.
3. r[std-collections.view.import] `ListView` is not a prelude name, so code that names it imports it, as in `use std.collections.ListView`.
4. r[std-collections.view.no-copy] Creating a view copies no element.
5. r[std-collections.view.range] A `start` or `end` that is negative or greater than the list's length is a checked runtime panic. Panic: `index-out-of-bounds`.
6. r[std-collections.view.reversed] A `start` greater than `end` is a checked runtime panic. Panic: `index-out-of-bounds`.
7. r[std-collections.view.len] `len` returns the number of elements in the view, `end - start`.
8. r[std-collections.view.index] `view[i]` reads the list's element at index `start + i`.
9. r[std-collections.view.index.range] A view index that is negative or not less than the view's `len` is a checked runtime panic, even when the list has an element there. Panic: `index-out-of-bounds`.
10. r[std-collections.view.iter] Iterating a view yields its elements in order, each as `T`.
11. r[std-collections.view.to-list.mut] `to_list` returns a new `mut List[T]` that holds the view's elements in order, as the slice `items[start..end]` does, so `let mut copy = view.to_list()` may grow the copy.
12. r[std-collections.view.read-only] A view has no `IndexSet` implementation, so assigning to `view[i]` is an error. Error: `invalid-assignment-target`.
13. r[std-collections.view.writes] An element write to the list, as `items[i] = v`, shows through every view of it, so a later read of the view gives the new value.
14. r[std-collections.view.invalidate] Inserting, removing, appending, clearing, or otherwise changing the list's shape invalidates every view of it, as [`flow.for.invalidate`](../lang/06-control-flow.md#r-flow.for.invalidate) does for an iterator.
15. r[std-collections.view.invalidate.anywhere] A change invalidates a view wherever it happens, so appending past a view's end invalidates it too.
16. r[std-collections.view.invalid-use] The next use of an invalidated view is a checked runtime panic: a `len`, index, or `to_list` call, an `iter()` call, or the next `next` call of an iterator taken from it. Panic: `iterator-invalidated`.

```text
use std.collections.ListView

fn invalid(view: mut ListView[i32]) -> void:
    view[0] = 7  # error: invalid-assignment-target
```

```text
fn stale(items: mut List[i32]) -> i32:
    window := items.view(0, 2)
    items.append(4)
    window.len()  # panics with iterator-invalidated
```

> **Why.** A view fails fast, as Java's `subList` does: a window whose
> list changed shape would otherwise read the wrong elements without
> notice.

See also: [List Indexing](../lang/05-expressions.md#list-indexing),
[List And Optional Map](iter.md#list-and-optional-map).

## Map Methods

`std` gives `Map[K, V]` these methods, beside the language-tier ones:

| Receiver | Methods |
| --- | --- |
| `Map[K, V]` | `contains_key(self, key: K) -> bool`; `keys(self) -> List[K]`; `values(self) -> List[V]` |

```text
fn summary(stock: Map[string, i32]) -> string:
    if stock.contains_key("tea"):
        return "${stock.keys().len()} items"
    "no tea"
```

1. r[std-collections.map.contains-key] `contains_key(key)` is true exactly when the map has an entry whose key equals `key`.
2. r[std-collections.map.keys] `keys` returns the map's keys in [insertion order](../lang/04-type-system.md#r-types.map.order).
3. r[std-collections.map.values] `values` returns the map's values in the same order.
4. r[std-collections.map.snapshot] Each returned list is a snapshot: a later change to the map does not change it.

See also: [Map Key Types](../lang/04-type-system.md#map-key-types).
