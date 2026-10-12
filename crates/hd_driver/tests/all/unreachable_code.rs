//! Unreachable statements (spec 06 Unreachable Code): statements after an
//! unconditional `return`, and after an infinite loop no `break` targets,
//! warn `unreachable-code` but are still checked.

use hd_cache::MemoryStore;
use hd_diag::{Code, Severity};
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn build_with(src: &MemorySources, store: &MemoryStore) -> Output {
    let host = Host {
        render_tir: &[],
        sources: src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", &Goal::Analyze)
}

fn analyze(main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    build_with(&src, &MemoryStore::default())
}

fn warnings(out: &Output) -> Vec<Code> {
    out.diags
        .content_order()
        .into_iter()
        .filter(|i| out.diags.severity[*i] == Severity::Warning)
        .map(|i| out.diags.code[i])
        .collect()
}

fn errors(out: &Output) -> Vec<Code> {
    out.diags
        .content_order()
        .into_iter()
        .filter(|i| out.diags.severity[*i] == Severity::Error)
        .map(|i| out.diags.code[i])
        .collect()
}

#[test]
fn statements_after_return_warn() {
    let out = analyze("fn example() -> i32:\n    return 1\n    2\n");
    assert!(errors(&out).is_empty(), "{}", out.render());
    assert_eq!(warnings(&out), vec![Code::UnreachableCode]);
}

#[test]
fn statements_after_an_infinite_loop_warn() {
    let out = analyze(
        "fn accept(port: i32) -> void:\n    pass\n\nfn serve(port: i32) -> void:\n    while true:\n        accept(port)\n    pass\n",
    );
    assert!(errors(&out).is_empty(), "{}", out.render());
    assert_eq!(warnings(&out), vec![Code::UnreachableCode]);
}

#[test]
fn statements_after_a_loop_with_a_break_do_not_warn() {
    let out = analyze("fn f() -> void:\n    while true:\n        break\n    pass\n");
    assert!(errors(&out).is_empty(), "{}", out.render());
    assert!(warnings(&out).is_empty(), "{}", out.render());
}
