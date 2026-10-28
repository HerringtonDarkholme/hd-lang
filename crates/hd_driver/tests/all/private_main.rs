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

fn codes(out: &Output) -> Vec<&str> {
    out.diags.code.iter().map(|c| c.as_str()).collect()
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

#[test]
fn private_main_is_quiet_in_analysis_and_rejected_as_a_program() {
    let text = "fn main() -> void:\n    pass\n";
    let analyzed = run(text, &Goal::Analyze);
    assert!(!analyzed.diags.has_errors(), "{}", analyzed.render());
    assert!(!codes(&analyzed).contains(&"private-main"));
    let program = run(
        text,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert_eq!(codes(&program), ["missing-entry-point"]);
    assert!(
        program
            .render()
            .contains("`app.main` has a private `main`; make it `pub`"),
        "{}",
        program.render()
    );
    assert!(program.wasm.is_none());
}

#[test]
fn entry_module_with_only_a_helper_builds_an_empty_script() {
    let program = run(
        "fn helper() -> i32:\n    1\n",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(!program.diags.has_errors(), "{}", program.render());
    assert!(program.wasm.is_some(), "{}", program.render());
}

#[test]
fn entry_module_with_only_statements_builds_a_script() {
    let program = run(
        "x := 40 + 2\nprintln(x)\n",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(!program.diags.has_errors(), "{}", program.render());
    assert!(program.wasm.is_some(), "{}", program.render());
}

#[test]
fn private_main_i32_is_rejected_before_entry_result_validation() {
    let program = run(
        "fn main() -> i32:\n    7\n",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert_eq!(codes(&program), ["missing-entry-point"]);
    assert!(
        !program.render().contains("unsupported"),
        "{}",
        program.render()
    );
}

#[test]
fn spacing_between_header_tokens_does_not_matter() {
    let program = run(
        "pub  fn  main ( ) -> void:\n    pass\n",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(!program.diags.has_errors(), "{}", program.render());
    assert!(program.wasm.is_some());
    assert!(!codes(&program).contains(&"private-main"));
    let private = run("fn  main() -> void:\n    pass\n", &Goal::Analyze);
    assert!(!codes(&private).contains(&"private-main"));
}

#[test]
fn public_main_is_still_the_entry() {
    let program = run(
        "pub fn main() -> void:\n    pass\n",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(!program.diags.has_errors(), "{}", program.render());
    assert!(program.wasm.is_some());
}
