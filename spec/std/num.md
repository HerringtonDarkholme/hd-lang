# Num

Status: standard library specification draft.

This chapter defines the part of `std.num` that `lib/std` writes in
ordinary hd over the language tier:

- the checked, wrapping, and saturating integer methods, which make
  overflow explicit;
- `abs_diff`, the distance between two integers;
- the bit counts `count_ones` and `leading_zeros`;
- `is_nan` and `is_finite` on `f64`;
- integer parsing: `parse_i32`, `parse_i64`, and `ParseNumberError`;
- float parsing: `parse_f64`;
- fixed-point float text: `to_fixed` on `f64`.

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

In this chapter, `N` is any integer type: `i8`, `i16`, `i32`, `i64`, `u8`,
`u16`, `u32`, or `u64`.

| Rule | Method | Result |
| --- | --- | --- |
| r[std-num.checked.add] `checked_add` | `fn checked_add(self, other: N) -> N?` | `.Some` of the exact sum when `N` holds it, and `.None` otherwise |
| r[std-num.checked.sub] `checked_sub` | `fn checked_sub(self, other: N) -> N?` | `.Some` of the exact difference when `N` holds it, and `.None` otherwise |
| r[std-num.checked.mul] `checked_mul` | `fn checked_mul(self, other: N) -> N?` | `.Some` of the exact product when `N` holds it, and `.None` otherwise |
| r[std-num.checked.div] `checked_div` | `fn checked_div(self, other: N) -> N?` | `.Some(self / other)` when `other` is not zero and `N` holds the quotient, and `.None` otherwise |

1. r[std-num.methods.every-width] `std.num` declares the checked, wrapping, saturating, `abs_diff`, and bit-count methods as inherent methods of every integer type, which need no `use`.
2. r[std-num.checked.no-panic] A checked method never panics.
3. r[std-num.checked.div.cases] `checked_div` truncates toward zero, as `/` does. It returns `.None` for a zero divisor, and for the minimum value divided by `-1`.

> **Note.** An unsigned `N` holds no value below zero, so its `checked_sub`
> returns `.None` whenever `other` is greater than `self`.

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

## Saturating Arithmetic

A saturating method stops at the nearest bound of `N`:

```text
fn remaining(stock: u32, sold: u32) -> u32:
    stock.saturating_sub(sold)
```

| Rule | Method | Result |
| --- | --- | --- |
| r[std-num.saturating.add] `saturating_add` | `fn saturating_add(self, other: N) -> N` | the exact sum, saturated to `N` |
| r[std-num.saturating.sub] `saturating_sub` | `fn saturating_sub(self, other: N) -> N` | the exact difference, saturated to `N` |

1. r[std-num.saturating.result] A value saturated to `N` is the exact result when `N` holds it. Above the range of `N` it is the largest `N`, and below it the smallest.
2. r[std-num.saturating.no-panic] A saturating method never panics. So a `u32` zero minus one, saturated, is zero.

## Absolute Difference

`abs_diff` returns the distance between two integers:

```text
fn gap(a: i32, b: i32) -> u32:
    a.abs_diff(b)
```

| Rule | Method |
| --- | --- |
| r[std-num.abs-diff.i8] On `i8` | `fn abs_diff(self, other: i8) -> u8` |
| r[std-num.abs-diff.i16] On `i16` | `fn abs_diff(self, other: i16) -> u16` |
| r[std-num.abs-diff.i32] On `i32` | `fn abs_diff(self, other: i32) -> u32` |
| r[std-num.abs-diff.i64] On `i64` | `fn abs_diff(self, other: i64) -> u64` |
| r[std-num.abs-diff.u8] On `u8` | `fn abs_diff(self, other: u8) -> u8` |
| r[std-num.abs-diff.u16] On `u16` | `fn abs_diff(self, other: u16) -> u16` |
| r[std-num.abs-diff.u32] On `u32` | `fn abs_diff(self, other: u32) -> u32` |
| r[std-num.abs-diff.u64] On `u64` | `fn abs_diff(self, other: u64) -> u64` |

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

## Bit Counts

The bit counts read the two's-complement form of an integer:

```text
fn flags_set(flags: u8) -> u32:
    flags.count_ones()

fn bit_length(value: u32) -> u32:
    32 - value.leading_zeros()

fn drop_flagged(value: u64, flags: u8) -> u64:
    value >> flags.count_ones()  # a u32 shift count, no cast
```

| Rule | Method | Result |
| --- | --- | --- |
| r[std-num.bits.count-ones-u32] `count_ones` | `fn count_ones(self) -> u32` | the number of 1 bits in `self` |
| r[std-num.bits.leading-zeros-u32] `leading_zeros` | `fn leading_zeros(self) -> u32` | the number of 0 bits above the highest 1 bit of `self`, which is the bit width of `N` when `self` is zero |

1. r[std-num.bits.signed] A signed value counts its two's-complement bits, so `-1` as `i8` has 8 ones, and a negative value has no leading zeros.
2. r[std-num.bits.no-panic] A bit count never panics.

> **Why.** A shift count is a `u32`
> ([`expr.op.std.shift-u32`](../lang/05-expressions.md#r-expr.op.std.shift-u32)),
> so a bit count feeds a shift without a cast. Rust's `count_ones` and
> `leading_zeros` return `u32` too.

## Floating-Point Classification

`is_nan` tells NaN apart, since NaN is unequal to itself, and `is_finite`
tells a NaN or an infinity apart from an ordinary value:

```text
fn usable(ratio: f64) -> bool:
    !ratio.is_nan()

fn bounded(ratio: f64) -> bool:
    ratio.is_finite()
```

1. r[std-num.is-nan] `std.num` declares `fn is_nan(self) -> bool` on `f64`. It returns `true` exactly when `self` is a NaN.
2. r[std-num.is-finite] `std.num` declares `fn is_finite(self) -> bool` on `f64`. It returns `true` exactly when `self` is neither a NaN nor an infinity.

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
    InvalidDigit(position: usize)
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
7. r[std-num.parse.invalid-digit-byte] A character that breaks the grammar gives `.Err(ParseNumberError.InvalidDigit(position))`. `position` is its byte offset into the text.
8. r[std-num.parse.end-of-text-bytes] Text that ends where the grammar needs a digit gives `InvalidDigit` at the text's length in bytes, so a lone `-` or `+` gives `InvalidDigit(1)`.
9. r[std-num.parse.out-of-range] A digit that takes the value read so far out of the result type's range gives `.Err(ParseNumberError.OutOfRange)`.
10. r[std-num.parse.error-traits] `ParseNumberError` implements `Eq` and `Display`.

| Text | `parse_i32` gives |
| --- | --- |
| `"42"`, `"+42"`, `"042"` | `.Ok(42)` |
| `"-2147483648"` | `.Ok` of the smallest `i32` |
| `""` | `.Err(Empty)` |
| `" 42"` | `.Err(InvalidDigit(0))` |
| `"-"`, `"+"` | `.Err(InvalidDigit(1))` |
| `"1_000"`, `"0x10"` | `.Err(InvalidDigit(1))` |
| `"2147483648"`, `"99999999999x"` | `.Err(OutOfRange)` |

> **Note.** Every character before `position` is ASCII, so `position` is
> also the character's index.

> **Why.** Parsing reads user input, which should not accept source
> literal syntax. The grammar is Rust's `str::parse` for integers.

## Float Parsing

`parse_f64` reads a decimal number, or one of the words `nan`, `inf`, and
`infinity`, as an `f64`:

```text
use std.num.parse_f64

fn ratio(text: string) -> f64:
    match parse_f64(text):
        .Ok(value) => value
        .Err(_) => 0.0
```

| Rule | Function |
| --- | --- |
| r[std-num.parse-f64] `parse_f64` | `pub fn parse_f64(text: string) -> Result[f64, ParseNumberError]` |

1. r[std-num.parse-f64.import] `std.num` declares `parse_f64`. It is not a prelude name; code imports it, as in `use std.num.parse_f64`.
2. r[std-num.parse-f64.float-grammar] The accepted text is an optional `+` or `-`, then a decimal number or a special word.
3. r[std-num.parse-f64.decimal] A decimal number is digits, then optionally `.` and more digits, then an optional exponent. Either side of the `.` may be empty, but not both.
4. r[std-num.parse-f64.exponent] An exponent is `e` or `E`, an optional `+` or `-`, and one or more digits.
5. r[std-num.parse-f64.special] A special word is `nan`, `inf`, or `infinity`, with each letter in either case.
6. r[std-num.parse-f64.excluded] So whitespace, `_` separators, hex digits, a type suffix, and a `,` decimal mark are not accepted.
7. r[std-num.parse-f64.decimal-value] A decimal number gives `.Ok` of the value that the [`parse_f64` primitive](README.md#standard-library-primitives) returns for it: the nearest `f64`, with a tie rounded to the even one.
8. r[std-num.parse-f64.sign] A leading `-` negates the value, so `-0` gives `-0.0`. A leading `+` changes nothing.
9. r[std-num.parse-f64.range] So a value past the finite `f64` range gives an infinity, and a value too small for the smallest subnormal gives a zero. Each keeps the text's sign.
10. r[std-num.parse-f64.special-value] `nan` gives a NaN, and `inf` and `infinity` give positive infinity, negated by a leading `-`.
11. r[std-num.parse-f64.empty] Empty text gives `.Err(ParseNumberError.Empty)`.
12. r[std-num.parse-f64.invalid-digit-byte] Other text is read from left to right. The first character that no continuation of the grammar allows gives `.Err(ParseNumberError.InvalidDigit(position))`, with its byte offset into the text.
13. r[std-num.parse-f64.text-end] Text that ends before the grammar is complete gives `InvalidDigit` at the text's length in characters, so a lone `-` gives `InvalidDigit(1)` and `in` gives `InvalidDigit(2)`.
14. r[std-num.parse-f64.no-out-of-range] `parse_f64` never gives `OutOfRange`.
15. r[std-num.parse-f64.round-trip] For every `f64` value `x`, `parse_f64(x.to_string())` gives `.Ok` of `x`: the same value with the same sign, or a NaN when `x` is a NaN.
16. r[std-num.parse-f64.hook] `lib/std` checks the grammar and reads the sign and the special words itself. It calls the primitive only with an unsigned decimal number.
17. r[std-num.parse-f64.hook.total] The primitive has no error or refusal signal, so `lib/std` gives its value as returned, an infinity or a zero included.

| Text | `parse_f64` gives |
| --- | --- |
| `"1.5"`, `"+1.5"`, `"15e-1"`, `"15E-1"` | `.Ok(1.5)` |
| `".5"`, `"-.5"` | `.Ok(0.5)`, `.Ok(-0.5)` |
| `"5."`, `"007"` | `.Ok(5.0)`, `.Ok(7.0)` |
| `"-0"`, `"-0.0"` | `.Ok(-0.0)` |
| `"9007199254740993"` | `.Ok(9007199254740992.0)`, the even one of the two nearest |
| `"1e400"`, `"-1e400"` | `.Ok` of positive infinity, and of negative infinity |
| `"1e-400"`, `"-1e-400"` | `.Ok(0.0)`, `.Ok(-0.0)` |
| `"NaN"`, `"nan"`, `"-nan"` | `.Ok` of a NaN |
| `"inf"`, `"Infinity"`, `"+INF"` | `.Ok` of positive infinity |
| `"-inf"`, `"-Infinity"` | `.Ok` of negative infinity |

| Text | `parse_f64` gives |
| --- | --- |
| `""` | `.Err(Empty)` |
| `" 1.5"`, `"e5"` | `.Err(InvalidDigit(0))` |
| `"1_000.5"`, `"0x1p-3"`, `"1,5"`, `"--1"` | `.Err(InvalidDigit(1))` |
| `"."`, `"+"`, `"-"` | `.Err(InvalidDigit(1))`, at the text's end |
| `"1e"`, `"in"` | `.Err(InvalidDigit(2))`, at the text's end |
| `"1.5 "`, `"1.5f"`, `"nana"` | `.Err(InvalidDigit(3))` |
| `"1e+"` | `.Err(InvalidDigit(3))`, at the text's end |
| `"+nan5"` | `.Err(InvalidDigit(4))` |
| `"infinit"` | `.Err(InvalidDigit(7))`, at the text's end |

> **Why.** The grammar is Rust's `str::parse::<f64>`, so every text that
> `Display` writes parses back, `NaN`, `inf`, and `-0.0` included. Correct
> rounding is what Rust and `serde_json` give. It needs big-number
> arithmetic, so the host supplies it, as it supplies `format_f64`.

> **Note.** `std.json` checks its own grammar, that of RFC 8259, before it
> calls `parse_f64`. So JSON text still rejects `NaN`, `+1`, `.5`, and
> `5.`, by [`std-json.parse.number.grammar`](json.md#r-std-json.parse.number.grammar).

See also: [Strings](../lang/04-type-system.md#strings), [Result](result.md), [JSON Numbers](json.md#number-syntax).

## Fixed-Point Text

`to_fixed` writes an `f64` with a fixed number of digits after the
decimal point:

```text
fn price(amount: f64) -> string:
    amount.to_fixed(2)  # "3.14" for 3.14159

fn seconds(elapsed: f64) -> string:
    elapsed.to_fixed(0)  # "2" for 2.5, a tie rounded to even
```

| Rule | Method |
| --- | --- |
| r[std-num.to-fixed.decl] `to_fixed` | `pub fn to_fixed(self, digits: usize) -> string`, on `f64` |

1. r[std-num.to-fixed.value] For a finite `self`, `to_fixed(digits)` writes the multiple of 10 to the power `-digits` nearest the exact value of `self`.
2. r[std-num.to-fixed.ties] When two multiples are equally near, it writes the one whose last digit is even.
3. r[std-num.to-fixed.form] The text is an optional `-`, the integer part in decimal with no leading zero but a lone `0`, and then, when `digits` is above 0, `.` and exactly `digits` digits.
4. r[std-num.to-fixed.no-exponent] The text never uses scientific notation, however large or small the value.
5. r[std-num.to-fixed.sign] The text starts with `-` exactly when `self` has its sign bit set, so `(-0.0).to_fixed(1)` is `"-0.0"` and `(-0.001).to_fixed(2)` is `"-0.00"`.
6. r[std-num.to-fixed.special] A NaN gives `NaN`, and the infinities give `inf` and `-inf`, the text that [`types.display.special`](../lang/04-type-system.md#r-types.display.special) gives.
7. r[std-num.to-fixed.digits-max] A `digits` above 100 panics. Panic: `explicit-panic`.
8. r[std-num.to-fixed.hook] `lib/std` checks `digits`, and the [`format_f64_fixed` primitive](README.md#standard-library-primitives) writes the text.

| Call | Result |
| --- | --- |
| `3.14159.to_fixed(2)` | `"3.14"` |
| `0.125.to_fixed(2)` | `"0.12"`, since 0.125 is exact and the tie goes to 2 |
| `0.1.to_fixed(20)` | `"0.10000000000000000555"`, the exact value of the nearest `f64` |
| `1e21.to_fixed(1)` | `"1000000000000000000000.0"` |
| `(-1.5).to_fixed(0)` | `"-2"` |
| `7.0.to_fixed(0)` | `"7"` |

> **Why.** Python's `f"{x:.2f}"` rounds the exact binary value, ties to
> even, and keeps the sign of a negative zero, as these rules do.
> JavaScript's `toFixed` rounds a tie away from zero and drops that sign. Correct rounding of up to 100 digits needs big-number
> arithmetic, so the host supplies it, as it supplies `format_f64`.

See also: [Numeric Display](../lang/04-type-system.md#numeric-display), [Float Parsing](#float-parsing).
