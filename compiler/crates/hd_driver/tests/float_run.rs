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
