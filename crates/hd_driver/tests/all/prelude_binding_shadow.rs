//! `module.prelude.no-shadow` for bindings: a local binding, a closure
//! parameter, a pattern name, a loop name, a test local, or a top-level
//! binding must not take a prelude name. Each conflict is one
//! `prelude-name-shadow` error at the name, with no error after it: the
//! binding still introduces the name for the rest of the body.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn run(text: &str, goal: &Goal) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", text);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build_packages(&host, "app", &Packages::default(), goal)
}

/// The `(code, line, column)` of each error, in source order.
fn errors(text: &str) -> Vec<(Code, usize, usize)> {
    let goal = if text.starts_with("tests:") {
        Goal::CheckTests
    } else {
        Goal::Analyze
    };
    let out = run(text, &goal);
    out.diags
        .content_order()
        .into_iter()
        .filter(|i| out.diags.severity[*i] == hd_diag::Severity::Error)
        .map(|i| {
            let (_, line, column) = out.locate(out.diags.primary[i]);
            (out.diags.code[i], line, column)
        })
        .collect()
}

#[track_caller]
fn shadows(text: &str, at: &[(usize, usize)]) {
    let want: Vec<_> = at
        .iter()
        .map(|&(l, c)| (Code::PreludeNameShadow, l, c))
        .collect();
    assert_eq!(errors(text), want, "{text}");
}

#[test]
fn test_local() {
    shadows(
        "tests:\n    it(\"x\"):\n        let Result = +1\n        _ := Result\n",
        &[(3, 13)],
    );
}

#[test]
fn test_local_named_it() {
    shadows(
        "tests:\n    it(\"x\"):\n        it := +1\n        _ := it\n",
        &[(3, 9)],
    );
}
