mod tests {
    use hd_run_wasmtime::internals::prim::{
        Big, Parts, fixed, layout, parse, shortest_f32, shortest_f64, special, split,
    };
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
