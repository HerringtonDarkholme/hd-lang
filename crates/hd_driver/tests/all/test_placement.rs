//! Test-code placement errors (spec 10 "Test Cases", "Test Modules"): a
//! registration call outside test position is `misplaced-test-case`, a
//! non-registration statement in test position is
//! `invalid-test-statement`, and non-test code using a test module is
//! `test-only-use`. (`misplaced-tests-block` lives in
//! `tests_block_shape.rs`.)

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn check(main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let goal = if main.starts_with("tests:") {
        Goal::CheckTests
    } else {
        Goal::Analyze
    };
    build(&host, "app", &goal)
}

fn check_files(files: &[(&str, &str)]) -> Output {
    let mut src = MemorySources::default();
    for (path, text) in files {
        src.insert(path, text);
    }
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build_packages(
        &host,
        "app",
        &hd_driver::Packages::default(),
        &Goal::Analyze,
    )
}

fn errors(out: &Output) -> Vec<Code> {
    out.diags
        .code
        .iter()
        .copied()
        .filter(|c| *c != Code::Unsupported)
        .collect()
}

const USE_IT: &str = "use std.testing.{it}\n\n";

#[test]
fn registration_call_in_a_function_is_misplaced() {
    let main = format!("{USE_IT}fn helper() -> void:\n    it(\"nested\"):\n        pass\n");
    assert_eq!(errors(&check(&main)), vec![Code::MisplacedTestCase]);
}

#[test]
fn registration_function_as_a_value_is_misplaced() {
    let main = format!("{USE_IT}fn register() -> void:\n    make := it\n    _ := make\n");
    assert_eq!(errors(&check(&main)), vec![Code::MisplacedTestCase]);
}

#[test]
fn non_registration_statement_in_test_position_is_invalid() {
    let main = "tests:\n    let shared: i32 = 0\n\n    it(\"counts\"):\n        pass\n";
    assert_eq!(errors(&check(main)), vec![Code::InvalidTestStatement]);
}

#[test]
fn non_test_code_using_a_test_module_is_test_only_use() {
    let out = check_files(&[
        (
            "src/main.hd",
            "use self.helper_test.{helper}\n\npub fn main() -> void:\n    helper()\n",
        ),
        ("src/helper_test.hd", "pub fn helper() -> void:\n    pass\n"),
    ]);
    assert_eq!(errors(&out), vec![Code::TestOnlyUse]);
}
