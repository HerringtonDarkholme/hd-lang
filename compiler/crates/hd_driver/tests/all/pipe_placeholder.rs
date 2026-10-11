//! The pipe placeholder (expr.pipe.slot.*, expr.pipe.placeholder-outside):
//! a step's own `_` is counted, a nested pipe's step keeps its own, and a
//! rejected `_` is reported once at that `_` with nothing after it.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const PRELUDE: &str = "\
fn add(left: i32, right: i32) -> i32: left + right
fn apply(value: i32, f: fn(i32) -> i32) -> i32: f(value)
fn pair(left: i32, right: i32) -> (i32, i32): (left, right)

";

/// The diagnostics of `body` after the prelude, as `(code, line, column)`
/// with lines counted in `body` and columns from 1.
fn found(body: &str) -> Vec<(Code, usize, usize)> {
    let text = format!("{PRELUDE}{body}");
    let skipped = PRELUDE.lines().count();
    let mut src = MemorySources::default();
    src.insert("main.hd", &text);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(&host, "app", &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| {
            let at = out.diags.primary[i].lo as usize;
            let before = &text[..at];
            let line = before.matches('\n').count() + 1 - skipped;
            let col = at - before.rfind('\n').map_or(0, |p| p + 1) + 1;
            (out.diags.code[i], line, col)
        })
        .collect()
}

#[test]
fn one_placeholder_in_any_position_is_a_substitution() {
    let body = "\
fn run(n: i32) -> i32:
    a := n |> add(_, 1)
    b := n |> add(1, _)
    c := n |> _ * 2
    d := n |> pair(_, 0)._0
    a + b + c + d
";
    assert_eq!(found(body), []);
}

#[test]
fn a_bare_step_has_no_placeholder() {
    let body = "\
fn inc(n: i32) -> i32: n + 1

fn run(n: i32) -> i32: n |> inc
";
    assert_eq!(found(body), []);
}

#[test]
fn two_placeholders_report_the_second_once() {
    let body = "\
fn run(n: i32) -> i32:
    n |> add(_, _)
";
    assert_eq!(found(body), [(Code::DuplicatePipePlaceholder, 2, 17)]);
}

#[test]
fn three_placeholders_report_one_error() {
    let body = "\
fn run(n: i32) -> i32:
    n |> add(_, _ + _)
";
    assert_eq!(found(body), [(Code::DuplicatePipePlaceholder, 2, 17)]);
}

#[test]
fn a_placeholder_in_a_closure_in_the_step_is_rejected() {
    let body = "\
fn run(n: i32) -> i32:
    n |> apply(1, fn(v): v + _)
";
    assert_eq!(found(body), [(Code::PipePlaceholderInClosure, 2, 30)]);
}

#[test]
fn a_closure_with_its_own_pipe_keeps_that_pipes_placeholder() {
    let body = "\
fn run(n: i32) -> i32:
    n |> apply(_, fn(v): v |> add(_, 1))
";
    assert_eq!(found(body), []);
}

#[test]
fn a_nested_pipe_step_keeps_its_own_placeholder() {
    let body = "\
fn run(x: i32, y: i32) -> (i32, i32):
    x |> pair(_, y |> add(_, 1))
";
    assert_eq!(found(body), []);
}

#[test]
fn the_left_side_of_a_nested_pipe_belongs_to_the_outer_step() {
    let body = "\
fn run(x: i32) -> i32:
    x |> add(_ |> add(_, 1), _)
";
    assert_eq!(found(body), [(Code::DuplicatePipePlaceholder, 2, 30)]);
}

#[test]
fn a_nested_step_with_two_placeholders_is_its_own_error() {
    let body = "\
fn run(x: i32, y: i32) -> (i32, i32):
    x |> pair(_, y |> add(_, _))
";
    assert_eq!(found(body), [(Code::DuplicatePipePlaceholder, 2, 30)]);
}

#[test]
fn a_placeholder_outside_any_pipe_is_reported_alone() {
    let body = "\
fn run() -> i32:
    apply(3, add(_, 8))
";
    assert_eq!(found(body), [(Code::PlaceholderOutsidePipe, 2, 18)]);
}

#[test]
fn a_placeholder_in_the_pipe_value_is_outside_the_step() {
    let body = "\
fn run() -> i32:
    _ |> add(_, 1)
";
    assert_eq!(found(body), [(Code::PlaceholderOutsidePipe, 2, 5)]);
}
