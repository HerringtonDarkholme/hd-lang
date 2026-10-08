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
