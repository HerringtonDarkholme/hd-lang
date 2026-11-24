# Convert

Status: standard library specification draft.

This chapter defines explicit fallible conversions in `std.convert`.
The language-tier [`From`](../lang/09-traits.md#conversion-trait)
remains the conversion used by postfix `?`.

## Fallible Conversions

A caller names the destination type and handles its result:

```text
use std.convert.TryFrom
use std.num.TryFromIntError

fn convert[T, U < TryFrom[T]](value: T) -> Result[U, U::Error]:
    U::try_from(value)
```

1. r[std-convert.try-from.decl] `std.convert` declares `pub trait TryFrom[T]`, with `type Error` and `fn try_from(value: T) -> Result[Self, Self::Error]`.
2. r[std-convert.try-from.result] A conversion returns `.Ok` with its converted value on success, or `.Err` with its associated `Error` on failure.
3. r[std-convert.try-from.ordinary] `TryFrom` implementations follow the ordinary implementation target, ownership, overlap, and method-signature rules.
4. r[std-convert.try-from.generic] In code bounded by `U < TryFrom[T]`, `U::try_from(value)` accepts a `T` and returns `Result[U, U::Error]`.
5. r[std-convert.try-from.import] `TryFrom` is not a prelude name; code that names it imports it from `std.convert`.
6. r[std-convert.try-from.independent] Implementing `From[T]` does not generate a `TryFrom[T]` implementation, or conversely.

> **Why.** A fallible conversion lets the caller handle a rejected value.
> Ordinary trait implementations provide this without a compiler intrinsic
> or an exception to the implementation rules.

See also: [Fallible Integer Conversions](num.md#fallible-integer-conversions),
[Implementation Targets](../lang/09-traits.md#implementation-targets), and
[Overlap](../lang/09-traits.md#overlap).
