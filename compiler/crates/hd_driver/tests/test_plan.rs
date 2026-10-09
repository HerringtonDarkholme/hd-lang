//! `Goal::Tests` (engines-and-test-runner.md §19.1, §19.2): the test plan
//! lists the cases in content order with their exports, `--filter`
//! selects by name, a second run is warm (no module checked, no instance
//! emitted), and the serial and pool executors build the same program.

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const MATH: &str = "\
use std.testing.{assert, assert_equal}

fn add(a: i32, b: i32) -> i32: a + b

tests:
    it(\"adds\"):
        assert_equal(add(2, 3), 5, reason=\"small sums\")

    it(\"skips\", ignore=\"slow\"):
        assert(false, reason=\"never runs\")

    it(\"times out\", timeout=.None):
        assert(true, reason=\"phase 2\")
";

const TEXT: &str = "\
use std.testing.assert

tests:
    it(\"compares\"):
        assert(\"a\" == \"a\", reason=\"equal text\")
";

fn sources() -> MemorySources {
    let mut s = MemorySources::default();
    s.insert("math.hd", MATH);
    s.insert("text.hd", TEXT);
    s
}

fn run(store: &MemoryStore, executor: Executor, filter: Option<&str>) -> Output {
    let src = sources();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
        clock: &NoClock,
        executor,
    };
    build(
        &host,
        "app",
        &Goal::Tests {
            module: None,
            filter: filter.map(str::to_owned),
        },
    )
}

/// A listed case: module, name, line and exports.
type Listed<'a> = (&'a str, &'a str, u32, Option<(u32, u32)>);

const SERIAL: Executor = Executor::Serial(SerialOrder::Priority);

#[test]
fn lists_cases_in_content_order_with_their_exports() {
    let out = run(&MemoryStore::default(), SERIAL, None);
    assert!(out.diags.is_empty(), "{}", out.render());
    let got: Vec<Listed<'_>> = out
        .tests
        .iter()
        .map(|c| (c.module.as_str(), c.name.as_str(), c.line, c.run))
        .collect();
    assert_eq!(
        got,
        [
            ("app.math", "adds", 6, Some((0, 0))),
            ("app.math", "skips", 9, None),
            ("app.math", "times out", 12, None),
            ("app.text", "compares", 4, Some((1, 1))),
        ]
    );
    assert_eq!(out.tests[1].ignore.as_deref(), Some("slow"));
    assert_eq!(
        out.tests[2].unsupported.as_deref(),
        Some("the `timeout` option")
    );
    assert!(out.wasm.is_some());
}

/// A comment edit keeps the `check` entries, and each case's line follows
/// the edit.
#[test]
fn a_comment_edit_moves_case_lines_without_a_recheck() {
    let store = MemoryStore::default();
    let cold = run(&store, SERIAL, None);
    let mut src = sources();
    src.insert(
        "math.hd",
        &MATH.replace("tests:\n", "# cases\n\ntests:\n    # first\n"),
    );
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: SERIAL,
    };
    let goal = Goal::Tests {
        module: None,
        filter: None,
    };
    let warm = build(&host, "app", &goal);
    assert!(
        !warm
            .counters
            .modules_checked
            .iter()
            .any(|m| m.starts_with("app.")),
        "{:?}",
        warm.counters.modules_checked
    );
    let lines = |o: &Output| o.tests.iter().map(|c| c.line).collect::<Vec<_>>();
    assert_eq!(lines(&cold), [6, 9, 12, 4]);
    assert_eq!(lines(&warm), [9, 12, 15, 4]);
    assert_eq!(warm.wasm, cold.wasm);
}

#[test]
fn filter_selects_by_name() {
    let out = run(&MemoryStore::default(), SERIAL, Some("comp"));
    let names: Vec<&str> = out.tests.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["compares"]);
    assert_eq!(out.tests[0].run, Some((0, 0)));
}

#[test]
fn a_second_run_is_warm_and_executors_agree() {
    let store = MemoryStore::default();
    let cold = run(&store, SERIAL, None);
    assert!(cold.counters.emitted > 0);
    let warm = run(&store, SERIAL, None);
    assert_eq!(warm.counters.emitted, 0);
    assert!(
        !warm
            .counters
            .modules_checked
            .iter()
            .any(|m| m.starts_with("app.")),
        "{:?}",
        warm.counters.modules_checked
    );
    assert_eq!(warm.wasm, cold.wasm);
    assert_eq!(warm.tests, cold.tests);
    let pool = run(&MemoryStore::default(), Executor::Pool(4), None);
    assert_eq!(pool.wasm, cold.wasm);
    assert_eq!(pool.tests, cold.tests);
}
