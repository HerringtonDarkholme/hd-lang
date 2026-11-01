# Standard Library For References

Status: proposal, 2026-11-01; domain: std and the `for` loop; not
accepted. It builds on [Non-Copyable Types](NON_COPYABLE.md), ch. 13's
[Local References](../../spec/lang/13-values-and-references.md#local-references)
and [Element References](../../spec/lang/13-values-and-references.md#element-references),
and argument promotion (#1151). Signatures and examples:
[stdlib-references/](stdlib-references/src/stdlib_references.hd)
(`hd check` and `hd fmt --check` pass in that folder).

## The Proposal

std hands out references, not copies. Accessors return `&T` or `&mut T`,
and a callback that looks at an item takes `&T`. `for x in xs` borrows
`xs` and binds `x: &T`, movers move, and read-only methods take `&self`. One
`List`, `Map`, `Set`, `Deque` and `Heap` serve copyable and non-copyable
items; only a method that builds a new collection of copies needs `Copy`.

```text
fn order_total(lines: &List[Line]) -> i64:
    let sum: i64 = 0
    for line in lines:  # line: &Line; no snapshot, no copy
        sum += line.cents * line.qty
    sum

fn priciest(lines: &List[Line]) -> string:
    match lines.max_by_key(fn(line: &Line) -> i64: line.cents * line.qty):
        .Some(line) => line.sku  # line: &Line into the caller's list
        .None => ""

fn count_of(counts: &Map[string, usize], word: string) -> usize:
    counts.get(word).copied().unwrap_or(0)  # get gives (&usize)?
```

## Open Questions

Language-level only. The std shape below proposes one choice per API.

1. Operators read through a local `&T` (`total += x`, `x > 1`, `name == "ada"`), and `==` of two `&T` compares values, as in Rust? Recommended: yes.
2. `Index.index(&self, key) -> &Self::Out`, so `c[k]` is a place for every `Index` type, as in Rust? Recommended: yes.
3. `for (k, v) in m` binds `(&K, &V)`, and `for (k, v) in &mut m` binds `(&K, &mut V)` instead of `map-value-not-addressable`? Recommended: yes.
4. `for p in &mut xs` uses `xs` only through `p`, as in Rust, instead of also allowing reads and other element writes? Recommended: yes, one loan rule for both loops.
5. Three body-only intrinsics, `std.ops.swap`, `list_swap` and `list_take_last`? Recommended: yes; every other mover is plain hd over them.
6. Lift [`ref.copy.collections`](../../spec/lang/13-values-and-references.md#r-ref.copy.collections) after slice 7 (movers), without waiting for a program? Recommended: yes.
7. Flip the generic default to move-only (`[T < Copy]` to copy)? Recommended: yes, after slice 8 and a re-measure (new evidence: 153 bounds, not 633, and about 36 after this redesign).
8. Implied bounds, where `List[T]` in a signature implies `T < Copy`? Recommended: no; those sites borrow after slices 2 and 4.

## Problems It Solves

**1. Order totals** (package: `order_total`, `tidy`). Today
`lines.get(0)` copies the whole `Line`, and std's `find` copies each
element twice per step. Proposed, accessors return `(&Line)?`, and
`retain` and `sort_by_key` work in place.

```text
first := lines.get(0)   # today: Line? (copy Line [name, tags]); proposed: (&Line)?
lines.retain(fn(line: &Line) -> bool: line.qty > 0)  # proposed, in place
```

**2. Word counts** (`word_counts`, `report`). `index.get(key)` on a
`Map[string, List[string]]` copies the list, and `for (_, tags) in index`
counts each entry. Proposed, `get` gives `(&V)?` and the loop `(&K, &V)`.

```text
n := counts.get(word).unwrap_or(0) + 1          # today
n := counts.get(word).copied().unwrap_or(0) + 1 # proposed; get gives (&usize)?
```

**3. JSON handling** (`setting`, `tags`). Each `doc.get(key)` returns a
copy of the subtree. Proposed, `(&Json)?`: `doc.get(section)?.get(key)`.

**4. A worker pool of connections** (`run_one`, `rotate`). With
`impl !Copy for Conn`, `[Conn { id: 1 }]` is `unsatisfied-trait-bound`
and `@derive(Debug)` is `missing-supertrait-implementation` (probe p8).
Proposed, `List[Conn]` holds them, and movers hand them on.

```text
let mut conn = pool.idle.pop().expect("idle")  # moves out; no copy exists
conn.served += 1
pool.idle.push(conn)                           # moves back
```

## How It Works

| Kind | Shape | Methods | Rust |
| --- | --- | --- | --- |
| Accessor | `&self -> (&T)?`, `&mut self -> (&mut T)?` | `get`, `first`, `last`, `find`, `min`, `max`, `*_by_key`, `front`, `back`, `peek`, `Map.get`, `Json.get` | `Vec::get`, `HashMap::get` |
| Looking callback | `fn(&T) -> R` | `find`, `any`, `all`, `map`, `filter`, `retain`, `sort_by`, `*_by_key`, `partition`, `group_by` | `retain(\|x\| ..)` |
| Mover | value in or out, `&mut self` | `push`, `pop`, `insert`, `remove_at`, `swap`, `Map.remove`, `pop_front`, `Heap.pop`, `Option.take`, `replace` | `Vec::pop`, `mem::replace` |
| Builder | `&self -> List[T]`, `T < Copy` | `filter`, `sorted*`, `reversed`, `take`, `unique`, `partition`, `view`, `chunks`, `keys`, `values`, `xs[a..b]` | `iter().cloned().collect()` |
| Key | by value, keys `Copy` | `Map.get(key)`, `contains_key`, `remove`, `Set.contains`, `List.contains` | `&Q` (differs) |
| Consumer | `self` | `Option.map`, `unwrap_or`, iterator adapters, `Iterable.iter` | same |

- **Keys by value.** Map keys and set elements stay copyable, as Swift's
  `Dictionary` keys do. Rust's `get(&Q)` needs `Borrow`; without it, hd
  would write `scores.get(&"ada")`.
- **Iterator adapters** keep `fn(T)`, with `T` the item. A borrowing
  cursor (`xs.iter()`) yields `&T`, so the callback is `fn(l: &Line)`.
  Rust's `filter(&Self::Item)` would need `&&T`, which is no hd type.
- **Getting a value.** `xs[i]` and `*r` copy, and `opt.copied()` turns
  `(&T)?` into `T?`; a non-copyable value is `move-out-of-place` there and
  needs a mover. No iterator `copied` is needed: a copyable list passed by
  value yields values, and `map(fn(x: &T) -> T: *x)` covers the rest.
- **Moves.** `swap(a: &mut T, b: &mut T)` is an intrinsic body;
  `replace` is three lines of hd over it, and `Option.take` is
  `replace(self, .None)`. `List.pop` and `List.swap` use `list_take_last`
  and `list_swap`; `insert`, `remove_at` and the heap sifts are hd.
- **Derivation.** `Structure::walk(&self, w)` lends each field as
  `value: &F`, and `FieldRecord.get(&self, s: &'a S) -> &'a F` reads in
  place. Read traits (`Debug`, `Display`, `Eq`, `Hash`, `Serialize`) take
  `&self` and declare `< ?Copy`, so a `!Copy` type derives them.

**The borrowing loop.** For `for x in xs` with `xs` a `List` place or a
reference to one:

1. The loop takes a shared loan of `xs` from its first step to its last,
   and binds `x: &T` to each slot. There is no snapshot.
2. A write to `xs` in the body is `conflicting-access`. Writes are a
   store, `push`, `&mut xs`, `&mut xs[i]`, and a `&mut self` call on a
   place holding `xs`. Reads, `&xs[i]` and a nested loop are valid.
3. A write through another `*mut` path panics `borrow-conflict` in a dev
   or test build. A length change also panics `collection-pinned` in
   every build, so `x` stays valid in release.
4. `for p in &mut xs` takes the loan as `&mut` and binds `p: &mut T`.
5. A comprehension `[for x in xs => e]` follows rules 1-3. Its `e` sees
   `x: &T`, so `[for x in xs => x]` is a `List[&T]`, local to the
   function.
6. Ranges, iterators, `Set`, `Deque`, `Heap` and user types iterate
   through `Iterable` and `Iterator` as today; their `iter()` borrows.

## Cost

Estimated bottom-up from the files named; only the second table is measured.

| Component | Change | Estimate |
| --- | --- | --- |
| Receivers and callbacks | `&self` and `fn(&T)` in about 150 std signatures, std callers, spec/std tables | about 400 line edits |
| Accessors | 25 methods to references; callers add `copied()` or `*` | about 300 line edits |
| Borrowing loop | the list loop becomes the element loop plus a shared loan; the snapshot path goes | +80 / -60 compiler lines |
| Questions 1-3 | operator reads, `Index` result, map loops | about +200 compiler lines |
| Movers | three intrinsic bodies; hd movers over them | +90 compiler, -40 std lines |
| Walk by reference | `walk` and walkers, six derive templates | +30 compiler, about 150 std lines |
| Wasm size and gzip | std bodies lose copy and release pairs; about 400 Rust lines | not measured; about +2 KB of 2 MB |

Measured with a scratch `counts` stage case at cf5040b4a
(`cargo test -p hd_driver --test stages`). Counts are per element of a
`List[Line]` with two counted fields:

| Code | Today, copying | Proposed, by reference |
| --- | --- | --- |
| std's `find` body | 6 count operations | 2 (the element reference's own count) |
| a `max_by_key` body | 6, plus a release per new best | 2 |
| `lines.get(0)` | 2 retains | 0 |
| `index.get(key)`, `List[string]` value | 1 retain | 0 |

A loop over a lent list already takes no count (#870). The
[move-only measurement](#appendix-d-today-in-hd) put 153 `Copy` bounds on
142 std lines. Of these lines, 88 go with this proposal and 10 in
`task.hd` likely go. Seven are `Integer` bounds, one supertrait, and 36
stay on builders.

## Appendix A. Stress Test

Each hd version was checked as the package or a probe where today's
compiler can say; "proposed" rows are reasoned from the rules above.

| # | Case | What happened | What the design does |
| --- | --- | --- | --- |
| 1 | Rust book ch. 8 word count, `*map.entry(word).or_insert(0) += 1` | `*counts.get_or_insert(word, 0) += 1` checks unchanged (package `word_counts`) | nothing to do |
| 2 | Rust `v.retain(\|l\| l.qty > 0); v.sort_by_key(\|l\| l.sku.clone())` | hd has no in-place forms; `filter` and `sorted_by_key` copy every kept item | `retain` and `sort_by_key` take `&mut self` (package `tidy`) |
| 3 | "Too Many Lists" `pop`: `self.head.take().map(\|n\| { self.head = n.next; n.elem })` | `take` works on the field, but `n.elem` through a `*Node` is `move-out-of-place` for a `!Copy` element | **gap**: a box cannot give its value back; non-copyable payloads in recursive data stay `*mut` handles |
| 4 | Swift `for (i, item) in items.enumerated() where item.isDone` | `for (i, item) in items.iter().enumerate()` checks today over a `Span` (p10) | `List.iter()` yields `(usize, &T)` the same way |
| 5 | Swift `counts[word, default: 0] += 1` | `get_or_insert` | nothing to do |
| 6 | `for item in self.items: self.record(item)` with `record(&mut self)` | proposed: `conflicting-access`, as Rust's E0502 | the fix is `items := self.items` (one count) or an index loop; the diagnostic names it |
| 7 | ch. 06's snapshot example: `for value in out: out.push(value)` | today valid; proposed `conflicting-access` (today's `&out` loop already says so, p9) | `snapshot := out`, then loop over `snapshot` |
| 8 | `fn ok(xs: List[T]) -> List[T]: [for x in xs if x.ok => x]` | `type-mismatch`: expected `T`, found `&T` | write `*x`; the message says so |
| 9 | `xs.contains(&name)` from Rust habit | `type-mismatch`: expected `string`, found `&string` | the message says the value goes by value; no fix-it while reference fix-its wait |
| 10 | `opt.map(fn(l: &Line) -> &string: &l.sku)` | today `type-mismatch` (p10): a closure result cannot hold a reference into its argument | **gap**: write a `match`; closure result lifetimes are ch. 07 work |
| 11 | `filter` on an owning iterator of `!Copy` items | `unsatisfied-trait-bound`: the item is copied to test it | `retain` in place, or borrow first: `conns.iter().filter(...)` |
| 12 | `for line in &lines: lines[0].done = false` | valid today (element loops allow element writes, p10) | `conflicting-access` under rule 2; question 4 asks the same for `&mut` |
| 13 | Rust `let first = v[0].clone()` on a `!Copy` item | `move-out-of-place` | `&v[0]`, or a mover; there is no `clone` |

Four translations. The first two check today as written (`hd check`);
the last two parse.

```rust
// The Rust book, ch. 8.3, and a cargo-style filter.
for word in text.split_whitespace() { *map.entry(word).or_insert(0) += 1; }
fn active_names(pkgs: &[Package]) -> Vec<&str> {
    pkgs.iter().filter(|p| p.active).map(|p| p.name.as_str()).collect()
}
```

```text
fn word_counts(text: string) -> Map[string, usize]:
    let mut counts: Map[string, usize] = {}
    for word in text.split_whitespace():
        *counts.get_or_insert(word, 0) += 1
    counts

fn active_names(pkgs: &List[Package]) -> List[string]:
    [for p in pkgs if p.active => p.name]  # proposed: p is &Package
```

```rust
// "Learning Rust With Entirely Too Many Linked Lists", second.rs; Swift.
pub fn pop(&mut self) -> Option<T> {
    self.head.take().map(|node| { self.head = node.next; node.elem })
}
for (i, task) in tasks.enumerated() where task.isDone { doneAt.append(i) }
tasks.swapAt(0, tasks.count - 1)
```

```text
struct Node[T]:
    elem: T
    next: (*mut Node[T])?

struct Stack[T]:
    head: (*mut Node[T])?

impl[T] Stack[T]:
    fn pop(&mut self) -> T?:
        node := self.head.take()?  # proposed Option.take
        self.head = node.next
        .Some(node.elem)  # a copy; for a `!Copy` T, move-out-of-place (case 3)

fn finished(tasks: &mut List[Task]) -> List[usize]:
    let mut done_at: List[usize] = []
    for (i, task) in tasks.iter().enumerate():
        if task.done:
            done_at.push(i)
    tasks.swap(0, tasks.len() - 1)  # proposed List.swap
    done_at
```

## Appendix B. Prior Art, With Code

**Rust** ([`Vec`](https://doc.rust-lang.org/std/vec/struct.Vec.html),
[`HashMap::get`](https://doc.rust-lang.org/std/collections/struct.HashMap.html#method.get),
[`mem::replace`](https://doc.rust-lang.org/std/mem/fn.replace.html),
[`Option::take`](https://doc.rust-lang.org/std/option/enum.Option.html#method.take)):

```rust
let first: Option<&Line> = lines.get(0);
for line in &lines { total += line.cents * line.qty; }
lines.retain(|l| l.qty > 0);
lines.sort_by_key(|l| l.cents);
let n = counts.get("tea").copied().unwrap_or(0);
*counts.entry(word).or_insert(0) += 1;
let old = std::mem::replace(&mut self.state, State::Closed);
let current = self.current.take();
```

Accessors return references, loops borrow with `&`, movers move, and
`copied` turns `Option<&T>` into `Option<T>`. hd takes all of it, borrows
in `for x in xs` without the `&`, and takes keys by value instead of
`Borrow<Q>`.

**Swift** ([`Array`](https://developer.apple.com/documentation/swift/array),
[SE-0390](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0390-noncopyable-structs-and-enums.md),
[SE-0447 `Span`](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0447-span-access-shared-contiguous-storage.md)):

```swift
for item in items { total += item.cents }   // copies (retains) each item
items[i].qty += 1                           // _modify: in place
items.swapAt(i, j)
counts[word, default: 0] += 1
let span = items.span                       // borrowed, non-escaping view
```

Swift's `for` copies each element, and its subscripts lend in place
through the underscored `_read` and `_modify` accessors. hd's element
places already lend in place; it rejects the copying loop. `Span` matches
hd's `Span[T, 'a]`. Swift 6 generalized only `Optional`, `Result` and the
pointers to non-copyable types
([SE-0437](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0437-noncopyable-stdlib-primitives.md)),
so `Dictionary` keys stay copyable, as hd's do.

**Go** ([range](https://go.dev/ref/spec#For_range)):

```go
for _, line := range lines { total += line.Cents }   // line is a copy
for i := range lines { lines[i].Qty++ }               // write through the index
```

Go copies every element in `range`; writing needs the index. This is
today's hd snapshot loop, and the cost this proposal removes.

## Appendix C. Alternatives Considered

- **Keep copies.** Copy-on-write makes a list copy one count, but each
  element read still copies its counted fields, and non-copyable items
  stay impossible.
- **Two list types**, a copyable `List` and a non-copyable one. The owner
  ruled it out on 2026-11-01: one collection type per kind.
- **Rust's `get(&Q)` keys.** It needs a `Borrow` trait, and every lookup
  writes `&`: `m.get(&"ada")`.
- **Keep the snapshot loop and borrow only in `for x in &xs`.** The owner
  chose a borrowing `for x in xs`; the snapshot hides a copy per change.
- **An iterator `copied()` adapter.** A default method cannot require
  `T = &U`, and a blanket impl is `bare-parameter-impl-target` (p7).
- **Rename `Iterable.iter` to `into_iter`.** The inherent `iter(&self)`
  wins at a concrete type (p7), so the names do not collide.

## Appendix D. Today In hd

- **Receivers.** `len`, `is_empty`, `get`, `first`, `last`, `contains`,
  `keys`, `values` and the helpers take `self`
  ([Built-In Methods](../../spec/lang/10-modules.md#built-in-methods),
  [spec/std/collections.md](../../spec/std/collections.md)).
- **Loops.** `for x in xs` iterates a snapshot
  ([Snapshot Iteration](../../spec/lang/06-control-flow.md#snapshot-iteration));
  `for p in &xs` and `&mut xs` bind element references
  ([Element Reference Loops](../../spec/lang/06-control-flow.md#element-reference-loops)).
- **Copy marker.** Built: generics default to `Copy`, and collections
  hold `Copy` items ([Copy](../../spec/lang/13-values-and-references.md#copy)).

Probes, `target/debug/hd check` at cf5040b4a:

| Probe | Code | Result |
| --- | --- | --- |
| p1 | a borrowing `ListIter[T, 'a]: Iterator[&'a T]`, `(&T)?` accessors, `fn(&T)` callbacks, `[for l in &lines => l]` | checks and runs |
| p3 | `I < Iterator[&'a string]`, iterated by `for` | checks |
| p4 | `sum += x`, `x > 1`, `out + w` with `x: &i32`, `w: &string` | `type-mismatch` three times; `"$w"` checks |
| p5 | `fn f(l: &&Line)` and `& &Line` | `syntax-error: expected a type` |
| p7 | inherent `iter(&self)` beside `Iterable.iter(self)`; blanket `impl[I < Iterator[&'a T]]` | the first checks; the second is `bare-parameter-impl-target` |
| p8 | `impl !Copy for Conn` with `@derive(Debug, Serialize)`, `[Conn {..}]`, `encode(*c)` | `missing-supertrait-implementation`, `unsatisfied-trait-bound`, `move-out-of-place` |
| p9 | `for x in &xs: xs.push(*x)` | `conflicting-access` |
| p10 | `opt.map(fn(l: &Line) -> &string: &l.sku)`; `enumerate` over a span; an element write in `for l in &lines` | `type-mismatch`; checks; checks |

**The move-only measurement** is on branch `measure/move-only-generics`
(05ad0e128, 72700faee). With `[T]` move-only, real programs (examples,
bench, server-sdk) needed 0 `Copy` bounds. Conformance fixtures needed 129
in 89 of 6,331 files, and std 153 on 142 lines. Each std line, read by
hand:

| Cause | Lines | After this proposal |
| --- | --- | --- |
| `walk(self)`, walkers and `by Structure` templates | 26 | gone: `walk(&self)`, `value: &F` |
| a loop or `Iterable` over items | 27 | gone: the loop borrows |
| an accessor copies (`get`, `first`, `peek`, `Index`, `find`) | 17 | gone: references |
| a read-only generic argument (`encode`, `dbg`, `interpolate`) | 15 | gone: `&T` |
| movers copy then truncate | 3 | gone: `swap`, `take` |
| `task.hd` results used twice | 10 | likely gone with `&` callbacks; unchecked |
| `N < Integer` | 7 | one `trait Integer < Copy` |
| builders: slicing (17), `filter`/`sorted`/`keys`, test inputs | 36 | stay |

## Appendix E. Details

**API table.**

| API | Today | Proposed | Rust |
| --- | --- | --- | --- |
| `len`, `is_empty` (all), `is_some`, `is_ok`, string methods, `join` | `self` | `&self` | `&self` |
| `split`, `lines`, `trim` and other text results | new strings | unchanged: hd has no string slice type | `&str` slices |
| `List.get`, `first`, `last` | `self -> T?` | `&self -> (&T)?` | `get(&self) -> Option<&T>` |
| `List.find`, `min`, `max`, `min_by_key`, `max_by_key` | `self`, `fn(T)`, `-> T?` | `&self`, `fn(&T)`, `-> (&T)?` | `iter().find`, `max_by_key` |
| `List.map`, `filter`, `any`, `all`, `partition`, `group_by`, `sorted_by_key` | `self`, `fn(T)` | `&self`, `fn(&T)`; builders `T < Copy` | `iter().filter(\|x\| ..)` |
| `List.sorted_by` | `fn(T, T) -> Ordering` | `fn(&T, &T) -> Ordering` | `sort_by(&mut self, ..)` |
| `List.sort`, `sort_by`, `sort_by_key`, `retain`, `swap` | none | new, `&mut self`, any `T` | same names |
| `List.pop`, `remove_at`, `insert` | copy then truncate | move (`list_take_last`, `list_swap`) | `Vec::pop` |
| `List.iter` | snapshot `ListIter[T]` | `iter['a](&'a self) -> ListIter[T, 'a]`, `&'a T` items | `iter()` |
| `Iterable for List` | yields `T` copies | yields `T`, consuming the list value | `IntoIterator for Vec<T>` |
| `List.contains`, `index_of` | `value: T` | unchanged, `&self` | `contains(&T)` |
| `Map.get` | `-> V?` | `-> (&V)?` | `get(&Q) -> Option<&V>` |
| `Map.keys`, `values`, `entries` | `self -> List` | `&self -> List`, copies | iterators |
| `Map.iter`, map loop | `(K, V)` copies | `(&'a K, &'a V)` | `iter()` |
| `Deque.front`, `back`, `get`; `Heap.peek` | `-> T?` | `-> (&T)?` | `front()`, `peek()` |
| `Deque.pop_front`, `pop_back`; `Heap.push`, `pop` | copy, `.None` store | move: `Option.take`, `list_swap` | same |
| `Set.contains`, `insert`, `remove`, `iter` | `self` | `&self`; elements `Copy`; `iter` borrows | `HashSet` |
| `ListView`, `Span`, `SpanMut` | `len(self)`, `view[i]` copies | `&self`; `[i]` a place by question 2 | `&[T]` |
| `SpanMut.swap`, `fill` | copies | `swap` moves; `fill` keeps `T < Copy` | `swap`, `fill(T: Clone)` |
| `Option.as_ref`, `as_mut`, `take`; `(&T)?.copied` | none | new | same names |
| `Option.map`, `and_then`, `unwrap_or`, `expect`; `Result` alike | `self` | unchanged | same |
| `Json.get`, `at`, `as_text` | `-> Json?`, `string?` | `-> (&Json)?`, `(&string)?` | `get(&self) -> Option<&Value>` |
| `Json.as_i64`, `as_bool`, numbers | `self` | `&self`, values | same |
| `encode`, `encode_pretty`, `set_json`, `post!` | `value: T` | `value: &T` | `to_string(&T)` |
| `Structure::walk`, `Walker.member`, `FieldRecord.get` | `self`, `value: F`, `get(s: S) -> F` | `&self`, `value: &F`, `get(s: &'a S) -> &'a F` | serde's `serialize(&self)` |
| `std.ops.swap`, `replace` | none | new | `mem::swap`, `mem::replace` |
| `min`, `max`, `clamp`, `debug`, `assert_equal` | `T` | unchanged | `min(T, T)` |

**Language checklist.**

- Grammar and formatting: nothing new; `(&T)?` and `fn(&T)` exist.
- Typing: use sites only; loop bindings and comprehension items gain `&`.
- Patterns: a tuple pattern binds `(&K, &V)`; ch. 13 rule 6 binds a
  non-copyable payload below `&` by reference.
- Derives: the six `by Structure` templates follow `walk(&self)`.
- Closures: a closure passed directly may capture the loop's `x`; a
  stored one is `local-reference-escapes`.
- Ownership and the inliner: loans exist already; the snapshot copy and
  the per-element copy disappear from TIR.
- Soundness: the length pin keeps release builds memory-safe; loans add
  write detection in dev.
- Diagnostics: `conflicting-access` in a loop names the loop; a `&T`
  where `T` is expected suggests `*x`.
- Spec: ch. 06 For Loops (snapshot rules replaced), ch. 10 Built-In
  Methods, ch. 05 map `get`, `spec/std/*` tables; fixtures per rule.
- No-feature alternative: keep copies (Appendix C).

**Implementation slices,** one agent each, about 400 lines with fixtures:

1. `&self` receivers and `fn(&T)` callbacks, with the new in-place
   `retain`, `sort`, `sort_by` and `sort_by_key`. Check: std, fixtures and
   examples pass, with the package's callback shapes.
2. Accessors return references, with `Option.as_ref`, `as_mut` and
   `copied`. Check: a counts case shows no copy for `lines.get(0)`.
3. Operator reads through `&` (question 1), before loops bind `&T`.
4. The borrowing loop and comprehensions; borrowing `iter()` cursors.
   Check: fixtures for rules 1-5, both build profiles.
5. Map loops (question 3) and the `&mut` loop rule (question 4).
6. `walk(&self)`, walkers and derive templates; read traits `< ?Copy`.
   Check: `@derive(Debug, Serialize, Eq)` on a `!Copy` struct checks.
7. Movers: the three intrinsics, `replace`, `take`, and hd movers over
   them. Check: a counts case shows no retain or release in `pop`.
8. Lift `ref.copy.collections` (question 6); `Index` returns a reference
   (question 2).
9. Separately, by owner decision: the generic default flip (question 7),
   after re-running the measurement branch.

**Parse log.** Every `text` block above was parsed with
`scripts/hd-conformance.sh parse`; parsing checks syntax only. The
package checks with `hd check` and `hd fmt --check`.

| Block | Result |
| --- | --- |
| 1, the proposal | parses |
| 2, order totals | parses |
| 3, word counts | parses |
| 4, worker pool | parses |
| 5, word count and filter | parses; also checks today (`hd check`: ok) |
| 6, stack pop and tasks | parses |
