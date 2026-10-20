//! The release profile (`cli.profile.release`, `types.arith.release`):
//! `+`, `-`, `*`, `**` and unary `-` wrap at every integer width, and a
//! shift count is masked to the width (`types.arith.shift-count.release`).
//! Division, remainder and `from_i64` behave as in a checked build
//! (`types.arith.always`, `trait.num.from-i64-checked`). The profile is in
//! every code and program key, so debug and release builds of one program
//! never share code; a test run stays checked (`cli.profile.test`).
//! Programs build through the driver and run on V8 (`host/run.mjs`).

use std::path::Path;
use std::process::{Command, Output as Run};

use hd_cache::MemoryStore;
use hd_driver::{BuildProfile, Executor, Goal, Host, NoClock, Output, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;
use hd_testkit::Scratch;

fn build_in(
    store: &MemoryStore,
    src: &MemorySources,
    goal: &Goal,
    profile: BuildProfile,
) -> Output {
    let host = Host {
        render_tir: &[],
        sources: src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        build_profile: profile,
        ..Packages::default()
    };
    let out = build_packages(&host, "app", &packages, goal);
    assert!(out.diags.is_empty(), "{}", out.render());
    out
}

fn program(store: &MemoryStore, src: &MemorySources, profile: BuildProfile) -> Output {
    let goal = Goal::Program {
        entry: "main".into(),
    };
    build_in(store, src, &goal, profile)
}

fn run_wasm(name: &str, wasm: &[u8]) -> Run {
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("release-profile-{name}.wasm"),
    );
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    ran
}

fn stdout(name: &str, ran: &Run) -> String {
    assert!(
        ran.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8(ran.stdout.clone()).expect("UTF-8")
}

fn panics(name: &str, ran: &Run, category: &str) {
    let stderr = String::from_utf8_lossy(&ran.stderr);
    assert!(
        !ran.status.success() && stderr.contains(&format!("panic: {category}:")),
        "{name}: expected {category}, got status {:?}, stderr {stderr}",
        ran.status
    );
}

fn one_file(text: &str) -> MemorySources {
    let mut src = MemorySources::default();
    src.insert("main.hd", text);
    src
}

const NEXT: &str = "\
fn next(value: i32) -> i32:
    value + 1

pub fn main() -> void $ Console:
    println(next(2147483647))
";

/// Debug, then release, then debug again from one store: each build runs
/// its own profile's code, and the second debug build reads the first's.
#[test]
fn debug_and_release_builds_never_share_code() {
    let store = MemoryStore::default();
    let src = one_file(NEXT);
    let debug = program(&store, &src, BuildProfile::Checked);
    let debug_wasm = debug.wasm.expect("wasm");
    panics("debug", &run_wasm("debug", &debug_wasm), "integer-overflow");

    let release = program(&store, &src, BuildProfile::Release);
    assert_eq!(
        release.counters.miss("link"),
        1,
        "release has its own program key"
    );
    let release_wasm = release.wasm.expect("wasm");
    assert_ne!(release_wasm, debug_wasm);
    assert_eq!(
        stdout("release", &run_wasm("release", &release_wasm)),
        "-2147483648\n"
    );

    let again = program(&store, &src, BuildProfile::Checked);
    assert_eq!(again.counters.hit("link"), 1);
    let again_wasm = again.wasm.expect("wasm");
    assert_eq!(again_wasm, debug_wasm);
    panics(
        "debug-again",
        &run_wasm("debug-again", &again_wasm),
        "integer-overflow",
    );

    let release_again = program(&store, &src, BuildProfile::Release);
    assert_eq!(release_again.counters.hit("link"), 1);
    assert_eq!(release_again.wasm.as_deref(), Some(release_wasm.as_slice()));
}

const WIDTHS: &str = "\
fn add8(a: i8, b: i8) -> i8: a + b
fn sub8u(a: u8, b: u8) -> u8: a - b
fn mul16(a: i16, b: i16) -> i16: a * b
fn add16u(a: u16, b: u16) -> u16: a + b
fn neg32(a: i32) -> i32: -a
fn mul32u(a: u32, b: u32) -> u32: a * b
fn add64(a: i64, b: i64) -> i64: a + b
fn sub64u(a: u64, b: u64) -> u64: a - b
fn mul64(a: i64, b: i64) -> i64: a * b
fn neg64(a: i64) -> i64: -a
fn sizes(a: usize, b: usize) -> usize: a - b
fn pow8(a: i8, e: u32) -> i8: a ** e
fn pow32(a: i32, e: u32) -> i32: a ** e
fn pow64u(a: u64, e: u32) -> u64: a ** e
fn shl8(a: u8, n: u32) -> u8: a << n
fn shr16(a: i16, n: u32) -> i16: a >> n
fn shl32(a: i32, n: u64) -> i32: a << n
fn shl64(a: i64, n: u8) -> i64: a << n
fn rem32(a: i32, b: i32) -> i32: a % b

pub fn main() -> void $ Console:
    println(add8(127, 1))
    println(sub8u(0, 1))
    println(mul16(300, 300))
    println(add16u(65535, 2))
    println(neg32(-2147483648))
    println(mul32u(65536, 65537))
    println(add64(9223372036854775807, 1))
    println(sub64u(0, 1))
    println(mul64(4294967296, 4294967297))
    println(neg64(-9223372036854775807 - 1))
    println(sizes(0, 1))
    println(pow8(3, 5))
    println(pow32(3, 40))
    println(pow64u(3, 50))
    println(pow32(2, 10))
    println(shl8(1, 9))
    println(shr16(-256, 17))
    println(shl32(1, 33))
    println(shl64(1, 65))
    println(rem32(-2147483648, -1))
";

#[test]
fn release_arithmetic_wraps_at_every_width() {
    let store = MemoryStore::default();
    let out = program(&store, &one_file(WIDTHS), BuildProfile::Release);
    let printed = stdout("widths", &run_wasm("widths", &out.wasm.expect("wasm")));
    let want = [
        "-128",
        "255",
        "24464",
        "1",
        "-2147483648",
        "65536",
        "-9223372036854775808",
        "18446744073709551615",
        "4294967296",
        "-9223372036854775808",
        "4294967295",
        "-13",
        "689956897",
        "6048575297968530377",
        "1024",
        "2",
        "-128",
        "2",
        "2",
        "0",
    ];
    assert_eq!(printed.lines().collect::<Vec<_>>(), want);
    // The same program panics in a debug build.
    let debug = program(&store, &one_file(WIDTHS), BuildProfile::Checked);
    panics(
        "widths-debug",
        &run_wasm("widths-debug", &debug.wasm.expect("wasm")),
        "integer-overflow",
    );
}

/// What a release build keeps checked.
#[test]
fn release_keeps_the_checks_of_every_build() {
    let cases = [
        (
            "min-div",
            "fn f(a: i32, b: i32) -> i32: a / b",
            "f(-2147483648, -1)",
            "integer-overflow",
        ),
        (
            "min-div-64",
            "fn f(a: i64, b: i64) -> i64: a / b",
            "f(-9223372036854775807 - 1, -1)",
            "integer-overflow",
        ),
        (
            "min-div-8",
            "fn f(a: i8, b: i8) -> i8: a / b",
            "f(-128, -1)",
            "integer-overflow",
        ),
        (
            "div-zero",
            "fn f(a: i32, b: i32) -> i32: a / b",
            "f(1, 0)",
            "integer-division-by-zero",
        ),
        (
            "from-i64",
            "use std.num.Num\n\nfn f[N < Num](n: i64) -> N:\n    N::from_i64(n)",
            "f::[i8](300)",
            "integer-overflow",
        ),
    ];
    let store = MemoryStore::default();
    for (name, items, call, category) in cases {
        let text = format!("{items}\n\npub fn main() -> void $ Console:\n    println({call})\n");
        let out = program(&store, &one_file(&text), BuildProfile::Release);
        panics(name, &run_wasm(name, &out.wasm.expect("wasm")), category);
    }
}

/// A test run is checked whatever profile it is given, so it shares the
/// checked build's program key.
#[test]
fn a_test_run_keeps_the_test_profile() {
    let text = format!("{NEXT}\ntests:\n    it(\"adds one\"):\n        next(2147483647)\n");
    let src = one_file(&text);
    let store = MemoryStore::default();
    let goal = Goal::Tests {
        module: None,
        filter: None,
    };
    let checked = build_in(&store, &src, &goal, BuildProfile::Checked);
    assert!(checked.wasm.is_some());
    let release = build_in(&store, &src, &goal, BuildProfile::Release);
    assert_eq!(release.counters.hit("link"), 1);
    assert_eq!(release.wasm, checked.wasm);
}
