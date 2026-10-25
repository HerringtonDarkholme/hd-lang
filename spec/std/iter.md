# Iterators

Status: standard library specification draft.

This chapter defines the part of `std.iter` that `lib/std` writes in
ordinary hd over the language tier:

- the iterator adapters `filter`, `take`, `enumerate`, `map`, `fold`, and
  `collect`, with their laziness and ordering;
- the adapters `skip`, `take_while`, `zip`, `chain`, and `flat_map`, and
  the draining `any`, `all`, `find`, and `count`;
- the `FromIterator` trait and the collections `collect` builds:
  `List`, `Map`, `Result`, and `T?`;
- `map` on `List[T]` and on `T?`.

The language tier keeps what the compiler knows by name
([Iteration Protocols](../lang/06-control-flow.md#iteration-protocols)):

| Item | Why it stays in the language tier |
| --- | --- |
| `Iterator[T]` | the prelude cursor trait that `for` advances through `next` |
| `ListIter[T]` and `MapEntries[K, V]` | the cursor types of a list's and a map's `iter`, which [Built-In Methods](../lang/10-modules.md#built-in-methods) names |
| `Iterable[T]` and its impls | the lang item that drives `for` and comprehensions; `List`, `Map`, and the [iterable ranges](../lang/06-control-flow.md#range-iteration) implement it, not `Iterator` |

Nothing in the language tier names an adapter, `collect`, or
`FromIterator`. `collect`'s default target, `List[T]`, is an ordinary
[type-argument default](../lang/04-type-system.md#type-argument-defaults),
which needs no compiler knowledge of `collect`.

## Collection Cursors

A list's `iter` returns a `ListIter[T]` and a map's a `MapEntries[K, V]`,
as their `Iterable` implementations' `IntoIter`:

```text
use std.iter.ListIter

fn evens(values: List[i32]) -> List[i32]:
    let cursor: ListIter[i32] = values.iter()
    cursor.filter(fn(value): value % 2 == 0).collect()
```

1. r[std-iter.cursor.list] `std.iter` declares `ListIter[T]`, whose `next` yields the list's elements in order, as the list held them when `iter` ran.
2. r[std-iter.cursor.map] `std.iter` declares `MapEntries[K < Eq & Hash, V]`, whose `next` yields the map's entries as `(K, V)` tuples in the map's iteration order.
3. r[std-iter.cursor.private] Both have private fields, so only `iter` makes one, and both iterate a snapshot ([Iterator Snapshots](../lang/06-control-flow.md#iterator-snapshots)).

## Optional Cursors

Option iterates without building a list for its payload.

1. r[std-iter.option.iterable-cursor] `Option[T]` implements `Iterable[T]`, with `std.option.OptionIter[T]` as its `IntoIter`.
2. r[std-iter.option.cursor.some] A `.Some(value)` cursor yields `value` once, then `.None` on every later call.
3. r[std-iter.option.cursor.none] A `.None` cursor yields `.None` on every call.
4. r[std-iter.option.cursor.independent] Each `iter` call returns an independent cursor and leaves the optional unchanged.

## Iterator Factory

`from_fn` wraps a step closure in the concrete `FromFn[T]` cursor.

1. r[std-iter.from-fn.value] `std.iter.from_fn[T](step: fn() -> T?) -> FromFn[T]` returns a cursor storing the closure.
2. r[std-iter.from-fn.every-next] Each `next` call on `FromFn[T]` calls its stored closure, including calls after `.None`.

## Iterator Adapters

The prelude `Iterator[T]` also has **iterator adapters**: methods that wrap
an iterator in a new one, or drain it.

```text
fn first_evens(values: List[i32]) -> List[(usize, i32)]:
    values.iter().filter(fn(value): value % 2 == 0).enumerate().take(2).collect()
```

| Rule | Method | Result |
| --- | --- | --- |
| `filter` | `fn filter(self, keep: fn(T) -> bool) -> Filter[Self, T]` | a new iterator over the items of `self` for which `keep` returns `true` |
| `take` | `fn take(self, count: usize) -> Take[Self, T]` | a new iterator over the first `count` items of `self`, or fewer when `self` ends first |
| `enumerate` | `fn enumerate(self) -> Enumerate[Self, T]` | a new iterator over `(index, item)` pairs, with `usize` indices counting from `0` |
| `map` | `fn map[U](self, transform: fn(T) -> U) -> MapIter[Self, T, U]` | a new iterator over `transform(item)` for each item of `self`, in order |
| r[std-iter.adapter.fold.value] `fold` | `fn fold[A, R](self, initial: A, step: fn(A, T) -> A $ R) -> A $ R` | the accumulator after `step` has combined it with each remaining item of `self`, in order, starting from `initial` |
| r[std-iter.adapter.collect.value-defaulted] `collect` | `fn collect[C < FromIterator[T] = List[T]](self) -> C` | a `C` built from the remaining items of `self`, as [What Collect Builds](#what-collect-builds) specifies |

1. r[std-iter.adapter.filter.value] The `filter` default method has signature `fn filter(self, keep: fn(T) -> bool) -> Filter[Self, T]`.
2. r[std-iter.adapter.take.value] The `take` default method has signature `fn take(self, count: usize) -> Take[Self, T]`.
3. r[std-iter.adapter.enumerate.value] The `enumerate` default method has signature `fn enumerate(self) -> Enumerate[Self, T]`.
4. r[std-iter.adapter.map.value] The `map` default method has signature `fn map[U](self, transform: fn(T) -> U) -> MapIter[Self, T, U]`.
5. r[std-iter.adapter.skip.value] The `skip` default method has signature `fn skip(self, count: usize) -> Skip[Self, T]`.
6. r[std-iter.adapter.take-while.value] The `take_while` default method has signature `fn take_while(self, keep: fn(T) -> bool) -> TakeWhile[Self, T]`.
7. r[std-iter.adapter.zip.value] The `zip` default method has signature `fn zip[U, J < Iterable[U]](self, other: J) -> Zip[Self, T, U]`.
8. r[std-iter.adapter.chain.value] The `chain` default method has signature `fn chain[J < Iterable[T]](self, other: J) -> Chain[Self, T]`.
9. r[std-iter.adapter.flat-map.owned-iterable] The `flat_map` default method has signature `fn flat_map[U, J < Iterable[U]](self, transform: fn(T) -> J) -> FlatMap[Self, T, U, J]`.

1. r[std-iter.adapter.trait-defaults] Adapters are default methods of the prelude `Iterator[T]` trait and require no import.
2. r[std-iter.adapter.lazy] Calling `filter`, `take`, or `enumerate` does not advance `self`.
3. r[std-iter.adapter.lazy.map] Calling `map` does not advance `self` either.
4. r[std-iter.adapter.lazy.stored-source] An adapter advances its stored source only when its own `next` is called.
5. r[std-iter.adapter.take.limit] The iterator that `take` returns calls `next` on `self` at most `count` times.
6. r[std-iter.adapter.fold.owned-drain] `fold` advances `self` until `next` returns `.None`, which exhausts the owned cursor.
7. r[std-iter.adapter.owned-source] Lazy adapters take `self` by value and store that source value in their returned concrete adapter.
8. r[std-iter.adapter.mutable-reference-forwarding] A mutable reference to an `I < Iterator[T]` implements `Iterator[T]` by forwarding `next` to the referenced cursor.
9. r[std-iter.adapter.draining-value] Draining methods take `self` by value and advance their owned cursor; passing a mutable cursor reference advances the referenced cursor instead.
10. r[std-iter.adapter.named-layout] Each adapter is a named generic struct holding its source and its callback or traversal state.
11. r[std-iter.adapter.dyn-bridge] `DynIterator[T]` is an ordinary concrete cursor storing an erased mutable iterator reference and forwarding `next` to it.
12. r[std-iter.adapter.dyn-forwarding] Inherent adapter and draining methods on `dyn Iterator[T]` take `&mut self` and delegate to a `DynIterator[T]` holding that reference.
13. r[std-iter.adapter.callback-row] The `keep` callback has the empty row. A function value whose row lists a requirement key does not fit it. Error: `type-mismatch`.
14. r[std-iter.adapter.callback-row.map] The `transform` callback of `map` has the empty row too.
15. r[std-iter.adapter.fold.row] The `step` callback of `fold` may have a requirement row `R`, and `fold` then requires `R`.
16. r[std-iter.adapter.callback-row.capture] A `keep` or `transform` closure whose body uses a requirement key has that key in its row, even inside a `$.with` block. So it does not fit. Error: `type-mismatch`.

```text
use std.iter.FromFn

trait Logger

fn noisy(value: i32) -> bool $ Logger:
    value > 0

fn positives(values: List[i32]) -> List[i32] $ Logger:
    values.iter().filter(noisy).collect()  # error: type-mismatch

fn scoped(values: List[i32], logger: *dyn Logger) -> List[i32]:
    $.with(Logger=logger):
        values.iter().filter(fn(value): noisy(value)).collect()  # error: type-mismatch

fn drain(source: FromFn[i32]) -> List[i32]:
    source.collect()
```

A callback that needs a provider captures the provider value from `$.use`
outside the closure, so its row stays empty:

```text
trait Logger:
    fn level(self) -> i32

fn above_level(values: List[i32]) -> List[i32] $ Logger:
    let logger: *dyn Logger = $.use(Logger)
    values.iter().filter(fn(value): value > logger.level()).collect()
```

> **Why.** A lazy adapter's iterator calls `keep` or `transform` from its
> `next`, whose row is empty, so a stored callback cannot wait for
> providers. `fold` calls `step` before it returns, so the row passes
> through.

### More Adapters

`Iterator[T]` also has adapters that skip, stop, pair, and join items,
and draining methods that search or count them:

```text
fn body(lines: List[string]) -> List[string]:
    lines.iter().skip(1).take_while(fn(line: string) -> bool: line != "").collect()

fn labeled(names: List[string]) -> List[(i32, string)]:
    [1, 2, 3].iter().zip(names).collect()

fn has_blank(lines: List[string]) -> bool:
    lines.iter().any(fn(line: string) -> bool: line.trim() == "")
```

| Rule | Method | Result |
| --- | --- | --- |
| `skip` | `fn skip(self, count: usize) -> Skip[Self, T]` | a new iterator over the items of `self` after the first `count` |
| `take_while` | `fn take_while(self, keep: fn(T) -> bool) -> TakeWhile[Self, T]` | a new iterator over the items of `self` before the first one for which `keep` returns `false` |
| `zip` | `fn zip[U, J < Iterable[U]](self, other: J) -> Zip[Self, T, U]` | a new iterator over pairs of the items of `self` and `other` at the same position, which ends when either ends |
| `chain` | `fn chain[J < Iterable[T]](self, other: J) -> Chain[Self, T]` | a new iterator over the items of `self`, then those of `other` |
| `flat_map` | `fn flat_map[U, J < Iterable[U]](self, transform: fn(T) -> J) -> FlatMap[Self, T, U, J]` | a new iterator over the items of each iterable that `transform(item)` returns, in order |
| r[std-iter.adapter.any.value] `any` | `fn any(self, test: fn(T) -> bool) -> bool` | whether `test` returns `true` for some remaining item of `self` |
| r[std-iter.adapter.all.value] `all` | `fn all(self, test: fn(T) -> bool) -> bool` | whether `test` returns `true` for every remaining item of `self` |
| r[std-iter.adapter.find.value] `find` | `fn find(self, test: fn(T) -> bool) -> T?` | the first remaining item of `self` for which `test` returns `true`, or `.None` |
| r[std-iter.adapter.count.value] `count` | `fn count(self) -> usize` | the number of remaining items of `self` |
| r[std-iter.adapter.sum.numeric-target] `sum` | `fn sum[S < Sum[T]](self) -> S` | the target built from the remaining items |

1. r[std-iter.adapter.lazy.more] Calling `skip`, `take_while`, `zip`, `chain`, or `flat_map` advances no iterator. The returned iterator pulls only when its own `next` is called.
2. r[std-iter.adapter.skip.first-next] The first `next` call of `skip`'s iterator reads and drops up to `count` items of `self` before it reads the item it returns.
3. r[std-iter.adapter.take-while.stop] `take_while`'s iterator reads and drops the first item that `keep` rejects, and calls `next` on `self` no more after it.
4. r[std-iter.adapter.iterable-arg] `zip` and `chain` call `other.iter()` once, when they are called, and read the items of `other` from that iterator only.
5. r[std-iter.adapter.iterable-arg.iterator-bound] An iterator argument without an `Iterable[T]` implementation is an error, by [`flow.for.iterator.iterable-bound`](../lang/06-control-flow.md#r-flow.for.iterator.iterable-bound). Error: `unsatisfied-trait-bound`.
6. r[std-iter.adapter.zip-iterable.order] `zip`'s iterator calls `next` on `self` first, and on the iterator of `other` only when `self` gave an item. So when `self` ends first, `other` loses no item.
7. r[std-iter.adapter.chain-iterable.order] `chain`'s iterator calls `next` on the iterator of `other` only after `self` has returned `.None`, and on `self` no more after that.
8. r[std-iter.adapter.stop-early.owned] `any`, `all`, and `find` advance their owned cursor only until an item decides the result, and leave its remaining items unread.
9. r[std-iter.adapter.exhausted] On an exhausted iterator, `any` is `false`, `all` is `true`, `find` is `.None`, and `count` is 0.
10. r[std-iter.adapter.count.owned-drain] `count` advances `self` until `next` returns `.None`, which exhausts the owned cursor.
11. r[std-iter.adapter.callback-row.more] The callbacks of these methods have the empty row, as `filter`'s `keep` does. A function value whose row lists a requirement key does not fit. Error: `type-mismatch`.
12. r[std-iter.sum.numeric-order] The numeric `Sum[T]` implementation starts at `T::zero()` and adds each remaining item in order.
13. r[std-iter.sum.numeric-overflow] An integer numeric sum outside the range of `T` panics, as `+` does. Panic: `integer-overflow`.
14. r[std-iter.sum.owned-drain] The numeric sum implementation advances its owned cursor until `next` returns `.None`.
15. r[std-iter.sum.target-bound] `sum` requires a result type implementing `Sum[T]`. Error: `unsatisfied-trait-bound`.

16. r[std-iter.adapter.flat-map.open-once] `flat_map` calls `iter()` once on each transformed result, only when that result is reached.
17. r[std-iter.adapter.flat-map.drain-inner] It drains that cursor before transforming the next source item, skipping empty results.

```text
fn joined(first: List[i32], rest: &mut dyn Iterator[i32]) -> List[i32]:
    first.iter().chain(rest).collect()  # error: unsatisfied-trait-bound

fn joined_eagerly(first: List[i32], rest: *mut dyn Iterator[i32]) -> List[i32]:
    let tail: List[i32] = rest.collect()
    first.iter().chain(tail).collect()
```

> **Note.** An iterator's remaining items pass as a list, collected first,
> as `joined_eagerly` shows. An iterator that may never end goes in the
> receiver position instead: `rest.zip(first)` pairs the same items in
> swapped order.

> **Note.** `[1, 2, 3].iter().sum()` is `6`, and `[0.5, 0.25].iter().sum()` is
> `0.75`. A list has the same method:
> [`std-collections.helper.sum`](collections.md#r-std-collections.helper.sum).

> **Why.** The names and the pulling order are Rust's, so `zip` and
> `take_while` drop the same items there and here. `zip` and `chain` take
> any `Iterable`, as Rust's two take any `IntoIterator`, so a list, set,
> or range passes directly. `I` is a bounded type parameter, so `other.iter()`
> is dispatched statically, with no trait value.

## Borrowed Cursors

`by_ref` creates a value cursor that advances an existing cursor:

```text
fn first_two[I < Iterator[i32]](source: *mut I) -> List[i32]:
    source.by_ref().take(2).collect()
```

1. r[std-iter.by-ref.method] `Iterator[T].by_ref(&mut self) -> ByRef[Self]` returns a borrowed cursor.
2. r[std-iter.by-ref.layout] `ByRef[I]` stores one mutable reference to its source cursor.
3. r[std-iter.by-ref.next] For `I < Iterator[T]`, `ByRef[I]` implements `Iterator[T]` by forwarding `next` to that source.
4. r[std-iter.by-ref.progress] Adapting or draining a `ByRef` value advances the original cursor.
5. r[std-iter.by-ref.permission] Calling `by_ref` requires a writable cursor place or a mutable reference. Error: `not-writable`.

## Numeric Aggregation

`Sum[T]` and `Product[T]` build an aggregate from cursor items.

1. r[std-iter.sum.trait] `Sum[T]` declares `fn sum[I < Iterator[T]](items: I) -> Self`.
2. r[std-iter.product.trait] `Product[T]` declares `fn product[I < Iterator[T]](items: I) -> Self`.
3. r[std-iter.sum.numeric] Every numeric `T` implements `Sum[T]`.
4. r[std-iter.product.numeric] Every numeric `T` implements `Product[T]`.
5. r[std-iter.sum.method] `Iterator[T]` provides `fn sum[S < Sum[T]](self) -> S`.
6. r[std-iter.product.method] `Iterator[T]` provides `fn product[P < Product[T]](self) -> P`.
7. r[std-iter.aggregate.target] The expected result type or an explicit type argument can determine the aggregate target.
8. r[std-iter.aggregate.inference] An aggregate target has no type-argument default and follows ordinary bound inference.
9. r[std-iter.aggregate.unsolved] An unsolved aggregate target requires an expected type or an explicit type argument. Error: `cannot-infer-type`.
10. r[std-iter.product.numeric-order] The numeric `Product[T]` implementation starts at `T::one()` and multiplies each remaining item in order.
11. r[std-iter.product.numeric-overflow] An integer numeric product outside the range of `T` panics, as `*` does. Panic: `integer-overflow`.
12. r[std-iter.product.owned-drain] The numeric product implementation advances its owned cursor until `next` returns `.None`.
13. r[std-iter.product.target-bound] `product` requires a result type implementing `Product[T]`. Error: `unsatisfied-trait-bound`.

```text
fn total(values: List[i32]) -> i32:
    values.iter().sum::[i32]()

fn multiplied(values: List[i32]) -> i32:
    let result: i32 = values.iter().product()
    result
```

## What Collect Builds

`collect` builds the collection that the expected type names:

```text
fn parse_port(text: string) -> Result[usize, string]:
    .Ok(text.len())

fn index(names: List[string]) -> Map[string, usize]:
    names.iter().map(fn(name): (name, name.len())).collect()

fn parse_all(lines: List[string]) -> Result[List[usize], string]:
    lines.iter().map(parse_port).collect()

fn count(values: List[i32]) -> usize:
    copied := values.iter().collect()
    copied.len()
```

The collection being built implements the `std.iter` trait
`FromIterator[T]`:

```text
trait FromIterator[T]:
    fn from_iter[I < Iterator[T]](items: I) -> Self
```

1. r[std-iter.collect.generic-owned-cursor] `FromIterator[T].from_iter[I < Iterator[T]](items: I) -> Self` builds a collection from the owned cursor.
2. r[std-iter.prelude.from-iterator] `std.iter` also declares `FromIterator`, which is not a prelude name. Code imports it to implement or name it, as in `use std.iter.FromIterator`, and a `collect` call needs no import.
3. r[std-iter.collect.call] `collect` returns `C::from_iter(self)`.
4. r[std-iter.collect.expected] `C` is solved like any call-site type argument: from the expected type, or from an explicit list such as `collect::[Map[string, i32]]()`.
5. r[std-iter.collect.default] When nothing determines `C`, its declared [default](../lang/04-type-system.md#type-argument-defaults) `List[T]` applies.
6. r[std-iter.collect.bound] A `C` that does not implement `FromIterator[T]` is an error. Error: `unsatisfied-trait-bound`.

The standard library implements `FromIterator` for these prelude types:

| Rule | Collection | Items | Result |
| --- | --- | --- | --- |
| r[std-iter.collect.list] List | `List[T]` | `T` | every remaining item, in order |
| r[std-iter.collect.map-last] Map | `Map[K, V]` | `(K, V)` | one entry per pair; for an equal key the later value wins |
| r[std-iter.collect.result] All results | `Result[C, E]`, where `C < FromIterator[T]` | `Result[T, E]` | `.Ok` of the `C` collected from the `.Ok` payloads, or the first `.Err` |
| r[std-iter.collect.option] All values | `C?`, where `C < FromIterator[T]` | `T?` | `.Some` of the `C` collected from the `.Some` payloads, or `.None` at the first `.None` |

1. r[std-iter.collect.owned-drain] Collecting into a `List` or a `Map` advances `self` until `next` returns `.None`, which exhausts the owned cursor.
2. r[std-iter.collect.owned-stop] Collecting into a `Result` or an optional stops at the first `.Err` or `.None` and leaves the rest of the owned cursor unread.
3. r[std-iter.collect.map-key] A `Map` collection needs `K < Eq & Hash`, as every map does.

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

```text
tests:
    use std.testing.assert_equal

    it("map doubles each item, in order"):
        assert_equal([+3, 1, 2].map(fn(value: i32) -> i32: value * 2), [+6, 2, 4], reason="doubled")

    it("optional map keeps .None"):
        let absent: i32? = .None
        assert_equal(absent.map(fn(value: i32) -> i32: value * 2), .None, reason="kept")
```

See also: [Built-In Methods](../lang/10-modules.md#built-in-methods), which
lists the methods of the built-in types that stay in the language tier.
