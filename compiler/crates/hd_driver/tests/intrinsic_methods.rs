//! The intrinsic methods of the number types' std operator and comparison
//! implementations (spec 05 `expr.op.std.intrinsic-method`,
//! `expr.eq.std.intrinsic`, `expr.ord.std.intrinsic`) reached through
//! generic bounds: each computes what its operator computes, including the
//! overflow panic, and `cmp`/`partial_cmp` answer an `Ordering`.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

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
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("intrinsic-{name}.wasm"));
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

/// The panic report line of the plain operator and of the generic method
/// on the same overflow.
fn panics(name: &str, src: &str) -> Vec<String> {
    run_tests(name, src)
        .into_iter()
        .inspect(|line| assert!(line.contains("\"trapped\":true"), "{line}"))
        .map(|line| {
            let at = line.find("panic: ").expect("a panic report");
            line[at..]
                .split(['"', '\\'])
                .next()
                .unwrap_or("")
                .to_owned()
        })
        .collect()
}

#[test]
fn generic_arithmetic_runs_at_i32_and_i64() {
    all_pass(
        "arith",
        r#"use std.ops.{Add, Sub, Mul, Div, Rem, Neg, BitAnd, BitXor, Not, Shl, Shr}
use std.testing.assert_equal

fn twice[T < Add[Out = T]](x: T) -> T:
    x + x

fn mixed[T < Sub[Out = T] & Mul[Out = T] & Div[Out = T] & Rem[Out = T]](a: T, b: T) -> T:
    (a - b) * b / b % (a - b)

fn negate[T < Neg[Out = T]](x: T) -> T:
    -x

fn bits[T < BitAnd[Out = T] & BitXor[Out = T] & Not[Out = T]](a: T, b: T) -> T:
    ~(a & b) ^ b

fn round_trip[T < Shl[u8, Out = T] & Shr[u8, Out = T]](x: T, n: u8) -> T:
    (x << n) >> n

tests:
    it("i32"):
        assert_equal(twice(+5), +10, reason="value")
        assert_equal(mixed(+17, +5), +0, reason="value")
        assert_equal(negate(+7), -7, reason="value")
        assert_equal(bits(+12, +10), -3, reason="value")
        assert_equal(round_trip(-8, 2), -8, reason="value")
    it("i64"):
        big := i64(4000000000)
        assert_equal(twice(big), i64(8000000000), reason="value")
        assert_equal(mixed(i64(17), i64(5)), i64(0), reason="value")
        assert_equal(negate(big), i64(-4000000000), reason="value")
        assert_equal(round_trip(i64(-8), 2), i64(-8), reason="value")
    it("unsigned shifts drop the high bits"):
        assert_equal(round_trip(u32(4026531840), 4), u32(0), reason="value")
"#,
    );
}

#[test]
fn generic_overflow_panics_as_the_operator_does() {
    let reports = panics(
        "overflow",
        r#"use std.ops.Add

fn plus[T < Add[Out = T]](a: T, b: T) -> T:
    a + b

tests:
    it("operator"):
        let largest: i32 = 2147483647
        _ := largest + 1
    it("method at i32"):
        let largest: i32 = 2147483647
        _ := plus(largest, 1)
    it("method at i64"):
        let largest: i64 = 9223372036854775807
        _ := plus(largest, 1)
    it("method at i8, called directly"):
        _ := Add::[i8]::add(i8(127), i8(1))
"#,
    );
    assert_eq!(reports.len(), 4);
    assert!(reports.iter().all(|r| *r == reports[0]), "{reports:?}");
}

#[test]
fn cmp_and_partial_cmp_order_signed_unsigned_and_char() {
    all_pass(
        "order",
        r#"use std.testing.assert_equal

fn order[T < Ord](a: T, b: T) -> i32:
    match a.cmp(b):
        .Less => -1
        .Equal => 0
        .Greater => 1

fn partial[T < PartialOrd](a: T, b: T) -> i32:
    match a.partial_cmp(b):
        .Some(.Less) => -1
        .Some(.Equal) => 0
        .Some(.Greater) => 1
        .None => 9

fn less[T < PartialOrd](a: T, b: T) -> bool:
    a < b

tests:
    it("signed"):
        assert_equal(order(+3, +4), -1, reason="value")
        assert_equal(order(+4, +4), 0, reason="value")
        assert_equal(order(+5, -4), 1, reason="value")
        assert_equal(partial(i64(-5), i64(4)), -1, reason="value")
        assert_equal(less(-5, +4), true, reason="value")
    it("unsigned"):
        assert_equal(order(u32(4000000000), u32(1)), 1, reason="value")
        assert_equal(order(u64(1), u64(18000000000000000000)), -1, reason="value")
        assert_equal(partial(u8(200), u8(100)), 1, reason="value")
        assert_equal(less(u32(1), u32(4000000000)), true, reason="value")
    it("char"):
        assert_equal(order('a', 'b'), -1, reason="value")
"#,
    );
}

#[test]
fn float_partial_cmp_and_eq_follow_nan() {
    all_pass(
        "float",
        r#"use std.testing.assert_equal

fn partial[T < PartialOrd](a: T, b: T) -> i32:
    match a.partial_cmp(b):
        .Some(.Less) => -1
        .Some(.Equal) => 0
        .Some(.Greater) => 1
        .None => 9

fn less[T < PartialOrd](a: T, b: T) -> bool:
    a < b

fn same[T < Eq](a: T, b: T) -> bool:
    a == b

tests:
    it("ordered"):
        assert_equal(partial(1.5, 0.5), 1, reason="value")
        assert_equal(partial(1.5, 1.5), 0, reason="value")
        assert_equal(partial(-1.5, 1.5), -1, reason="value")
        assert_equal(same(0.0, -0.0), true, reason="value")
    it("NaN is unordered and unequal"):
        nan := 0.0 / 0.0
        assert_equal(partial(nan, 1.0), 9, reason="value")
        assert_equal(partial(1.0, nan), 9, reason="value")
        assert_equal(partial(nan, nan), 9, reason="value")
        assert_equal(less(nan, 1.0), false, reason="value")
        assert_equal(same(nan, nan), false, reason="value")
"#,
    );
}
