//! `**` end to end (`expr.power.*`): integer powers are checked in the
//! base's type and take an unsigned exponent of any width, and float
//! powers are IEEE 754 `pow`. Programs build through the driver and run on
//! V8 (`host/run.mjs`), as the conformance runner does.

use std::path::{Path, PathBuf};
use std::process::{Command, Output as Run};

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// Builds `body` as the body of `main` and runs it.
fn run(name: &str, items: &str, body: &str) -> Run {
    let mut main = format!("{items}\npub fn main() -> void $ Console:\n");
    for line in body.lines() {
        main.push_str("    ");
        main.push_str(line);
        main.push('\n');
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
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("power-{name}.wasm"));
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    ran
}

/// The lines a program that must succeed prints.
fn prints(name: &str, items: &str, body: &str) -> Vec<String> {
    let ran = run(name, items, body);
    assert!(
        ran.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8(ran.stdout)
        .expect("UTF-8")
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Asserts that a program panics with category `code`.
fn panics(name: &str, body: &str, code: &str) {
    let ran = run(name, "", body);
    let stderr = String::from_utf8_lossy(&ran.stderr);
    assert!(
        !ran.status.success() && stderr.contains(&format!("panic: {code}:")),
        "{name}: expected {code}, got status {:?}, stdout {:?}, stderr {stderr}",
        ran.status,
        String::from_utf8_lossy(&ran.stdout),
    );
}

#[test]
fn integer_powers_compute_in_the_base_type() {
    let body = "\
println(i32(2) ** 10)
println(i32(3) ** 3 ** 2)
println(-i32(2) ** 2)
println(i32(-2) ** 3)
println(i32(-3) ** 4)
println(i64(10) ** 18)
println(u64(10) ** 19)
println(u8(2) ** 7)
println(i8(-2) ** 7)
println(i16(-2) ** 15)
println(i32(-2) ** 31)
println(i32(3) ** 19)
println(u32(3) ** 20)
println(i32(0) ** 5)
println(i32(1) ** 4000000000)
println(i32(-1) ** 4000000001)
println(2 ** 31)
";
    assert_eq!(
        prints("values", "", body),
        [
            "1024",
            "19683",
            "-4",
            "-8",
            "81",
            "1000000000000000000",
            "10000000000000000000",
            "128",
            "-128",
            "-32768",
            "-2147483648",
            "1162261467",
            "3486784401",
            "0",
            "1",
            "-1",
            "2147483648",
        ]
    );
}

#[test]
fn a_zero_exponent_gives_one() {
    let body = "\
println(i32(0) ** 0)
println(i32(7) ** 0)
println(i32(-7) ** 0)
println(i64(-9) ** 0)
println(u8(255) ** 0)
println(0.0 ** 0.0 == 1.0)
println(2.5 ** 0.0 == 1.0)
";
    assert_eq!(
        prints("zero", "", body),
        ["1", "1", "1", "1", "1", "true", "true"]
    );
}

#[test]
fn any_unsigned_exponent_type_is_accepted() {
    let items = "\
fn p8(b: i32, e: u8) -> i32: b ** e
fn p16(b: i32, e: u16) -> i32: b ** e
fn p32(b: i32, e: u32) -> i32: b ** e
fn p64(b: i32, e: u64) -> i32: b ** e
fn pz(b: i64, e: usize) -> i64: b ** e
";
    let body = "\
println(p8(2, 5))
println(p16(3, 4))
println(p32(5, 3))
println(p64(7, 2))
println(pz(10, 12))
println(p64(1, 18446744073709551615))
println(p64(0, 18446744073709551615))
";
    assert_eq!(
        prints("exponents", items, body),
        ["32", "81", "125", "49", "1000000000000", "1", "0"]
    );
}

#[test]
fn an_overflowing_integer_power_panics() {
    for (name, body) in [
        ("i32", "println(i32(2) ** 31)"),
        ("i32-neg", "println(i32(-2) ** 33)"),
        ("i32-big", "println(i32(10) ** 10)"),
        ("usize", "println(2 ** 32)"),
        ("u8", "println(u8(2) ** 8)"),
        ("u8-mid", "println(u8(16) ** 2)"),
        ("i8", "println(i8(2) ** 7)"),
        ("i16", "println(i16(3) ** 11)"),
        ("u32", "println(u32(3) ** 21)"),
        ("i64", "println(i64(2) ** 63)"),
        ("i64-neg", "println(i64(-3) ** 41)"),
        ("u64", "println(u64(2) ** 64)"),
        ("u64-big", "println(u64(10) ** 20)"),
        ("huge-exponent", "println(i32(2) ** 4000000000)"),
    ] {
        panics(name, body, "integer-overflow");
    }
}

#[test]
fn float_powers_follow_ieee_pow() {
    let items = "\
fn same(got: f64, want: f64) -> bool:
    if want != want:
        return got != got
    got == want && 1.0 / got == 1.0 / want
";
    let body = "\
inf := 1.0 / 0.0
nan := 0.0 / 0.0
zero := 0.0
nzero := -0.0
println(same(9.0 ** 0.5, 3.0))
println(same(2.0 ** 0.5, 1.4142135623730951))
println(same(2.0 ** 10.0, 1024.0))
println(same(40.0 ** 23.0, 7.0368744177664e36))
println(same(10.0 ** (-2.0), 0.01))
println(same(2.0 ** (-1074.0), 5e-324))
println(same(2.0 ** (-1075.0), 0.0))
println(same(2.0 ** 1024.0, inf))
println(same((-2.0) ** 3.0, -8.0))
println(same((-2.0) ** 0.5, nan))
println(same(nan ** zero, 1.0))
println(same(1.0 ** nan, 1.0))
println(same((-1.0) ** inf, 1.0))
println(same(zero ** (-1.0), inf))
println(same(nzero ** (-1.0), -inf))
println(same(nzero ** 3.0, -0.0))
println(same(nzero ** 2.0, 0.0))
println(same(inf ** (-1.0), 0.0))
println(same((-inf) ** 3.0, -inf))
println(same((-inf) ** (-3.0), -0.0))
println(same(0.5 ** inf, 0.0))
println(same(2.0 ** (-inf), 0.0))
println(same(nan ** 1.0, nan))
println(same(1.0000000000000002 ** 1e300, inf))
";
    let got = prints("floats", items, body);
    assert_eq!(got.len(), 24);
    assert!(got.iter().all(|line| line == "true"), "{got:?}");
}
