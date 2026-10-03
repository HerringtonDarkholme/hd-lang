# Num

Status: standard library specification draft.

This chapter defines the part of `std.num` that `lib/std` writes in
ordinary hd over the language tier:

- the checked and wrapping integer methods, which make overflow explicit;
- `abs_diff`, the distance between two integers;
- `is_nan` on `f64`;
- integer parsing: `parse_i32`, `parse_i64`, and `ParseNumberError`.

The language tier keeps what the compiler knows by name:

| Item | Why it stays in the language tier |
| --- | --- |
| `Num`, `Integer`, and `Float` | sealed traits that only the standard library implements ([Numeric Traits](../lang/09-traits.md#numeric-traits)) |
| the numeric families and casts | [Numeric Conversions](../lang/04-type-system.md#numeric-conversions) and [Numeric Casts](../lang/04-type-system.md#numeric-casts) |
| checked operators and their panics | [Integer Arithmetic](../lang/04-type-system.md#integer-arithmetic) |

> **Note.** No numeric value converts implicitly, by
> [`types.num.no-implicit`](../lang/04-type-system.md#r-types.num.no-implicit).
> Each method below takes `other` of its receiver's type, so a value of
> another width is cast first, as in `i64(count).checked_add(total)`.

## Checked Arithmetic

A checked method returns `.None` where the operator would panic:

```text
fn total(prices: List[i64]) -> i64?:
    let sum: i64 = 0
    for price in prices:
        sum = sum.checked_add(price)?
    .Some(sum)
```

In this table and the next, `N` is `i32` or `i64`.

| Rule | Method | Result |
| --- | --- | --- |
| r[std-num.checked.add] `checked_add` | `fn checked_add(self, other: N) -> N?` | `.Some` of the exact sum when `N` holds it, and `.None` otherwise |
| r[std-num.checked.sub] `checked_sub` | `fn checked_sub(self, other: N) -> N?` | `.Some` of the exact difference when `N` holds it, and `.None` otherwise |
| r[std-num.checked.mul] `checked_mul` | `fn checked_mul(self, other: N) -> N?` | `.Some` of the exact product when `N` holds it, and `.None` otherwise |
| r[std-num.checked.div] `checked_div` | `fn checked_div(self, other: N) -> N?` | `.Some(self / other)` when `other` is not zero and `N` holds the quotient, and `.None` otherwise |

1. r[std-num.methods.types] `std.num` declares the checked, wrapping, and `abs_diff` methods as inherent methods of `i32` and `i64`, which need no `use`.
2. r[std-num.checked.no-panic] A checked method never panics.
3. r[std-num.checked.div.cases] `checked_div` truncates toward zero, as `/` does. It returns `.None` for a zero divisor, and for the minimum value divided by `-1`.

## Wrapping Arithmetic

A wrapping method keeps the low bits of the exact result:

```text
fn next_ticket(counter: i32) -> i32:
    counter.wrapping_add(1)
```

| Rule | Method | Result |
| --- | --- | --- |
| r[std-num.wrapping.add] `wrapping_add` | `fn wrapping_add(self, other: N) -> N` | the exact sum, wrapped to `N` |
| r[std-num.wrapping.sub] `wrapping_sub` | `fn wrapping_sub(self, other: N) -> N` | the exact difference, wrapped to `N` |

1. r[std-num.wrapping.result] A value wrapped to `N` is the low bits of its two's-complement form, read as `N`, as an integer cast reads them by [`types.cast.wrap`](../lang/04-type-system.md#r-types.cast.wrap).
2. r[std-num.wrapping.no-panic] A wrapping method never panics. So the largest `i32` plus one, wrapped, is the smallest `i32`.

## Absolute Difference

`abs_diff` returns the distance between two integers:

```text
fn gap(a: i32, b: i32) -> u32:
    a.abs_diff(b)
```

| Rule | Method |
| --- | --- |
| r[std-num.abs-diff.i32] On `i32` | `fn abs_diff(self, other: i32) -> u32` |
| r[std-num.abs-diff.i64] On `i64` | `fn abs_diff(self, other: i64) -> u64` |

1. r[std-num.abs-diff] `a.abs_diff(b)` returns the distance between `a` and `b` as the unsigned type of the same width.
2. r[std-num.abs-diff.no-panic] It never panics, since that unsigned type holds every distance, including the one between the minimum and the maximum.
3. r[std-num.abs-diff.unsigned] The result is unsigned, so a signed operand beside it needs an explicit cast. Without one, the operation is an error. Error: `mixed-signedness`.

```text
fn close(a: i32, b: i32, limit: i32) -> bool:
    a.abs_diff(b) < limit  # error: mixed-signedness
```

> **Why.** The distance between the minimum and the maximum of `i32` does
> not fit in `i32`, so a signed result would have to panic. Rust's
> `i32::abs_diff` returns `u32` for the same reason.

## Floating-Point Classification

`is_nan` tells NaN apart, since NaN is unequal to itself:

```text
fn usable(ratio: f64) -> bool:
    !ratio.is_nan()
```

1. r[std-num.is-nan] `std.num` declares `fn is_nan(self) -> bool` on `f64`. It returns `true` exactly when `self` is a NaN.

## Integer Parsing

`parse_i32` and `parse_i64` read a decimal integer from text:

```text
use std.num.{parse_i32, ParseNumberError}

fn port(text: string) -> i32:
    match parse_i32(text):
        .Ok(value) => value
        .Err(ParseNumberError.OutOfRange) => 65535
        .Err(_) => 8080
```

A failed parse reports one of three errors:

```text
pub enum ParseNumberError:
    Empty
    InvalidDigit(position: i32)
    OutOfRange
```

| Rule | Function |
| --- | --- |
| r[std-num.parse.i32] `parse_i32` | `pub fn parse_i32(text: string) -> Result[i32, ParseNumberError]` |
| r[std-num.parse.i64] `parse_i64` | `pub fn parse_i64(text: string) -> Result[i64, ParseNumberError]` |

1. r[std-num.parse.import] `std.num` declares `ParseNumberError`, `parse_i32`, and `parse_i64`. None is a prelude name; code imports them, as in `use std.num.parse_i32`.
2. r[std-num.parse.grammar] The accepted text is an optional `+` or `-`, then one or more decimal digits `0` to `9`, and nothing else.
3. r[std-num.parse.no-literal-syntax] So whitespace, `_` separators, radix prefixes such as `0x`, and type suffixes are not accepted.
4. r[std-num.parse.value] Accepted text gives `.Ok` of its digits read in base ten, negated after a `-`.
5. r[std-num.parse.empty] Empty text gives `.Err(ParseNumberError.Empty)`.
6. r[std-num.parse.order] Non-empty text is read from left to right, and the first error reached is the result.
7. r[std-num.parse.invalid-digit] A character that breaks the grammar gives `.Err(ParseNumberError.InvalidDigit(position))`. `position` is its index, counted in characters from 0.
8. r[std-num.parse.lone-sign] A sign with no digit after it gives `InvalidDigit(0)`.
9. r[std-num.parse.out-of-range] A digit that takes the value read so far out of the result type's range gives `.Err(ParseNumberError.OutOfRange)`.
10. r[std-num.parse.error-traits] `ParseNumberError` implements `Eq` and `Display`.

| Text | `parse_i32` gives |
| --- | --- |
| `"42"`, `"+42"`, `"042"` | `.Ok(42)` |
| `"-2147483648"` | `.Ok` of the smallest `i32` |
| `""` | `.Err(Empty)` |
| `"-"`, `"+"` | `.Err(InvalidDigit(0))` |
| `" 42"` | `.Err(InvalidDigit(0))` |
| `"1_000"`, `"0x10"` | `.Err(InvalidDigit(1))` |
| `"2147483648"`, `"99999999999x"` | `.Err(OutOfRange)` |

> **Note.** Every character before the first invalid one is ASCII, so
> `position` is also that character's byte offset.

> **Why.** Parsing reads user input, which should not accept source
> literal syntax. The grammar is Rust's `str::parse` for integers.

See also: [Strings](../lang/04-type-system.md#strings), [Result](result.md).
