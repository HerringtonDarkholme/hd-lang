//! `module.entry.private-main`: a `main` that is not `pub` is an ordinary
//! function during analysis and tests, but not a runnable entry point.

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn run(text: &str, goal: &Goal) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", text);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", goal)
}

const PRIVATE_I32: &str = "\
use std.testing.assert_equal

fn main() -> i32:
    7

tests:
    it(\"calls the ordinary function\"):
        assert_equal(main(), 7, reason=\"private main\")
";

#[test]
fn private_main_with_i32_result_runs_from_a_test() {
    let out = run(
        PRIVATE_I32,
        &Goal::Tests {
            module: None,
            filter: None,
        },
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    assert!(out.wasm.is_some(), "{}", out.render());
}
