# Result

Status: standard library specification draft.

This chapter defines the part of `std.result` that `lib/std` writes in
ordinary hd over the language tier: the inherent methods `and_then`,
`map_err`, and `unwrap_or` of `Result[T, E]`.

The language tier keeps what the compiler knows by name
([Result Types](../lang/04-type-system.md#result-types)):

| Item | Why it stays in the language tier |
| --- | --- |
| `Result[T, E]`, `.Ok`, and `.Err` | the prelude enum and its construction rules |
| postfix `?` on a `Result` | [Result Propagation](../lang/04-type-system.md#result-propagation), which may convert the error through `From` |
| invariance in `T` and `E` | [`types.option.invariant.result`](../lang/04-type-system.md#r-types.option.invariant.result) |
| a variant constructor as a function value | [`data.enum.fn-value.use`](../lang/08-data-and-enums.md#r-data.enum.fn-value.use), as in `map_err(ConfigError.Number)` |

## Result Methods

`std.result` gives every `Result[T, E]` these methods:

```text
use std.num.{parse_i32, ParseNumberError}

enum ConfigError:
    Missing
    Number(error: ParseNumberError)

fn setting(settings: Map[string, string], key: string) -> Result[string, ConfigError]:
    match settings.get(key):
        .Some(text) => .Ok(text)
        .None => .Err(ConfigError.Missing)

fn number(text: string) -> Result[i32, ConfigError]:
    parse_i32(text).map_err(ConfigError.Number)

fn port(settings: Map[string, string]) -> i32:
    setting(settings, "port").and_then(number).unwrap_or(8080)
```

| Rule | Method | Result |
| --- | --- | --- |
| r[std-result.and-then] `and_then` | `fn and_then[U](self, transform: fn(T) -> Result[U, E]) -> Result[U, E]` | `transform(value)` for `.Ok(value)`, and `.Err(error)` for `.Err(error)` |
| r[std-result.map-err] `map_err` | `fn map_err[F](self, transform: fn(E) -> F) -> Result[T, F]` | `.Ok(value)` for `.Ok(value)`, and `.Err(transform(error))` for `.Err(error)` |
| r[std-result.unwrap-or] `unwrap_or` | `fn unwrap_or(self, fallback: T) -> T` | `value` for `.Ok(value)`, and `fallback` for `.Err(_)` |

1. r[std-result.methods.inherent] The three are inherent methods that `std` declares on `Result[T, E]`, so every result has them without a `use`, by [`trait.own.inherent.std.no-use`](../lang/09-traits.md#r-trait.own.inherent.std.no-use).
2. r[std-result.and-then.once] `and_then` calls `transform` once for `.Ok`, and never for `.Err`.
3. r[std-result.map-err.once] `map_err` calls `transform` once for `.Err`, and never for `.Ok`.
4. r[std-result.and-then.same-error] `and_then` keeps the error type `E`, and converts no error. A `transform` whose result has another error type is an error. Error: `type-mismatch`.
5. r[std-result.methods.non-suspending] No method suspends. Each `transform` callback is a plain function type with the empty row.

```text
use std.num.{parse_i32, ParseNumberError}

enum ConfigError:
    Missing
    Number(error: ParseNumberError)

fn setting(settings: Map[string, string], key: string) -> Result[string, ConfigError]:
    match settings.get(key):
        .Some(text) => .Ok(text)
        .None => .Err(ConfigError.Missing)

fn parsed(text: string) -> Result[i32, ParseNumberError]:
    parse_i32(text)

fn port(settings: Map[string, string]) -> Result[i32, ConfigError]:
    setting(settings, "port").and_then(parsed)  # error: type-mismatch
```

> **Note.** A step with another error type is mapped first, as
> `and_then(number)` does above with `map_err`. Postfix `?` is the one
> place an error converts through `From` by itself.

> **Note.** `fallback` is an ordinary argument, so it is evaluated before
> the call, even when the result is `.Ok`.

> **Why.** Postfix `?` works only where the enclosing function or closure
> returns a `Result`. `and_then` chains two fallible steps in any
> expression, as Rust's `Result::and_then` does.

See also: [Option](option.md), [Variance](../lang/04-type-system.md#variance).
