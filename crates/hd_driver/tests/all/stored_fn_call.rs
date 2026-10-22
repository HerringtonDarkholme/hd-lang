//! A method call never reads a field (spec/lang/05-expressions.md,
//! `expr.member.no-field-call`, `expr.member.stored-fn`,
//! `expr.member.stored-fn.error`, `expr.member.stored-fn.hint`): a function
//! stored in a field is called as `(x.f)(args)`; `x.f(args)` looks for a
//! method `f` and is `unknown-method` when there is none.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn analyzed(main: &str) -> Output {
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

fn codes(out: &Output) -> Vec<Code> {
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

const BUTTON: &str = "\
struct Button:
    on_click: fn(i32) -> i32

fn double(x: i32) -> i32: x * 2

";

#[test]
fn method_call_on_a_function_field_is_unknown_method_with_a_hint() {
    let out = analyzed(&format!(
        "{BUTTON}fn run(button: Button) -> i32:\n    button.on_click(41)\n"
    ));
    assert_eq!(codes(&out), vec![Code::UnknownMethod], "{}", out.render());
    assert!(
        out.render().contains("(x.on_click)(..)"),
        "{}",
        out.render()
    );
}

#[test]
fn no_hint_without_a_field() {
    let out = analyzed(&format!(
        "{BUTTON}fn run(button: Button) -> i32:\n    button.missing(41)\n"
    ));
    assert_eq!(codes(&out), vec![Code::UnknownMethod], "{}", out.render());
    assert!(!out.render().contains("(x.missing)"), "{}", out.render());
}
