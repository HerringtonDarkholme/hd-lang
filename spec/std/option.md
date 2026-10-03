# Option

Status: standard library specification draft.

This chapter defines the part of `std.option` that `lib/std` writes in
ordinary hd over the language tier: the inherent methods `and_then` and
`unwrap_or` of `T?`.

The language tier keeps what the compiler knows by name
([Optional Types](../lang/04-type-system.md#optional-types)):

| Item | Why it stays in the language tier |
| --- | --- |
| `Option[T]`, `T?`, `.Some`, and `.None` | the prelude enum, its sugar, and its construction rules |
| implicit wrapping and nesting | [Implicit Wrapping And Nesting](../lang/04-type-system.md#implicit-wrapping-and-nesting) |
| postfix `?` on an optional | [Representation And Propagation](../lang/04-type-system.md#representation-and-propagation) |
| invariance in `T` | [`types.option.invariant`](../lang/04-type-system.md#r-types.option.invariant) |

`map` on `T?` is in [List And Optional Map](iter.md#list-and-optional-map).

## Optional Methods

`std.option` gives every `T?` two more methods:

```text
fn first_even(values: List[i32]) -> i32?:
    for value in values:
        if value % 2 == 0:
            return .Some(value)
    .None

fn quarter(value: i32) -> i32?:
    if value % 4 == 0: .Some(value / 4) else: .None

fn quarter_of_first_even(values: List[i32]) -> i32:
    first_even(values).and_then(quarter).unwrap_or(0)
```

| Rule | Method | Result |
| --- | --- | --- |
| r[std-option.and-then] `and_then` | `fn and_then[U](self, transform: fn(T) -> U?) -> U?` | `transform(value)` for `.Some(value)`, and `.None` for `.None` |
| r[std-option.unwrap-or] `unwrap_or` | `fn unwrap_or(self, fallback: T) -> T` | `value` for `.Some(value)`, and `fallback` for `.None` |

1. r[std-option.methods.inherent] Both are inherent methods that `std` declares on `T?`, so every optional has them without a `use`, by [`trait.own.inherent.std.no-use`](../lang/09-traits.md#r-trait.own.inherent.std.no-use).
2. r[std-option.and-then.once] `and_then` calls `transform` once for `.Some`, and never for `.None`.
3. r[std-option.and-then.flat] `and_then` adds no optional layer: its result is the `U?` that `transform` returns, not a `U??`.
4. r[std-option.methods.non-suspending] Neither method suspends. The `transform` callback is a plain function type with the empty row.

> **Note.** `fallback` is an ordinary argument, so it is evaluated before
> the call, even when the optional is present.

> **Why.** Postfix `?` works only where the enclosing function or closure
> returns an optional. `and_then` chains two optional steps in any
> expression, as Rust's `Option::and_then` does.

See also: [Result](result.md), [Variance](../lang/04-type-system.md#variance).
