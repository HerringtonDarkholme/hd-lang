# Collecting Iterators Into Collections: Survey And Design Options

Status: design exploration, 2026-09-29. Owner decisions CO1-CO4 are
applied (2026-09-29), and the specification is authoritative for them:
[Collect Targets](../spec/06-control-flow.md#collect-targets) and
[Comprehension Restrictions](../spec/05-expressions.md#comprehension-restrictions).
CO3's helpers are std-only, in [STDLIB](STDLIB.md#stditer). The rest of
the record is the survey behind the decisions.

[Chaining Study CS7](CHAINING_STUDY.md#owner-decisions) makes
`Iterator[T]` a closure-backed `data` type whose adapters are ordinary
methods, generic ones included. Its one draining method, `collect`, returns
a `List[T]`. This record asks how an iterator ends in a `Map`, a `Set`, or
an all-or-nothing `Result`. It follows the
[brainstorm](../.agents/skills/brainstorm/SKILL.md) method and reviews:

- [Iterator Adapters](../spec/06-control-flow.md#iterator-adapters) and
  [Iteration Protocols](../spec/06-control-flow.md#iteration-protocols);
- [Map Literals](../spec/05-expressions.md#map-literals),
  [Comprehensions](../spec/05-expressions.md#comprehensions), and
  [Map Key Types](../spec/04-type-system.md#map-key-types);
- [Built-In Methods](../spec/10-modules.md#built-in-methods) and the
  [Prelude](../spec/10-modules.md#prelude);
- [Inherent Member Names](../spec/09-traits.md#inherent-member-names);
- [`std.collections`](STDLIB.md#stdcollections) and
  [`std.iter`](STDLIB.md#stditer) in the standard-library draft;
- the [Spec Scope For The Standard Library](../AGENTS.md#spec-scope-for-the-standard-library)
  rule.

## Owner Decisions

Decided 2026-09-29.

1. **CO1: a generic `collect` with a `FromIterator` trait,** as in Rust.
   The expected type picks the target:
   `let by_id: Map[UserId, User] = pairs.collect()` and
   `let rows: Result[List[Row], ParseError] = results.collect()`.
   `FromIterator` enters the spec, because `collect` on the prelude
   iterator names it.

   Open for the apply pass: what `xs := it.collect()` means with no
   expected type. hd has no `_` in type arguments, so either a default
   (recommended: `List[T]`) or a required annotation. Recommend the
   default.
2. **CO2: duplicate keys when collecting into `Map`:** the last value
   wins, and the key keeps its first position, matching map literals and
   comprehensions.
3. **CO3: the other helpers are STDLIB only.** Convenience names such as
   `to_map` or `try_collect`, if any, are std-only. The all-or-nothing
   `Result` and `T?` collection comes from `FromIterator` impls in std.
4. **CO4: `?` is allowed inside a comprehension.** It propagates out of
   the enclosing function, and the comprehension stops at that point. This
   closes the spec gap; `return` stays banned there.

## Still Open

Points the apply pass met (2026-09-29). Each waits for the owner.

| # | Point | Applied | **Recommendation** |
| --- | --- | --- | --- |
| 1 | CO1's open point: what `xs := it.collect()` means with no expected type | `C` is `List[T]` when nothing determines it ([`flow.collect.default`](../spec/06-control-flow.md#r-flow.collect.default)), as the record recommends | Keep. |
| 2 | Is `FromIterator` a prelude name? | The specification says only that `std.iter` declares it ([`flow.collect.trait`](../spec/06-control-flow.md#r-flow.collect.trait)); no fixture names it | Not a prelude name: code that implements it imports it, as `std.convert.From` is imported, and callers of `collect` never write it. |

## Contents

1. [Problem](#problem)
2. [What hd Has Today](#what-hd-has-today)
3. [Use Cases](#use-cases)
4. [Survey](#survey)
5. [Option 1: Comprehensions And Loops](#option-1-comprehensions-and-loops)
6. [Option 2: Concrete Draining Methods](#option-2-concrete-draining-methods)
7. [Option 3: Generic Collect With A Target Trait](#option-3-generic-collect-with-a-target-trait)
8. [Option 4: Target Constructors](#option-4-target-constructors)
9. [Option 5: Collector Values](#option-5-collector-values)
10. [Duplicate Keys](#duplicate-keys)
11. [All-Or-Nothing Collection](#all-or-nothing-collection)
12. [Spec Or Standard Library](#spec-or-standard-library)
13. [Comparison](#comparison)
14. [Ranking By Design Cost Order](#ranking-by-design-cost-order)
15. [Recommendation](#recommendation)
16. [Questions For The Owner](#questions-for-the-owner)
17. [Sources](#sources)
18. [Parse Log](#parse-log)

## Problem

Which API turns an `Iterator[T]` into a collection other than `List[T]`?
Three sub-questions travel with it:

1. What happens when two pairs have equal keys?
2. How does an `Iterator[Result[T, E]]` become a `Result[List[T], E]`, and
   an `Iterator[T?]` a `List[T]?`?
3. Which parts belong in the specification, given that `Set` is not in it?

## What hd Has Today

| Area | Today | Source |
| --- | --- | --- |
| `collect` | `fn collect(mut self) -> List[T]` drains the iterator in order | `flow.adapter.collect` (retired; see [Collect Targets](../spec/06-control-flow.md#collect-targets)) |
| `Iterator` shape | a `data` type with a `step` closure; generic methods raise no dynamic-safety question (decided, not yet applied) | [CS7](CHAINING_STUDY.md#owner-decisions) |
| Comprehensions | `[for ...]` and `{for ... => k: v}` accept an iterator as well as an iterable | [`flow.for.comprehension`](../spec/06-control-flow.md#r-flow.for.comprehension) |
| Duplicate keys | in a map literal and a map comprehension the later value wins, and the key keeps its first position | [`expr.map.duplicate`](../spec/05-expressions.md#r-expr.map.duplicate), [`expr.comp.map.duplicate`](../spec/05-expressions.md#r-expr.comp.map.duplicate) |
| Comprehension limits | eager, no suspension, no `return`, `break` or `continue` | [Comprehension Restrictions](../spec/05-expressions.md#comprehension-restrictions) |
| Map keys | `Map[K, V]` requires `K < Eq & Hash` | [`types.map-key.bound`](../spec/04-type-system.md#r-types.map-key.bound) |
| Prelude | `List`, `Map`, `Option`, `Result`, `Iterator` and `Iterable` are prelude names; `Set` is not | [Prelude](../spec/10-modules.md#prelude), [`module.method.no-set`](../spec/10-modules.md#r-module.method.no-set) |
| `Set` | a `std.collections` type in the draft only | [`std.collections`](STDLIB.md#stdcollections) |
| Instantiation impls | an inherent `impl` may target one instantiation, as in `impl Box[i32]`; disjoint targets may reuse a member name | [`trait.inherent.unique-unifying.disjoint`](../spec/09-traits.md#r-trait.inherent.unique-unifying.disjoint), [`trait.inherent.target-match`](../spec/09-traits.md#r-trait.inherent.target-match) |
| Use-site inference | a type argument may be solved from the expected result type | [`fn.generic.placeholder.solve`](../spec/07-functions.md#r-fn.generic.placeholder.solve) |
| `_` in type arguments | `_` is allowed as a whole call-site type argument, never inside a type such as `List[_]` | [`fn.generic.placeholder.not-type`](../spec/07-functions.md#r-fn.generic.placeholder.not-type) |

One gap found while checking: the spec forbids `return` inside a
comprehension, and `?` returns from the nearest function, but no rule says
whether `?` is allowed inside one.

## Use Cases

Every option shows the same six cases.

| # | Case | Target |
| --- | --- | --- |
| C1 | Drain with no expected type, then use the result | `List[string]` |
| C2 | Index users by email | `Map[string, User]` |
| C3 | Index users by domain, where domains repeat | `Map[string, User]` |
| C4 | Distinct domains | `Set[string]`, standard library only |
| C5 | Parse every line or fail on the first error | `Result[List[i32], ParseError]` |
| C6 | Look up every name or get nothing | `List[i32]?` |

Shared declarations. They parse:

```text
data User:
    name: string
    email: string
    active: bool

impl User:
    fn domain(self) -> string:
        self.email.split("@")[1]

data ParseError:
    line: string

fn parse_port(text: string) -> Result[i32, ParseError]:
    pass

fn find_port(name: string) -> i32?:
    pass
```

## Survey

| Language | List | Map | Set | Duplicate keys | Sources |
| --- | --- | --- | --- | --- | --- |
| Rust | `collect()` with the target from an annotation or turbofish | `collect::<HashMap<_, _>>()` over pairs | `collect::<HashSet<_>>()` | "all but one of the corresponding values will be dropped" | [`FromIterator`](https://doc.rust-lang.org/std/iter/trait.FromIterator.html), [`HashMap`](https://doc.rust-lang.org/std/collections/struct.HashMap.html) |
| Kotlin | `toList()` | `toMap()` over pairs; `associateBy(key)` | `toSet()` | "the last one gets added to the map" | [`toMap`](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.collections/to-map.html), [`associateBy`](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.collections/associate-by.html), [`toSet`](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.collections/to-set.html) |
| Gleam | `yielder.to_list` | `dict.from_list(pairs)` | `set.from_list` | "the last one in the list will be the one that is present" | [`yielder`](https://hexdocs.pm/gleam_yielder/gleam/yielder.html), [`dict`](https://gleam-stdlib.hexdocs.pm/gleam/dict.html) |
| Swift | `Array(seq)` | `Dictionary(uniqueKeysWithValues:)`; `Dictionary(_:uniquingKeysWith:)` takes a combine closure; `Dictionary(grouping:by:)` | `Set(seq)` | precondition: "The sequence must not have duplicate keys" | [`Dictionary.swift`](https://github.com/swiftlang/swift/blob/main/stdlib/public/core/Dictionary.swift) |
| C# | `ToList()` | `ToDictionary()` over pairs, or with a key selector | `ToHashSet()` | throws `ArgumentException` | [`ToDictionary`](https://learn.microsoft.com/en-us/dotnet/api/system.linq.enumerable.todictionary) |
| Java | `Collectors.toList()` | `Collectors.toMap(key, value)`; a third argument merges | `Collectors.toSet()` | throws `IllegalStateException` unless a merge function is given | [`Collectors`](https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/util/stream/Collectors.html) |
| Scala 2.13+ | `to(List)` or `toList` | `to(Map)` or `toMap` | `to(Set)` or `toSet` | not surveyed; `to(factory)` passes the target's companion as a value, replacing `to[Collection]` | [Migration guide](https://docs.scala-lang.org/overviews/core/collections-migration-213.html), [`IterableOnceOps`](https://www.scala-lang.org/api/current/scala/collection/IterableOnceOps.html) |
| Python | `list(it)` | `dict(pairs)` | `set(it)` | "the last value for that key becomes the corresponding value" | [`dict`](https://docs.python.org/3/library/stdtypes.html#dict) |

All-or-nothing collection:

| Language | Mechanism | Sources |
| --- | --- | --- |
| Rust | `impl FromIterator<Result<A, E>> for Result<V, E>`: collect stops at the first `Err`; the same for `Option` | [`FromIterator`](https://doc.rust-lang.org/std/iter/trait.FromIterator.html) |
| Rust (unstable) | `Iterator::try_collect` | [`try_collect`](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.try_collect) |
| Gleam | `result.all(results) -> Result(List(a), e)` returns "the first error" | [`result`](https://gleam-stdlib.hexdocs.pm/gleam/result.html) |
| Haskell | `sequence` and `traverse` over any `Traversable` | [`Data.Traversable`](https://hackage.haskell.org/package/base/docs/Data-Traversable.html) |

### Takeaways

1. **Most languages name the target in the call.** Kotlin, C#, Gleam,
   Swift and Python write `toMap`, `ToDictionary`, `from_list` or
   `Dictionary(...)`. Only Rust infers the target from an annotation.
2. **Duplicate keys split two ways.** Kotlin, Gleam, Python and hd's own
   map literals keep the last value. Swift, C# and Java fail loudly and
   offer a merge variant.
3. **All-or-nothing collection is a Rust and Gleam idea.** Rust hides it
   behind `FromIterator`; Gleam gives it a plain function.

## Option 1: Comprehensions And Loops

**Idea.** The radical simplification. `collect()` stays the only draining
method. Map comprehensions already accept iterators and already keep the
last value, so they cover C2 and C3. Sets and all-or-nothing results use a
loop.

```text
fn index(users: List[User]) -> Map[string, User]:
    {for user in users.iter().filter(fn(u): u.active) => user.email: user}

fn parse_all(lines: List[string]) -> Result[List[i32], ParseError]:
    let ports: mut List[i32] = []
    for line in lines:
        ports.append(parse_port(line)?)
    .Ok(ports)
```

C4, using the draft `Set` API:

```text
use std.collections.Set

fn domains(users: List[User]) -> Set[string]:
    let seen: mut Set[string] = Set::new()
    for user in users:
        seen.insert(user.domain())
    seen
```

- **Changes:** none.
- **Fit:** C1, C2, C3 today; C4 to C6 are three to five lines each.
- **Chains:** a comprehension can't end a method chain, so the chain moves
  inside the `for` clause.

## Option 2: Concrete Draining Methods

**Idea.** Kotlin, C# and Gleam. Each target gets its own named method on
`Iterator`. Methods that need a special element type live in inherent
`impl` blocks for that instantiation, which hd already allows.

```text
impl[K < Eq & Hash, V] Iterator[(K, V)]:
    pub fn to_map(mut self) -> Map[K, V]:
        pass

impl[T < Eq & Hash] Iterator[T]:
    pub fn to_set(mut self) -> Set[T]:
        pass

impl[T, E] Iterator[Result[T, E]]:
    pub fn try_collect(mut self) -> Result[List[T], E]:
        pass

impl[T] Iterator[T?]:
    pub fn try_collect(mut self) -> List[T]?:
        pass
```

The two `try_collect` targets can't unify, so both may use the name
([`trait.inherent.unique-unifying.disjoint`](../spec/09-traits.md#r-trait.inherent.unique-unifying.disjoint)).
The use cases:

```text
fn report(users: List[User], lines: List[string], names: List[string]) -> Result[void, ParseError]:
    active := users.iter().filter(fn(u): u.active).map(fn(u): u.name).collect()
    by_email := users.iter().map(fn(u): (u.email, u)).to_map()
    by_domain := users.iter().map(fn(u): (u.domain(), u)).to_map()
    domains := users.iter().map(fn(u): u.domain()).to_set()
    ports := lines.iter().map(parse_port).try_collect()?
    found := names.iter().map(find_port).try_collect()
    .Ok()
```

- **Changes:** four `std` methods; no language rule.
- **C1:** unchanged; `collect()` keeps its type without an annotation.
- **A key-function form** (`to_map_by(fn(u): u.email)`, Kotlin
  `associateBy`) is a fifth method and fits the
  [CS6](CHAINING_STUDY.md#owner-decisions) key-function helpers.
- **Extensibility:** a user collection can't add a method to `Iterator`
  (only `std` owns it); it offers a constructor instead (Option 4).

## Option 3: Generic Collect With A Target Trait

**Idea.** Rust. A trait `FromIterator[T]` says how a type is built from an
iterator. A generic `collect[C]` returns any implementing type, which the
caller fixes by an annotation or an explicit type argument.

```text
pub trait FromIterator[T]:
    fn from_iter(items: mut Iterator[T]) -> Self

impl[T] Iterator[T]:
    pub fn collect_into[C < FromIterator[T]](mut self) -> C:
        C::from_iter(self)
```

```text
fn report(users: List[User], lines: List[string]) -> Result[void, ParseError]:
    let by_email: Map[string, User] = users.iter().map(fn(u): (u.email, u)).collect_into()
    domains := users.iter().map(fn(u): u.domain()).collect_into[Set[string]]()
    let ports: List[i32] = lines.iter().map(parse_port).collect_into[Result[List[i32], ParseError]]()?
    .Ok()
```

- **Changes:** one trait, one method, and implementations for `List`,
  `Map`, `Set`, `Result` and `Option`.
- **C1 breaks if `collect` itself becomes generic.** `xs := it.collect()`
  then has no expected type and is `unresolved-generic-placeholder`. Rust's
  answer, `Vec<_>`, is not available: `_` can't appear inside a type
  argument. So the generic method needs its own name, or hd needs
  default type arguments, a new language feature.
- **Result collection** needs
  `impl[T, E, C < FromIterator[T]] FromIterator[Result[T, E]] for Result[C, E]`.
  `T` appears only in the trait's arguments; the spec's implementation
  rules would need checking for that shape.
- **Extensibility:** any user collection implements `FromIterator`.
- **Spec scope:** a prelude method's signature names `FromIterator`, which
  pulls the trait into the spec.

## Option 4: Target Constructors

**Idea.** Swift's `Array(seq)` and `Dictionary(...)`, Rust's
`HashMap::from_iter`. Each collection offers an associated function that
takes an iterator. All-or-nothing collection is a plain function, as in
Gleam's `result.all`.

```text
use std.collections.Set
use std.result
use std.option

fn report(users: List[User], lines: List[string], names: List[string]) -> Result[void, ParseError]:
    by_email := Map::from_pairs(users.iter().map(fn(u): (u.email, u)))
    domains := Set::from_iter(users.iter().map(fn(u): u.domain()))
    ports := result.all(lines.iter().map(parse_port))?
    found := option.all(names.iter().map(find_port))
    .Ok()
```

With the pipe, the call reads left to right. [CS2](CHAINING_STUDY.md#owner-decisions)
allows a bare path step; whether `Type::name` counts as one is
[Method And Field References question 6](METHOD_REFERENCES.md#6-is-a-callee-path-a-bare-pipe-step):

```text
fn index(users: List[User]) -> Map[string, User]:
    users.iter().map(fn(u): (u.email, u)) |> Map::from_pairs  # hypothetical syntax
```

- **Changes:** four `std` functions; no language rule.
- **Reading:** without the pipe the target comes first and the chain is
  nested inside it.
- **Extensibility:** a user collection adds its own `from_iter`, with the
  same shape as `std`'s.

## Option 5: Collector Values

**Idea.** Java's `Collectors` and Scala's `to(factory)`. A collector is a
value that says how to build the target; one method drains into any
collector. With CS7's closure-backed style, a collector is a `data` type
holding closures.

```text
pub data Collector[T, C]:
    add: fn(T) -> void
    finish: fn() -> C

impl[T] Iterator[T]:
    pub fn collect_with[C](mut self, collector: Collector[T, C]) -> C:
        pass

fn report(users: List[User]) -> void:
    by_email := users.iter().map(fn(u): (u.email, u)).collect_with(map_collector())
```

- **Changes:** one type, one method, and one function per target.
- **Strength:** collectors compose, as Java's `groupingBy(key, counting())`
  does.
- **Cost:** a fresh collector per call, and the most machinery for the
  six use cases.

## Duplicate Keys

Applies to every option that builds a `Map` from pairs.

| Policy | Precedent | Effect in hd |
| --- | --- | --- |
| **Last wins, first position kept** | hd map literals and comprehensions; Kotlin, Gleam, Python | `to_map` and `{for ... => k: v}` agree; a lost value is silent |
| **Panic** | Swift `uniqueKeysWithValues`; C# and Java throw | a data bug stops the program; no silent loss |
| **`Result`** | none in the survey | `to_map()?` everywhere; most callers can't act on the error |
| **Merge closure** | Swift `uniquingKeysWith`, Java's third argument | a second method, such as `to_map_with(fn(old, new): old + new)` |
| **First wins** | none in the survey | disagrees with map literals |

A last-wins `to_map` and a map comprehension give the same map for the same
input, which the other policies do not.

## All-Or-Nothing Collection

| Choice | Example | Notes |
| --- | --- | --- |
| Loop with `?` | Option 1 | three to five lines; works today |
| `try_collect()` on `Iterator[Result[T, E]]` and `Iterator[T?]` | Option 2 | Rust's unstable name; two disjoint inherent impls |
| `FromIterator` impl for `Result` | Option 3 | Rust's stable way; needs the generic `collect` |
| `result.all(items)`, `option.all(items)` | Option 4 | Gleam's way; free functions in `std` |

Common points:

- All four stop at the first `.Err` or `.None` and leave the rest of the
  iterator undrained, as Rust does.
- "All or nothing" differs from "drop the missing ones". The second is a
  lazy adapter such as `filter_map`, and the names must not suggest each
  other.
- The error type isn't converted: `try_collect()?` converts at the `?`, by
  [Error Conversion](../spec/05-expressions.md#error-conversion).

## Spec Or Standard Library

The [scope rule](../AGENTS.md#spec-scope-for-the-standard-library) names a
std item in the spec only when the compiler or runtime supports it, it is
in the prelude, or syntax refers to it.

| Item | Where | Why |
| --- | --- | --- |
| `collect() -> List[T]` | spec (already) | a method of the prelude `Iterator` |
| `to_map`, `try_collect` (Option 2) | spec or STDLIB | methods of a prelude type returning prelude types; the owner picks |
| `to_set`, `Set::from_iter` | STDLIB only | `Set` is not in the spec; spec examples must not use it |
| `FromIterator` (Option 3) | spec | a prelude method's signature names it |
| `Map::from_pairs` (Option 4) | STDLIB | a std function on a built-in type; nothing in the language needs it |
| `result.all`, `option.all` (Option 4) | STDLIB | functions of non-prelude modules |
| Duplicate-key policy | with the method | the spec already fixes it for literals and comprehensions |

## Comparison

| | 1 Comprehensions | 2 Concrete methods | 3 Generic + trait | 4 Constructors | 5 Collectors |
| --- | --- | --- | --- | --- | --- |
| C1 no annotation | yes | yes | only if `collect` stays separate | yes | yes |
| C2, C3 map | comprehension | `to_map()` | annotation or type argument | `Map::from_pairs(...)` | `collect_with(map_collector())` |
| C4 set | loop | `to_set()` | `collect_into[Set[string]]()` | `Set::from_iter(...)` | `collect_with(set_collector())` |
| C5, C6 | loop | `try_collect()` | long type argument | `result.all(...)` | a collector per case |
| Ends a chain | no | yes | yes | with the pipe | yes |
| User collections | loop | constructor | implement the trait | constructor | write a collector |
| Rules added | 0 | 0 | 0 (or default type arguments) | 0 | 0 |
| `std` items added | 0 | 4 | 1 trait, 1 method, 5 impls | 4 | 1 type, 1 method, 1 per target |
| Agent-writability | a new idiom per target | one name per target | needs the target type spelled | nested or piped | extra concept |
| Spec growth | none | small, optional | a trait | none | a type |

## Ranking By Design Cost Order

Options 2 to 5 are all core library additions (kind 4), so the
[order](../AGENTS.md#design-cost-order) breaks the tie by counting changes.

| Rank | Option | Costliest change | Changes |
| --- | --- | --- | --- |
| 1 | Comprehensions and loops | none | 0 |
| 2 | Concrete methods | library (4) | 4 methods |
| 2 | Target constructors | library (4) | 4 functions |
| 4 | Collector values | library (4) | 1 type, 1 method, 3 or more functions |
| 5 | Generic collect with a trait | library (4) | 1 trait, 1 method, 5 impls |

Option 3 grows to a language change if the owner wants `collect()` itself
generic without breaking C1.

## Recommendation

**Recommendation:** Option 2, concrete draining methods, with last-wins
duplicate keys.

- `collect()` keeps its meaning, so C1 needs no annotation.
- Each name says its target, which suits code written by agents and read
  by people. It is what Kotlin, C# and Gleam ship.
- It needs no new rule: instantiation-specific inherent `impl` blocks
  already exist.
- Last-wins makes `to_map()` and `{for ... => k: v}` agree.
- It gives up user extensibility of the iterator side; a user collection
  offers a constructor, as in Option 4, and the two coexist.

Next best: Option 4, which ties on cost but reads inside-out without the
pipe. Option 1 remains a fine answer if the owner wants zero growth.

## Questions For The Owner

### 1. How does an iterator end in a non-list collection?

Effect: decides the spelling of C2 to C6.

- **A.** Comprehensions and loops only (Option 1).
- **B.** Concrete methods: `to_map`, `to_set`, `try_collect` (Option 2).
- **C.** Target constructors: `Map::from_pairs(items)` (Option 4).
- **D.** Generic `collect_into[C]` with `FromIterator` (Option 3).

**Recommendation:** B.

```text
fn index(users: List[User]) -> Map[string, User]:
    users.iter().map(fn(u): (u.email, u)).to_map()
```

### 2. What does `to_map` do with equal keys?

Effect: C3 has two users with one domain.

- **A.** The last value wins and the key keeps its first position, as in
  map literals.
- **B.** Panic.
- **C.** Return a `Result`.

**Recommendation:** A. A merge-closure variant can come later.

```text
fn by_domain(users: List[User]) -> Map[string, User]:
    users.iter().map(fn(u): (u.domain(), u)).to_map()
```

### 3. How are results collected all-or-nothing?

Effect: C5 and C6.

- **A.** A loop with `?`.
- **B.** `try_collect()` on iterators of `Result` and of `T?`.
- **C.** `result.all(items)` and `option.all(items)` in `std`.

**Recommendation:** B, if question 1 is B; C otherwise.

```text
fn parse_all(lines: List[string]) -> Result[List[i32], ParseError]:
    .Ok(lines.iter().map(parse_port).try_collect()?)
```

### 4. Are `to_map` and `try_collect` in the specification?

Effect: they are methods of a prelude type that return prelude types, so
the scope rule allows either place.

- **A.** Yes, in the [Iterator Adapters](../spec/06-control-flow.md#iterator-adapters)
  table beside `collect`.
- **B.** No, only in STDLIB.md, like the other `std` inherent methods.

**Recommendation:** B. The language needs neither, and
[STDLIB decision 8](STDLIB.md#owner-decisions) puts extra built-in methods
in `std`. `to_set` is STDLIB-only either way.

```text
fn active_names(users: List[User]) -> List[string]:
    users.iter().filter(fn(u): u.active).map(fn(u): u.name).collect()
```

### 5. May a comprehension use `?`?

Effect: the spec bans `return` in a comprehension but is silent on `?`,
which also leaves the function. Option 1's C5 depends on it.

- **A.** No, like `return`.
- **B.** Yes; it leaves the enclosing function.

**Recommendation:** A, which keeps comprehensions free of jumps.

```text
fn parse_all(lines: List[string]) -> Result[List[i32], ParseError]:
    .Ok([for line in lines => parse_port(line)?])
```

## Sources

- Rust: [`FromIterator`](https://doc.rust-lang.org/std/iter/trait.FromIterator.html), [`HashMap`](https://doc.rust-lang.org/std/collections/struct.HashMap.html), [`Iterator::try_collect`](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.try_collect)
- Kotlin: [`toMap`](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.collections/to-map.html), [`associateBy`](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.collections/associate-by.html), [`toSet`](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.collections/to-set.html)
- Gleam: [`gleam/yielder`](https://hexdocs.pm/gleam_yielder/gleam/yielder.html), [`gleam/dict`](https://gleam-stdlib.hexdocs.pm/gleam/dict.html), [`gleam/result`](https://gleam-stdlib.hexdocs.pm/gleam/result.html)
- Swift: [`Dictionary.swift`](https://github.com/swiftlang/swift/blob/main/stdlib/public/core/Dictionary.swift)
- C#: [`Enumerable.ToDictionary`](https://learn.microsoft.com/en-us/dotnet/api/system.linq.enumerable.todictionary)
- Java: [`Collectors`](https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/util/stream/Collectors.html)
- Scala: [Collections migration to 2.13](https://docs.scala-lang.org/overviews/core/collections-migration-213.html), [`IterableOnceOps`](https://www.scala-lang.org/api/current/scala/collection/IterableOnceOps.html)
- Python: [`dict`](https://docs.python.org/3/library/stdtypes.html#dict)
- Haskell: [`Data.Traversable`](https://hackage.haskell.org/package/base/docs/Data-Traversable.html)

## Parse Log

Every `text` block was parsed with the reference parser (`parseSource` in
[spec/reference-parser/parser.ts](../spec/reference-parser/parser.ts)) on
2026-09-29. Parsing checks syntax only; no block is claimed to type-check.
The new methods and functions need no new syntax, so their blocks parse
today. The one pipe line is marked `# hypothetical syntax`. There the
parser reports `deferred-method-value`, since `Map::from_pairs` without
arguments is a reserved spelling. With `|>` replaced by `|` and
`Map::from_pairs` by a name, that block parses.

| Block | Section | Result |
| --- | --- | --- |

| 1 | Use Cases | parses |
| 2 | Option 1: Comprehensions And Loops | parses |
| 3 | Option 1: Comprehensions And Loops | parses |
| 4 | Option 2: Concrete Draining Methods | parses |
| 5 | Option 2: Concrete Draining Methods | parses |
| 6 | Option 3: Generic Collect With A Target Trait | parses |
| 7 | Option 3: Generic Collect With A Target Trait | parses |
| 8 | Option 4: Target Constructors | parses |
| 9 | Option 4: Target Constructors | `deferred-method-value` on line 2; desugared: parses |
| 10 | Option 5: Collector Values | parses |
| 11 | 1. How does an iterator end in a non-list collection? | parses |
| 12 | 2. What does `to_map` do with equal keys? | parses |
| 13 | 3. How are results collected all-or-nothing? | parses |
| 14 | 4. Are `to_map` and `try_collect` in the specification? | parses |
| 15 | 5. May a comprehension use `?`? | parses |
