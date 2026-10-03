# Collections

Status: standard library specification draft.

This chapter defines the part of `std.collections` that `lib/std` writes
in ordinary hd over the language tier:

- the `List` methods `view` and `chunks`;
- the `ListView[T]` type that `view` returns;
- the `Map` methods `contains_key`, `keys`, and `values`;
- the double-ended queue `Deque[T]` and the max-heap `Heap[T]`.

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

## Deque

A `Deque[T]` is a double-ended queue: a sequence that grows and shrinks
at both ends.

```text
use std.collections.Deque

fn rotate(items: List[i32]) -> List[i32]:
    let queue: mut Deque[i32] = Deque::new()
    for item in items:
        queue.push_back(item)
    match queue.pop_front():
        .Some(first) => queue.push_back(first)
        .None => pass
    let rotated: mut List[i32] = []
    for item in queue:
        rotated.append(item)
    rotated  # [2, 3, 1] for [1, 2, 3]
```

`Deque[T]` has this surface:

| Item | Signature |
| --- | --- |
| `new` | `Deque::new() -> mut Deque[T]` |
| `len`, `is_empty` | `len(self) -> i32`; `is_empty(self) -> bool` |
| `push_front`, `push_back` | `push_front(mut self, value: T) -> void`, and the same for `push_back` |
| `pop_front`, `pop_back` | `pop_front(mut self) -> T?`, and the same for `pop_back` |
| `front`, `back` | `front(self) -> T?`, and the same for `back` |
| `get` | `get(self, index: i32) -> T?` |
| iteration | `Iterable[T]` |
| equality | `Eq` when `T < Eq` |
| `Debug` | when `T < Debug` |

1. r[std-collections.deque.decl] `std.collections` declares `Deque[T]` with private fields, the surface in the table above, and no other method or trait implementation. Code imports it, as in `use std.collections.Deque`.
2. r[std-collections.deque.order] A deque holds a sequence of elements from its front to its back. `new` returns an empty deque.
3. r[std-collections.deque.len] `len` returns the number of elements, and `is_empty` is true exactly when that number is 0.
4. r[std-collections.deque.push] `push_front(value)` adds `value` before the front, and `push_back(value)` adds it after the back.
5. r[std-collections.deque.pop] `pop_front` removes the front element and returns it in `.Some`, and `pop_back` does the same at the back. On an empty deque, each returns `.None` and changes nothing.
6. r[std-collections.deque.ends] `front` and `back` return the front and the back element without removing it, or `.None` when the deque is empty.
7. r[std-collections.deque.get] `get(i)` returns the element `i` places from the front in `.Some`, so `get(0)` is the front. An `i` that is negative or not less than `len` gives `.None`.
8. r[std-collections.deque.iter] Iterating a deque yields its elements from front to back, each as `T`.
9. r[std-collections.deque.invalidate] A push, or a pop that removes an element, invalidates every iterator taken from the deque. The next `next` call of such an iterator is a checked runtime panic. Panic: `iterator-invalidated`.
10. r[std-collections.deque.eq] Two deques are equal when they have the same length and equal elements in the same order from the front, as two lists are.

```text
use std.collections.Deque

fn grow(queue: mut Deque[i32]) -> void:
    for item in queue:  # panics with iterator-invalidated after the push
        queue.push_back(item)
```

> **Note.** `lib/std` builds a deque as a ring buffer over a `List[T?]`.
> A push into a full buffer first copies the elements into a buffer
> twice its size, so each push and pop takes amortized constant time,
> and `get` takes constant time.

> **Why.** `get` returns an optional, as Rust's `VecDeque::get` does: a
> queue's length changes often, so a missing element is an expected
> case. A deque fails fast during iteration, as a list does, since a push
> can move every element.

## Heap

A `Heap[T]` is a binary max-heap, as Rust's `BinaryHeap` is: `pop`
removes its largest element.

```text
use std.cmp.Reverse
use std.collections.Heap

fn largest(items: List[i32]) -> i32?:
    let heap: mut Heap[i32] = Heap::new()
    for item in items:
        heap.push(item)
    heap.pop()  # .Some(9) for [5, 9, 2]

fn smallest(items: List[i32]) -> i32?:
    let heap: mut Heap[Reverse[i32]] = Heap::new()
    for item in items:
        heap.push(Reverse { value: item })
    heap.pop().map(fn(top: Reverse[i32]) -> i32: top.value)  # .Some(2) for [5, 9, 2]
```

`Heap[T < Ord]` has this surface:

| Item | Signature |
| --- | --- |
| `new` | `Heap::new() -> mut Heap[T]` |
| `len`, `is_empty` | `len(self) -> i32`; `is_empty(self) -> bool` |
| `push` | `push(mut self, value: T) -> void` |
| `pop` | `pop(mut self) -> T?` |
| `peek` | `peek(self) -> T?` |
| `to_sorted_list` | `to_sorted_list(self) -> List[T]` |
| `Debug` | when `T < Debug` |

1. r[std-collections.heap.decl] `std.collections` declares `Heap[T < Ord]` with private fields, the surface in the table above, and no other method or trait implementation. Code imports it, as in `use std.collections.Heap`.
2. r[std-collections.heap.len] `new` returns an empty heap. `len` returns the number of elements, and `is_empty` is true exactly when that number is 0.
3. r[std-collections.heap.push] `push(value)` adds `value` to the heap.
4. r[std-collections.heap.pop] `pop` removes a largest element by `Ord` and returns it in `.Some`. On an empty heap, it returns `.None`.
5. r[std-collections.heap.peek] `peek` returns a largest element without removing it, or `.None` when the heap is empty.
6. r[std-collections.heap.ties] When several elements are equal by `cmp`, which of them `pop` and `peek` return is not specified.
7. r[std-collections.heap.sorted] `to_sorted_list` returns a new list of every element in ascending order by `Ord`, and leaves the heap unchanged.
8. r[std-collections.heap.min] `std` has no separate min-heap: a `Heap[Reverse[T]]` pops the smallest `T` first, by [`std-cmp.reverse.order`](cmp.md#r-std-cmp.reverse.order).

> **Note.** `lib/std` keeps the elements in a `List[T?]` in heap order,
> so `push` and `pop` take logarithmic time and `peek` constant time.

> **Why.** `to_sorted_list` copies, as `sorted_by` does, because hd has no
> consuming methods; its ascending order is that of Rust's
> `into_sorted_vec`. A heap has no `Eq` and no iteration, as in Rust:
> two heaps with the same elements may hold them in different orders.

See also: [Reverse](cmp.md#reverse), [Debug For Standard Types](format.md#debug-for-standard-types).
