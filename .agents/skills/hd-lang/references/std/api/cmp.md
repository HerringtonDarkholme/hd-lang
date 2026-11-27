# `std.cmp`

Generated from `hd doc std.cmp`. Run `hd doc std.cmp.NAME` in a package for one item's full text.

## `Ordering`

```text
pub enum Ordering
```

How two values order: `Less`, `Equal`, or `Greater`.

- `pub fn then(self, other: Ordering) -> Ordering`: `self` unless it is `.Equal`, and `other` for `.Equal`: chains the keys of a comparator.

## `PartialOrd`

```text
pub trait PartialOrd < ?Copy & Eq
```

Partial ordering: `partial_cmp` answers `.None` when the two values are unordered, as NaN is.

## `Ord`

```text
pub trait Ord < ?Copy & PartialOrd
```

Total ordering: `cmp` always answers, and agrees with the type's `PartialOrd`.

## `min`

```text
pub fn min[T < Ord](a: T, b: T) -> T
```

The smaller of the two; the first on a tie.

## `max`

```text
pub fn max[T < Ord](a: T, b: T) -> T
```

The larger of the two; the second on a tie.

## `clamp`

```text
pub fn clamp[T < Ord](value: T, low: T, high: T) -> T
```

`value` held between `low` and `high`.

## `Reverse`

```text
pub struct Reverse[T]
```

A wrapper whose order is the wrapped value's order reversed.

- `pub value: T`: The wrapped value.

## `Id`

```text
pub struct Id[T]
```

A reference compared and hashed by the place it refers to, not by the value there, so a map keyed by `Id[T]` keys by identity.

- `pub target: *T`: The reference.

## `DeepVisits`

```text
pub struct DeepVisits
```

The pairs of references a `deep_equal` call is comparing; a pair met again counts as equal, which ends a walk around a cycle.

## `DeepEq`

```text
pub trait DeepEq
```

Values that `deep_equal` compares member by member, through references, ending on cycles; `@derive(DeepEq)` writes it.

## `deep_equal`

```text
pub fn deep_equal[T < DeepEq](left: T, right: T) -> bool
```

Whether `left` and `right` are equal member by member, through references; a pair of references met again while comparing counts as equal, so a comparison of cyclic structures ends.
