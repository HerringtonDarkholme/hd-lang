# Cmp

Status: standard library specification draft.

This chapter defines the part of `std.cmp` that `lib/std` writes in
ordinary hd over the language tier:

- what a derived `Eq`, `PartialOrd`, or `Ord` compares, through the
  traits' templates;
- how tuples compare, through the traits' tuple templates;
- the function `clamp`;
- the `Reverse[T]` wrapper.

The language tier keeps what the compiler knows by name
([Comparison Traits](../lang/09-traits.md#comparison-traits)):

| Item | Why it stays in the language tier |
| --- | --- |
| `Eq`, `PartialOrd`, `Ord`, `Ordering` | lang items that `==` and the relational operators call |
| `@derive` and its checks | `derive-field-missing-trait`, `missing-derived-bound`, and `mixed-derived-law` are compiler diagnostics ([Derived Implementations](../lang/09-traits.md#derived-implementations), [Law Partners](../lang/09-traits.md#law-partners)) |
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
