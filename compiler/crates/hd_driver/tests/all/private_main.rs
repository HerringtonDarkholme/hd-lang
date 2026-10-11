//! `module.entry.private-main` and `module.entry.private-main.warn`: a
//! `main` that is not `pub` is an ordinary function. It is no entry point,
//! so its result owes no `Termination`, and it only earns a warning.

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
fn private_main_alone_warns_and_is_no_entry() {
    let text = "fn main() -> void:\n    pass\n";
    let analyzed = run(text, &Goal::Analyze);
    assert!(!analyzed.diags.has_errors(), "{}", analyzed.render());
    assert!(
        codes(&analyzed).contains(&"private-main"),
        "{}",
        analyzed.render()
    );
    let program = run(
        text,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(
        codes(&program).contains(&"missing-entry-point"),
        "{}",
        program.render()
    );
    assert!(program.wasm.is_none());
}

#[test]
fn private_main_i32_in_a_program_owes_no_termination() {
    let program = run(
        "fn main() -> i32:\n    7\n",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert_eq!(
        codes(&program)
            .iter()
            .filter(|c| **c == "missing-entry-point")
            .count(),
        1,
        "{}",
        program.render()
    );
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
    assert!(
        codes(&private).contains(&"private-main"),
        "{}",
        private.render()
    );
}

#[test]
fn a_suspending_private_main_warns() {
    let out = run("fn main!() -> void:\n    pass\n", &Goal::Analyze);
    assert!(codes(&out).contains(&"private-main"), "{}", out.render());
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
