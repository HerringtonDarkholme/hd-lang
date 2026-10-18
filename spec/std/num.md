# Num

Status: standard library specification draft.

This chapter defines the part of `std.num` that `lib/std` writes in
ordinary hd over the language tier:

- the checked, wrapping, and saturating integer methods, which make
  overflow explicit;
- `abs_diff`, the distance between two integers;
- `to_u64` and `from_u64`, the conversions of `Integer` to and from `u64`;
- the bit counts `count_ones` and `leading_zeros`;
- the rotations `rotate_left` and `rotate_right`;
- `is_nan` and `is_finite` on `f32` and `f64`;
- `total_cmp`, the total order of floats;
- `to_bits` and `from_bits`, the bit patterns of `f32` and `f64`;
- `abs` on the signed integers and the floats, and `pow` on the integers;
- `trunc`, `floor`, `ceil`, and `round` on the floats;
- `sqrt` on the floats, an intrinsic method;
- integer parsing: `parse_i32`, `parse_i64`, `parse_u32`, `parse_u64`,
  `parse_usize`, and `ParseNumberError`;
- radix text: `to_radix` and `parse_radix` on the integers;
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
`u16`, `u32`, `u64`, or `usize`.

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
| r[std-num.wrapping.mul] `wrapping_mul` | `fn wrapping_mul(self, other: N) -> N` | the exact product, wrapped to `N` |

1. r[std-num.wrapping.result] A value wrapped to `N` is the low bits of its two's-complement form, read as `N`, as an integer cast reads them by [`types.cast.wrap`](../lang/04-type-system.md#r-types.cast.wrap).
2. r[std-num.wrapping.no-panic] A wrapping method never panics. So the largest `i32` plus one, wrapped, is the smallest `i32`, and the smallest `i64` times `-1`, wrapped, is the smallest `i64`.

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
| r[std-num.abs-diff.usize] On `usize` | `fn abs_diff(self, other: usize) -> usize` |

1. r[std-num.abs-diff] `a.abs_diff(b)` returns the distance between `a` and `b` as the table's result type. That is the receiver's own type when it is unsigned, and otherwise the unsigned type of the same width.
2. r[std-num.abs-diff.no-panic] It never panics, since that unsigned type holds every distance, including the one between the minimum and the maximum.
3. r[std-num.abs-diff.unsigned] The result is unsigned, so a signed operand beside it needs an explicit cast. Without one, the operation is an error. Error: `mixed-signedness`.

```text
fn close(a: i32, b: i32, limit: i32) -> bool:
    a.abs_diff(b) < limit  # error: mixed-signedness
```

> **Why.** The distance between the minimum and the maximum of `i32` does
> not fit in `i32`, so a signed result would have to panic. Rust's
> `i32::abs_diff` returns `u32` for the same reason.

## Conversion To And From u64

`Integer` declares two conversions, so generic code can move any integer
through `u64`, as a range over any integer type does:

```text
use std.num.Integer

fn widen[N < Integer](n: N) -> u64:
    n.to_u64()

fn narrow[N < Integer](n: u64) -> N:
    N::from_u64(n)
```

| Rule | Member | Result |
| --- | --- | --- |
| r[std-num.u64.to] `to_u64` | `fn to_u64(self) -> u64` | the value as a `u64` |
| r[std-num.u64.from] `from_u64` | `fn from_u64(n: u64) -> Self` | `n` as the integer type |

1. r[std-num.u64.members] `to_u64` and `from_u64` are members of the sealed `Integer` trait, implemented for every integer type, so `n.to_u64()` and `N::from_u64(n)` work on a bound `N < Integer` and on each concrete integer type.
2. r[std-num.u64.checked] Both are checked in every build, as [`trait.num.from-i64-checked`](../lang/09-traits.md#r-trait.num.from-i64-checked) is for `from_i64`. `to_u64` panics on a negative value, and `from_u64` panics when the integer type cannot hold `n`. Error: `integer-overflow`.
3. r[std-num.u64.exact] A conversion that does not panic is exact: the result is the same number, never a wrapped one. A [numeric cast](../lang/04-type-system.md#numeric-casts) `u64(n)` stays the way to wrap.
4. r[std-num.u64.same-type] On `u64`, both conversions return their argument.
5. r[std-num.u64.cost] Each conversion is a comparison and a cast, with no text round trip.

> **Why.** `from_i64` is already checked, so the new members match it.
> A wrapping conversion would turn a bad range bound into a silent wrong
> number.

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

> **Why.** `u32` is an accepted shift-count type
> ([`expr.op.std.shift-unsigned`](../lang/05-expressions.md#r-expr.op.std.shift-unsigned)),
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

1. r[std-num.is-nan] `std.num` declares `fn is_nan(self) -> bool` on `f32` and on `f64`. It returns `true` exactly when `self` is a NaN.
2. r[std-num.is-finite] `std.num` declares `fn is_finite(self) -> bool` on `f32` and on `f64`. It returns `true` exactly when `self` is neither a NaN nor an infinity.

## Float Bit Patterns

`to_bits` reads a float's IEEE 754 bit pattern and `from_bits` builds a float from one, as Rust's do:

```text
fn is_negative_zero(x: f64) -> bool:
    x.to_bits() == 9223372036854775808
```

| Rule | Method | Result |
| --- | --- | --- |
| r[std-num.float-bits.to] `to_bits` | `fn to_bits(self) -> U` | the bit pattern of `self`, sign bit most significant |
| r[std-num.float-bits.from] `from_bits` | `fn from_bits(bits: U) -> F` | the float whose bit pattern is `bits` |

In this section, `F` is `f64` with `U` as `u64`, or `F` is `f32` with `U` as `u32`.

1. r[std-num.float-bits.widths] The two methods exist on `f32` and on `f64`. `from_bits` takes no receiver, so a call reads `f64::from_bits(bits)`.
2. r[std-num.float-bits.exact] `to_bits` returns the IEEE 754 binary64 pattern of an `f64` and the binary32 pattern of an `f32`. So `1.0.to_bits()` is `4607182418800017408` and `(-0.0).to_bits()` is `9223372036854775808`.
3. r[std-num.float-bits.round-trip] `from_bits(x.to_bits())` has the bits of `x`, and `from_bits(b).to_bits()` is `b`, for every `b`, a NaN's sign and payload included.
4. r[std-num.float-bits.nan] Neither method canonicalizes a NaN. [`types.display.nan-canonical`](../lang/04-type-system.md#r-types.display.nan-canonical) applies only before hashing, boundary serialization, and `Display`, so a NaN's sign and payload are visible to `to_bits` inside the program.
5. r[std-num.float-bits.no-panic] Neither method panics.

> **Why.** Rust has the same pair. No float operation reveals the sign of a
> NaN, so `total_cmp` needs the bits. Each method is one reinterpret
> instruction on Wasm.

## Float Total Order

Floats are not `Ord`, since NaN is unordered. `total_cmp` orders every float, so a list of floats sorts:

```text
fn sorted(times: List[f64]) -> List[f64]:
    times.sorted_by(fn(a: f64, b: f64) -> Ordering: a.total_cmp(b))
```

| Rule | Method | Result |
| --- | --- | --- |
| r[std-num.float-total-cmp.order] `total_cmp` | `fn total_cmp(self, other: F) -> Ordering` | the IEEE 754 totalOrder of `self` and `other` |

In this section, `F` is `f32` or `f64`.

1. r[std-num.float-total-cmp.widths] `total_cmp` exists on `f32` and on `f64`, and takes `other` of the receiver's type.
2. r[std-num.float-total-cmp.sequence] The order is, from least to greatest: a negative NaN, negative infinity, the negative finite values, `-0.0`, `0.0`, the positive finite values, positive infinity, a positive NaN. A NaN's sign is the sign bit of its [bit pattern](#r-std-num.float-bits.to).
3. r[std-num.float-total-cmp.nan-payload] Among NaNs of one sign, the order follows the payload bits read as an unsigned integer, reversed for a negative NaN. So two NaNs with the same bits are `.Equal`.
4. r[std-num.float-total-cmp.equal] `a.total_cmp(b)` is `.Equal` exactly when `a` and `b` have the same bit pattern. So `-0.0` and `0.0` are not equal here, although `==` finds them equal.
5. r[std-num.float-total-cmp.no-panic] `total_cmp` never panics.

> **Why.** This is Rust's `f64::total_cmp`. It compares the bit patterns
> as sign-magnitude integers, which needs no allocation.

## Integer Parsing

`parse_i32` and `parse_i64` read a decimal integer from text, and
`parse_u32`, `parse_u64`, and `parse_usize` read an unsigned one:

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
| r[std-num.parse.u32] `parse_u32` | `pub fn parse_u32(text: string) -> Result[u32, ParseNumberError]` |
| r[std-num.parse.u64] `parse_u64` | `pub fn parse_u64(text: string) -> Result[u64, ParseNumberError]` |
| r[std-num.parse.usize] `parse_usize` | `pub fn parse_usize(text: string) -> Result[usize, ParseNumberError]` |

1. r[std-num.parse.import] `std.num` declares `ParseNumberError`, `parse_i32`, `parse_i64`, `parse_u32`, `parse_u64`, and `parse_usize`. None is a prelude name; code imports them, as in `use std.num.parse_i32`.
2. r[std-num.parse.grammar] The accepted text is an optional `+` or `-`, then one or more decimal digits `0` to `9`, and nothing else.
3. r[std-num.parse.no-literal-syntax] So whitespace, `_` separators, radix prefixes such as `0x`, and type suffixes are not accepted.
4. r[std-num.parse.value] Accepted text gives `.Ok` of its digits read in base ten, negated after a `-`.
5. r[std-num.parse.empty] Empty text gives `.Err(ParseNumberError.Empty)`.
6. r[std-num.parse.order] Non-empty text is read from left to right, and the first error reached is the result.
7. r[std-num.parse.invalid-digit-byte] A character that breaks the grammar gives `.Err(ParseNumberError.InvalidDigit(position))`. `position` is its byte offset into the text.
8. r[std-num.parse.end-of-text-bytes] Text that ends where the grammar needs a digit gives `InvalidDigit` at the text's length in bytes, so a lone `-` or `+` gives `InvalidDigit(1)`.
9. r[std-num.parse.out-of-range] A digit that takes the value read so far out of the result type's range gives `.Err(ParseNumberError.OutOfRange)`.
10. r[std-num.parse.error-traits] `ParseNumberError` implements `Eq` and `Display`.
11. r[std-num.parse.unsigned] The unsigned functions read the same grammar without the `-`: an optional `+`, then one or more digits. Every other rule above holds for them, with the result type's range.
12. r[std-num.parse.unsigned.minus] A `-` at the start of the text is an invalid digit, so `"-1"`, `"-0"`, and `"-"` give `.Err(ParseNumberError.InvalidDigit(0))`.
13. r[std-num.parse.usize-range] `parse_usize` reads the range of `usize`, whose width is the target's by [`types.usize.width`](../lang/04-type-system.md#r-types.usize.width). On Wasm32 it accepts the text that `parse_u32` accepts.

| Text | `parse_i32` gives |
| --- | --- |
| `"42"`, `"+42"`, `"042"` | `.Ok(42)` |
| `"-2147483648"` | `.Ok` of the smallest `i32` |
| `""` | `.Err(Empty)` |
| `" 42"` | `.Err(InvalidDigit(0))` |
| `"-"`, `"+"` | `.Err(InvalidDigit(1))` |
| `"1_000"`, `"0x10"` | `.Err(InvalidDigit(1))` |
| `"2147483648"`, `"99999999999x"` | `.Err(OutOfRange)` |

| Text | `parse_u32` gives |
| --- | --- |
| `"42"`, `"+42"` | `.Ok(42)` |
| `"4294967295"` | `.Ok` of the largest `u32` |
| `"4294967296"` | `.Err(OutOfRange)` |
| `"-1"`, `"-0"`, `"-"` | `.Err(InvalidDigit(0))` |
| `"+"` | `.Err(InvalidDigit(1))` |
| `""` | `.Err(Empty)` |

> **Note.** Every character before `position` is ASCII, so `position` is
> also the character's index.

> **Why.** Parsing reads user input, which should not accept source
> literal syntax. The grammar is Rust's `str::parse` for integers, and
> Rust's unsigned types reject a leading `-` as an invalid digit too.

### Parse Impls

Every number type implements [`Parse`](text.md#parsing-text), so
`"42".parse::[u8]()` reads a `u8`:

```text
fn byte(text: string) -> Result[u8, ParseNumberError]:
    text.parse::[u8]()
```

1. r[std-num.parse.trait-impls] `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `usize`, `f32`, and `f64` each implement `Parse` with `Error` set to `ParseNumberError`.
2. r[std-num.parse.trait-integer] An integer type's `parse` reads the grammar of `parse_i32` for a signed type, and of `parse_u32` for an unsigned one, within that type's range. Its errors and positions are the ones those functions give.
3. r[std-num.parse.trait-same] For `i32`, `i64`, `u32`, `u64`, and `usize`, `parse(text)` returns what the `parse_` function of that type returns for `text`.
4. r[std-num.parse.trait-narrow] For `i8`, `i16`, `u8`, and `u16`, a value past the type's range gives `.Err(ParseNumberError.OutOfRange)`, so `"256".parse::[u8]()` and `"-129".parse::[i8]()` fail.
5. r[std-num.parse.trait-f64] `f64`'s `parse(text)` returns what `parse_f64(text)` returns.
6. r[std-num.parse.trait-f32] `f32`'s `parse(text)` reads the same grammar, so its errors are `parse_f64`'s, and gives the `f64` value that `parse_f64` returns, rounded to `f32`.

> **Why.** The text of a number is the same whatever the type that reads
> it, so each width reuses one grammar. Rust parses `f32` text directly, so
> a few decimals that sit between two `f32` values may round differently
> here, since the value is rounded twice.

## Radix Text

`to_radix` writes an integer in a base from 2 to 36, and `parse_radix` reads
it back, for hex ids, colors, and file modes:

```text
fn id_text(id: u32) -> string:
    id.to_radix(16).pad_start(8, '0')

fn mode(text: string) -> Result[u32, ParseNumberError]:
    u32::parse_radix(text, 8)
```

| Rule | Method | Result |
| --- | --- | --- |
| r[std-num.radix.to-radix] `to_radix` | `fn to_radix(self, radix: u32) -> string` | the digits of `self` in `radix` |
| r[std-num.radix.parse-radix] `parse_radix` | `fn parse_radix(text: string, radix: u32) -> Result[N, ParseNumberError]` | the integer that `text` spells in `radix`, or the first error |

1. r[std-num.radix.widths] Both exist on every integer type, `i8` to `i64` and `u8` to `u64` including `usize`, and use that type's range. `parse_radix` takes no receiver: `i32::parse_radix("ff", 16)`.
2. r[std-num.radix.bad-radix] A `radix` below 2 or above 36 panics with `explicit-panic`, in both methods, as [`std-text.char.to-digit.radix`](text.md#r-std-text.char.to-digit.radix) does.
3. r[std-num.radix.digits] The digits are `0` to `9` and then `a` to `z`, so `z` is 35.
4. r[std-num.radix.to-radix.format] `to_radix` writes no leading zeros, writes letters in lowercase, writes no prefix such as `0x`, and writes `0` for zero.
5. r[std-num.radix.to-radix.sign] A negative value is written as `-` and then the digits of its magnitude. The minimum value of a signed type is written in full, so `i32` minimum in radix 16 is `-80000000`.
6. r[std-num.radix.parse.grammar] `parse_radix` accepts an optional `+` or `-`, then one or more digits of `radix`, and nothing else. A letter digit is accepted in either case.
7. r[std-num.radix.parse.no-prefix] So whitespace, `_` separators, and prefixes such as `0x` are not accepted; `"0xff"` in radix 16 gives `InvalidDigit(1)`.
8. r[std-num.radix.parse.unsigned] An unsigned type rejects a leading `-` as an invalid digit at position 0, as [`std-num.parse.unsigned.minus`](#r-std-num.parse.unsigned.minus) does.
9. r[std-num.radix.parse.errors] The errors, their positions, and their order are those of [`std-num.parse.empty`](#r-std-num.parse.empty), [`std-num.parse.order`](#r-std-num.parse.order), [`std-num.parse.invalid-digit-byte`](#r-std-num.parse.invalid-digit-byte), [`std-num.parse.end-of-text-bytes`](#r-std-num.parse.end-of-text-bytes), and [`std-num.parse.out-of-range`](#r-std-num.parse.out-of-range). A character that is not a digit of `radix` is an invalid digit.
10. r[std-num.radix.round-trip] For every integer `x` of type `N` and every `radix` from 2 to 36, `N::parse_radix(x.to_radix(radix), radix)` gives `.Ok(x)`, the minimum and maximum of each width included.
11. r[std-num.radix.cost] `to_radix` builds its text in one buffer, not one string per digit, so its cost is linear in the number of digits.

| Call | Result |
| --- | --- |
| `(255).to_radix(16)` | `"ff"` |
| `(-255).to_radix(2)` | `"-11111111"` |
| the `u64` maximum, `to_radix(36)` | `"3w5e11264sgsf"` |
| `i32::parse_radix("-FF", 16)` | `.Ok(-255)` |
| `i32::parse_radix("12", 2)` | `.Err(ParseNumberError.InvalidDigit(1))` |
| `u8::parse_radix("100", 16)` | `.Err(ParseNumberError.OutOfRange)` |
| `u8::parse_radix("-1", 10)` | `.Err(ParseNumberError.InvalidDigit(0))` |
| `i32::parse_radix("1", 37)` | panics |

> **Why.** The names and the 2 to 36 range follow Go's `FormatInt` and
> `ParseInt` and Rust's `from_str_radix`. Lowercase output, no prefix, an
> optional sign, and a panic for a bad radix are what those APIs share.

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
7. r[std-num.parse-f64.decimal-value] A decimal number gives `.Ok` of the value that the [`parse_f64` primitive](README.md#standard-library-primitives) returns for it. That is the nearest `f64`, with a tie rounded to the even one.
8. r[std-num.parse-f64.sign] A leading `-` negates the value, so `-0` gives `-0.0`. A leading `+` changes nothing.
9. r[std-num.parse-f64.range] So a value past the finite `f64` range gives an infinity, and a value too small for the smallest subnormal gives a zero. Each keeps the text's sign.
10. r[std-num.parse-f64.special-value] `nan` gives a NaN, and `inf` and `infinity` give positive infinity, negated by a leading `-`.
11. r[std-num.parse-f64.empty] Empty text gives `.Err(ParseNumberError.Empty)`.
12. r[std-num.parse-f64.invalid-digit-byte] Other text is read from left to right. The first character that no continuation of the grammar allows gives `.Err(ParseNumberError.InvalidDigit(position))`, with its byte offset into the text.
13. r[std-num.parse-f64.text-end] Text that ends before the grammar is complete gives `InvalidDigit` at the text's length in characters. So a lone `-` gives `InvalidDigit(1)` and `in` gives `InvalidDigit(2)`.
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
3. r[std-num.to-fixed.form] The text is an optional `-`, then the integer part in decimal with no leading zero but a lone `0`. When `digits` is above 0, `.` and exactly `digits` digits follow.
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

## Rotation

`rotate_left` and `rotate_right` turn the bits of an integer around its two
ends, so no bit is lost:

```text
fn next_state(state: u32) -> u32:
    state.rotate_left(5) ^ state

fn swap_nibbles(byte: u8) -> u8:
    byte.rotate_left(4)
```

| Rule | Method | Result |
| --- | --- | --- |
| r[std-num.rotate.left] `rotate_left` | `fn rotate_left(self, count: u32) -> N` | the bits of `self` moved `count` places toward the high end, with the bits that leave the top entering at the bottom |
| r[std-num.rotate.right] `rotate_right` | `fn rotate_right(self, count: u32) -> N` | the bits of `self` moved `count` places toward the low end, with the bits that leave the bottom entering at the top |

1. r[std-num.rotate.widths] Both methods exist on every integer type, `i8` to `i64` and `u8` to `u64`, and return the receiver's type.
2. r[std-num.rotate.count-modulo] The count is taken modulo the bit width of the type, so a count of 0 or of the width returns `self`.
3. r[std-num.rotate.signed] A signed value rotates its two's-complement bits, so the sign bit moves like any other bit.
4. r[std-num.rotate.inverse] `x.rotate_right(n)` undoes `x.rotate_left(n)`.
5. r[std-num.rotate.no-panic] A rotation never panics.

| Call | Result |
| --- | --- |
| the `u8` value `150` (`10010110`), `rotate_left(1)` | `45` (`00101101`) |
| the `u8` value `150`, `rotate_right(3)` | `210` (`11010010`) |
| the `u8` value `150`, `rotate_left(9)` | `45`, since 9 is 1 modulo 8 |
| the `i8` value `-128`, `rotate_left(1)` | `1` |
| the `i32` value `-2`, `rotate_right(1)` | `2147483647` |
| the `u32` value `0x12345678`, `rotate_left(8)` | `0x34567812` |

> **Why.** The names and the `u32` count are Rust's `rotate_left` and
> `rotate_right`.

## Absolute Value

`abs` returns the magnitude of a signed integer:

```text
fn distance_from_zero(offset: i32) -> i32:
    offset.abs()
```

| Rule | Method |
| --- | --- |
| r[std-num.abs.i8] On `i8` | `fn abs(self) -> i8` |
| r[std-num.abs.i16] On `i16` | `fn abs(self) -> i16` |
| r[std-num.abs.i32] On `i32` | `fn abs(self) -> i32` |
| r[std-num.abs.i64] On `i64` | `fn abs(self) -> i64` |

1. r[std-num.abs.value] `x.abs()` returns `x` when `x` is not negative, and `-x` otherwise.
2. r[std-num.abs.widths] `abs` exists on the signed integer types only. An unsigned value is its own magnitude, so `abs` on an unsigned type is an unknown method. Error: `unknown-method`.
3. r[std-num.abs.min] The smallest value of a width has no positive twin, so its `abs` overflows exactly as `-x` does, by [`types.arith.checked`](../lang/04-type-system.md#r-types.arith.checked). It panics in a debug or test build and wraps to the smallest value in a release build. Panic: `integer-overflow`.

> **Why.** Rust's `i32::abs` behaves the same way. `abs_diff` is the call
> that never overflows, since its result is unsigned.

## Powers

`pow` raises an integer to a `u32` exponent:

```text
fn cube(side: i32) -> i32:
    side.pow(3)
```

| Rule | Method | Result |
| --- | --- | --- |
| r[std-num.pow.decl] `pow` | `fn pow(self, exp: u32) -> N` | `self` multiplied by itself `exp` times |

1. r[std-num.pow.widths] `pow` exists on every integer type, `i8` to `i64` and `u8` to `u64` and `usize`, and returns the receiver's type.
2. r[std-num.pow.value] `x.pow(n)` is the product of `n` copies of `x`, computed with the `*` operator of `N`.
3. r[std-num.pow.zero-exponent] `x.pow(0)` is 1 for every `x`, so `0.pow(0)` is 1.
4. r[std-num.pow.overflow] A power that `N` cannot hold overflows as `*` does. It panics in a debug or test build and wraps in a release build. Panic: `integer-overflow`.
5. r[std-num.pow.no-early-overflow] A power that `N` holds never overflows, whatever the exponent's bits. So `(-2).pow(7)` is the smallest `i8`.

| Call | Result |
| --- | --- |
| `2.pow(10)` | `1024` |
| the `i8` value `-2`, `pow(7)` | `-128` |
| the `u8` value `3`, `pow(5)` | `243` |
| the `i32` value `5`, `pow(0)` | `1` |
| the `i8` value `-2`, `pow(8)` | panics in a debug or test build, since 256 is out of range |

> **Why.** Rust's `pow` takes a `u32` exponent and overflows like `*`.

## Float Rounding

`trunc`, `floor`, `ceil`, and `round` turn a float into a whole-valued
float of the same type. `abs` returns its magnitude:

```text
fn sale_price(price: f64) -> f64:
    price.round()

fn whole_boxes(items: f64, per_box: f64) -> f64:
    (items / per_box).ceil()
```

| Rule | Method | Result |
| --- | --- | --- |
| r[std-num.float-abs] `abs` | `fn abs(self) -> F` | the magnitude of `self` |
| r[std-num.float-round.trunc] `trunc` | `fn trunc(self) -> F` | the whole part, toward zero |
| r[std-num.float-round.floor] `floor` | `fn floor(self) -> F` | the largest whole value not above `self` |
| r[std-num.float-round.ceil] `ceil` | `fn ceil(self) -> F` | the smallest whole value not below `self` |
| r[std-num.float-round.round] `round` | `fn round(self) -> F` | the nearest whole value, a tie going away from zero |

In this section, `F` is `f32` or `f64`.

1. r[std-num.float-round.widths] The five methods exist on `f32` and on `f64`, and return the receiver's type.
2. r[std-num.float-round.exact] Each result is exact: the value an infinitely precise computation gives, with no rounding error. So `0.49999999999999994.round()` is `0.0`, and `2.5.round()` is `3.0`.
3. r[std-num.float-round.unchanged] `trunc`, `floor`, `ceil`, and `round` return a NaN, an infinity, and a finite value of magnitude at least 2 to the power 52 unchanged. For `f32` the power is 23. Those values are already whole, and a NaN stays a NaN.
4. r[std-num.float-round.zero-sign] A zero result takes the sign of the value that produced it, and a zero input is returned as is. So `(-0.4).round()`, `(-0.5).ceil()`, `(-0.2).trunc()`, and `(-0.0).floor()` are `-0.0`, and `0.4.round()` is `0.0`.
5. r[std-num.float-round.tie] `round` sends a tie to the whole value of larger magnitude, as Rust's `f64::round` does. So `(-2.5).round()` is `-3.0` and `(-0.5).round()` is `-1.0`.
6. r[std-num.float-abs.value] `abs` returns `-x` for a negative `x`, `0.0` for either zero, and `x` otherwise. So `(-0.0).abs()` is `0.0`, a NaN stays a NaN, and the infinities give positive infinity.
7. r[std-num.float-round.no-panic] None of the five methods panics.

| Call | Result |
| --- | --- |
| `2.7.floor()`, `(-2.1).floor()` | `2.0`, `-3.0` |
| `2.1.ceil()`, `(-2.7).ceil()` | `3.0`, `-2.0` |
| `7.9.trunc()`, `(-7.9).trunc()` | `7.0`, `-7.0` |
| `2.4.round()`, `2.5.round()`, `3.5.round()` | `2.0`, `3.0`, `4.0` |
| `(-2.5).round()` | `-3.0` |
| `(-0.0).abs()` | `0.0` |

> **Why.** The methods and their edge cases are Rust's. Each needs only
> the saturating `f64` to `i64` cast
> ([`types.cast.saturate`](../lang/04-type-system.md#r-types.cast.saturate))
> below 2 to the power 52, where every `f64` fits an `i64`, so no new
> primitive is needed. `sqrt` is not here: no float operation builds it,
> so it is an intrinsic method ([Square Root](#square-root)).

## Square Root

`sqrt` returns the square root of a float, in the float's own type:

```text
fn distance(dx: f64, dy: f64) -> f64:
    (dx * dx + dy * dy).sqrt()
```

| Rule | Method | Result |
| --- | --- | --- |
| r[std-num.float-sqrt] `sqrt` | `fn sqrt(self) -> F` | the square root of `self` |

In this section, `F` is `f32` or `f64`.

1. r[std-num.float-sqrt.widths] `sqrt` exists on `f32` and on `f64`, and returns the receiver's type. It is an [intrinsic method](../lang/09-traits.md#intrinsic-methods): its signature is a plain declaration, and the compiler supplies its body.
2. r[std-num.float-sqrt.rounded] The result is the square root of `self` correctly rounded to `F`, as IEEE 754 `squareRoot` rounds to nearest. So `4.0.sqrt()` is exactly `2.0`, and `2.0.sqrt()` is the `f64` nearest the square root of 2.
3. r[std-num.float-sqrt.zero] `sqrt` returns a zero as is, so `(-0.0).sqrt()` is `-0.0` and `0.0.sqrt()` is `0.0`.
4. r[std-num.float-sqrt.special] Positive infinity gives positive infinity, and a NaN gives a NaN.
5. r[std-num.float-sqrt.negative] A value below zero, negative infinity included, gives a NaN. So `(-1.0).sqrt()` is a NaN.
6. r[std-num.float-sqrt.no-panic] `sqrt` never panics.

| Call | Result |
| --- | --- |
| `4.0.sqrt()`, `2.25.sqrt()` | `2.0`, `1.5` |
| `2.0.sqrt()` | `1.4142135623730951` |
| `(-0.0).sqrt()` | `-0.0` |
| `(-1.0).sqrt()` | a NaN |

> **Why.** The method and its edge cases are Rust's `f64::sqrt`. One Wasm
> instruction, `f64.sqrt` or `f32.sqrt`, computes the correctly rounded
> root, while an hd body built from `+`, `*`, and `/` would be slower and
> could round the last bit wrong.
