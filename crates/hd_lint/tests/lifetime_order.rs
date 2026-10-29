use hd_cache::MemoryStore;
use hd_diag::{Code, DiagBuf};
use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages_with_analysis};
use hd_lint::{ANALYZER, Policy};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn lint(source: &str) -> DiagBuf {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build_packages_with_analysis(
        &host,
        "app",
        &Packages::default(),
        &Goal::Analyze,
        Some(&ANALYZER),
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    Policy::default().apply(&out.analysis_diags)
}

#[test]
fn a_lifetime_before_a_type_parameter_warns() {
    let source = "fn first['a, T](x: &'a T) -> &'a T:\n    x\n";
    let findings = lint(source);
    assert_eq!(findings.code, [Code::LifetimeOrder]);
    let span = findings.primary[0];
    assert_eq!(&source[span.lo as usize..span.hi as usize], "'a");
    assert!(findings.get_text(findings.message[0]).contains("`'a`"));
}

#[test]
fn a_method_list_is_checked_too() {
    let source =
        "struct S:\n    n: i32\n\nimpl S:\n    fn pick['a, T](x: &'a T) -> &'a T:\n        x\n";
    assert_eq!(lint(source).code, [Code::LifetimeOrder]);
}

#[test]
fn type_parameters_first_is_quiet() {
    assert!(lint("fn first[T, 'a](x: &'a T) -> &'a T:\n    x\n").is_empty());
    assert!(lint("fn id[T](x: T) -> T:\n    x\n").is_empty());
}
