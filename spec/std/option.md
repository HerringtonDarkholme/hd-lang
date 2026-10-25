# Option

Status: standard library specification draft.

This chapter defines the part of `std.option` that `lib/std` writes in
ordinary hd over the language tier. That is the inherent methods
`and_then`, `unwrap_or`, `or`, `ok_or`, `is_some`, `is_none`, and `expect`
of `T?`.

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

`std.option` gives every `T?` three more methods:

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
| r[std-option.or] `or` | `fn or(self, other: T?) -> T?` | the optional itself for `.Some(_)`, and `other` for `.None` |

1. r[std-option.methods.inherent] All three are inherent methods that `std` declares on `T?`, so every optional has them without a `use`, by [`trait.own.inherent.std.no-use`](../lang/09-traits.md#r-trait.own.inherent.std.no-use).
2. r[std-option.and-then.once] `and_then` calls `transform` once for `.Some`, and never for `.None`.
3. r[std-option.and-then.flat] `and_then` adds no optional layer: its result is the `U?` that `transform` returns, not a `U??`.
4. r[std-option.methods.non-suspending] No method suspends. The `transform` callback is a plain function type with the empty row.
5. r[std-option.or.layers] `or` returns an optional, not a `T`, so calls chain: `flag.or(variable).or(fixed)` is `.Some` of the first present value, and `.None` when all are `.None`.

> **Note.** `fallback` and `other` are ordinary arguments, so each is
> evaluated before the call, even when the optional is present.

> **Why.** Postfix `?` works only where the enclosing function or closure
> returns an optional. `and_then` chains two optional steps in any
> expression, as Rust's `Option::and_then` does. `or` layers fallbacks, as
> Rust's `Option::or` does; a costly fallback is a `match`, which is lazy.

## Variant Tests And Conversions

`std.option` also gives every `T?` four methods that test the variant, or
turn it into a value:

```text
fn port(text: string?) -> Result[string, string]:
    text.ok_or("no port given")

fn greeting(name: string?) -> string:
    if name.is_none():
        return "hello"
    known := name.expect("checked above")
    "hello, ${known}"
```

| Rule | Method | Result |
| --- | --- | --- |
| r[std-option.ok-or] `ok_or` | `fn ok_or[E](self, error: E) -> Result[T, E]` | `.Ok(value)` for `.Some(value)`, and `.Err(error)` for `.None` |
| r[std-option.is-some] `is_some` | `fn is_some(self) -> bool` | `true` for `.Some(_)`, and `false` for `.None` |
| r[std-option.is-none] `is_none` | `fn is_none(self) -> bool` | `false` for `.Some(_)`, and `true` for `.None` |
| r[std-option.expect] `expect` | `fn expect(self, message: string) -> T` | `value` for `.Some(value)`, and a panic for `.None` |

1. r[std-option.tests.inherent] The four are inherent methods that `std` declares on `T?` as well, so they need no `use` either.
2. r[std-option.expect.panic] `expect` on `.None` calls `panic(message)`, so its category is `explicit-panic`. Panic: `explicit-panic`.
3. r[std-option.tests.non-suspending] None of the four suspends.

> **Note.** `error` and `message` are ordinary arguments, so each is
> evaluated before the call, whichever variant the optional holds.

> **Why.** `ok_or` joins an optional step to a fallible one, as Rust's
> `Option::ok_or` does. `expect` names the broken assumption in its panic
> message, where a bare `match` would repeat it at every call.

## Optional Context

`context` turns a `.None` into an error with a message, as anyhow's
`Context` does for `Option`:

```text
use std.error.Error

fn first_word(words: List[string]) -> Result[string, *dyn Error]:
    word := words.first().context("no words")?
    .Ok(word)
```

For an empty list, `first_word` returns an error that displays as `no words`.

1. r[std-option.context.reference-result] `std` declares the inherent method `pub fn context(self, text: string) -> Result[T, *dyn Error]` on `T?`, so every optional has it without a `use`, by [`trait.own.inherent.std.no-use`](../lang/09-traits.md#r-trait.own.inherent.std.no-use).
2. r[std-option.context.some] `.Some(value).context(text)` returns `.Ok(value)`.
3. r[std-option.context.none-reference] `.None.context(text)` returns `.Err(message(text))`, where `message` is [`std-error.message.reference`](error.md#r-std-error.message.reference).
4. r[std-option.context.non-suspending] `context` does not suspend.

> **Note.** `text` is an ordinary argument, evaluated before the call,
> whichever variant the optional holds.

> **Why.** An optional step in a function that returns
> `Result[T, *dyn Error]` otherwise needs `ok_or` and an error type.

See also: [Result](result.md), [Runtime Panics](../lang/06-control-flow.md#runtime-panics), [Variance](../lang/04-type-system.md#variance).
