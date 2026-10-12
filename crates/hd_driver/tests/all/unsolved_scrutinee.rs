//! A variant pattern on a value whose type is still unsolved reports one
//! `cannot-infer-type` at the pattern; its names bind to the poison type,
//! so nothing downstream reports again.

use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn analyze(main: &str) -> Output {
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
    build(&host, "app", &Goal::Analyze)
}

/// The codes of a run's errors.
fn codes(out: &Output) -> Vec<&'static str> {
    (0..out.diags.len())
        .filter(|&i| out.diags.severity[i] == hd_diag::Severity::Error)
        .map(|i| out.diags.code[i].as_str())
        .collect()
}

const MAKE: &str = "fn make[T]() -> T:\n    panic(\"no\")\n\n";

#[test]
fn an_unsolved_match_scrutinee_reports_once() {
    let src = format!(
        "{MAKE}fn main() -> void $ Console:\n    match make():\n        .Some(x) => println(x)\n        .None => println(0)\n"
    );
    assert_eq!(codes(&analyze(&src)), ["cannot-infer-type"]);
}

#[test]
fn nested_names_bind_without_a_second_error() {
    let src = format!(
        "{MAKE}fn main() -> void $ Console:\n    match make():\n        .Some(.Some(x), y) => println(x + y)\n        _ => println(0)\n"
    );
    assert_eq!(codes(&analyze(&src)), ["cannot-infer-type"]);
}

#[test]
fn an_unsolved_let_else_scrutinee_reports_once() {
    let src = format!(
        "{MAKE}fn main() -> void $ Console:\n    let .Some(y) = make() else:\n        return\n    println(y)\n"
    );
    assert_eq!(codes(&analyze(&src)), ["cannot-infer-type"]);
}

#[test]
fn a_solved_scrutinee_still_checks() {
    let src = format!(
        "{MAKE}fn main() -> void $ Console:\n    let v: i32? = make()\n    match v:\n        .Some(x) => println(x)\n        .None => println(0)\n"
    );
    assert_eq!(codes(&analyze(&src)), Vec::<&str>::new());
}
