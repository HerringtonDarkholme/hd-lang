//! Trailing-block calls (spec/lang/07-functions.md, Trailing Callback
//! Blocks; checking-and-tir.md, "What The Checker Desugars"): the block is
//! a zero-argument closure typed by the callee's final parameter, passed
//! as one more final argument. Then top-level `it(...)` registrations of a
//! test module and an integration test file (spec/lang/10-modules.md,
//! Test Cases).

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn build_files(files: &[(&str, &str)], goal: &Goal, render: &[&str]) -> Output {
    let mut src = MemorySources::default();
    for (path, text) in files {
        src.insert(path, text);
    }
    let store = MemoryStore::default();
    let host = Host {
        render_tir: render,
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", goal)
}

fn errors(main: &str) -> Vec<Code> {
    let out = build_files(&[("main.hd", main)], &Goal::Analyze, &[]);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

/// Runs a built program on V8 and returns its standard output.
fn run(out: &Output, name: &str) -> String {
    let wasm = out.wasm.as_ref().expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("trailing-{name}.wasm"));
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
    String::from_utf8(ran.stdout).expect("UTF-8")
}

/// A trailing block as a statement, on the right of `:=`, after ordinary
/// arguments (`fn.trailing.arguments`), with `()` omitted
/// (`fn.trailing.empty-parentheses`), and with a `return` that leaves only
/// the callback (`fn.trailing.return`). The block runs only when the
/// callee calls it.
#[test]
fn trailing_blocks_run_as_closures() {
    let main = "fn run(body: fn() -> void $ Console) -> void $ Console:
    body()

fn measure(body: fn() -> i32) -> i32:
    body() * 2

fn add(base: i32, times: i32, body: fn() -> i32) -> i32:
    let total = base
    for _ in 0..times:
        total = total + body()
    total

fn skip(body: fn() -> void $ Console) -> void:
    pass

fn pick(flag: bool) -> i32:
    value := measure:
        if flag:
            return 20
        1
    value + 2

pub fn main() -> void $ Console:
    run:
        println(\"statement\")
    skip:
        println(\"not called\")
    x := measure:
        5
    println(x)
    println(add(1, 3):
        10
    )
";
    // A trailing block inside parentheses is not a trailing-block call.
    assert_eq!(
        errors(main).first().copied(),
        Some(Code::TrailingBlockPosition),
        "a trailing block inside brackets"
    );
    let main = main.replace(
        "    println(add(1, 3):\n        10\n    )\n",
        "    y := add(1, 3):\n        10\n    println(y)\n    println(pick(true))\n    println(pick(false))\n",
    );
    let out = build_files(
        &[("main.hd", &main)],
        &Goal::Program {
            entry: "main".into(),
        },
        &[],
    );
    assert!(out.diags.is_empty(), "{}", out.render());
    assert_eq!(run(&out, "closures"), "statement\n10\n31\n42\n4\n");
}

/// A trailing block for an `fn!` parameter is a suspending closure, so it
/// may make bang calls (`fn.trailing.suspending`,
/// `fn.trailing.suspending.body`); one for a plain `fn` parameter may not.
#[test]
fn a_suspending_parameter_makes_the_block_suspend() {
    let main = "fn fetch_count!() -> i32: 3

fn twice!(body: fn!() -> i32) -> i32:
    body!() + body!()

fn once(body: fn() -> i32) -> i32:
    body()

fn total!() -> i32:
    count := twice!():
        fetch_count!()
    count
";
    let out = build_files(&[("main.hd", main)], &Goal::Analyze, &["app/main/total"]);
    assert!(out.diags.is_empty(), "{}", out.render());
    let tir = out.tir_text.get("app/main/total").expect("rendered total");
    assert!(tir.contains("Closure"), "{tir}");
    let plain = main.replace("count := twice!():", "count := once:");
    assert_eq!(errors(&plain), [Code::BangCallOutsideSuspension]);
}

const LIB: &str = "pub fn double(value: i32) -> i32:
    value * 2
";

const UNIT: &str = "use pkg.{double}
use std.testing.assert_equal

it(\"doubles\"):
    assert_equal(double(2), 4, reason=\"double doubles\")

it(\"ignored\", ignore=\"not today\"):
    assert_equal(double(2), 5, reason=\"never runs\")
";

const INTEGRATION: &str = "use pkg.{double}
use std.testing.assert_equal

it(\"prints through the profile\"):
    println(double(21))
";

/// A test module's and an integration test file's top-level `it(...)`
/// calls register test cases, as a `tests:` block's do
/// (`module.testing.test-position`). An integration test case gets the
/// profile's host traits (`module.testing.integration-row`); a unit test
/// case does not (`module.testing.unit-row.test-runner`).
#[test]
fn top_level_registrations_are_test_cases() {
    let files = [
        ("src/lib.hd", LIB),
        ("src/lib_test.hd", UNIT),
        ("tests/flow.hd", INTEGRATION),
    ];
    let goal = Goal::Tests {
        module: None,
        filter: None,
    };
    let out = build_files(&files, &goal, &[]);
    assert!(out.diags.is_empty(), "{}", out.render());
    let got: Vec<(&str, Option<&str>, bool)> = out
        .tests
        .iter()
        .map(|c| (c.name.as_str(), c.ignore.as_deref(), c.run.is_some()))
        .collect();
    assert_eq!(
        got,
        [
            ("doubles", None, true),
            ("ignored", Some("not today"), false),
            ("prints through the profile", None, true),
        ]
    );
    assert!(out.wasm.is_some());
    let unit = UNIT.replace(
        "assert_equal(double(2), 4, reason=\"double doubles\")",
        "println(double(2))",
    );
    let out = build_files(
        &[("src/lib.hd", LIB), ("src/lib_test.hd", &unit)],
        &goal,
        &[],
    );
    let codes: Vec<Code> = out
        .diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect();
    assert_eq!(codes, [Code::MissingRequirement], "{}", out.render());
}
