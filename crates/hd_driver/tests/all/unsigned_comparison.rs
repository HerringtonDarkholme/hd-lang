//! Unsigned comparisons remain accepted and quiet in ordinary checking.
//! Advice lives in `hd_lint` (`expr.ord.unsigned-zero.lint`).

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

fn check(body: &str) -> Output {
    analyze(&format!("fn f(a: u32) -> bool:\n    {body}\n"))
}

#[test]
fn each_fixed_form_is_quiet_in_check() {
    for body in ["a >= 0", "0 <= a", "a < 0", "0 > a", "a >= 0x0", "a >= 0_0"] {
        let out = check(body);
        assert!(errors(&out).is_empty(), "{body}: {}", out.render());
        assert!(warnings(&out).is_empty(), "{body}: {}", out.render());
    }
}

#[test]
fn a_usize_default_is_quiet_in_check() {
    let out = analyze("fn f() -> bool:\n    let t = 10\n    t >= 0\n");
    assert!(errors(&out).is_empty(), "{}", out.render());
    assert!(warnings(&out).is_empty(), "{}", out.render());
}

#[test]
fn signed_operands_do_not_warn() {
    let out = analyze("fn f(s: i32) -> bool:\n    s >= 0\n");
    assert!(errors(&out).is_empty(), "{}", out.render());
    assert!(warnings(&out).is_empty(), "{}", out.render());
    let out = analyze("fn f(s: i32) -> bool:\n    s < 0\n");
    assert!(errors(&out).is_empty(), "{}", out.render());
    assert!(warnings(&out).is_empty(), "{}", out.render());
}

#[test]
fn other_operators_do_not_warn() {
    for body in ["a <= 0", "a > 0", "a == 0", "a != 0", "0 >= a", "0 < a"] {
        let out = check(body);
        assert!(errors(&out).is_empty(), "{body}: {}", out.render());
        assert!(warnings(&out).is_empty(), "{body}: {}", out.render());
    }
}
