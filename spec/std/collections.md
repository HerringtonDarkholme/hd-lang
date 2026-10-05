# Collections

Status: standard library specification draft.

This chapter defines the part of `std.collections` that `lib/std` writes
in ordinary hd over the language tier:

- the `List` methods `view` and `chunks`, and the access and building
  methods such as `get`, `take`, and `extend`;
- the `ListView[T]` type that `view` returns;
- the `List` helpers, such as `sorted_by_key`, `group_by`, and `windows`,
  and the function `counts`;
- the `Map` methods `contains_key`, `keys`, `values`, and `get_or`;
- the set `Set[T]`, the double-ended queue `Deque[T]`, and the max-heap
  `Heap[T]`.

The language tier keeps what the compiler knows about a list
([List Indexing](../lang/05-expressions.md#list-indexing),
[Built-In Methods](../lang/10-modules.md#built-in-methods),
[Iterator Invalidation](../lang/06-control-flow.md#iterator-invalidation)):

| Item | Why it stays in the language tier |
| --- | --- |
| `items[i]`, `len`, `iter`, `push`, `pop`, `insert`, `remove_at`, `clear` | built-in indexing and intrinsics over the list's representation |
| the captured length | [`flow.for.length`](../lang/06-control-flow.md#r-flow.for.length), a language rule, defines when a change invalidates an iterator |

## List Methods

`std` gives `List[T]` these methods, beside the language-tier ones:

| Receiver | Methods |
| --- | --- |
| `List[T]` | `view(self, start: usize, end: usize) -> ListView[T]`; `chunks(self, size: usize) -> List[List[T]]` |

1. r[std-collections.list.chunks] `chunks(size)` returns the list's elements in order, in consecutive pieces of `size` elements. Only the last piece may be shorter.
2. r[std-collections.list.chunks.size] A `size` below 1 panics. Panic: `explicit-panic`.

```text
fn pairs(items: List[i32]) -> List[List[i32]]:
    items.chunks(2)  # [[1, 2], [3]] for [1, 2, 3]
```

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
    sum + view[view.len() - 1]
```

`ListView[T]` has this surface:

| Item | Signature |
| --- | --- |
| `len` | `len(self) -> usize` |
| `to_list` | `to_list(self) -> mut List[T]` |
| index read | `Index[usize]` with `Out = T` |
| iteration | `Iterable[T]` |

1. r[std-collections.view] `view(start, end)` returns a `ListView[T]` of the list's elements at indices `start` up to, not including, `end`.
2. r[std-collections.view.type] `std.collections` declares `ListView[T]` with private fields and the surface in the table above, and no other method or trait implementation.
3. r[std-collections.view.import] `ListView` is not a prelude name, so code that names it imports it, as in `use std.collections.ListView`.
4. r[std-collections.view.no-copy] Creating a view copies no element.
5. r[std-collections.view.range] A `start` or `end` that is greater than the list's length is a checked runtime panic. Panic: `index-out-of-bounds`.
6. r[std-collections.view.reversed] A `start` greater than `end` is a checked runtime panic. Panic: `index-out-of-bounds`.
7. r[std-collections.view.len] `len` returns the number of elements in the view, `end - start`.
8. r[std-collections.view.index] `view[i]` reads the list's element at index `start + i`.
9. r[std-collections.view.index.range] A view index that is not less than the view's `len` is a checked runtime panic, even when the list has an element there. Panic: `index-out-of-bounds`.
10. r[std-collections.view.iter] Iterating a view yields its elements in order, each as `T`.
11. r[std-collections.view.to-list.mut] `to_list` returns a new `mut List[T]` that holds the view's elements in order, as the slice `items[start..end]` does, so `let mut copy = view.to_list()` may grow the copy.
12. r[std-collections.view.read-only] A view has no `IndexSet` implementation, so assigning to `view[i]` is an error. Error: `invalid-assignment-target`.
13. r[std-collections.view.writes] An element write to the list, as `items[i] = v`, shows through every view of it, so a later read of the view gives the new value.
14. r[std-collections.view.invalidate] Changing the list's length invalidates every view of it, as [`flow.for.invalidate`](../lang/06-control-flow.md#r-flow.for.invalidate) does for an iterator.
15. r[std-collections.view.invalidate.anywhere] A length change invalidates a view wherever it happens, so appending past a view's end invalidates it too.
16. r[std-collections.view.invalid-use] The next use of an invalidated view is a checked runtime panic: a `len`, index, or `to_list` call, an `iter()` call, or the next `next` call of an iterator taken from it. Panic: `iterator-invalidated`.

```text
use std.collections.ListView

fn invalid(view: mut ListView[i32]) -> void:
    view[0] = 7  # error: invalid-assignment-target
```

```text
fn stale(items: mut List[i32]) -> usize:
    window := items.view(0, 2)
    items.push(4)
    window.len()  # panics with iterator-invalidated
```

> **Why.** A view fails fast, as Java's `subList` does: a window whose
> list changed length would otherwise read the wrong elements without
> notice.

See also: [List Indexing](../lang/05-expressions.md#list-indexing),
[List And Optional Map](iter.md#list-and-optional-map).

## List Helpers

`std` also gives `List[T]` helpers that sort, group, split, and search a
list without a hand-written loop:

```text
data Task:
    name: string
    owner: string
    hours: i32

fn plan(tasks: List[Task]) -> List[Task]:
    tasks.sorted_by_key(fn(task: Task) -> i32: task.hours)

fn by_owner(tasks: List[Task]) -> Map[string, List[Task]]:
    tasks.group_by(fn(task: Task) -> string: task.owner)

fn has_long(tasks: List[Task]) -> bool:
    tasks.any(fn(task: Task) -> bool: task.hours > 8)

fn steps(readings: List[i32]) -> List[List[i32]]:
    readings.windows(2)  # [[1, 4], [4, 9]] for [1, 4, 9]
```

| Receiver | Methods |
| --- | --- |
| `List[T]` | `sorted_by_key[K < Ord](self, key: fn(T) -> K) -> List[T]`; `group_by[K < Eq & Hash](self, key: fn(T) -> K) -> Map[K, List[T]]`; `partition(self, keep: fn(T) -> bool) -> (List[T], List[T])`; `any(self, test: fn(T) -> bool) -> bool`; `all(self, test: fn(T) -> bool) -> bool`; `find(self, test: fn(T) -> bool) -> T?`; `flat_map[U](self, transform: fn(T) -> List[U]) -> List[U]`; `windows(self, size: usize) -> List[List[T]]` |
| `List[T]`, when `T < Eq` | `contains(self, value: T) -> bool`; `index_of(self, value: T) -> usize?` |
| `List[T]`, when `T < Ord` | `sorted(self) -> List[T]`; `min(self) -> T?`; `max(self) -> T?` |

1. r[std-collections.helper.unchanged] Each helper leaves its receiver unchanged, and each one that returns a list returns a new list.
2. r[std-collections.helper.callback-order] A helper calls its callback on the elements in list order, at most once each.
3. r[std-collections.helper.callback-row] Each callback has the empty row, as `Iterator.filter`'s `keep` does. A function value whose row lists a requirement key does not fit it. Error: `type-mismatch`.
4. r[std-collections.helper.sorted-by-key] `sorted_by_key(key)` returns the elements in ascending order of `key(element)` by `Ord`, and calls `key` once per element.
5. r[std-collections.helper.sorted] `sorted` returns the elements in ascending order by `Ord`.
6. r[std-collections.helper.stable] Both sorts are stable: elements that compare equal keep their order.
7. r[std-collections.helper.group-by] `group_by(key)` maps each key that `key` returns to the list of the elements that gave it, in list order.
8. r[std-collections.helper.group-by.order] The map's keys are in the order each was first returned, by the map's [insertion order](../lang/04-type-system.md#r-types.map.order).
9. r[std-collections.helper.partition] `partition(keep)` returns the elements for which `keep` is true, then the rest, each in list order.
10. r[std-collections.helper.any-all] `any(test)` is true when `test` is true for some element, and `all(test)` when it is true for every element. So on an empty list `any` is false and `all` is true.
11. r[std-collections.helper.find] `find(test)` returns the first element for which `test` is true in `.Some`, or `.None` when there is none.
12. r[std-collections.helper.short-circuit] `any`, `all`, and `find` call `test` on no element after the first one that decides the result.
13. r[std-collections.helper.flat-map] `flat_map(transform)` joins the lists that `transform` returns, in list order.
14. r[std-collections.helper.windows] `windows(size)` returns every run of `size` consecutive elements, in order of its first index, so the runs overlap.
15. r[std-collections.helper.windows.short] A list with fewer than `size` elements gives `[]`.
16. r[std-collections.helper.windows.size] A `size` below 1 panics, as it does for `chunks`. Panic: `explicit-panic`.
17. r[std-collections.helper.contains] `contains(value)` is true when an element equals `value` by `Eq`. `index_of(value)` returns the index of the first such element in `.Some`, or `.None`.
18. r[std-collections.helper.min-max.first] `min` returns the first smallest element and `max` the first largest one by `Ord`, each in `.Some`. An empty list gives `.None`.

| Call | Result |
| --- | --- |
| `[3, 1, 2].partition(fn(n: i32) -> bool: n > 1)` | `([3, 2], [1])` |
| `[1, 2].flat_map(fn(n: i32) -> List[i32]: [n, n])` | `[1, 1, 2, 2]` |
| `[1, 2, 3].windows(2)` | `[[1, 2], [2, 3]]` |
| `[1, 2].windows(3)` | `[]` |
| `[5, 7, 5].index_of(5)` | `.Some(0)` |
| `[2, 9, 4].max()` | `.Some(9)` |

```text
fn paged(items: List[i32]) -> List[List[i32]]:
    items.windows(0)  # panics with explicit-panic
```

> **Why.** The names are Rust's (`sort_by_key`, `partition`, `windows`,
> `any`, `all`, `find`, `flat_map`, `contains`, `min`, `max`), Kotlin's
> `groupBy`, and Python's `list.index`. A sort copies, as `sorted_by`
> does, since hd has no consuming methods. `min` and `max` both pick the
> first of equal elements, as Python's `min` and `max` do, so one rule
> covers both.

### Counts

`counts` tallies how often each element occurs:

```text
use std.collections.counts

fn tally(words: List[string]) -> Map[string, usize]:
    counts(words)  # {"a": 2, "b": 1} for ["a", "b", "a"]
```

1. r[std-collections.counts.decl] `std.collections` declares `pub fn counts[T < Eq & Hash](items: List[T]) -> Map[T, usize]`. Code imports it, as in `use std.collections.counts`.
2. r[std-collections.counts.value] The map has one entry per distinct element of `items`, and its value is the number of elements equal to that one.
3. r[std-collections.counts.order] The keys are in the order of their first occurrence in `items`, so an empty list gives an empty map.

> **Why.** `counts` is Python's `collections.Counter` as a plain map, so
> the `Map` methods read it.

## Map Methods

`std` gives `Map[K, V]` these methods, beside the language-tier ones:

| Receiver | Methods |
| --- | --- |
| `Map[K, V]` | `contains_key(self, key: K) -> bool`; `keys(self) -> List[K]`; `values(self) -> List[V]`; `get_or(self, key: K, fallback: V) -> V`; `is_empty(self) -> bool` |

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
5. r[std-collections.map.get-or] `get_or(key, fallback)` returns the value of the entry whose key equals `key`, or `fallback` when there is none.
6. r[std-collections.map.is-empty] `is_empty` is true exactly when `len()` is 0.

See also: [Map Key Types](../lang/04-type-system.md#map-key-types).

## Set

A `Set[T]` holds distinct elements, in the order they were inserted:

```text
use std.collections.Set

fn unique(words: List[string]) -> List[string]:
    let seen: mut Set[string] = Set::new()
    let kept: mut List[string] = []
    for word in words:
        if seen.insert(word):
            kept.push(word)
    kept  # ["b", "a"] for ["b", "a", "b"]
```

`Set[T < Eq & Hash]` has this surface:

| Item | Signature |
| --- | --- |
| `new` | `Set::new() -> mut Set[T]` |
| `len`, `is_empty` | `len(self) -> usize`; `is_empty(self) -> bool` |
| `contains` | `contains(self, value: T) -> bool` |
| `insert`, `remove` | `insert(mut self, value: T) -> bool`, and the same for `remove` |
| iteration | `Iterable[T]` |
| equality | `Eq` |
| `Debug` | when `T < Debug` |

1. r[std-collections.set.decl] `std.collections` declares `Set[T < Eq & Hash]` with private fields, the surface in the table above, and no other method or trait implementation. Code imports it, as in `use std.collections.Set`.
2. r[std-collections.set.distinct] A set holds elements no two of which are equal by `Eq`. `new` returns an empty set.
3. r[std-collections.set.len] `len` returns the number of elements, and `is_empty` is true exactly when that number is 0.
4. r[std-collections.set.contains] `contains(value)` is true exactly when the set has an element equal to `value`.
5. r[std-collections.set.insert] `insert(value)` adds `value` and returns `true` when the set has no element equal to it. Otherwise it changes nothing and returns `false`.
6. r[std-collections.set.remove] `remove(value)` removes the element equal to `value` and returns `true`. When there is none, it changes nothing and returns `false`.
7. r[std-collections.set.iter] Iterating a set yields its elements in the order that a map's keys would have after the same inserts and removes, by [`types.map.replace`](../lang/04-type-system.md#r-types.map.replace).
8. r[std-collections.set.invalidate] An `insert` or `remove` that changes the set invalidates its iterators, as [`flow.for.invalidate`](../lang/06-control-flow.md#r-flow.for.invalidate) does for a map. Panic: `iterator-invalidated`.
9. r[std-collections.set.eq] Two sets are equal when each element of one equals an element of the other, in any order.

> **Note.** `lib/std` builds a set over a `Map` whose values carry no
> meaning, so `insert`, `remove`, and `contains` take expected constant
> time, by [`module.map.complexity`](../lang/10-modules.md#r-module.map.complexity).

> **Why.** `insert` and `remove` report whether they changed the set, as
> Rust's `HashSet` methods do, so a loop that skips duplicates needs no
> second lookup. A set keeps insertion order, as a map does, so its
> iteration and `Debug` text are deterministic.

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
        rotated.push(item)
    rotated  # [2, 3, 1] for [1, 2, 3]
```

`Deque[T]` has this surface:

| Item | Signature |
| --- | --- |
| `new` | `Deque::new() -> mut Deque[T]` |
| `len`, `is_empty` | `len(self) -> usize`; `is_empty(self) -> bool` |
| `push_front`, `push_back` | `push_front(mut self, value: T) -> void`, and the same for `push_back` |
| `pop_front`, `pop_back` | `pop_front(mut self) -> T?`, and the same for `pop_back` |
| `front`, `back` | `front(self) -> T?`, and the same for `back` |
| `get` | `get(self, index: usize) -> T?` |
| iteration | `Iterable[T]` |
| equality | `Eq` when `T < Eq` |
| `Debug` | when `T < Debug` |

1. r[std-collections.deque.decl] `std.collections` declares `Deque[T]` with private fields, the surface in the table above, and no other method or trait implementation. Code imports it, as in `use std.collections.Deque`.
2. r[std-collections.deque.order] A deque holds a sequence of elements from its front to its back. `new` returns an empty deque.
3. r[std-collections.deque.len] `len` returns the number of elements, and `is_empty` is true exactly when that number is 0.
4. r[std-collections.deque.push] `push_front(value)` adds `value` before the front, and `push_back(value)` adds it after the back.
5. r[std-collections.deque.pop] `pop_front` removes the front element and returns it in `.Some`, and `pop_back` does the same at the back. On an empty deque, each returns `.None` and changes nothing.
6. r[std-collections.deque.ends] `front` and `back` return the front and the back element without removing it, or `.None` when the deque is empty.
7. r[std-collections.deque.get] `get(i)` returns the element `i` places from the front in `.Some`, so `get(0)` is the front. An `i` that is not less than `len` gives `.None`.
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
| `len`, `is_empty` | `len(self) -> usize`; `is_empty(self) -> bool` |
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
    let all: mut List[i32] = []
    all.extend(head.filter(fn(n: i32) -> bool: n > 0))
    all.extend(tail)
    all.sorted_by(fn(a: i32, b: i32) -> Ordering: b.cmp(a))
```

| Receiver | Methods |
| --- | --- |
| `List[T]` | `get(self, index: usize) -> T?`; `is_empty(self) -> bool`; `first(self) -> T?`; `last(self) -> T?`; `take(self, count: usize) -> List[T]`; `filter(self, keep: fn(T) -> bool) -> List[T]`; `reversed(self) -> List[T]`; `sorted_by(self, compare: fn(T, T) -> Ordering) -> List[T]`; `zip[U](self, other: List[U]) -> List[(T, U)]` |
| `mut List[T]` | `extend(mut self, other: List[T]) -> void` |

1. r[std-collections.access.get] `get(index)` returns `.Some` of the element at `index`, or `.None` when `index` is not less than `len()`. It never panics.
2. r[std-collections.access.is-empty] `is_empty` is true exactly when `len()` is 0.
3. r[std-collections.access.first-last] `first` returns the element at index 0 and `last` the element at index `len() - 1`, each in `.Some`. An empty list gives `.None`.
4. r[std-collections.access.take] `take(count)` returns a new list of the first `count` elements, or of every element when `count` is at least `len()`.
5. r[std-collections.access.filter] `filter(keep)` returns a new list of the elements for which `keep` is true, in list order.
6. r[std-collections.access.reversed] `reversed` returns a new list of the elements in reverse order.
7. r[std-collections.access.sorted-by] `sorted_by(compare)` returns a new list in ascending order by `compare`. The sort is stable.
8. r[std-collections.access.zip] `zip(other)` returns a new list of pairs `(self[i], other[i])` for each index `i` of both lists, so it stops at the shorter one.
9. r[std-collections.access.extend] `extend(other)` appends each element of `other` to the receiver, in order, as `push` does. It changes the length, so it invalidates iterators by [`flow.for.invalidate`](../lang/06-control-flow.md#r-flow.for.invalidate).
10. r[std-collections.access.extend.self] `items.extend(items)` appends a copy of the elements `items` held before the call, so it doubles the list.
11. r[std-collections.access.helper-rules] The callbacks of `filter` and `sorted_by` follow [`std-collections.helper.callback-row`](#r-std-collections.helper.callback-row): each has the empty row.

| Call | Result |
| --- | --- |
| `[4, 5].get(1)` | `.Some(5)` |
| `[4, 5].get(2)` | `.None` |
| `[1, 2, 3].take(2)` | `[1, 2]` |
| `[1, 2].take(5)` | `[1, 2]` |
| `[1, 2, 3].zip(["a", "b"])` | `[(1, "a"), (2, "b")]` |

> **Why.** `get` is Rust's `slice::get`, the checked read beside
> `items[i]`. `take` is Kotlin's `List.take`: like `List.map`, the list
> method is eager and returns a list, while `Iterator.take` stays lazy.
> `extend` is Rust's `Vec::extend`, and replaces `items = items + more`.

See also: [Built-In Methods](../lang/10-modules.md#built-in-methods),
[Iterator Adapters](iter.md#iterator-adapters).
