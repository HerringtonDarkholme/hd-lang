# Iterators

Status: standard library specification draft.

This chapter defines the part of `std.iter` that `lib/std` writes in
ordinary hd over the language tier:

- the iterator adapters `filter`, `take`, `enumerate`, `map`, `fold`, and
  `collect`, with their laziness and ordering;
- the `FromIterator` trait and the collect targets `List`, `Map`,
  `Result`, and `T?`;
- `map` on `List[T]` and on `T?`.

The language tier keeps what the compiler knows by name
([Iteration Protocols](../06-control-flow.md#iteration-protocols)):

| Item | Why it stays in the language tier |
| --- | --- |
| `Iterator[T]` | a prelude type that `Iterable.iter` returns and `for` accepts directly, so `for` depends on it |
| `Iterator::from_fn`, `next` | the only way to build and advance an `Iterator[T]`, since `step` is private |
| `Iterable[T]` and its impls | the lang item that drives `for` and comprehensions; only `List` and `Map` implement it, not `Iterator` |

Nothing in the language tier names an adapter, `collect`, or
`FromIterator`. `collect`'s default target, `List[T]`, is an ordinary
[type-argument default](../04-type-system.md#type-argument-defaults),
which needs no compiler knowledge of `collect`.

## Iterator Adapters

The prelude `Iterator[T]` also has **iterator adapters**: methods that wrap
an iterator in a new one, or drain it.

```text
fn first_evens(values: List[i32]) -> List[(i32, i32)]:
    values.iter().filter(fn(value): value % 2 == 0).enumerate().take(2).collect()
```

| Rule | Method | Result |
| --- | --- | --- |
| r[std-iter.adapter.filter] `filter` | `fn filter(mut self, keep: fn(T) -> bool) -> mut Iterator[T]` | a new iterator over the items of `self` for which `keep` returns `true` |
| r[std-iter.adapter.take] `take` | `fn take(mut self, count: i32) -> mut Iterator[T]` | a new iterator over the first `count` items of `self`, or fewer when `self` ends first |
| r[std-iter.adapter.enumerate] `enumerate` | `fn enumerate(mut self) -> mut Iterator[(i32, T)]` | a new iterator over `(index, item)` pairs, with indices counting from `0` |
| r[std-iter.adapter.map] `map` | `fn map[U](mut self, transform: fn(T) -> U) -> mut Iterator[U]` | a new iterator over `transform(item)` for each item of `self`, in order |
| r[std-iter.adapter.fold] `fold` | `fn fold[A, R](mut self, initial: A, step: fn(A, T) -> A $ R) -> A $ R` | the accumulator after `step` has combined it with each remaining item of `self`, in order, starting from `initial` |
| r[std-iter.adapter.collect-defaulted] `collect` | `fn collect[C < FromIterator[T] = List[T]](mut self) -> C` | a `C` built from the remaining items of `self`, as [Collect Targets](#collect-targets) specifies |

1. r[std-iter.adapter.methods] The adapters are ordinary methods of the prelude `Iterator[T]`, so every iterator has them without a `use`.
2. r[std-iter.adapter.lazy] Calling `filter`, `take`, or `enumerate` does not advance `self`.
3. r[std-iter.adapter.lazy.map] Calling `map` does not advance `self` either.
4. r[std-iter.adapter.lazy.next] The returned iterator advances `self` only when its own `next` is called.
5. r[std-iter.adapter.take.limit] The iterator that `take` returns calls `next` on `self` at most `count` times.
6. r[std-iter.adapter.take.negative] A negative `count` panics when `take` is called. Panic: `explicit-panic`.
7. r[std-iter.adapter.fold.drain] `fold` advances `self` until `next` returns `.None`, which leaves `self` exhausted.
8. r[std-iter.adapter.mut-receiver] Each adapter takes `mut self`. Calling one on a readonly iterator is an error. Error: `mutable-receiver-required`.
9. r[std-iter.adapter.callback-row] The `keep` callback has the empty row. A function value whose row lists a requirement key does not fit it. Error: `type-mismatch`.
10. r[std-iter.adapter.callback-row.map] The `transform` callback of `map` has the empty row too.
11. r[std-iter.adapter.fold.row] The `step` callback of `fold` may have a requirement row `R`, and `fold` then requires `R`.
12. r[std-iter.adapter.callback-row.capture] A `keep` or `transform` closure whose body uses a requirement key has that key in its row, even inside a `$.with` block, so it does not fit. Error: `type-mismatch`.

```text
trait Logger

fn noisy(value: i32) -> bool $ Logger:
    value > 0

fn positives(values: List[i32]) -> List[i32] $ Logger:
    values.iter().filter(noisy).collect()  # error: type-mismatch

fn scoped(values: List[i32], logger: Logger) -> List[i32]:
    $.with(Logger=logger):
        values.iter().filter(fn(value): noisy(value)).collect()  # error: type-mismatch

fn drain(source: Iterator[i32]) -> List[i32]:
    source.collect()  # error: mutable-receiver-required
```

A callback that needs a provider captures the provider value from `$.use`
outside the closure, so its row stays empty:

```text
trait Logger:
    fn level(self) -> i32

fn above_level(values: List[i32]) -> List[i32] $ Logger:
    logger := $.use(Logger)
    values.iter().filter(fn(value): value > logger.level()).collect()
```

> **Why.** A lazy adapter's iterator calls `keep` or `transform` from its
> `next`, whose row is empty, so a stored callback cannot wait for
> providers. `fold` calls `step` before it returns, so the row passes
> through.

## Collect Targets

`collect` builds the collection that the expected type names:

```text
fn parse_port(text: string) -> Result[i32, string]:
    .Ok(text.len())

fn index(names: List[string]) -> Map[string, i32]:
    names.iter().map(fn(name): (name, name.len())).collect()

fn parse_all(lines: List[string]) -> Result[List[i32], string]:
    lines.iter().map(parse_port).collect()

fn count(values: List[i32]) -> i32:
    copied := values.iter().collect()
    copied.len()
```

The target implements the `std.iter` trait `FromIterator[T]`:

```text
trait FromIterator[T]:
    fn from_iter(items: mut Iterator[T]) -> Self
```

1. r[std-iter.collect.trait] `std.iter` declares `FromIterator[T]`, whose `from_iter` builds a `Self` from the items of an iterator.
2. r[std-iter.prelude.from-iterator] `std.iter` also declares `FromIterator`, which is not a prelude name. Code imports it to implement or name it, as in `use std.iter.FromIterator`, and a `collect` call needs no import.
3. r[std-iter.collect.call] `collect` returns `C::from_iter(self)`.
4. r[std-iter.collect.target] `C` is solved like any call-site type argument: from the expected type, or from an explicit list such as `collect::[Map[string, i32]]()`.
5. r[std-iter.collect.target-default] When nothing determines `C`, its declared [default](../04-type-system.md#type-argument-defaults) `List[T]` applies.
6. r[std-iter.collect.bound] A target that does not implement `FromIterator[T]` is an error. Error: `unsatisfied-trait-bound`.

The standard library implements `FromIterator` for these prelude types:

| Rule | Target | Items | Result |
| --- | --- | --- | --- |
| r[std-iter.collect.list] List | `List[T]` | `T` | every remaining item, in order |
| r[std-iter.collect.map] Map | `Map[K, V]` | `(K, V)` | one entry per pair; for an equal key the later value wins, and the key keeps its first position |
| r[std-iter.collect.result] All results | `Result[C, E]`, where `C < FromIterator[T]` | `Result[T, E]` | `.Ok` of the `C` collected from the `.Ok` payloads, or the first `.Err` |
| r[std-iter.collect.option] All values | `C?`, where `C < FromIterator[T]` | `T?` | `.Some` of the `C` collected from the `.Some` payloads, or `.None` at the first `.None` |

1. r[std-iter.collect.drain] Collecting into a `List` or a `Map` advances `self` until `next` returns `.None`, which leaves `self` exhausted.
2. r[std-iter.collect.stop] Collecting into a `Result` or an optional stops at the first `.Err` or `.None` and leaves the rest of `self` unread.
3. r[std-iter.collect.map-key] A `Map` target needs `K < Eq & Hash`, as every map does.

```text
fn total(values: List[i32]) -> i32:
    let size: i32 = values.iter().collect()  # error: unsatisfied-trait-bound
    size
```

> **Why.** The expected type already names the target, as Rust's
> `collect` does. A `Map` built by `collect` agrees with a map literal and
> a map comprehension on equal keys.

> **Note.** The standard library writes both of its `Map` implementations
> with that bound, as `impl[K < Eq & Hash, V] FromIterator[(K, V)] for Map[K, V]`
> and `impl[K < Eq & Hash, V] Iterable[(K, V)] for Map[K, V]`. Neither
> leaves the key unbounded.

## List And Optional Map

`std` also gives `List[T]` and `T?` a `map` method:

| Receiver | Method |
| --- | --- |
| `List[T]` | `map[U](self, transform: fn(T) -> U) -> List[U]` |
| `T?` | `map[U](self, transform: fn(T) -> U) -> U?` |

1. r[std-iter.method.map] `List.map` and optional `map` are non-suspending and evaluate the transform in source order.

See also: [Built-In Methods](../10-modules.md#built-in-methods), which
lists the methods of the built-in types that stay in the language tier.
