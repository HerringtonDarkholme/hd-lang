# Option

Status: standard library specification draft.

This chapter defines the part of `std.option` that `lib/std` writes in
ordinary hd over the language tier. That is the inherent methods
`and_then`, `unwrap_or`, `unwrap_or_else`, `or`, `or_else`, `filter`,
`ok_or`, `is_some`, `is_none`, `unwrap`, and `expect`
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
| r[std-option.is-some] `is_some` | `fn is_some(&self) -> bool` | `true` for `.Some(_)`, and `false` for `.None` |
| r[std-option.is-none] `is_none` | `fn is_none(&self) -> bool` | `false` for `.Some(_)`, and `true` for `.None` |
| r[std-option.expect] `expect` | `fn expect(self, message: string) -> T` | `value` for `.Some(value)`, and a panic for `.None` |

1. r[std-option.tests.inherent] The four are inherent methods that `std` declares on `T?` as well, so they need no `use` either.
2. r[std-option.expect.panic] `expect` on `.None` calls `panic(message)`, so its category is `explicit-panic`. Panic: `explicit-panic`.
3. r[std-option.tests.non-suspending] None of the four suspends.

> **Note.** `error` and `message` are ordinary arguments, so each is
> evaluated before the call, whichever variant the optional holds.

> **Why.** `ok_or` joins an optional step to a fallible one, as Rust's
> `Option::ok_or` does. `expect` names the broken assumption in its panic
> message, where a bare `match` would repeat it at every call.

## Borrowed Payloads

`as_ref` and `as_mut` lend the payload of an optional without moving it
out, and `std` gives an optional reference, `(&T)?`, a `copied` method that
copies the value it refers to:

```text
fn count_of(found: (&usize)?) -> usize:
    found.copied().unwrap_or(0)
```

| Rule | Method | Result |
| --- | --- | --- |
| r[std-option.as-ref] `as_ref` | `fn as_ref(&self) -> (&T)?` | `.Some` of a reference to the payload for `.Some(_)`, and `.None` for `.None` |
| r[std-option.as-mut] `as_mut` | `fn as_mut(&mut self) -> (&mut T)?` | `.Some` of a writable reference to the payload for `.Some(_)`, and `.None` for `.None` |
| r[std-option.copied] `copied` | `fn copied(self) -> T?` on `(&T)?` | `.Some(*value)` for `.Some(value)`, and `.None` for `.None` |

1. r[std-option.copied.inherent] `copied` is an inherent method that `std` declares on `(&T)?` for a `T` that is copied, so it needs no `use`.
2. r[std-option.as-ref.in-place] `as_ref` and `as_mut` leave the optional in place, for a payload that is copied or not. A write through the reference that `as_mut` gives changes the payload inside the optional.

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

## Lazy Fallbacks And Filtering

Lazy fallbacks run only for an absent optional; filtering borrows a present payload before deciding whether to keep it.

```text
fn port(value: i32?) -> i32:
    value.filter(fn(port: &i32) -> bool: *port > 0).unwrap_or_else(fn() -> i32: 8080)
```

| Rule | Method | Result |
| --- | --- | --- |
| r[std-option.unwrap-or-else.value] `unwrap_or_else` | `fn unwrap_or_else[F < ?Copy & FnMut() -> T $ R, $ R](self, mut fallback: F) -> T $ R` | The payload of `.Some`, or `fallback()` for `.None` |
| r[std-option.or-else.value] `or_else` | `fn or_else[F < ?Copy & FnMut() -> T? $ R, $ R](self, mut fallback: F) -> T? $ R` | The optional itself for `.Some`, or `fallback()` for `.None` |
| r[std-option.filter.value] `filter` | `fn filter[F < ?Copy & FnMut(&T) -> bool $ R, $ R](self, mut predicate: F) -> T? $ R` | Keeps `.Some(value)` exactly when `predicate(&value)` is true; otherwise `.None` |

1. r[std-option.lazy.inherent] These inherent methods need no import and accept payloads and callbacks that are not copied.
2. r[std-option.lazy.fallback-once] Each fallback is called exactly once for `.None`, and never for `.Some`.
3. r[std-option.filter.once] `filter` calls its predicate exactly once for `.Some`, and never for `.None`.
4. r[std-option.filter.borrow] The predicate borrows the payload; an accepted payload is returned unchanged, and a rejected payload is discarded.
5. r[std-option.lazy.row] The methods propagate the callback's requirement row and do not suspend.

> **Why.** These names and variant behavior follow Rust's `Option` methods.
> hd accepts generic `FnMut` callbacks, the callable trait hd provides for mutable captures; each callback is invoked at most once.
> `unwrap_or` and `or` retain their eager-argument behavior.

## Unwrapping Without A Message

`unwrap` extracts a required optional payload with a standard panic message.

```text
fn required(value: i32?) -> i32:
    value.unwrap()
```

1. r[std-option.unwrap.value] The inherent `fn unwrap(self) -> T` returns the payload of `.Some`, including a payload that is not copied.
2. r[std-option.unwrap.panic] For `.None`, `unwrap` panics with category `explicit-panic` and message ``called `Option::unwrap()` on a `None` value``. Panic: `explicit-panic`.

> **Note.** `expect(message)` remains the method for a caller-supplied panic message.
