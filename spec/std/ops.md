# Ops

Status: standard library specification draft.

This chapter defines the part of `std.ops` that `lib/std` writes in
ordinary hd over the language tier:

- the `Default` trait;
- the standard `Default` implementations, including its tuple template.

The language tier keeps the rest of `std.ops`, since the compiler knows
those items by name:

| Item | Why it stays in the language tier |
| --- | --- |
| the operator traits, such as `Add` and `Neg` | lang items that operators call ([Operator Traits](../lang/05-expressions.md#operator-traits)) |
| `NumSuffix`, `StrPrefix`, `Template` | the compiler recognizes them by their qualified names ([Literal Suffixes](../lang/05-expressions.md#literal-suffixes)) |

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
