//! Floating-point arithmetic, comparison, remainder and casts end to end
//! (`expr.float.*`, `expr.eq.float`, `types.cast.*`). Each program holds
//! its operands and the results Rust computes for them as lists, and
//! prints the index of every case where the hd result differs: bit for
//! bit, so signed zeros, infinities and NaN count. Programs build through
//! the driver and run on V8 (`host/run.mjs`), as the conformance runner
//! does.

use std::cmp::Ordering;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// One program: lists `c0`, `c1`, ... of operands and expected results,
/// and checks. A check is a tag and a condition that is true on a wrong
/// result, evaluated for each index `i` of the lists.
struct Case {
    name: &'static str,
    items: String,
    cols: Vec<(String, Vec<String>)>,
    checks: Vec<(String, String)>,
    /// Describes case `i` for a failure message.
    describe: Box<dyn Fn(usize) -> String>,
}

impl Case {
    fn new(name: &'static str, describe: impl Fn(usize) -> String + 'static) -> Case {
        Case {
            name,
            items: PRELUDE.to_owned(),
            cols: Vec::new(),
            checks: Vec::new(),
            describe: Box::new(describe),
        }
    }

    /// Adds a list of type `ty`; returns its name.
    fn col(&mut self, ty: &str, items: Vec<String>) -> String {
        if let Some((_, first)) = self.cols.first() {
            assert_eq!(first.len(), items.len(), "{}: column length", self.name);
        }
        self.cols.push((ty.to_owned(), items));
        format!("c{}", self.cols.len() - 1)
    }

    fn check(&mut self, tag: &str, wrong: String) {
        self.checks.push((tag.to_owned(), wrong));
    }

    fn run(&self) {
        let mut main = format!("{}\npub fn main() -> void $ Console:\n", self.items);
        for (i, (ty, items)) in self.cols.iter().enumerate() {
            writeln!(main, "    let c{i}: List[{ty}] = [{}]", items.join(", ")).unwrap();
        }
        writeln!(main, "    for i in 0..c0.len():").unwrap();
        for (tag, wrong) in &self.checks {
            writeln!(
                main,
                "        if {wrong}:\n            println(\"{tag} ${{i}}\")"
            )
            .unwrap();
        }
        let mut src = MemorySources::default();
        src.insert("main.hd", &main);
        let store = MemoryStore::default();
        let host = Host {
            render_tir: &[],
            sources: &src,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
        };
        let out = build(
            &host,
            "app",
            &Goal::Program {
                entry: "main".into(),
            },
        );
        let name = self.name;
        assert!(out.diags.is_empty(), "{name}: {}", out.render());
        let wasm = out.wasm.expect("wasm");
        let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("float-{name}.wasm"));
        std::fs::write(&path, wasm).expect("write wasm");
        let ran = Command::new("node")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
            .arg(&path)
            .output()
            .expect("node");
        let _ = std::fs::remove_file(&path);
        assert!(
            ran.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&ran.stderr)
        );
        let stdout = String::from_utf8(ran.stdout).expect("UTF-8");
        let wrong: Vec<String> = stdout
            .lines()
            .take(20)
            .map(|line| {
                let i: usize = line
                    .rsplit(' ')
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0);
                format!("{line}: {}", (self.describe)(i))
            })
            .collect();
        assert!(
            wrong.is_empty(),
            "{name}: {} wrong results, the first:\n{}",
            stdout.lines().count(),
            wrong.join("\n")
        );
    }
}

/// `got` and `want` are the same bits as floats: NaN with NaN, and zeros of
/// one sign.
const PRELUDE: &str = "\
fn zero32() -> f32:
    0.0
fn inf32() -> f32:
    1.0 / zero32()
fn nan32() -> f32:
    zero32() / zero32()
fn zero64() -> f64:
    0.0
fn inf64() -> f64:
    1.0 / zero64()
fn nan64() -> f64:
    zero64() / zero64()
fn same32(got: f32, want: f32) -> bool:
    if want != want:
        return got != got
    got == want && 1.0 / got == 1.0 / want
fn same(got: f64, want: f64) -> bool:
    if want != want:
        return got != got
    got == want && 1.0 / got == 1.0 / want
";

/// An `f64` as hd source; the infinities and NaN are functions of the
/// prelude, which build them by division.
fn lit(x: f64, suffix: &str) -> String {
    if x.is_nan() {
        format!("nan{suffix}()")
    } else if x.is_infinite() {
        format!("{}inf{suffix}()", if x < 0.0 { "-" } else { "" })
    } else {
        format!("{x:?}")
    }
}

/// An `f32` as hd source: its exact `f64` value, which the literal
/// converts back without error.
fn lit32(x: f32) -> String {
    lit(f64::from(x), "32")
}

fn lits32(xs: &[f32]) -> Vec<String> {
    xs.iter().map(|x| lit32(*x)).collect()
}

/// A xorshift generator, so the random operands are the same on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn f32_bits(&mut self) -> f32 {
        f32::from_bits(u32::try_from(self.next() >> 32).expect("32 bits"))
    }
}

/// Every pair of the values, as two columns.
fn pairs<T: Copy>(xs: &[T]) -> (Vec<T>, Vec<T>) {
    let mut a = Vec::new();
    let mut b = Vec::new();
    for x in xs {
        for y in xs {
            a.push(*x);
            b.push(*y);
        }
    }
    (a, b)
}

const F32_VALUES: [f32; 22] = [
    0.0,
    -0.0,
    1.0,
    -1.0,
    0.1,
    -0.3,
    3.0,
    7.5,
    16_777_216.0,
    1.0e-3,
    123_456.79,
    3.0e30,
    -2.5e-20,
    f32::MAX,
    f32::MIN,
    f32::MIN_POSITIVE,
    1.0e-40,
    -1.4e-45,
    f32::INFINITY,
    f32::NEG_INFINITY,
    f32::NAN,
    0.333_333_34,
];

type Binary = (&'static str, &'static str, fn(f32, f32) -> f32);
type Compare = (&'static str, &'static str, fn(f32, f32) -> bool);

const BINARY_F32: [Binary; 4] = [
    ("add", "+", |a, b| a + b),
    ("sub", "-", |a, b| a - b),
    ("mul", "*", |a, b| a * b),
    ("div", "/", |a, b| a / b),
];

const COMPARE_F32: [Compare; 6] = [
    ("lt", "<", |a, b| a < b),
    ("le", "<=", |a, b| a <= b),
    ("gt", ">", |a, b| a > b),
    ("ge", ">=", |a, b| a >= b),
    ("eq", "==", |a, b| {
        a.partial_cmp(&b) == Some(Ordering::Equal)
    }),
    ("ne", "!=", |a, b| {
        a.partial_cmp(&b) != Some(Ordering::Equal)
    }),
];

/// `f32` operators on the given operand pairs, against Rust's.
fn f32_operators(name: &'static str, a: &[f32], b: &[f32]) {
    let (da, db) = (a.to_vec(), b.to_vec());
    let mut case = Case::new(name, move |i| format!("a={:e} b={:e}", da[i], db[i]));
    let ca = case.col("f32", lits32(a));
    let cb = case.col("f32", lits32(b));
    // The operands go through functions, so each operator is a run-time
    // `f32` operation.
    for (tag, op, f) in BINARY_F32 {
        writeln!(case.items, "fn {tag}(a: f32, b: f32) -> f32:\n    a {op} b").unwrap();
        let want: Vec<f32> = a.iter().zip(b).map(|(x, y)| f(*x, *y)).collect();
        let w = case.col("f32", lits32(&want));
        case.check(tag, format!("!same32({tag}({ca}[i], {cb}[i]), {w}[i])"));
    }
    for (tag, op, f) in COMPARE_F32 {
        writeln!(
            case.items,
            "fn {tag}(a: f32, b: f32) -> bool:\n    a {op} b"
        )
        .unwrap();
        let want: Vec<String> = a
            .iter()
            .zip(b)
            .map(|(x, y)| f(*x, *y).to_string())
            .collect();
        let w = case.col("bool", want);
        case.check(tag, format!("{tag}({ca}[i], {cb}[i]) != {w}[i]"));
    }
    writeln!(case.items, "fn neg(a: f32) -> f32:\n    -a").unwrap();
    let want: Vec<f32> = a.iter().map(|x| -*x).collect();
    let w = case.col("f32", lits32(&want));
    case.check("neg", format!("!same32(neg({ca}[i]), {w}[i])"));
    case.run();
}

#[test]
fn f32_arithmetic_and_comparison_match_rust() {
    let (a, b) = pairs(&F32_VALUES);
    f32_operators("f32-arith", &a, &b);
}

#[test]
fn f32_arithmetic_rounds_each_result_to_f32() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let a: Vec<f32> = (0..400).map(|_| rng.f32_bits()).collect();
    let b: Vec<f32> = (0..400).map(|_| rng.f32_bits()).collect();
    f32_operators("f32-random", &a, &b);
}

fn lits64(xs: &[f64]) -> Vec<String> {
    xs.iter().map(|x| lit(*x, "64")).collect()
}

/// Float values around every integer type's bounds, and the cases of
/// truncation, infinity and NaN.
fn cast_sources() -> Vec<f64> {
    vec![
        0.0,
        -0.0,
        0.5,
        -0.5,
        0.9999,
        -0.9999,
        1.0,
        -1.0,
        1.9,
        -1.9,
        127.0,
        127.9,
        128.0,
        -128.0,
        -128.9,
        -129.0,
        255.0,
        255.5,
        256.0,
        32_767.0,
        32_768.0,
        -32_768.0,
        -32_769.0,
        65_535.0,
        65_535.9,
        65_536.0,
        16_777_216.0,
        2_147_483_520.0,
        2_147_483_647.0,
        2_147_483_648.0,
        -2_147_483_648.0,
        -2_147_483_649.0,
        4_294_967_040.0,
        4_294_967_295.0,
        4_294_967_296.0,
        9_223_371_487_098_961_920.0,
        9_223_372_036_854_775_807.0,
        -9_223_372_036_854_775_808.0,
        -9_223_373_136_366_403_584.0,
        18_446_742_974_197_923_840.0,
        18_446_744_073_709_551_615.0,
        1.0e20,
        -1.0e20,
        1.0e300,
        f64::MAX,
        f64::MIN,
        f64::MIN_POSITIVE,
        5e-324,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
    ]
}

/// The `f32` nearest to `x`, a tie going to the even significand. Rust
/// parses the exact decimal expansion of the `f64` with correct rounding.
fn to_f32(x: f64) -> f32 {
    format!("{x:.800e}").parse().expect("a float")
}

/// The integer `x` truncates to, saturated to `lo..=hi`, NaN giving zero:
/// the cast Rust's `as` makes (`types.cast.saturate`).
fn saturate(x: f64, lo: i128, hi: i128) -> i128 {
    if x.is_nan() {
        return 0;
    }
    // An integral `f64` prints exactly; one past `i128` does not parse.
    format!("{:.0}", x.trunc())
        .parse::<i128>()
        .map_or(if x < 0.0 { lo } else { hi }, |v| v.clamp(lo, hi))
}

/// The bounds of every integer type a cast reaches, in the order of
/// `INTS`; `usize` is 32 bits wide on Wasm32.
const BOUNDS: [(i128, i128); 9] = [
    (-128, 127),
    (0, 255),
    (-32_768, 32_767),
    (0, 65_535),
    (-2_147_483_648, 2_147_483_647),
    (0, 4_294_967_295),
    (-9_223_372_036_854_775_808, 9_223_372_036_854_775_807),
    (0, 18_446_744_073_709_551_615),
    (0, 4_294_967_295),
];

const INTS: [&str; 9] = [
    "i8", "u8", "i16", "u16", "i32", "u32", "i64", "u64", "usize",
];

/// Casts of the float column `src` (of type `ty`, the values `xs`) to
/// every integer type.
fn float_to_ints(name: &'static str, ty: &str, src: Vec<String>, xs: &[f64]) {
    let shown = src.clone();
    let mut case = Case::new(name, move |i| format!("x={}", shown[i]));
    let c = case.col(ty, src);
    for (t, (lo, hi)) in INTS.iter().zip(BOUNDS) {
        let want = xs.iter().map(|x| saturate(*x, lo, hi).to_string());
        let w = case.col(t, want.collect());
        case.check(t, format!("{t}({c}[i]) != {w}[i]"));
    }
    case.run();
}

#[test]
fn f64_to_integer_saturates() {
    let values = cast_sources();
    float_to_ints("f64-to-int", "f64", lits64(&values), &values);
}

#[test]
fn f32_to_integer_saturates() {
    let values: Vec<f32> = cast_sources().into_iter().map(to_f32).collect();
    let wide: Vec<f64> = values.iter().map(|x| f64::from(*x)).collect();
    float_to_ints("f32-to-int", "f32", lits32(&values), &wide);
}

/// Integer operands: the bounds, the powers of two where an integer stops
/// fitting a significand, ties between two floats, and random bits.
fn int_values(bits: u32, signed: bool, rng: &mut Rng) -> Vec<i128> {
    let (lo, hi) = if signed {
        (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
    } else {
        (0, (1i128 << bits) - 1)
    };
    let mut v = vec![lo, hi, 0, 1, hi - 1, lo + 1];
    if signed {
        v.push(-1);
    }
    for k in [24, 25, 53, 54, 62, 63] {
        for d in [-3i128, -2, -1, 0, 1, 2, 3] {
            v.push((1i128 << k) + d);
            v.push((1i128 << k) * 3 + d);
            if signed {
                v.push(-(1i128 << k) + d);
            }
        }
    }
    if bits <= 8 {
        v.extend(lo..=hi);
    }
    for _ in 0..80 {
        let r = i128::from(rng.next());
        let masked = r & ((1i128 << bits) - 1);
        v.push(if signed && masked > hi {
            masked - (1i128 << bits)
        } else {
            masked
        });
    }
    v.retain(|x| (lo..=hi).contains(x));
    v
}

#[test]
fn integer_to_float_rounds_to_nearest() {
    let mut rng = Rng(0xdead_beef_cafe_f00d);
    for (name, bits, signed) in [
        ("i8", 8, true),
        ("u8", 8, false),
        ("i16", 16, true),
        ("u16", 16, false),
        ("i32", 32, true),
        ("u32", 32, false),
        ("i64", 64, true),
        ("u64", 64, false),
    ] {
        let values = int_values(bits, signed, &mut rng);
        let shown = values.clone();
        let mut case = Case::new("from-int", move |i| format!("x={}", shown[i]));
        let c = case.col(name, values.iter().map(ToString::to_string).collect());
        // Rust parses a decimal integer to the nearest float.
        let wide: Vec<f64> = values
            .iter()
            .map(|v| v.to_string().parse().expect("f64"))
            .collect();
        let single: Vec<f64> = values
            .iter()
            .map(|v| f64::from(v.to_string().parse::<f32>().expect("f32")))
            .collect();
        let (w64, w32) = (
            case.col("f64", lits64(&wide)),
            case.col("f64", lits64(&single)),
        );
        case.check(
            &format!("{name}-to-f64"),
            format!("!same(f64({c}[i]), {w64}[i])"),
        );
        case.check(
            &format!("{name}-to-f32"),
            format!("!same(f64(f32({c}[i])), {w32}[i])"),
        );
        case.run();
    }
}

#[test]
fn float_width_casts_round_to_nearest_even() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let mut values: Vec<f64> = vec![
        0.0,
        -0.0,
        0.1,
        16_777_217.0,
        16_777_219.0,
        -16_777_217.0,
        f64::from(f32::MAX),
        3.402_823_567_797_336_6e38,
        3.402_823_466_385_288_6e38,
        3.402_823_466_385_288_7e38,
        1.0e39,
        -1.0e39,
        1.0e-46,
        7.006_492_321_624_085e-46,
        7.006_492_321_624_087e-46,
        1.0e-45,
        f64::from(f32::MIN_POSITIVE),
        1.175_494_350_822_287_5e-38,
        f64::MAX,
        5e-324,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
    ];
    for _ in 0..250 {
        // A value within a few float32 steps of a float32 one.
        let near = f64::from(rng.f32_bits());
        let delta = i64::try_from(rng.next() % (1 << 30)).expect("small") - (1 << 29);
        let bits = i64::try_from(near.to_bits() & !(1 << 63)).expect("bits") + delta;
        let sign = near.to_bits() & (1 << 63);
        values.push(f64::from_bits(
            u64::try_from(bits.max(0)).expect("positive") | sign,
        ));
    }
    let shown = values.clone();
    let mut case = Case::new("f64-to-f32", move |i| format!("x={:e}", shown[i]));
    let c = case.col("f64", lits64(&values));
    let singles: Vec<f32> = values.iter().map(|x| to_f32(*x)).collect();
    let narrowed: Vec<f64> = singles.iter().map(|x| f64::from(*x)).collect();
    let w = case.col("f64", lits64(&narrowed));
    // The `f32`s themselves widen exactly.
    let s = case.col("f32", lits32(&singles));
    case.check("narrow", format!("!same(f64(f32({c}[i])), {w}[i])"));
    case.check("widen", format!("!same(f64({s}[i]), {w}[i])"));
    case.run();
}
