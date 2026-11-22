# Collection, Iterator, Optional And Result Methods

None of these need an import, except `Set`, `Deque`, `Heap` and `counts`
from `std.collections`.

## List[T]

| Call | Does |
| --- | --- |
| `xs.len()`, `xs.is_empty()` | size |
| `xs[i]` | the element; panics when out of range |
| `xs.get(i)`, `xs.first()`, `xs.last()` | the element as `T?` |
| `xs.contains(v)`, `xs.index_of(v)` | search by value; `index_of` gives `usize?` |
| `xs.slice(start, end)`, `xs.take(n)` | a new list |
| `xs.sorted()`, `xs.reversed()`, `xs.unique()` | a new list |
| `xs.sorted_by_key(f)`, `xs.min()`, `xs.max()`, `xs.sum()` | ordering and totals |
| `xs.map(f)`, `xs.filter(f)`, `xs.flat_map(f)` | a new list, eagerly |
| `xs.any(f)`, `xs.all(f)`, `xs.find(f)` | search with a callback |
| `xs.group_by(f)` | `Map[K, List[T]]` |
| `xs.partition(f)` | two lists: matching and not |
| `xs.zip(ys)`, `xs.chunks(n)`, `xs.windows(n)` | pairs and runs |
| `xs.iter()` | a lazy iterator |

These change the list, so they need `let mut xs` (or a `&mut List[T]`):

| Call | Does |
| --- | --- |
| `xs.push(v)` | add one element at the end |
| `xs.extend(ys)` | add every element of another list |
| `xs.pop()` | remove and return the last, as `T?` |
| `xs.insert(i, v)`, `xs.remove_at(i)` | at an index; `remove_at` returns the element |
| `xs[i] = v`, `xs.swap(i, j)` | replace or exchange |
| `xs.sort()`, `xs.sort_by_key(f)`, `xs.reverse()`, `xs.dedup()` | reorder in place |
| `xs.retain(f)`, `xs.truncate(n)`, `xs.clear()` | drop elements |

Leave a callback's parameter type off, as in `fn(task): task.hours`, and
it is inferred: most list helpers lend the element as `&T`. `sorted`, `min` and `max` need
the element to be `Ord`.

```hd
struct Task:
    name: string
    owner: string
    hours: i32

tasks := [
    Task { name: "write", owner: "ada", hours: 5 },
    Task { name: "test", owner: "grace", hours: 2 },
    Task { name: "ship", owner: "ada", hours: 1 },
]
quick := tasks.sorted_by_key(fn(task): task.hours)
by_owner := tasks.group_by(fn(task): task.owner)
names := tasks.map(fn(task): task.name)
long := tasks.filter(fn(task): task.hours > 1)
let mut hours = tasks.map(fn(task): task.hours)
hours.sort()
println("${quick[0].name} ${by_owner["ada"].len()} ${names.join(",")} ${long.len()} ${debug(hours)} ${hours.sum()}")
```

```output
ship 2 write,test,ship 2 [1, 2, 5] 8
```

## Map[K, V]

| Call | Does |
| --- | --- |
| `m.len()`, `m.is_empty()` | size |
| `m.get(key)` | the value as `V?` |
| `m[key]` | the value; panics when the key is missing |
| `m.contains_key(key)` | `bool` |
| `m.keys()`, `m.values()`, `m.entries()` | lists of copies |
| `for (key, value) in m` | each entry, as `(&K, &V)` |
| `m[key] = v` | insert or replace (needs `let mut`) |
| `m.get_or_insert(key, default)` | `&mut V`, inserting the default when missing |
| `m.remove(key)` | remove and return, as `V?` |
| `m.retain(f)`, `m.extend(other)` | bulk changes |

`m.remove(key)` returns an optional that must be used: write
`_ := m.remove(key)` to drop it.

```hd
use std.collections.counts

words := "the cat and the hat".split_whitespace()
tally := counts(words)
let mut lengths: Map[usize, List[string]] = {}
for word in words:
    lengths.get_or_insert(word.len(), []).push(word)
println("${tally["the"]} ${tally.most_common(1)[0]._0} ${lengths[3].len()} ${lengths.keys().sorted().len()}")
```

```output
2 the 5 1
```

## Set, Deque, Heap

```hd
use std.collections.{Deque, Heap, Set}

let mut seen: Set[string] = Set::new()
let fresh = 0
for tag in ["a", "b", "a"]:
    if seen.insert(tag):            # true when the value was new
        fresh += 1

let mut queue: Deque[i32] = Deque::new()
queue.push_back(1)
queue.push_back(2)
front := queue.pop_front()

let mut heap: Heap[i32] = Heap::new()
heap.push(5)
heap.push(9)
heap.push(1)
println("$fresh ${seen.contains("b")} ${debug(front)} ${debug(heap.pop())} ${heap.len()}")
```

```output
2 true Option.Some(1) Option.Some(9) 2
```

`Heap.pop()` returns the largest element.

## Iterators

`xs.iter()`, `m.iter()`, a range, and `s.chars()` give lazy iterators.

| Adapter (lazy) | Gives |
| --- | --- |
| `map(f)`, `filter(f)`, `flat_map(f)` | changed or kept items |
| `filter_map(f)` | the present results of `f(item)` |
| `enumerate()` | `(index, item)` pairs; lists also offer `xs.enumerate()` |
| `zip(other)`, `chain(other)` | paired or appended items |
| `take(n)`, `skip(n)`, `take_while(f)` | part of the items |

| Draining call | Gives |
| --- | --- |
| `collect()` | a collection chosen by the expected type; `char` items also build a `string` |
| `count()` | how many items |
| `last()` | the final remaining item, or `.None` |
| `position(f)` | the first matching remaining index; borrows and advances the cursor |
| `for_each(f)` | visits each remaining item by value |
| `sum()`, `product()` | a total, of the expected type |
| `fold(initial, f)` | an accumulated value |
| `any(f)`, `all(f)`, `find(f)` | search |

```hd
let squares: List[i32] = (1..=5).iter().map(fn(n): n * n).filter(fn(n): n % 2 == 1).collect()
let index: Map[string, usize] = ["a", "bb"].iter().map(fn(name): (name, name.len())).collect()
total := (1..=4).iter().fold(0, fn(sum, n): sum + n)
println("${debug(squares)} ${index["bb"]} $total")
```

```output
[1, 9, 25] 2 10
```

## Optional and Result

| Call | On | Gives |
| --- | --- | --- |
| `unwrap()` | both | the value, or a panic; `Result` errors require `Debug` |
| `unwrap_or(fallback)` | both | the value, or the fallback |
| `unwrap_or_else(f)` | both | the value, or a fallback computed only on failure |
| `or_else(f)` | both | this success, or an optional/result computed on failure |
| `filter(f)` | `T?` | keeps a present value when `f(&value)` returns true |
| `expect(message)` | both | the value, or a panic |
| `map(f)` | both | the same wrapper around `f(value)` |
| `and_then(f)` | both | chains a step that returns the same kind of wrapper |
| `is_some()`, `is_none()` | `T?` | `bool` |
| `or(other)` | `T?` | this optional, or `other` when it is `.None` |
| `is_ok()`, `is_err()` | `Result` | `bool` |
| `ok()`, `err()` | `Result` | the value or the error, as an optional |
| `map_err(f)` | `Result` | the same result with a changed error |

`unwrap()` uses the standard panic message; `expect(message)` supplies your message.
See [language/errors.md](../language/errors.md).
