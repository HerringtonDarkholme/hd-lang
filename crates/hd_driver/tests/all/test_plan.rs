//! `Goal::Tests` (`docs/compiler/engines-and-test-runner.md` §19.1, §19.2): the test plan
//! lists the cases in content order with their exports, a second run is
//! warm (no module checked), and the serial and pool executors build the
//! same program. `--filter` is the stage case `test-plan/filter-selects-by-name`.

use hd_base::SliceSort;
use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;
use std::fmt::Write;

const MATH: &str = "\
use std.testing.{assert, assert_equal}

fn add(a: i32, b: i32) -> i32: a + b

tests:
    it(\"adds\"):
        assert_equal(add(2, 3), 5, reason=\"small sums\")

    it(\"skips\", ignore=\"slow\"):
        assert(false, reason=\"never runs\")

    it(\"times out\", timeout=.None):
        assert(true, reason=\"no limit\")
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

fn run(store: &MemoryStore, executor: Executor) -> Output {
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
            filter: None,
        },
    )
}

/// A listed case: module, name, line and exports.
type Listed<'a> = (&'a str, &'a str, u32, Option<(u32, u32)>);

const SERIAL: Executor = Executor::Serial(SerialOrder::Priority);

#[test]
fn lists_cases_in_content_order_with_their_exports() {
    let out = run(&MemoryStore::default(), SERIAL);
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
            ("app.math", "times out", 12, Some((1, 0))),
            ("app.text", "compares", 4, Some((2, 1))),
        ]
    );
    assert_eq!(out.tests[1].ignore.as_deref(), Some("slow"));
    // A `timeout` runs: the runner checks it (`std-testing.runner.timeout`).
    assert_eq!(out.tests[2].unsupported, None);
    assert!(out.wasm.is_some());
}

/// A comment edit keeps the `check` entries, and each case's line follows
/// the edit.
#[test]
fn a_comment_edit_moves_case_lines_without_a_recheck() {
    let store = MemoryStore::default();
    let cold = run(&store, SERIAL);
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
fn a_second_run_is_warm_and_executors_agree() {
    let store = MemoryStore::default();
    let cold = run(&store, SERIAL);
    let warm = run(&store, SERIAL);
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
    let pool = run(&MemoryStore::default(), Executor::Pool(4));
    assert_eq!(pool.wasm, cold.wasm);
    assert_eq!(pool.tests, cold.tests);
}

/// Dropping a case's `ignore` changes no body's TIR, but the case now
/// runs: the program links again with an export for it (#316).
#[test]
fn an_ignore_edit_relinks() {
    let store = MemoryStore::default();
    let cold = run(&store, SERIAL);
    assert_eq!(cold.tests[1].run, None);
    let mut src = sources();
    src.insert(
        "math.hd",
        &MATH.replace("it(\"skips\", ignore=\"slow\")", "it(\"skips\")"),
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
    assert_eq!(warm.counters.miss("link"), 1);
    assert_eq!(warm.tests[1].run, Some((1, 0)));
}

/// The exported functions, the defined functions and the most arms of any
/// one function's `br_table`s of a test program with `k` cases in one
/// module and one in another (#1902's generator).
fn entry_counts(k: usize) -> (Vec<String>, u32, usize) {
    let mut main = String::from(
        "use std.testing.assert_equal\n\nfn edit_me(x: i32) -> i32: x * 3 + 1\n\ntests:\n",
    );
    for i in 0..k {
        writeln!(
            main,
            "    it(\"case {i}\"):\n        assert_equal(edit_me(+{i}), +{i} * 3 + 1)"
        )
        .unwrap();
    }
    let mut src = MemorySources::default();
    src.insert("main.hd", &main);
    src.insert("text.hd", TEXT);
    let store = MemoryStore::default();
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
    let out = build(&host, "app", &goal);
    assert!(out.diags.is_empty(), "{}", out.render());
    assert_eq!(out.tests.len(), k + 1);
    assert_eq!(out.tests[k].run, Some((u32::try_from(k).unwrap(), 1)));
    let wasm = out.wasm.expect("a test program");
    wasmparser::validate(&wasm).expect("a valid module");
    let mut exports = Vec::new();
    let mut functions = 0;
    let mut widest = 0;
    for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
        match payload.expect("payload") {
            wasmparser::Payload::ExportSection(r) => {
                for e in r {
                    let e = e.expect("export");
                    if e.kind == wasmparser::ExternalKind::Func {
                        exports.push(e.name.to_owned());
                    }
                }
            }
            wasmparser::Payload::FunctionSection(r) => functions = r.count(),
            wasmparser::Payload::CodeSectionEntry(body) => {
                let mut ops = body.get_operators_reader().expect("operators");
                let mut arms = 0;
                while !ops.eof() {
                    if let wasmparser::Operator::BrTable { targets } = ops.read().expect("op") {
                        arms += usize::try_from(targets.len()).unwrap();
                    }
                }
                widest = widest.max(arms);
            }
            _ => {}
        }
    }
    exports.sort_stable();
    (exports, functions, widest)
}

/// An engine that builds each instance's exports, and its functions, per
/// instantiate does constant export work per case: a test program
/// exports two entries however many cases it has, which dispatch by
/// number, and each case adds one function, its body (#1902). The
/// dispatch splits into functions of at most [`hd_wasm::DISPATCH_ARMS`]
/// arms, one per run of 256 cases here, so no one function grows with the
/// cases, which an engine's optimizing compile is superlinear in (#2049).
#[test]
fn a_test_program_exports_two_entries_and_a_function_per_case() {
    let arms = hd_wasm::DISPATCH_ARMS;
    let (small, at_small, widest_small) = entry_counts(2_000);
    let (large, at_large, widest_large) = entry_counts(20_000);
    assert_eq!(small, ["hd.test", "hd.test.init"]);
    assert_eq!(large, small);
    let parts = |k: u32| k.div_ceil(u32::try_from(arms).unwrap());
    assert_eq!(at_large - at_small, 18_000 + parts(20_000) - parts(2_000));
    assert!(widest_small <= arms, "{widest_small} arms at 2,000 cases");
    assert!(widest_large <= arms, "{widest_large} arms at 20,000 cases");
}
