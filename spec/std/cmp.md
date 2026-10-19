# Cmp

Status: standard library specification draft.

This chapter defines the part of `std.cmp` that `lib/std` writes in
ordinary hd over the language tier:

- what a derived `Eq`, `PartialOrd`, or `Ord` compares, through the
  traits' templates;
- how tuples compare, through the traits' tuple templates;
- the functions `min`, `max`, and `clamp`;
- the `Reverse[T]` wrapper;
- the traits that `Ordering` implements;
- equality of references, `Id[T]`, which compares references by place,
  and `deep_equal`, which ends on cyclic structures.

The language tier keeps what the compiler knows by name
([Comparison Traits](../lang/09-traits.md#comparison-traits)):

| Item | Why it stays in the language tier |
| --- | --- |
| `Eq`, `PartialOrd`, `Ord`, `Ordering` | lang items that `==` and the relational operators call |
| `@derive` and its checks | `derive-field-missing-trait`, `missing-derived-bound`, and `mixed-derived-law` are compiler diagnostics ([Derived Implementations](../lang/09-traits.md#derived-implementations), [Related Traits](../lang/09-traits.md#related-traits)) |
| templates and tuple templates | [Typed Derivation](../lang/14-annotations.md#typed-derivation) |

## Derived Equality

`@derive(Eq)` compares two values member by member:

```text
@derive(Eq)
enum State:
    Active(code: i32, label: string)
    Paused(code: i32)

fn same(a: State, b: State) -> bool:
    a == b
```

1. r[std-cmp.derive.eq.template] `std.cmp` declares the [template](../lang/14-annotations.md#templates) of `Eq`, which `@derive(Eq)` instantiates.
2. r[std-cmp.derive.eq.compare-fields] Derived `Eq` compares every declared data field, including embedded fields, by its `Eq` implementation.
3. r[std-cmp.derive.eq.no-exclusion] No field is implicitly excluded.
4. r[std-cmp.derive.eq.enum] Derived enum equality first compares the variant, then every payload field of that variant, including common enum fields.
5. r[std-cmp.derive.eq.variants] Different variants are unequal.
6. r[std-cmp.derive.eq.eq] Derived `Eq` requires every compared field to satisfy `Eq`.
7. r[std-cmp.derive.eq.cycles] Derived equality does not detect cycles or track previously compared objects: it recursively invokes each field's `Eq` implementation.
8. r[std-cmp.derive.eq.stack] A comparison that repeatedly traverses a cycle may exhaust the execution stack.
9. r[std-cmp.derive.eq.references] A reference field compares by the values the two references refer to, through [Reference Equality](#reference-equality). So two trees built alike through references are equal.

## Derived Ordering

`@derive(PartialOrd, Ord)` orders values member by member:

```text
@derive(Eq, PartialOrd, Ord)
data Key:
    first: i32
    second: i32

fn before(a: Key, b: Key) -> bool:
    a < b
```

1. r[std-cmp.derive.ord.template] `std.cmp` declares the templates of `PartialOrd` and `Ord`, which `@derive` instantiates.
2. r[std-cmp.derive.ord.support] `@derive(PartialOrd, Ord)` supports data and enums.
3. r[std-cmp.derive.ord.data] Derived ordering is lexicographic in declared data-field order, including embedded fields.
4. r[std-cmp.derive.ord.variants] For enums, distinct variants compare by variant declaration order.
5. r[std-cmp.derive.ord.same-variant] Values of the same variant compare shared enum data in declaration order, followed by that variant's payload parameters in declaration order.
6. r[std-cmp.derive.ord.argument-order] Constructor argument order does not affect comparison.
7. r[std-cmp.derive.ord.partial] Derived `PartialOrd` requires every compared field to satisfy `PartialOrd`. It returns `.None` if a field comparison is unordered before a comparison result is determined.
8. r[std-cmp.derive.ord.ord] Derived `Ord` requires every compared field to satisfy `Ord`.

## Tuple Comparison

Tuples compare element by element, at every size:

```text
fn compare(a: (i32, string), b: (i32, string)) -> bool:
    a == b || a < b
```

1. r[std-cmp.tuple.templates] `std.cmp` declares [tuple templates](../lang/14-annotations.md#tuple-templates) for `Eq`, `PartialOrd`, and `Ord`. A tuple implements each when every element does.
2. r[std-cmp.tuple.elementwise] Equality compares every element, in order. Ordering is lexicographic.
3. r[std-cmp.tuple.partial] Tuple `PartialOrd` returns `.None` when an element comparison is unordered before a result is determined.
4. r[std-cmp.tuple.rest] A tuple's [rest element](../lang/04-type-system.md#rest-elements) `List[T]...` is its last element, of type `List[T]`, so it compares as that list.

> **Note.** A 13-element tuple compares as a 2-element one does: no
> tuple template stops at a size.

See also: [Derived Tuple Implementations](../lang/09-traits.md#derived-tuple-implementations),
[Tuple Structure](../lang/14-annotations.md#tuple-structure),
[Hash](hash.md).

## Min And Max

`min` and `max` pick one of two values:

```text
use std.cmp.{max, min}

fn spread(a: i32, b: i32) -> i32:
    max(a, b) - min(a, b)
```

1. r[std-cmp.min-max.decl] `std.cmp` declares `pub fn min[T < Ord](a: T, b: T) -> T` and `pub fn max[T < Ord](a: T, b: T) -> T`. Code imports them, as in `use std.cmp.{max, min}`.
2. r[std-cmp.min-max.result] `min` returns the smaller of `a` and `b`, and `max` the larger, by `cmp`.
3. r[std-cmp.min-max.tie] When `a.cmp(b)` is `.Equal`, both `min` and `max` return `a`.
4. r[std-cmp.min-max.not-prelude] Neither name is in the prelude.

> **Why.** One first-among-equals rule serves `min`, `max`, `List.min`,
> and `List.max`, as in Python. Rust's `max` returns `b` on a tie.

## Clamp

`clamp` limits a value to a range:

```text
use std.cmp.clamp

fn percent(value: i32) -> i32:
    clamp(value, 0, 100)
```

1. r[std-cmp.clamp.decl] `std.cmp` declares `pub fn clamp[T < Ord](value: T, low: T, high: T) -> T`. Code imports it, as in `use std.cmp.clamp`.
2. r[std-cmp.clamp.result] `clamp` returns `low` when `value` is below `low`, `high` when `value` is above `high`, and `value` otherwise.
3. r[std-cmp.clamp.order] A `low` above `high` panics. Panic: `explicit-panic`.

## Reverse

`Reverse[T]` wraps a value and inverts its order, as Rust's
`std::cmp::Reverse` does:

```text
use std.cmp.Reverse

fn later_first(a: i32, b: i32) -> Ordering:
    Reverse { value: a }.cmp(Reverse { value: b })  # .Less when a > b
```

1. r[std-cmp.reverse.decl] `std.cmp` declares `pub data Reverse[T]` with one public field, `value: T`. Code imports it, as in `use std.cmp.Reverse`.
2. r[std-cmp.reverse.order] When `T < Ord`, `Reverse[T]` implements `Eq`, `PartialOrd`, and `Ord`, and `Reverse { value: a }.cmp(Reverse { value: b })` is `b.cmp(a)`.
3. r[std-cmp.reverse.eq] Two `Reverse` values are equal exactly when `cmp` of their values gives `.Equal`.

## Ordering Values

`Ordering` is a lang item; `std.cmp` gives it its traits:

```text
fn is_before(a: string, b: string) -> bool:
    a.cmp(b) == Ordering.Less
```

1. r[std-cmp.ordering.eq] `Ordering` implements `Eq`. Two values are equal exactly when they are the same case.
2. r[std-cmp.ordering.debug-text] `Ordering` implements `Debug`, and writes its qualified case: `Ordering.Less`, `Ordering.Equal`, or `Ordering.Greater`.
3. r[std-cmp.ordering.no-display] `Ordering` does not implement `Display`; `debug(o)` gives its text.
4. r[std-cmp.ordering.then.decl] `std.cmp` declares the inherent method `pub fn then(self, other: Ordering) -> Ordering` on `Ordering`, so every `Ordering` has it without a `use`, by [`trait.own.inherent.std.no-use`](../lang/09-traits.md#r-trait.own.inherent.std.no-use).
5. r[std-cmp.ordering.then.result] `then` returns `self` unless `self` is `.Equal`, and returns `other` for `.Equal`. `.Less.then(o)` and `.Greater.then(o)` are `.Less` and `.Greater`; `.Equal.then(o)` is `o`.
6. r[std-cmp.ordering.then.chain] `then` chains the keys of a comparator: `b.score.cmp(a.score).then(a.name.cmp(b.name))` orders by score, highest first, and by name among equal scores.
7. r[std-cmp.ordering.then.eager] `other` is an ordinary argument, evaluated before the call, whatever `self` is. `then` does not suspend.

## Reference Equality

`std.cmp` implements `Eq` for references by the values they refer to:

```text
fn has_value(items: List[&i32], value: &i32) -> bool:
    items.contains(value)  # compares the values the references refer to
```

1. r[std-cmp.ref.eq] `std.cmp` implements `Eq` for `&T` and for `&mut T` when `T < Eq`. Two references are equal when the values they refer to are, so `r.eq(s)` is `*r == *s`.
2. r[std-cmp.ref.eq-operator] `==` written between two references stays an error, by [`ref.identity.no-equality`](../lang/13-values-and-references.md#r-ref.identity.no-equality). Generic code and derived implementations reach this implementation.

> **Why.** A reference is equal by value, as in Rust, so a derived `Eq`
> of an expression tree compares its shape. `is` keeps identity for code
> that needs it.

## Identity Keys

`Id[T]` wraps a reference and compares it by the place it refers to, so
a map keyed by `Id[T]` keys by identity:

```text
use std.cmp.Id

struct Node:
    label: string

fn visit(seen: mut Map[Id[Node], bool], node: &Node) -> bool:
    key := Id { target: node }
    if seen.get(key).is_some():
        return false
    seen[key] = true
    true
```

1. r[std-cmp.id.decl] `std.cmp` declares `pub struct Id[T]` with one public field, `target: &T`. Code imports it, as in `use std.cmp.Id`.
2. r[std-cmp.id.eq] `Id[T]` implements `Eq` for every `T`. Two ids are equal exactly when their targets refer to the same place, as [`is`](../lang/13-values-and-references.md#identity) decides.
3. r[std-cmp.id.hash] `Id[T]` implements `Hash` for every `T`, so it may be a map key. Two equal ids hash alike; the bytes written are unspecified.

## Deep Equality

`deep_equal` compares two values member by member, through references,
and ends on cyclic structures:

```text
use std.cmp.{DeepEq, deep_equal}

@derive(DeepEq)
struct Node:
    value: i32
    next: &mut Node?

fn same_ring(a: &mut Node, b: &mut Node) -> bool:
    deep_equal(a, b)
```

1. r[std-cmp.deep-equal.decl] `std.cmp` declares `pub fn deep_equal[T < DeepEq](left: T, right: T) -> bool`. Code imports it, as in `use std.cmp.{DeepEq, deep_equal}`.
2. r[std-cmp.deep-equal.trait] `std.cmp` declares `pub trait DeepEq` with the one method `fn deep_eq(self, other: Self, visits: mut DeepVisits) -> bool`.
3. r[std-cmp.deep-equal.visits] `std.cmp` declares `pub data DeepVisits` with private fields. It holds the pairs of references that one `deep_equal` call is comparing, and `deep_equal` starts with none.
4. r[std-cmp.deep-equal.template] `std.cmp` declares the template of `DeepEq`, which `@derive(DeepEq)` instantiates, and a tuple template.
5. r[std-cmp.deep-equal.derive] A derived `DeepEq` compares the variant, then every member in declaration order by its `DeepEq`, as a derived `Eq` does.
6. r[std-cmp.deep-equal.leaves] The number types, `char`, `bool`, and `string` implement `DeepEq` by `==`.
7. r[std-cmp.deep-equal.containers] Optionals, results, lists, and tuples implement `DeepEq` when their elements do, and compare element by element. `Map[K, V]` implements it when `K < Eq & Hash` and `V < DeepEq`: both maps have the same keys, and the values under each key are deeply equal.
8. r[std-cmp.deep-equal.references] `&T` and `&mut T` implement `DeepEq` when `T < DeepEq`. Two references to one place are deeply equal. Otherwise a pair the call is already comparing counts as equal, and a new pair is remembered before the values it refers to are compared.
9. r[std-cmp.deep-equal.ends] So `deep_equal` ends on every structure, cyclic ones included, as Go's `reflect.DeepEqual` does.

> **Why.** A derived `Eq` stays as light as a field-by-field comparison, so
> it keeps no record of the pairs it has compared and runs around a cycle
> until the stack is exhausted. `deep_equal` is the cycle-aware comparison
> for the structures that need one.

> **Why.** `deep_equal` reaches members through a `Structure` template, as
> the derived `Eq` and `Debug` do. hd has no runtime reflection, and
> `Structure` exists only inside a template
> ([`annot.structure.generated`](../lang/14-annotations.md#r-annot.structure.generated)).
> So a derived trait is how a std function reaches every member of a
> user type. The derived `Eq` cannot serve: its walker calls `==`, which
> carries no record of the pairs being compared.
