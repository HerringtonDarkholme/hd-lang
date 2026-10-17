//! The host primitives of std (`hd:prim`; spec/std/README.md, "Standard
//! Library Primitives"), as `host/float.mjs` writes them, so a
//! program prints the same text on wasmtime as on Node:
//!
//! - `shortest`: the Display text of a float at its own width
//!   (spec/lang/04-type-system.md, numeric display), by Steele and White
//!   free-format printing in Burger and Dybvig's form over exact integers;
//!   an exact tie between two shortest strings goes to the even digit.
//! - `fixed`: `to_fixed` (spec/std/num.md, fixed-point text), the exact
//!   quotient rounded half to even, the sign bit kept.
//! - `parse`: the `f64` nearest an unsigned decimal number, a tie to the
//!   even significand. Rust's `str::parse` is correctly rounded, as
//!   float.mjs's exact rational is, so the two give the same bits.
//! - `lower` and `upper`: Unicode Default Case Conversion with full
//!   mappings and the final sigma (`ß` gives `SS`).

use std::cmp::Ordering;
use std::fmt::Write as _;

/// A non-negative integer of any size: 32-bit limbs, least significant
/// first, with no high zero limbs.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Big(Vec<u32>);

impl Big {
    fn from_u64(v: u64) -> Big {
        let mut b = Big(vec![low32(v), low32(v >> 32)]);
        b.trim();
        b
    }

    fn trim(&mut self) {
        while self.0.last() == Some(&0) {
            self.0.pop();
        }
    }

    fn is_zero(&self) -> bool {
        self.0.is_empty()
    }

    fn mul_small(&mut self, m: u32) {
        let mut carry = 0u64;
        for limb in &mut self.0 {
            let v = u64::from(*limb) * u64::from(m) + carry;
            *limb = low32(v);
            carry = v >> 32;
        }
        if carry > 0 {
            self.0.push(low32(carry));
        }
        self.trim();
    }

    /// Multiplies by `10^k`.
    fn mul_pow10(&mut self, mut k: u32) {
        while k >= 9 {
            self.mul_small(1_000_000_000);
            k -= 9;
        }
        if k > 0 {
            self.mul_small(10u32.pow(k));
        }
    }

    /// Multiplies by `2^bits`.
    fn shl(&self, bits: u32) -> Big {
        if self.is_zero() {
            return Big(Vec::new());
        }
        let words = (bits / 32) as usize;
        let rest = bits % 32;
        let mut out = vec![0u32; words];
        let mut carry = 0u32;
        for &limb in &self.0 {
            if rest == 0 {
                out.push(limb);
            } else {
                out.push((limb << rest) | carry);
                carry = limb >> (32 - rest);
            }
        }
        if carry > 0 {
            out.push(carry);
        }
        Big(out)
    }

    /// Divides by `2^bits`, rounding down.
    fn shr(&self, bits: u32) -> Big {
        let words = (bits / 32) as usize;
        let rest = bits % 32;
        if words >= self.0.len() {
            return Big(Vec::new());
        }
        let src = &self.0[words..];
        let mut out = Vec::with_capacity(src.len());
        for (i, &limb) in src.iter().enumerate() {
            let high = src.get(i + 1).copied().unwrap_or(0);
            out.push(if rest == 0 {
                limb
            } else {
                (limb >> rest) | (high << (32 - rest))
            });
        }
        let mut b = Big(out);
        b.trim();
        b
    }

    /// The remainder of a division by `2^bits`.
    fn low_bits(&self, bits: u32) -> Big {
        let words = (bits / 32) as usize;
        let rest = bits % 32;
        let mut out: Vec<u32> = self.0.iter().take(words + 1).copied().collect();
        if out.len() > words {
            if rest == 0 {
                out.truncate(words);
            } else {
                out[words] &= (1u32 << rest) - 1;
            }
        }
        let mut b = Big(out);
        b.trim();
        b
    }

    fn add(&self, other: &Big) -> Big {
        let n = self.0.len().max(other.0.len());
        let mut out = Vec::with_capacity(n + 1);
        let mut carry = 0u64;
        for i in 0..n {
            let v = u64::from(self.0.get(i).copied().unwrap_or(0))
                + u64::from(other.0.get(i).copied().unwrap_or(0))
                + carry;
            out.push(low32(v));
            carry = v >> 32;
        }
        if carry > 0 {
            out.push(low32(carry));
        }
        Big(out)
    }

    /// `self - other`, with `self >= other`.
    #[cfg(test)]
    fn sub_assign(&mut self, other: &Big) {
        let mut borrow = 0i64;
        for i in 0..self.0.len() {
            let v = i64::from(self.0[i]) - i64::from(other.0.get(i).copied().unwrap_or(0)) - borrow;
            if v < 0 {
                self.0[i] = low32(u64::try_from(v + (1i64 << 32)).unwrap_or(0));
                borrow = 1;
            } else {
                self.0[i] = low32(u64::try_from(v).unwrap_or(0));
                borrow = 0;
            }
        }
        self.trim();
    }

    fn cmp(&self, other: &Big) -> Ordering {
        self.0
            .len()
            .cmp(&other.0.len())
            .then_with(|| self.0.iter().rev().cmp(other.0.iter().rev()))
    }

    /// Divides by a small `d`, returning the remainder.
    fn div_small(&mut self, d: u32) -> u32 {
        let mut rem = 0u64;
        for limb in self.0.iter_mut().rev() {
            let v = (rem << 32) | u64::from(*limb);
            *limb = low32(v / u64::from(d));
            rem = v % u64::from(d);
        }
        self.trim();
        low32(rem)
    }

    fn is_odd(&self) -> bool {
        self.0.first().is_some_and(|l| l & 1 == 1)
    }

    fn to_decimal(&self) -> String {
        if self.is_zero() {
            return "0".to_owned();
        }
        let mut n = self.clone();
        let mut chunks = Vec::new();
        while !n.is_zero() {
            chunks.push(n.div_small(1_000_000_000));
        }
        let mut out = chunks.pop().map(|c| c.to_string()).unwrap_or_default();
        for c in chunks.iter().rev() {
            let _ = write!(out, "{c:09}");
        }
        out
    }
}

fn low32(v: u64) -> u32 {
    u32::try_from(v & 0xffff_ffff).unwrap_or(0)
}

/// A finite float's parts: its sign, its significand `f` (an integer) and
/// its exponent `e` (the value is `f * 2^e`).
struct Parts {
    negative: bool,
    f: u64,
    e: i32,
}

fn split(raw: u64, mantissa: u32, exp_bits: u32, bias: i32) -> Parts {
    let fraction = raw & ((1u64 << mantissa) - 1);
    let biased = i32::try_from((raw >> mantissa) & ((1u64 << exp_bits) - 1)).unwrap_or(0);
    let negative = (raw >> (mantissa + exp_bits)) != 0;
    let mantissa_i = i32::try_from(mantissa).unwrap_or(0);
    if biased == 0 {
        Parts {
            negative,
            f: fraction,
            e: 1 - bias - mantissa_i,
        }
    } else {
        Parts {
            negative,
            f: fraction | (1u64 << mantissa),
            e: biased - bias - mantissa_i,
        }
    }
}

/// The Display text of an `f64`.
#[must_use]
pub fn shortest_f64(value: f64) -> String {
    if let Some(text) = special(value) {
        return text;
    }
    let parts = split(value.to_bits(), 52, 11, 1023);
    let a = value.abs();
    native(&parts, &format!("{a:e}"), |s| s.parse::<f64>() == Ok(a))
}

/// The Display text of an `f32`, with its own `f32` digits.
#[must_use]
pub fn shortest_f32(value: f32) -> String {
    if let Some(text) = special(f64::from(value)) {
        return text;
    }
    let parts = split(u64::from(value.to_bits()), 23, 8, 127);
    let a = value.abs();
    native(&parts, &format!("{a:e}"), |s| s.parse::<f32>() == Ok(a))
}

/// The text of a NaN or an infinity.
fn special(value: f64) -> Option<String> {
    if value.is_nan() {
        Some("NaN".to_owned())
    } else if value.is_infinite() {
        Some(if value > 0.0 { "inf" } else { "-inf" }.to_owned())
    } else {
        None
    }
}

/// The Display text of a finite float from Rust's own shortest printing
/// (`text_e`, the `{:e}` text of its magnitude), which is the shortest
/// digits that read back, closest to the value. Where the value is
/// exactly halfway between the two closest shortest strings Rust takes
/// the larger; Display takes the one with an even last digit, unless that
/// one does not read back (`reads_back`).
fn native(parts: &Parts, text_e: &str, reads_back: impl Fn(&str) -> bool) -> String {
    let sign = if parts.negative { "-" } else { "" };
    if parts.f == 0 {
        return format!("{sign}0.0");
    }
    let (mantissa, exponent) = text_e.split_once('e').unwrap_or((text_e, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let mut ds: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
    if ds.last().is_some_and(|d| d % 2 == 1) && halfway(parts, ds.len()) {
        let last = ds.len() - 1;
        ds[last] -= 1;
        let places = exponent - i32::try_from(last).unwrap_or(0);
        let lower = format!("{}e{places}", String::from_utf8_lossy(&ds));
        if !reads_back(&lower) {
            ds[last] += 1;
        }
    }
    layout(sign, &String::from_utf8_lossy(&ds), exponent + 1)
}

/// Whether the exact value of `parts` has `n + 1` significant digits, the
/// last a 5: then it is halfway between two `n`-digit strings. A value
/// `f * 2^e` with `e < 0` and `f` odd has exactly `-e` fractional digits,
/// and as many as `f * 5^-e` has digits; past 25 of them the expansion is
/// longer than any shortest text, so no tie is possible there.
fn halfway(parts: &Parts, n: usize) -> bool {
    let shift = parts.f.trailing_zeros();
    let places = -i64::from(parts.e) - i64::from(shift);
    if places <= 0 || places > 25 {
        return false;
    }
    let exact = u128::from(parts.f >> shift) * 5u128.pow(u32::try_from(places).unwrap_or(0));
    usize::try_from(exact.ilog10()).unwrap_or(0) == n
}

/// The Display text of the digits `ds` whose first digit has the decimal
/// exponent `k - 1`: fixed notation for an exponent in [-6, 21),
/// lowercase scientific notation otherwise.
fn layout(sign: &str, ds: &str, k: i32) -> String {
    let exponent = k - 1;
    if (-6..21).contains(&exponent) {
        if exponent < 0 {
            let zeros = "0".repeat(usize::try_from(-exponent - 1).unwrap_or(0));
            return format!("{sign}0.{zeros}{ds}");
        }
        let whole_len = usize::try_from(exponent + 1).unwrap_or(0);
        let whole = if ds.len() > whole_len {
            ds[..whole_len].to_owned()
        } else {
            format!("{ds:0<whole_len$}")
        };
        let rest = ds.get(whole_len..).unwrap_or("");
        return format!("{sign}{whole}.{}", if rest.is_empty() { "0" } else { rest });
    }
    let mantissa = if ds.len() > 1 {
        format!("{}.{}", &ds[..1], &ds[1..])
    } else {
        ds.to_owned()
    };
    format!(
        "{sign}{mantissa}e{}{}",
        if exponent < 0 { "-" } else { "+" },
        exponent.abs()
    )
}

/// The `to_fixed` text of `value` with `places` digits after the point.
#[must_use]
pub fn fixed(value: f64, places: u32) -> String {
    if let Some(text) = special(value) {
        return text;
    }
    let Parts { negative, f, e, .. } = split(value.to_bits(), 52, 11, 1023);
    let mut scaled;
    if e >= 0 {
        scaled = Big::from_u64(f).shl(e.unsigned_abs());
        scaled.mul_pow10(places);
    } else {
        let mut numerator = Big::from_u64(f);
        numerator.mul_pow10(places);
        let shift = e.unsigned_abs();
        scaled = numerator.shr(shift);
        let twice = numerator.low_bits(shift).shl(1);
        let denominator = Big::from_u64(1).shl(shift);
        match twice.cmp(&denominator) {
            Ordering::Greater => scaled = scaled.add(&Big::from_u64(1)),
            Ordering::Equal if scaled.is_odd() => scaled = scaled.add(&Big::from_u64(1)),
            _ => {}
        }
    }
    let places = places as usize;
    let text = format!("{:0>width$}", scaled.to_decimal(), width = places + 1);
    let (whole, frac) = text.split_at(text.len() - places);
    format!(
        "{}{whole}{}",
        if negative { "-" } else { "" },
        if places > 0 {
            format!(".{frac}")
        } else {
            String::new()
        }
    )
}

/// Whether `text` is an unsigned decimal number
/// (spec/std/num.md, `std-num.parse-f64.decimal`): digits, then
/// optionally `.` and more digits (either side may be empty, not both),
/// then an optional exponent.
fn decimal(text: &str) -> bool {
    let b = text.as_bytes();
    let digits_at = |mut i: usize| {
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        i
    };
    let whole_end = digits_at(0);
    let mut i = whole_end;
    let mut any = whole_end > 0;
    if b.get(i) == Some(&b'.') {
        let frac_end = digits_at(i + 1);
        any |= frac_end > i + 1;
        i = frac_end;
    }
    if !any {
        return false;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let end = digits_at(i);
        if end == i {
            return false;
        }
        i = end;
    }
    i == b.len()
}

/// The `f64` nearest the unsigned decimal number `text`; `None` when the
/// text is outside the grammar, which is a bug in std.
#[must_use]
pub fn parse(text: &str) -> Option<f64> {
    if !decimal(text) {
        return None;
    }
    text.parse().ok()
}

#[must_use]
pub fn lower(text: &str) -> String {
    text.to_lowercase()
}

#[must_use]
pub fn upper(text: &str) -> String {
    text.to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::{Big, Parts, fixed, layout, parse, shortest_f32, shortest_f64, special, split};
    use std::cmp::Ordering;

    /// The shortest digits that read back as `f * 2^e`, and `k`, with the
    /// value near `0.DIGITS * 10^k` (the reference the native printing is held to).
    fn digits(p: &Parts, mantissa: u32, min: i32) -> (String, i32) {
        let Parts { f, e, .. } = *p;
        let hidden = 1u64 << mantissa;
        let narrow = f == hidden && e != min;
        let fb = Big::from_u64(f);
        let (mut r, mut s, mut up, mut down);
        if e >= 0 {
            let scale = Big::from_u64(1).shl(e.unsigned_abs());
            if narrow {
                r = fb.shl(e.unsigned_abs() + 2);
                s = Big::from_u64(4);
                up = scale.shl(1);
                down = scale;
            } else {
                r = fb.shl(e.unsigned_abs() + 1);
                s = Big::from_u64(2);
                up = scale.clone();
                down = scale;
            }
        } else if narrow {
            r = fb.shl(2);
            s = Big::from_u64(1).shl(e.unsigned_abs() + 2);
            up = Big::from_u64(2);
            down = Big::from_u64(1);
        } else {
            r = fb.shl(1);
            s = Big::from_u64(1).shl(e.unsigned_abs() + 1);
            up = Big::from_u64(1);
            down = Big::from_u64(1);
        }
        let closed = f & 1 == 0;
        let reaches = |r: &Big, up: &Big, s: &Big| {
            let o = r.add(up).cmp(s);
            if closed {
                o != Ordering::Less
            } else {
                o == Ordering::Greater
            }
        };
        let fits = |r: &Big, up: &Big, s: &Big| {
            let mut t = r.add(up);
            t.mul_small(10);
            let o = t.cmp(s);
            if closed {
                o == Ordering::Less
            } else {
                o != Ordering::Greater
            }
        };
        // A first estimate of `k`: the value is below `2^(e + bits)`, and
        // 78913 / 2^18 is just under log10(2). The loops below correct it.
        let bits = 64 - i32::try_from(f.leading_zeros()).unwrap_or(0);
        let mut k = ((e + bits) * 78_913).div_euclid(1 << 18);
        if k >= 0 {
            s.mul_pow10(k.unsigned_abs());
        } else {
            r.mul_pow10(k.unsigned_abs());
            up.mul_pow10(k.unsigned_abs());
            down.mul_pow10(k.unsigned_abs());
        }
        while reaches(&r, &up, &s) {
            s.mul_small(10);
            k += 1;
        }
        while fits(&r, &up, &s) {
            r.mul_small(10);
            up.mul_small(10);
            down.mul_small(10);
            k -= 1;
        }
        let mut out = String::new();
        loop {
            r.mul_small(10);
            up.mul_small(10);
            down.mul_small(10);
            let mut d = 0u8;
            while r.cmp(&s) != Ordering::Less {
                r.sub_assign(&s);
                d += 1;
            }
            let low = match r.cmp(&down) {
                Ordering::Less => true,
                Ordering::Equal => closed,
                Ordering::Greater => false,
            };
            let high = reaches(&r, &up, &s);
            if !low && !high {
                out.push(char::from(b'0' + d));
                continue;
            }
            if low && high {
                let twice = r.shl(1);
                match twice.cmp(&s) {
                    Ordering::Greater => d += 1,
                    Ordering::Equal if d & 1 == 1 => d += 1,
                    _ => {}
                }
            } else if high {
                d += 1;
            }
            out.push(char::from(b'0' + d));
            return (out, k);
        }
    }

    /// The Display text by Steele and White free-format printing on exact
    /// integers, which the native printing replaced.
    fn reference(parts: &Parts, mantissa: u32, min: i32) -> String {
        let sign = if parts.negative { "-" } else { "" };
        if parts.f == 0 {
            return format!("{sign}0.0");
        }
        let (ds, k) = digits(parts, mantissa, min);
        layout(sign, &ds, k)
    }

    fn reference_f64(value: f64) -> String {
        special(value)
            .unwrap_or_else(|| reference(&split(value.to_bits(), 52, 11, 1023), 52, -1074))
    }

    fn reference_f32(value: f32) -> String {
        special(f64::from(value))
            .unwrap_or_else(|| reference(&split(u64::from(value.to_bits()), 23, 8, 127), 23, -149))
    }

    fn exact(n: u64) -> f64 {
        n.to_string().parse().unwrap_or(0.0)
    }

    /// A xorshift generator, so the samples are the same on every run.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    #[test]
    fn native_printing_matches_the_exact_reference_f64() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let mut values = vec![
            0.0,
            -0.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MAX,
            f64::MIN_POSITIVE,
            f64::from_bits(1),
            f64::from_bits(0x000f_ffff_ffff_ffff),
            f64::EPSILON,
        ];
        for e in -1074..=1023 {
            let p = 2f64.powi(e);
            if p > 0.0 {
                values.extend([p, -p, f64::from_bits(p.to_bits() + 1)]);
                if p.to_bits() > 1 {
                    values.push(f64::from_bits(p.to_bits() - 1));
                }
            }
        }
        values.extend((0..200_000).map(|_| f64::from_bits(rng.next())));
        // Short decimals and integers, where the digits stop early.
        values.extend((0..20_000).map(|_| {
            let n = i32::try_from(rng.next() % 2_000_000).unwrap_or(0) - 1_000_000;
            f64::from(n) / 10f64.powi(i32::try_from(rng.next() % 9).unwrap_or(0))
        }));
        // Exact binary fractions at every magnitude, where the value can
        // sit exactly halfway between two shortest strings.
        values.extend((0..200_000).map(|_| {
            let whole = rng.next() % (1u64 << (1 + rng.next() % 52));
            let places = rng.next() % 8;
            let odd = u32::try_from(rng.next() % (1 << places)).unwrap_or(0);
            exact(whole) + f64::from(odd) / f64::from(1u32 << places)
        }));
        for x in values {
            assert_eq!(shortest_f64(x), reference_f64(x), "bits {:x}", x.to_bits());
        }
    }

    #[test]
    fn native_printing_matches_the_exact_reference_f32() {
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        let mut values = vec![
            0.0,
            -0.0,
            f32::NAN,
            f32::INFINITY,
            f32::MAX,
            f32::MIN_POSITIVE,
        ];
        for e in -149..=127 {
            let p = 2f32.powi(e);
            if p > 0.0 {
                values.extend([p, -p, f32::from_bits(p.to_bits() + 1)]);
                if p.to_bits() > 1 {
                    values.push(f32::from_bits(p.to_bits() - 1));
                }
            }
        }
        values.extend(
            (0..200_000).map(|_| f32::from_bits(u32::try_from(rng.next() >> 32).unwrap_or(0))),
        );
        values.extend((0..200_000).map(|_| {
            let whole = rng.next() % (1u64 << (1 + rng.next() % 24));
            let places = rng.next() % 8;
            let odd = u32::try_from(rng.next() % (1 << places)).unwrap_or(0);
            format!(
                "{}",
                exact(whole) + f64::from(odd) / f64::from(1u32 << places)
            )
            .parse()
            .unwrap_or(0.0)
        }));
        for x in values {
            assert_eq!(shortest_f32(x), reference_f32(x), "bits {:x}", x.to_bits());
        }
    }

    fn exact_text(text: &str) -> f64 {
        text.parse().unwrap_or(0.0)
    }

    #[test]
    fn halfway_values_take_the_even_digit() {
        // 123456789012345.125 is exactly between 123456789012345.12 and
        // 123456789012345.13, both of which read back.
        assert_eq!(
            shortest_f64(exact_text("123456789012345.125")),
            "123456789012345.12"
        );
        assert_eq!(
            shortest_f64(exact_text("123456789012345.375")),
            "123456789012345.38"
        );
    }

    #[test]
    fn big_arithmetic() {
        let mut b = Big::from_u64(u64::MAX);
        b.mul_pow10(20);
        assert_eq!(b.to_decimal(), format!("{}{}", u64::MAX, "0".repeat(20)));
        assert_eq!(Big::from_u64(5).shl(70).shr(70), Big::from_u64(5));
        assert_eq!(Big::from_u64(0b1011).low_bits(2), Big::from_u64(0b11));
    }

    #[test]
    fn display_text() {
        for (x, t) in [
            (1.5, "1.5"),
            (0.1, "0.1"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1e20, "100000000000000000000.0"),
            (1e21, "1e+21"),
            (1e-6, "0.000001"),
            (1.5e-7, "1.5e-7"),
            (5e-324, "5e-324"),
            (f64::MAX, "1.7976931348623157e+308"),
            (-0.0, "-0.0"),
            (f64::NAN, "NaN"),
            (f64::NEG_INFINITY, "-inf"),
        ] {
            assert_eq!(shortest_f64(x), t);
        }
        assert_eq!(shortest_f32(0.1), "0.1");
        assert_eq!(shortest_f32(f32::MAX), "3.4028235e+38");
        assert_eq!(shortest_f32(f32::from_bits(1)), "1e-45");
        // 2^-1074 * 3 is between "1e-323" and "1.5e-323": the shortest that
        // reads back.
        assert_eq!(shortest_f64(f64::from_bits(3)), "1.5e-323");
    }

    #[test]
    fn fixed_text() {
        for (x, n, t) in [
            (0.125, 2, "0.12"),
            (0.375, 2, "0.38"),
            (2.5, 0, "2"),
            (-0.5, 0, "-0"),
            (1.005, 2, "1.00"),
            (0.1, 20, "0.10000000000000000555"),
            (1e21, 1, "1000000000000000000000.0"),
            (-0.001, 2, "-0.00"),
        ] {
            assert_eq!(fixed(x, n), t);
            assert_eq!(fixed(x, n), format!("{x:.p$}", p = n as usize));
        }
    }

    #[test]
    fn parses_the_grammar_only() {
        assert_eq!(parse("1.5"), Some(1.5));
        assert_eq!(parse(".5"), Some(0.5));
        assert_eq!(parse("5."), Some(5.0));
        assert_eq!(parse("1e400"), Some(f64::INFINITY));
        assert_eq!(parse("15E-1"), Some(1.5));
        for bad in ["", ".", "1e", "+1", "1.5f", "inf", "1e+"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }
}
