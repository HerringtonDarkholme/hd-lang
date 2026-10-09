//! Range patterns (`flow.match.range.*`): the open forms `a..`, `..b`,
//! `..=b` check like the closed ones, cover an integer type together with
//! literal patterns, and compile to bound comparisons; the full-range
//! expression `..` builds `RangeFull {}`.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn program(main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

/// The codes of the diagnostics `items` produces, with an empty `main`.
fn codes_of(items: &str) -> Vec<Code> {
    let out = program(&format!(
        "{items}\npub fn main() -> void $ Console:\n    println(0)\n"
    ));
    out.diags.code.clone()
}

#[test]
fn open_ranges_and_literals_cover_an_integer_type() {
    let ok = "fn f(n: i8) -> i32:\n    match n:\n        ..=-1 => -1\n        0 => 0\n        1.. => 1\n";
    assert_eq!(codes_of(ok), vec![]);
    let halves = "fn f(n: u8) -> i32:\n    match n:\n        ..100 => 0\n        100.. => 1\n";
    assert_eq!(codes_of(halves), vec![]);
    // Overlap in part is allowed (`flow.match.range.overlap`).
    let overlap = "fn f(n: u8) -> i32:\n    match n:\n        0..=200 => 0\n        100.. => 1\n";
    assert_eq!(codes_of(overlap), vec![]);
    // The whole 64-bit range, and a range under a variant and a tuple.
    let wide = "fn f(n: u64) -> i32:\n    match n:\n        ..=5 => 0\n        6.. => 1\n";
    assert_eq!(codes_of(wide), vec![]);
    let nested = "fn f(t: (u8, i8?)) -> i32:\n    match t:\n        (0, .Some(..0)) => 0\n        (0, _) => 1\n        (1.., .Some(0..)) => 2\n        (1.., _) => 3\n";
    assert_eq!(codes_of(nested), vec![]);
}

#[test]
fn a_gap_in_the_ranges_is_nonexhaustive() {
    let gap = "fn f(n: u8) -> i32:\n    match n:\n        0..=127 => 0\n        129.. => 1\n";
    assert_eq!(codes_of(gap), vec![Code::NonexhaustiveMatch]);
    let below = "fn f(n: i8) -> i32:\n    match n:\n        ..0 => 0\n        1.. => 1\n";
    assert_eq!(codes_of(below), vec![Code::NonexhaustiveMatch]);
    // A guarded arm covers nothing (`flow.match.guard.coverage`).
    let guarded = "fn f(n: u8, g: bool) -> i32:\n    match n:\n        0.. if g => 0\n";
    assert_eq!(codes_of(guarded), vec![Code::NonexhaustiveMatch]);
}

#[test]
fn an_arm_the_earlier_arms_cover_is_unreachable() {
    let covered = "fn f(n: i32) -> i32:\n    match n:\n        ..10 => 0\n        3..=5 => 1\n        _ => 2\n";
    assert_eq!(codes_of(covered), vec![Code::UnreachableMatchArm]);
    let after = "fn f(n: u8) -> i32:\n    match n:\n        0.. => 0\n        7 => 1\n";
    assert_eq!(codes_of(after), vec![Code::UnreachableMatchArm]);
    let literals =
        "fn f(n: u8) -> i32:\n    match n:\n        1 => 0\n        1 => 1\n        _ => 2\n";
    assert_eq!(codes_of(literals), vec![Code::UnreachableMatchArm]);
    // A guarded arm does not cover its values for later arms.
    let guarded =
        "fn f(n: u8, g: bool) -> i32:\n    match n:\n        0.. if g => 0\n        0.. => 1\n";
    assert_eq!(codes_of(guarded), vec![]);
}

#[test]
fn an_empty_range_is_an_unreachable_arm() {
    for pat in ["5..5", "3..=1", "..0"] {
        let src =
            format!("fn f(n: u8) -> i32:\n    match n:\n        {pat} => 0\n        _ => 1\n");
        assert_eq!(codes_of(&src), vec![Code::UnreachableMatchArm], "{pat}");
    }
}

#[test]
fn a_non_integer_subject_is_a_type_mismatch() {
    for ty in ["string", "f64", "bool"] {
        let src =
            format!("fn f(n: {ty}) -> i32:\n    match n:\n        1.. => 0\n        _ => 1\n");
        assert_eq!(codes_of(&src), vec![Code::TypeMismatch], "{ty}");
    }
}

#[test]
fn a_bound_outside_the_subject_type_is_an_error() {
    let wide = "fn f(n: u8) -> i32:\n    match n:\n        0..=300 => 0\n";
    assert_eq!(codes_of(wide), vec![Code::IntegerLiteralRange]);
    let open = "fn f(n: u8) -> i32:\n    match n:\n        300.. => 0\n        _ => 1\n";
    assert_eq!(codes_of(open), vec![Code::IntegerLiteralRange]);
    let negative = "fn f(n: u8) -> i32:\n    match n:\n        ..=-1 => 0\n        _ => 1\n";
    assert_eq!(codes_of(negative), vec![Code::IntegerLiteralRange]);
    // A literal pattern is checked the same way (`types.literal.int-range`).
    let literal = "fn f(n: u8) -> i32:\n    match n:\n        300 => 0\n        _ => 1\n";
    assert_eq!(codes_of(literal), vec![Code::IntegerLiteralRange]);
    let signed = "fn f(n: i8) -> i32:\n    match n:\n        -129.. => 0\n";
    assert_eq!(codes_of(signed), vec![Code::IntegerLiteralRange]);
}

#[test]
fn the_full_range_is_a_range_full_value() {
    let src = "use std.ops.RangeFull\n\nfn f() -> RangeFull:\n    ..\n";
    assert_eq!(codes_of(src), vec![]);
}

/// Builds `items`, runs it on V8, and returns the lines it prints.
fn prints(name: &str, items: &str) -> Vec<String> {
    let out = program(items);
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("range-patterns-{name}.wasm"));
    std::fs::write(&path, out.wasm.expect("wasm")).expect("write wasm");
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
    String::from_utf8(ran.stdout)
        .expect("UTF-8")
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn open_range_arms_select_the_first_match_at_run_time() {
    let items = "\
fn bucket(n: i32) -> string:
    match n:
        ..=-1 => \"negative\"
        0 => \"zero\"
        1..10 => \"small\"
        10..=99 => \"medium\"
        100.. => \"large\"

fn first(n: u8) -> string:
    match n:
        0..=200 => \"low\"
        100.. => \"high\"

fn edge(n: i64) -> string:
    match n:
        ..-9_000_000_000 => \"below\"
        -9_000_000_000..=9_000_000_000 => \"inside\"
        9_000_000_001.. => \"above\"

fn pair(t: (u8, i8)) -> string:
    match t:
        (0, ..0) => \"zero-neg\"
        (0, _) => \"zero\"
        (1.., 0..) => \"pos-pos\"
        (_, ..=-1) => \"other-neg\"

pub fn main() -> void $ Console:
    for n in [-5, -1, 0, 1, 9, 10, 99, 100, 1000]:
        println(bucket(n))
    println(first(150))
    println(first(201))
    println(edge(-9_000_000_001))
    println(edge(-9_000_000_000))
    println(edge(9_000_000_000))
    println(edge(9_000_000_001))
    println(pair((0, -3)))
    println(pair((0, 3)))
    println(pair((7, 0)))
    println(pair((7, -1)))
    full := ..
    println(full == RangeFull {})
";
    let items = format!("use std.ops.RangeFull\n\n{items}");
    let got = prints("open", &items);
    let want = [
        "negative",
        "negative",
        "zero",
        "small",
        "small",
        "medium",
        "medium",
        "large",
        "large",
        "low",
        "high",
        "below",
        "inside",
        "inside",
        "above",
        "zero-neg",
        "zero",
        "pos-pos",
        "other-neg",
        "true",
    ];
    assert_eq!(got, want);
}
