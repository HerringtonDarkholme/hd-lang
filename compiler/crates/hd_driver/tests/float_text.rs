//! The float and case primitives (`format_f64`, `format_f32`,
//! `format_f64_fixed`, `string_lower`, `string_upper`; spec 04 `types.display.*`, std `num` fixed-point
//! text): the host's text against Rust's own float printing, which is an
//! independent shortest round-trip and an exact fixed-point writer, then
//! the same texts through compiled programs.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

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

/// Runs `float.mjs` on one request per line (`s32 BITS`, `s64 BITS`, or
/// `fixed BITS DIGITS`, bits in hex; `lower HEX` and
/// `upper HEX`, the UTF-8 bytes in hex) and returns one answer per line.
fn host_text(requests: &[String]) -> Vec<String> {
    let float = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/float.mjs");
    let float = std::fs::canonicalize(float).expect("float.mjs");
    let script = r#"const { shortest, fixed, lower, upper } = await import(process.argv[1]);
import { readFileSync } from "node:fs";
const view = new DataView(new ArrayBuffer(8));
const out = [];
for (const line of readFileSync(0, "utf8").split("\n")) {
  if (!line) continue;
  const [kind, bits, digits] = line.split(" ");
  if (kind === "lower" || kind === "upper") {
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
    let mut child = Command::new("node")
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
    }
}

/// Runs the program's `tests:` block on the Node host and returns one
/// result line per test.
fn run_tests(name: &str, src: &str) -> Vec<String> {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", src);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let goal = Goal::Tests {
        module: None,
        filter: None,
    };
    let out = build(&host, "app", &goal);
    assert!(!out.diags.has_errors(), "{}", out.render());
    let runs: Vec<String> = out
        .tests
        .iter()
        .map(|t| {
            let (test, init) = t.run.expect("a runnable test");
            format!("{test}:{init}")
        })
        .collect();
    assert!(!runs.is_empty(), "no tests ran");
    let wasm = out.wasm.expect("wasm");
    let path: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("float-text-{name}.wasm"));
    std::fs::write(&path, wasm).expect("write wasm");
    let result = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/test.mjs"))
        .arg(&path)
        .arg(runs.join(","))
        .output()
        .expect("node");
    let _ = std::fs::remove_file(path);
    let stdout = String::from_utf8_lossy(&result.stdout);
    let lines: Vec<String> = stdout.lines().map(str::to_owned).collect();
    assert_eq!(
        lines.len(),
        runs.len(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
    lines
}

fn all_pass(name: &str, src: &str) {
    for line in run_tests(name, src) {
        assert!(line.contains("\"status\":0,"), "{line}");
        assert!(!line.contains("\"trapped\":true"), "{line}");
    }
}

#[test]
fn display_of_floats_through_programs() {
    all_pass(
        "display",
        r#"use std.testing.assert_equal

fn text[T < Display](value: T) -> string:
    "$value"

tests:
    it("f64 digits and notation"):
        assert_equal(text(1.5), "1.5", reason="plain")
        assert_equal(text(1.0), "1.0", reason="whole")
        assert_equal(text(0.1), "0.1", reason="shortest")
        assert_equal(text(0.30000000000000004), "0.30000000000000004", reason="seventeen digits")
        assert_equal(text(1e20), "100000000000000000000.0", reason="last fixed exponent")
        assert_equal(text(1e21), "1e+21", reason="first scientific exponent")
        assert_equal(text(1.5e300), "1.5e+300", reason="large")
        assert_equal(text(1e-6), "0.000001", reason="last fixed small")
        assert_equal(text(1.5e-7), "1.5e-7", reason="first scientific small")
        assert_equal(text(5e-324), "5e-324", reason="smallest subnormal")
        assert_equal(text(1.7976931348623157e308), "1.7976931348623157e+308", reason="largest")
        assert_equal(text(-2.5), "-2.5", reason="negative")
    it("f32 digits are the f32 digits"):
        let tenth: f32 = 0.1
        let big: f32 = 16777216.0
        let max: f32 = 3.4028235e38
        let tiny: f32 = 1e-45
        let third: f32 = 0.33333334
        assert_equal(text(tenth), "0.1", reason="f32 0.1")
        let wide_tenth: f64 = 0.10000000149011612
        assert_equal(text(wide_tenth), "0.10000000149011612", reason="the same value as f64")
        assert_equal(text(big), "16777216.0", reason="whole")
        assert_equal(text(max), "3.4028235e+38", reason="largest")
        assert_equal(text(tiny), "1e-45", reason="smallest subnormal")
        assert_equal(text(third), "0.33333334", reason="eight digits")
        let wide_third: f64 = 0.3333333432674408
        assert_equal(text(wide_third), "0.3333333432674408", reason="the same value as f64")
    it("interpolation and to_string at the float type"):
        let tenth: f32 = 0.1
        wide := 0.1 + 0.2
        assert_equal("${1.5}", "1.5", reason="literal")
        assert_equal("$tenth", "0.1", reason="f32 value")
        assert_equal("$wide", "0.30000000000000004", reason="f64 value")
        assert_equal(tenth.to_string(), "0.1", reason="f32 to_string")
        assert_equal(wide.to_string(), "0.30000000000000004", reason="f64 to_string")
    it("specials"):
        zero := 0.0
        let nan = zero / zero
        assert_equal(text(nan), "NaN", reason="NaN")
        assert_equal(text(-nan), "NaN", reason="NaN has no sign")
        assert_equal(text(1.0 / zero), "inf", reason="infinity")
        assert_equal(text(-1.0 / zero), "-inf", reason="negative infinity")
        assert_equal(text(-zero), "-0.0", reason="negative zero")
        assert_equal(text(zero), "0.0", reason="zero")
        let small: f32 = -0.0
        assert_equal(text(small), "-0.0", reason="f32 negative zero")
"#,
    );
}

#[test]
fn to_fixed_through_programs() {
    all_pass(
        "fixed",
        r#"use std.testing.assert_equal

tests:
    it("rounds the exact binary value, a tie to even"):
        assert_equal(0.125.to_fixed(2), "0.12", reason="tie to even, down")
        assert_equal(0.375.to_fixed(2), "0.38", reason="tie to even, up")
        assert_equal(2.5.to_fixed(0), "2", reason="tie")
        assert_equal(3.5.to_fixed(0), "4", reason="tie")
        assert_equal(0.5.to_fixed(0), "0", reason="tie to zero")
        assert_equal((-0.5).to_fixed(0), "-0", reason="tie to negative zero")
        assert_equal(1.005.to_fixed(2), "1.00", reason="1.005 is below the tie")
        assert_equal(0.1.to_fixed(20), "0.10000000000000000555", reason="exact digits")
        assert_equal(3.14159.to_fixed(2), "3.14", reason="plain")
    it("never uses an exponent"):
        assert_equal(1e21.to_fixed(1), "1000000000000000000000.0", reason="large")
        assert_equal(1e-7.to_fixed(3), "0.000", reason="small")
        assert_equal(1e-7.to_fixed(10), "0.0000001000", reason="small, more digits")
        assert_equal(5e-324.to_fixed(2), "0.00", reason="subnormal")
    it("one hundred digits"):
        assert_equal(1.0.to_fixed(100).len(), 102, reason="the point and the digits")
    it("signs and specials"):
        zero := 0.0
        assert_equal((-zero).to_fixed(1), "-0.0", reason="negative zero")
        assert_equal((-0.001).to_fixed(2), "-0.00", reason="rounds to a negative zero")
        assert_equal((zero / zero).to_fixed(2), "NaN", reason="NaN")
        assert_equal((1.0 / zero).to_fixed(2), "inf", reason="infinity")
        assert_equal((-1.0 / zero).to_fixed(2), "-inf", reason="negative infinity")
"#,
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
    }
}

#[test]
fn case_mapping_through_programs() {
    all_pass(
        "case",
        r#"use std.testing.assert_equal

tests:
    it("maps ASCII"):
        assert_equal("Hello, World 42".lower(), "hello, world 42", reason="lower")
        assert_equal("Hello, World 42".upper(), "HELLO, WORLD 42", reason="upper")
        assert_equal("".lower(), "", reason="empty")
    it("maps beyond ASCII"):
        assert_equal("\u{c9}COLE".lower(), "\u{e9}cole", reason="e acute")
        assert_equal("\u{e9}cole".upper(), "\u{c9}COLE", reason="e acute up")
        assert_equal("\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}".upper(), "\u{41f}\u{420}\u{418}\u{412}\u{415}\u{422}", reason="Cyrillic")
        assert_equal("\u{1f600}".upper(), "\u{1f600}", reason="no case")
    it("changes the length with full mappings"):
        assert_equal("stra\u{df}e".upper(), "STRASSE", reason="sharp s")
        assert_equal("stra\u{df}e".upper().len(), 7, reason="seven bytes")
        assert_equal("\u{fb01}".upper(), "FI", reason="ligature")
        assert_equal("\u{130}".lower(), "i\u{307}", reason="dotted capital I")
        assert_equal("\u{3a3}A\u{3a3}".lower(), "\u{3c3}a\u{3c2}", reason="final sigma")
        assert_equal("\u{3a3}".lower(), "\u{3c3}", reason="lone sigma")
"#,
    );
}
