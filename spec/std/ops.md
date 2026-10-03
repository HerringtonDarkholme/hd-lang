# Ops

Status: standard library specification draft.

This chapter defines the part of `std.ops` that `lib/std` writes in
ordinary hd over the language tier:

- the `Default` trait;
- the standard `Default` implementations, including its tuple template;
- derived `Default`, through the trait's template, and the `@default`
  variant marker.

The language tier keeps the rest of `std.ops`, since the compiler knows
those items by name:

| Item | Why it stays in the language tier |
| --- | --- |
| the operator traits, such as `Add` and `Neg` | lang items that operators call ([Operator Traits](../lang/05-expressions.md#operator-traits)) |
| `NumSuffix`, `StrPrefix`, `Template` | the compiler recognizes them by their qualified names ([Literal Suffixes](../lang/05-expressions.md#literal-suffixes)) |
| `Range`, `RangeFrom`, `RangeTo`, `RangeFull` | lang items that range expressions build ([Range Expressions](../lang/05-expressions.md#range-expressions)) |

## Default Trait

`Default` gives a type's default value:

```text
use std.ops.Default

fn fresh[T < Default]() -> T:
    T::default()

fn start() -> (i32, string, bool):
    fresh()
```

1. r[std-ops.default.trait] `std.ops` declares `trait Default` with one associated function, `fn default() -> Self`, which returns the type's default value.
2. r[std-ops.default.import] `Default` is not a prelude name, so code imports it, as in `use std.ops.Default`.
3. r[std-ops.default.call] Generic code calls it through a bound, as in `T::default()`, by [`trait.assoc-call.parameter`](../lang/09-traits.md#r-trait.assoc-call.parameter).

### Standard Implementations

`std` implements `Default` for these types:

| Rule | Type | Default value |
| --- | --- | --- |
| r[std-ops.default.std.integer] Integers | `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, and `u64` | `0` |
| r[std-ops.default.std.float] Floats | `f32` and `f64` | `0.0` |
| r[std-ops.default.std.bool] Boolean | `bool` | `false` |
| r[std-ops.default.std.string] String | `string` | `""` |
| r[std-ops.default.std.list] List | `List[T]`, for any `T` | the empty list `[]` |
| r[std-ops.default.std.map] Map | `Map[K, V]`, for any valid key type `K` and any `V` | the empty map `{}` |
| r[std-ops.default.std.optional] Optional | `T?`, for any `T` | `.None` |
| r[std-ops.default.std.tuple.any-size] Tuple | a tuple of any size, each of whose elements implements `Default`; `()` is included | each element's default, as in `(0, "", false)` |
| r[std-ops.default.std.tuple.rest-empty] Rest tuple | a tuple with a [rest element](../lang/04-type-system.md#rest-elements) `List[T]...` whose fixed elements implement `Default` | each fixed element's default, then an empty rest |

1. r[std-ops.default.std.tuple-template] `std.ops` declares a [tuple template](../lang/14-annotations.md#tuple-templates) for `Default`, which builds each element's default through the tuple's `Structure`.
2. r[std-ops.default.std.rest-any] A rest tuple's `T` need not implement `Default`, since its default rest holds no items.
3. r[std-ops.default.std.missing-element] A tuple with an element that does not implement `Default` does not implement `Default`. Using it where `Default` is required is an error. Error: `unsatisfied-trait-bound`.

```text
use std.ops.Default

data Secret:
    value: i32

fn fresh[T < Default]() -> T:
    T::default()

fn start() -> (i32, Secret):
    fresh()  # error: unsatisfied-trait-bound
```

> **Note.** The default of `(i32, string, List[i32]...)` is `(0, "")`:
> each fixed element's default, then an empty rest.

> **Why.** Generic code needs a starting value it cannot write as a
> literal, as `T::zero()` gives numbers one. A rest tuple's rest is a list,
> and an empty list needs nothing from its items.

See also: [Associated Function Calls](../lang/09-traits.md#associated-function-calls),
[Derived Tuple Implementations](../lang/09-traits.md#derived-tuple-implementations).

## Derived Default

`@derive(Default)` builds a data type's or an enum's default from its
members' defaults. A member's declared default wins over its type's. An
enum marks its default variant with `@default`:

```text
use std.ops.{Default, default}

@derive(Default)
data Settings:
    name: string
    retries: i32 = 3
    tags: List[string]

@derive(Default)
enum Shape:
    Circle(radius: f64)
    @default
    Square(side: f64, label: string)

fn start() -> (Settings, Shape):
    (Settings::default(), Shape::default())
```

`Settings::default()` is `Settings { name: "", retries: 3, tags: [] }`,
and `Shape::default()` is `.Square(0.0, "")`.

`std.ops` declares the marker:

```text
@annotate(.Variant)
pub data DefaultVariant: pass

pub fn default() -> DefaultVariant
```

1. r[std-ops.default.derive.template] `std.ops` declares the [template](../lang/14-annotations.md#templates) of `Default`, which `@derive(Default)` instantiates. It is ordinary `std.ops` code over `std.structure`, as the templates of `Eq` and `Hash` are.
2. r[std-ops.default.derive.data.declared] A derived data type's default sets a member that declares a default, as `retries: i32 = 3`, to that default.
3. r[std-ops.default.derive.data.type-default] It sets every other member to its own type's default, `F::default()`.
4. r[std-ops.default.derive.member-bound.undeclared] The template requires `Default` only of the type of a member that declares no default. Such a member whose type does not implement it makes the type not derivable, reported at the opt-in and naming the member. Error: `member-not-derivable`.
5. r[std-ops.default.derive.member-bound.declared] A member that declares a default needs no `Default` on its type, as `secret: Secret = Secret { value: 7 }` shows below.
6. r[std-ops.default.derive.marker] `std.ops` declares the fact type `DefaultVariant` and the function `default`. Code imports `default`, as in `use std.ops.default`, and writes `@default` on a variant, which attaches `default()`.
7. r[std-ops.default.derive.enum] A derived enum's default is its variant marked `@default`.
8. r[std-ops.default.derive.payload] The default of a marked payload variant fills each payload member with its own type's default.
9. r[std-ops.default.derive.one-variant] A derived enum must mark exactly one variant `@default`. An enum with no marked variant is an error, reported on the `@derive` line. Error: `invalid-default-variant`.
10. r[std-ops.default.derive.one-variant.several] An enum with several marked variants is the same error, reported on the second `@default`. Error: `invalid-default-variant`.

```text
use std.ops.{Default, default}

@derive(Default)  # error: invalid-default-variant
enum Mode:
    Fast
    Slow

@derive(Default)
enum Level:
    @default
    Low
    @default  # error: invalid-default-variant
    High
```

```text
use std.ops.Default

data Secret:
    value: i32

@derive(Default)  # error: member-not-derivable
data Vault:
    secret: Secret
```

```text
use std.ops.Default

data Secret:
    value: i32

@derive(Default)
data Safe:
    secret: Secret = Secret { value: 7 }
    count: i32

fn opened() -> i32:
    Safe::default().secret.value
```

> **Note.** A generic type gets `T < Default` for each type parameter that
> a built member uses, by [`annot.bound.params`](../lang/14-annotations.md#r-annot.bound.params).
> A member that a derivation block omits takes its declared default, by
> [`annot.omit.default`](../lang/14-annotations.md#r-annot.omit.default).

> **Why.** No variant is more neutral than another, so the enum names its
> default, as Rust's `#[default]` does. A data type has one shape, so its
> default needs no marker.

See also: [Derived Implementations](../lang/09-traits.md#derived-implementations),
[Templates](../lang/14-annotations.md#templates).
