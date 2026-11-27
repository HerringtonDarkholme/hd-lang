# `std.num`

Generated from `hd doc std.num`. Run `hd doc std.num.NAME` in a package for one item's full text.

## `Num`

```text
pub trait Num < AnyVal & PartialOrd & Display & Add[Out = Self] & Sub[Out = Self] & Mul[
```

The trait every built-in number type implements: ordered, displayable, and closed under `+`, `-`, `*`, `/`, and `%`.

## `Integer`

```text
pub trait Integer < Num & Ord & BitAnd[Out = Self] & BitOr[Out = Self] & BitXor[Out = Self] & Not[
```

The trait every built-in integer type implements: `Num` plus a total order, the bitwise operators, and the shifts.

- `fn max_value() -> Self`: The greatest value this integer type can hold.
- `fn min_value() -> Self`: The least value this integer type can hold.

## `Float`

```text
pub trait Float < Num & PartialOrd & Neg[Out = Self]
```

The trait the built-in floating-point types implement: `Num` plus negation.

## `ParseNumberError`

```text
pub enum ParseNumberError
```

Why number parsing failed: `Empty`, `InvalidDigit(position)`, or `OutOfRange`.

## `TryFromIntError`

```text
pub struct TryFromIntError
```

An integer conversion failed because the value is outside the destination type's range.

## `N`

- `pub fn checked_add(self, other: N) -> N?`: The sum, or `.None` when it is out of the type's range.
- `pub fn checked_sub(self, other: N) -> N?`: The difference, or `.None` when it is out of the type's range.
- `pub fn checked_mul(self, other: N) -> N?`: The product, or `.None` when it is out of the type's range.
- `pub fn checked_div(self, other: N) -> N?`: The quotient, or `.None` for a zero divisor or the one overflowing case.
- `pub fn wrapping_add(self, other: N) -> N`: The sum, wrapping around the type's range.
- `pub fn wrapping_sub(self, other: N) -> N`: The difference, wrapping around the type's range.
- `pub fn wrapping_mul(self, other: N) -> N`: The product, wrapping around the type's range.
- `pub fn saturating_add(self, other: N) -> N`: The sum, capped at the type's largest or smallest value.
- `pub fn saturating_sub(self, other: N) -> N`: The difference, capped at the type's largest or smallest value.
- `pub fn count_ones(self) -> u32`: The number of one bits in the value.
- `pub fn leading_zeros(self) -> u32`: The number of zero bits before the value's first one bit.
- `pub fn trailing_zeros(self) -> u32`: The number of zero bits after the value's last one bit, or its width for zero.
- `pub fn rotate_left(self, count: u32) -> N`: The bits rotated `count` places toward the most significant bit.
- `pub fn rotate_right(self, count: u32) -> N`: The bits rotated `count` places toward the least significant bit.
- `pub fn pow(self, exp: u32) -> N`: The value raised to the power `exp`; `exp` of 0 gives 1.
- `pub fn to_radix(self, radix: u32) -> string`: The value's digits in `radix` (2 to 36), lowercase, with a leading `-` when negative.
- `pub fn parse_radix(text: string, radix: u32) -> Result[N, ParseNumberError]`: Reads `text` as an integer in `radix` (2 to 36), or the first error.

## `i8`

- `pub fn abs_diff(self, other: i8) -> u8`: The absolute difference of the two values.
- `pub fn abs(self) -> i8`: The absolute value; the minimum value overflows as `-self` does.

## `i16`

- `pub fn abs_diff(self, other: i16) -> u16`: The absolute difference of the two values.
- `pub fn abs(self) -> i16`: The absolute value; the minimum value overflows as `-self` does.

## `i32`

- `pub fn abs_diff(self, other: i32) -> u32`: The absolute difference of the two values.
- `pub fn abs(self) -> i32`: The absolute value; the minimum value overflows as `-self` does.

## `i64`

- `pub fn abs_diff(self, other: i64) -> u64`: The absolute difference of the two values.
- `pub fn abs(self) -> i64`: The absolute value; the minimum value overflows as `-self` does.

## `u8`

- `pub fn abs_diff(self, other: u8) -> u8`: The absolute difference of the two values.

## `u16`

- `pub fn abs_diff(self, other: u16) -> u16`: The absolute difference of the two values.

## `u32`

- `pub fn abs_diff(self, other: u32) -> u32`: The absolute difference of the two values.

## `usize`

- `pub fn abs_diff(self, other: usize) -> usize`: The absolute difference of the two values.

## `u64`

- `pub fn abs_diff(self, other: u64) -> u64`: The absolute difference of the two values.

## `f64`

- `pub fn to_bits(self) -> u64`: The IEEE 754 bit pattern of the value.
- `pub fn from_bits(bits: u64) -> f64`: The value whose IEEE 754 bit pattern is `bits`.
- `pub fn pi() -> f64`: The circle constant pi, rounded to binary64.
- `pub fn e() -> f64`: Euler's number, rounded to binary64.
- `pub fn total_cmp(self, other: f64) -> Ordering`: The IEEE 754 total order of two values, which orders NaNs and the zeros.
- `pub fn is_nan(self) -> bool`: True when the value is NaN.
- `pub fn is_finite(self) -> bool`: True when the value is neither NaN nor an infinity.
- `pub fn to_fixed(self, digits: usize) -> string`: The value in fixed-point text with `digits` digits after the point.
- `pub fn abs(self) -> f64`: The absolute value; `-0.0` gives `0.0` and NaN stays NaN.
- `pub fn trunc(self) -> f64`: The integer part, toward zero.
- `pub fn floor(self) -> f64`: The largest integer not above the value.
- `pub fn ceil(self) -> f64`: The smallest integer not below the value.
- `pub fn round(self) -> f64`: The nearest integer, with a tie rounded away from zero.
- `pub fn round_ties_even(self) -> f64`: The nearest integer, with a tie rounded to the even integer.
- `pub fn copysign(self, sign: f64) -> f64`: The magnitude of this value with the sign of `sign`.
- `pub fn sqrt(self) -> f64`: The correctly rounded square root; a negative value gives NaN.
- `pub fn min(self, other: f64) -> f64`: The smaller input, returning the numeric input when the other is NaN.
- `pub fn max(self, other: f64) -> f64`: The larger input, returning the numeric input when the other is NaN.
- `pub fn ln(self) -> f64`: The natural logarithm.
- `pub fn exp(self) -> f64`: The natural exponential.
- `pub fn powf(self, exponent: f64) -> f64`: This value raised to a floating-point exponent.
- `pub fn powi(self, exponent: i32) -> f64`: This value raised to a signed integer exponent.
- `pub fn signum(self) -> f64`: The sign as negative one, positive one, or NaN.
- `pub fn log2(self) -> f64`: The base-two logarithm.
- `pub fn log10(self) -> f64`: The base-ten logarithm.
- `pub fn atan(self) -> f64`: The principal arctangent in radians.
- `pub fn atan2(self, x: f64) -> f64`: The four-quadrant angle of (x, self), in radians.
- `pub fn asin(self) -> f64`: The principal arcsine in radians.
- `pub fn acos(self) -> f64`: The principal arccosine in radians.
- `pub fn sin(self) -> f64`: The sine of this angle in radians.
- `pub fn cos(self) -> f64`: The cosine of this angle in radians.
- `pub fn tan(self) -> f64`: The tangent of this angle in radians.

## `f32`

- `pub fn to_bits(self) -> u32`: The IEEE 754 bit pattern of the value.
- `pub fn from_bits(bits: u32) -> f32`: The value whose IEEE 754 bit pattern is `bits`.
- `pub fn pi() -> f32`: The circle constant pi, rounded to binary32.
- `pub fn e() -> f32`: Euler's number, rounded to binary32.
- `pub fn total_cmp(self, other: f32) -> Ordering`: The IEEE 754 total order of two values, which orders NaNs and the zeros.
- `pub fn is_nan(self) -> bool`: True when the value is NaN.
- `pub fn is_finite(self) -> bool`: True when the value is neither NaN nor an infinity.
- `pub fn abs(self) -> f32`: The absolute value; `-0.0` gives `0.0` and NaN stays NaN.
- `pub fn trunc(self) -> f32`: The integer part, toward zero.
- `pub fn floor(self) -> f32`: The largest integer not above the value.
- `pub fn ceil(self) -> f32`: The smallest integer not below the value.
- `pub fn round(self) -> f32`: The nearest integer, with a tie rounded away from zero.
- `pub fn round_ties_even(self) -> f32`: The nearest integer, with a tie rounded to the even integer.
- `pub fn copysign(self, sign: f32) -> f32`: The magnitude of this value with the sign of `sign`.
- `pub fn sqrt(self) -> f32`: The correctly rounded square root; a negative value gives NaN.
- `pub fn min(self, other: f32) -> f32`: The smaller input, returning the numeric input when the other is NaN.
- `pub fn max(self, other: f32) -> f32`: The larger input, returning the numeric input when the other is NaN.
- `pub fn ln(self) -> f32`: The natural logarithm.
- `pub fn exp(self) -> f32`: The natural exponential.
- `pub fn powf(self, exponent: f32) -> f32`: This value raised to a floating-point exponent.
- `pub fn powi(self, exponent: i32) -> f32`: This value raised to a signed integer exponent.
- `pub fn signum(self) -> f32`: The sign as negative one, positive one, or NaN.
- `pub fn log2(self) -> f32`: The base-two logarithm.
- `pub fn log10(self) -> f32`: The base-ten logarithm.
- `pub fn atan(self) -> f32`: The principal arctangent in radians.
- `pub fn atan2(self, x: f32) -> f32`: The four-quadrant angle of (x, self), in radians.
- `pub fn asin(self) -> f32`: The principal arcsine in radians.
- `pub fn acos(self) -> f32`: The principal arccosine in radians.
- `pub fn sin(self) -> f32`: The sine of this angle in radians.
- `pub fn cos(self) -> f32`: The cosine of this angle in radians.
- `pub fn tan(self) -> f32`: The tangent of this angle in radians.
