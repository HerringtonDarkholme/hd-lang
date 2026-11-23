//! The float and case primitives (`format_f64`, `format_f32`,
//! `format_f64_fixed`, `parse_f64`, `string_lower`, `string_upper`; spec 04 `types.display.*`, std `num` fixed-point
//! text): the host's text against Rust's own float printing, which is an
//! independent shortest round-trip and an exact fixed-point writer. With
//! the `wasmtime` feature, the wasmtime host's text
//! (`hd_run_wasmtime::prim`) must equal the Node host's on every sample.
//! The same texts through compiled programs are stage cases
//! (`tests/cases/std-num/`, `tests/cases/std-text/case-mapping-*`).

use std::io::Write as _;
use std::path::Path;
use std::process::Stdio;

#[cfg(feature = "wasmtime")]
use hd_run_wasmtime::prim;

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

/// The Display text of a finite nonzero float, from Rust's shortest digits
/// (`{:e}`) and the notation rules of `types.display.notation`. `exact` is
/// the float's exact decimal expansion in `{:e}` form. Where the exact
/// value is halfway between the two closest shortest strings, Rust takes
/// the larger; Display takes the one with an even last digit, as
/// JavaScript, Python and Go do, unless that string does not read back
/// as the float (the gap below a power of two is half the gap above).
fn display_text(
    text_e: &str,
    exact: &str,
    negative: bool,
    reads_back: &dyn Fn(&str) -> bool,
) -> String {
    let (mantissa, exponent) = text_e.split_once('e').expect("an exponent");
    let exponent: i32 = exponent.parse().expect("exponent digits");
    let mut digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
    let exact_digits: Vec<u8> = exact
        .split_once('e')
        .expect("an exponent")
        .0
        .bytes()
        .filter(u8::is_ascii_digit)
        .collect();
    let n = digits.len();
    let halfway =
        exact_digits.get(n) == Some(&b'5') && exact_digits[n + 1..].iter().all(|d| *d == b'0');
    if halfway && digits[n - 1] % 2 == 1 {
        digits[n - 1] -= 1;
        let places = exponent - i32::try_from(n - 1).expect("small");
        let lower = format!("{}e{places}", String::from_utf8_lossy(&digits));
        if !reads_back(&lower) {
            digits[n - 1] += 1;
        }
    }
    let digits = String::from_utf8(digits).expect("ascii");
    let sign = if negative { "-" } else { "" };
    if (-6..21).contains(&exponent) {
        if exponent < 0 {
            let zeros = "0".repeat(usize::try_from(-exponent - 1).expect("small"));
            return format!("{sign}0.{zeros}{digits}");
        }
        let whole = usize::try_from(exponent).expect("small") + 1;
        let padded = format!("{digits:0<whole$}");
        let (int, frac) = padded.split_at(whole);
        return format!("{sign}{int}.{}", if frac.is_empty() { "0" } else { frac });
    }
    let mantissa = if digits.len() > 1 {
        format!("{}.{}", &digits[..1], &digits[1..])
    } else {
        digits
    };
    format!(
        "{sign}{mantissa}e{}{}",
        if exponent < 0 { '-' } else { '+' },
        exponent.abs()
    )
}

fn display_f64(x: f64) -> String {
    if x.is_nan() {
        "NaN".into()
    } else if x.is_infinite() {
        if x < 0.0 { "-inf" } else { "inf" }.into()
    } else if x == 0.0 {
        if x.is_sign_negative() { "-0.0" } else { "0.0" }.into()
    } else {
        let a = x.abs();
        let reads_back = |s: &str| s.parse::<f64>() == Ok(a);
        display_text(
            &format!("{a:e}"),
            &format!("{a:.80e}"),
            x < 0.0,
            &reads_back,
        )
    }
}

fn display_f32(x: f32) -> String {
    if x.is_nan() {
        "NaN".into()
    } else if x.is_infinite() {
        if x < 0.0 { "-inf" } else { "inf" }.into()
    } else if x == 0.0 {
        if x.is_sign_negative() { "-0.0" } else { "0.0" }.into()
    } else {
        let a = x.abs();
        let reads_back = |s: &str| s.parse::<f32>() == Ok(a);
        display_text(
            &format!("{a:e}"),
            &format!("{a:.80e}"),
            x < 0.0,
            &reads_back,
        )
    }
}

/// Runs `float.ts` on one request per line (`s32 BITS`, `s64 BITS`, or
/// `fixed BITS DIGITS`, bits in hex; `parse TEXT`; `lower HEX` and
/// `upper HEX`, the UTF-8 bytes in hex) and returns one answer per line.
fn host_text(requests: &[String]) -> Vec<String> {
    let float = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/float.ts");
    let float = hd_run::canonicalize(float).expect("float.ts");
    let script = r#"const { shortest, fixed, parse, lower, upper } = await import(process.argv[1]);
import { readFileSync } from "node:fs";
const view = new DataView(new ArrayBuffer(8));
const out = [];
for (const line of readFileSync(0, "utf8").split("\n")) {
  if (!line) continue;
  const [kind, bits, digits] = line.split(" ");
  if (kind === "parse") {
    view.setFloat64(0, parse(bits));
    out.push(view.getBigUint64(0).toString(16));
  } else if (kind === "lower" || kind === "upper") {
    const text = Buffer.from(bits, "hex").toString("utf8");
    const mapped = kind === "lower" ? lower(text) : upper(text);
    out.push(Buffer.from(mapped, "utf8").toString("hex"));
  } else if (kind === "s32") {
    view.setUint32(0, Number("0x" + bits));
    out.push(shortest(view.getFloat32(0), 32));
  } else {
    view.setBigUint64(0, BigInt("0x" + bits));
    const x = view.getFloat64(0);
    out.push(kind === "s64" ? shortest(x, 64) : fixed(x, Number(digits)));
  }
}
process.stdout.write(out.join("\n") + "\n");
"#;
    let mut child = hd_testkit::node::command()
        .args(["--input-type=module", "-e", script])
        .arg(&float)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("node");
    let input = requests.join("\n") + "\n";
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write requests");
    let out = child.wait_with_output().expect("node output");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .expect("utf8")
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Edge values of each width: the extremes, the powers of two with their
/// neighbors (where the gap below is half the gap above), the
/// subnormals, and the specials.
fn edge_f64() -> Vec<f64> {
    let mut v = vec![
        0.0,
        -0.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MAX,
        f64::MIN_POSITIVE,
        f64::from_bits(1),
        f64::from_bits(0x000f_ffff_ffff_ffff),
        0.1,
        0.3,
        1.0,
        1.5,
        1e21,
        1e20,
        1e-6,
        1e-7,
        123_456_789_012_345_680_000.0,
        9_007_199_254_740_993.0,
        5e-324,
    ];
    for e in -1074..=1023 {
        let p = 2f64.powi(e);
        if p.is_normal() || p > 0.0 {
            v.extend([p, f64::from_bits(p.to_bits() + 1), -p]);
            if p.to_bits() > 1 {
                v.push(f64::from_bits(p.to_bits() - 1));
            }
        }
    }
    v
}

fn edge_f32() -> Vec<f32> {
    let mut v = vec![
        0.0,
        -0.0,
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::MAX,
        f32::MIN_POSITIVE,
        f32::from_bits(1),
        0.1,
        0.3,
        1.0,
        16_777_216.0,
        16_777_218.0,
        1e-45,
        1e21,
        1e20,
        1e-6,
        1e-7,
    ];
    for e in -149..=127 {
        let p = 2f32.powi(e);
        if p > 0.0 {
            v.extend([p, f32::from_bits(p.to_bits() + 1), -p]);
            if p.to_bits() > 1 {
                v.push(f32::from_bits(p.to_bits() - 1));
            }
        }
    }
    v
}

#[test]
fn shortest_text_of_f64_matches_rust() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut values = edge_f64();
    values.extend((0..4000).map(|_| f64::from_bits(rng.next())));
    // Short decimals and integers, where the digits stop early.
    values.extend((0..500).map(|_| {
        let n = i32::try_from(rng.next() % 2_000_000).expect("small") - 1_000_000;
        f64::from(n) / 10f64.powi(i32::try_from(rng.next() % 9).expect("small"))
    }));
    let requests: Vec<String> = values
        .iter()
        .map(|x| format!("s64 {:x}", x.to_bits()))
        .collect();
    let got = host_text(&requests);
    assert_eq!(got.len(), values.len());
    for (x, text) in values.iter().zip(&got) {
        assert_eq!(*text, display_f64(*x), "bits {:x}", x.to_bits());
        #[cfg(feature = "wasmtime")]
        assert_eq!(
            *text,
            prim::shortest_f64(*x),
            "wasmtime, bits {:x}",
            x.to_bits()
        );
    }
}

#[test]
fn shortest_text_of_f32_matches_rust() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let mut values = edge_f32();
    values.extend((0..4000).map(|_| f32::from_bits(u32::try_from(rng.next() >> 32).expect("32"))));
    values.extend((0..500).map(|_| {
        let n = i32::try_from(rng.next() % 2_000_000).expect("small") - 1_000_000;
        // A decimal, read to the nearest `f32`.
        let places = rng.next() % 9;
        format!("{n}e-{places}").parse::<f32>().expect("a decimal")
    }));
    let requests: Vec<String> = values
        .iter()
        .map(|x| format!("s32 {:x}", x.to_bits()))
        .collect();
    let got = host_text(&requests);
    assert_eq!(got.len(), values.len());
    for (x, text) in values.iter().zip(&got) {
        assert_eq!(*text, display_f32(*x), "bits {:x}", x.to_bits());
        #[cfg(feature = "wasmtime")]
        assert_eq!(
            *text,
            prim::shortest_f32(*x),
            "wasmtime, bits {:x}",
            x.to_bits()
        );
    }
}

#[test]
fn fixed_text_matches_rust() {
    let mut rng = Rng(0xd1b5_4a32_d192_ed03);
    let mut cases: Vec<(f64, usize)> = Vec::new();
    for x in edge_f64() {
        for digits in [0, 1, 2, 17, 100] {
            cases.push((x, digits));
        }
    }
    // Exact binary fractions, so ties come up at every digit count.
    for _ in 0..3000 {
        let n = i32::try_from(rng.next() % 40_000).expect("small") - 20_000;
        let shift = i32::try_from(rng.next() % 12).expect("small");
        let digits = usize::try_from(rng.next() % 8).expect("small");
        cases.push((f64::from(n) / 2f64.powi(shift), digits));
    }
    for _ in 0..1500 {
        let digits = usize::try_from(rng.next() % 101).expect("small");
        cases.push((f64::from_bits(rng.next()), digits));
    }
    let requests: Vec<String> = cases
        .iter()
        .map(|(x, d)| format!("fixed {:x} {d}", x.to_bits()))
        .collect();
    let got = host_text(&requests);
    assert_eq!(got.len(), cases.len());
    for ((x, digits), text) in cases.iter().zip(&got) {
        assert_eq!(
            *text,
            format!("{x:.digits$}"),
            "bits {:x}, {digits} digits",
            x.to_bits()
        );
        #[cfg(feature = "wasmtime")]
        {
            let places = u32::try_from(*digits).expect("small");
            assert_eq!(
                *text,
                prim::fixed(*x, places),
                "wasmtime, bits {:x}, {digits} digits",
                x.to_bits()
            );
        }
    }
}

/// `parse_f64`'s unsigned decimal number texts against Rust's own
/// correctly rounded `str::parse`.
#[test]
fn parse_matches_rust() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut texts: Vec<String> = [
        "0",
        "0.0",
        "000",
        ".5",
        "5.",
        "007",
        "1.5",
        "15e-1",
        "15E-1",
        "1e400",
        "1e-400",
        "9007199254740993",
        "9007199254740992.5",
        "9007199254740991",
        "2.2250738585072014e-308",
        "2.2250738585072011e-308",
        "4.9406564584124654e-324",
        "2.4703282292062327e-324",
        "2.4703282292062328e-324",
        "1.7976931348623157e308",
        "1.7976931348623158e308",
        "1.7976931348623159e308",
        "1e309",
        "0.000000000000000000001e22",
        "1e+2",
        "1E-2",
        "00.00e0",
        "123456789012345678901234567890",
        "1e99999999999999999999",
        "1e-99999999999999999999",
        "0e99999999999999999999",
    ]
    .iter()
    .map(|t| (*t).to_owned())
    .collect();
    // Random finite values, in shortest and in long forms.
    for _ in 0..1500 {
        let x = f64::from_bits(rng.next() & 0x7fff_ffff_ffff_ffff);
        if x.is_finite() {
            texts.push(format!("{x:e}"));
            texts.push(format!("{x:.30e}"));
        }
    }
    // Exact ties: an odd 54-bit integer over a power of two is halfway
    // between two doubles. N * 5^j / 10^j writes it in decimal, and a
    // trailing 1 pushes it past the tie.
    for _ in 0..1500 {
        let n = (1u128 << 53) | u128::from(rng.next() >> 11) | 1;
        let j = u32::try_from(rng.next() % 30).expect("small");
        let scaled = (n * 5u128.pow(j)).to_string();
        let split = scaled.len() - j as usize;
        let tie = format!("{}.{}", &scaled[..split], &scaled[split..]);
        texts.push(tie.clone());
        texts.push(format!("{tie}0000000000000000000000001"));
        if scaled.len() > 20 {
            texts.push(tie.trim_end_matches('.').to_owned());
        }
    }
    // Long digit strings, past the twentieth digit.
    for _ in 0..1000 {
        let n = 1 + usize::try_from(rng.next() % 60).expect("small");
        let digits: String = (0..n)
            .map(|_| char::from(b'0' + u8::try_from(rng.next() % 10).expect("digit")))
            .collect();
        let point =
            usize::try_from(rng.next() % u64::try_from(n + 1).expect("small")).expect("small");
        let exponent = i32::try_from(rng.next() % 700).expect("small") - 350;
        texts.push(format!(
            "{}.{}e{exponent}",
            &digits[..point],
            &digits[point..]
        ));
    }
    let requests: Vec<String> = texts.iter().map(|t| format!("parse {t}")).collect();
    let got = host_text(&requests);
    assert_eq!(got.len(), texts.len());
    for (text, bits) in texts.iter().zip(&got) {
        let want: f64 = text.parse().expect("Rust reads the text");
        assert_eq!(*bits, format!("{:x}", want.to_bits()), "parse {text}");
        #[cfg(feature = "wasmtime")]
        {
            let wasmtime = prim::parse(text).expect("in the grammar");
            assert_eq!(
                *bits,
                format!("{:x}", wasmtime.to_bits()),
                "wasmtime, parse {text}"
            );
        }
    }
}

/// Runs the host's `float.ts` and the exact reference `float_exact.ts`
/// (the `BigInt` printer and parser the host used before it took the
/// engine's native ones) side by side on one request per line (`s32 BITS`,
/// `s64 BITS`, `parse TEXT`) and returns the requests where they differ.
fn reference_mismatches(requests: &[String]) -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let host = hd_run::canonicalize(dir.join("../../host/float.ts")).expect("float.ts");
    let exact = hd_run::canonicalize(dir.join("tests/all/float_exact.ts")).expect("float_exact");
    let script = r#"const host = await import(process.argv[1]);
const exact = await import(process.argv[2]);
import { readFileSync } from "node:fs";
const view = new DataView(new ArrayBuffer(8));
const bits = (x) => { view.setFloat64(0, x); return view.getBigUint64(0); };
const bad = [];
for (const line of readFileSync(0, "utf8").split("\n")) {
  if (!line) continue;
  const [kind, arg] = line.split(" ");
  let a;
  let b;
  if (kind === "parse") {
    a = bits(host.parse(arg));
    b = bits(exact.parse(arg));
  } else if (kind === "s32") {
    view.setUint32(0, Number("0x" + arg));
    const x = view.getFloat32(0);
    a = host.shortest(x, 32);
    b = exact.shortest(x, 32);
  } else {
    view.setBigUint64(0, BigInt("0x" + arg));
    const x = view.getFloat64(0);
    a = host.shortest(x, 64);
    b = exact.shortest(x, 64);
  }
  if (a !== b) bad.push(`${line}: host ${a}, reference ${b}`);
}
process.stdout.write(bad.join("\n") + "\n");
"#;
    let mut child = hd_testkit::node::command()
        .args(["--input-type=module", "-e", script])
        .arg(&host)
        .arg(&exact)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("node");
    let input = requests.join("\n") + "\n";
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write requests");
    let out = child.wait_with_output().expect("node output");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .expect("utf8")
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The float text the hosts take from the engine's own printing and
/// parsing equals the exact `BigInt` reference on every sample: random bit
/// patterns of every class, powers of two and their neighbors, binary
/// fractions where a value sits exactly halfway between two shortest
/// strings, short decimals, random and halfway decimal texts, long
/// mantissas and the conformance float values.
#[test]
fn native_float_text_equals_the_exact_reference() {
    let mut rng = Rng(0x0123_4567_89ab_cdef);
    let mut requests: Vec<String> = Vec::new();
    for x in edge_f64() {
        requests.push(format!("s64 {:x}", x.to_bits()));
    }
    for x in edge_f32() {
        requests.push(format!("s32 {:x}", x.to_bits()));
    }
    for _ in 0..100_000 {
        requests.push(format!("s64 {:x}", rng.next()));
        requests.push(format!("s32 {:x}", rng.next() >> 32));
    }
    // Binary fractions at every magnitude: exact ties between shortest
    // strings happen here.
    for _ in 0..100_000 {
        let bits = 1 + rng.next() % 52;
        let whole = (rng.next() % (1u64 << bits))
            .to_string()
            .parse::<f64>()
            .expect("a number");
        let places = rng.next() % 9;
        let odd = f64::from(u32::try_from(rng.next() % (1 << places)).expect("small"));
        let x = whole + odd / f64::from(1u32 << places);
        requests.push(format!("s64 {:x}", x.to_bits()));
        requests.push(format!("s64 {:x}", (-x).to_bits()));
    }
    // Short decimals and integers, where the digits stop early.
    for _ in 0..50_000 {
        let n = i32::try_from(rng.next() % 2_000_000).expect("small") - 1_000_000;
        let x = f64::from(n) / 10f64.powi(i32::try_from(rng.next() % 9).expect("small"));
        requests.push(format!("s64 {:x}", x.to_bits()));
        let y = format!("{n}e-{}", rng.next() % 9)
            .parse::<f32>()
            .expect("a decimal");
        requests.push(format!("s32 {:x}", y.to_bits()));
    }
    // Decimal texts of 1 to 40 digits, with a point and an exponent, and
    // the shortest and long texts of random doubles.
    for _ in 0..150_000 {
        let n = 1 + usize::try_from(rng.next() % 40).expect("small");
        let digits: String = (0..n)
            .map(|_| char::from(b'0' + u8::try_from(rng.next() % 10).expect("digit")))
            .collect();
        let point =
            usize::try_from(rng.next() % u64::try_from(n + 1).expect("small")).expect("small");
        let exponent = i32::try_from(rng.next() % 700).expect("small") - 350;
        requests.push(format!(
            "parse {}.{}e{exponent}",
            &digits[..point],
            &digits[point..]
        ));
        requests.push(format!("parse {digits}"));
    }
    for _ in 0..50_000 {
        let x = f64::from_bits(rng.next() & 0x7fff_ffff_ffff_ffff);
        if x.is_finite() {
            requests.push(format!("parse {x:e}"));
            requests.push(format!("parse {x:.30e}"));
        }
    }
    // Exact ties between two doubles, and just past them.
    for _ in 0..30_000 {
        let n = (1u128 << 53) | u128::from(rng.next() >> 11) | 1;
        let j = u32::try_from(rng.next() % 30).expect("small");
        let scaled = (n * 5u128.pow(j)).to_string();
        let split = scaled.len() - j as usize;
        let tie = format!("{}.{}", &scaled[..split], &scaled[split..]);
        requests.push(format!("parse {tie}"));
        requests.push(format!("parse {tie}0000000000000000000000001"));
        requests.push(format!("parse {}", tie.trim_end_matches('.')));
    }
    // Twenty and twenty-one digit boundaries, and the extremes.
    for text in [
        "0",
        "0.0",
        ".5",
        "5.",
        "1e400",
        "1e-400",
        "0e99999999999999999999",
        "1e99999999999999999999",
        "12345678901234567890",
        "123456789012345678901",
        "9007199254740993",
        "9007199254740992.5",
        "2.2250738585072014e-308",
        "2.2250738585072011e-308",
        "4.9406564584124654e-324",
        "2.4703282292062327e-324",
        "2.4703282292062328e-324",
        "1.7976931348623157e308",
        "1.7976931348623158e308",
        "1.7976931348623159e308",
        "00000000000000000000000000001",
        "0.00000000000000000001",
    ] {
        requests.push(format!("parse {text}"));
    }
    let bad = reference_mismatches(&requests);
    assert!(
        bad.is_empty(),
        "{} of {} differ, first: {:?}",
        bad.len(),
        requests.len(),
        &bad[..bad.len().min(10)]
    );
}

/// The UTF-8 bytes of `text` in hex.
fn hex(text: &str) -> String {
    use std::fmt::Write as _;
    text.bytes().fold(String::new(), |mut out, b| {
        write!(out, "{b:02x}").expect("write to a string");
        out
    })
}

/// `string_lower` and `string_upper` against fixed expectations of Unicode
/// Default Case Conversion with full mappings.
#[test]
fn case_mapping_of_host_matches_unicode() {
    let cases: [(&str, &str, &str); 9] = [
        ("Hello, World", "hello, world", "HELLO, WORLD"),
        ("stra\u{df}e", "stra\u{df}e", "STRASSE"),
        ("\u{3a3}A\u{3a3}", "\u{3c3}a\u{3c2}", "\u{3a3}A\u{3a3}"),
        ("\u{130}", "i\u{307}", "\u{130}"),
        ("\u{fb01}", "\u{fb01}", "FI"),
        ("\u{e9}\u{c9}", "\u{e9}\u{e9}", "\u{c9}\u{c9}"),
        (
            "\u{1f600}\u{10400}",
            "\u{1f600}\u{10428}",
            "\u{1f600}\u{10400}",
        ),
        ("", "", ""),
        ("\u{1c5}", "\u{1c6}", "\u{1c4}"),
    ];
    let mut requests = Vec::new();
    for (text, _, _) in &cases {
        requests.push(format!("lower {}", hex(text)));
        requests.push(format!("upper {}", hex(text)));
    }
    let got = host_text(&requests);
    for (k, (_, lower, upper)) in cases.iter().enumerate() {
        assert_eq!(got[2 * k], hex(lower), "lower, case {k}");
        assert_eq!(got[2 * k + 1], hex(upper), "upper, case {k}");
        #[cfg(feature = "wasmtime")]
        {
            let text = cases[k].0;
            assert_eq!(prim::lower(text), *lower, "wasmtime lower, case {k}");
            assert_eq!(prim::upper(text), *upper, "wasmtime upper, case {k}");
        }
    }
}
