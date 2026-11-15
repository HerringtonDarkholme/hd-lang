//! Trailing-block calls (spec/lang/07-functions.md, Trailing Callback
//! Blocks; `crates/hd_tir/DESIGN.md`, "What The Checker Desugars"): the block is
//! a zero-argument closure typed by the callee's final parameter, passed
//! as one more final argument. Then top-level `it(...)` registrations of a
//! test module and an integration test file (spec/lang/10-modules.md,
//! Test Cases).

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
