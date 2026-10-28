use hd_cache::MemoryStore;
use hd_diag::{Code, DiagBuf, Severity};
use hd_driver::{
    Executor, Goal, Host, NoClock, Output, Packages, build_packages, build_packages_with_analysis,
};
use hd_lint::{ANALYZER, Level, Policy, UNUSED_BINDING};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn checked(source: &str, store: &MemoryStore, collect: bool, executor: Executor) -> Output {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store,
        clock: &NoClock,
        executor,
    };
    if collect {
        build_packages_with_analysis(
            &host,
            "app",
            &Packages::default(),
            &Goal::Analyze,
            Some(&ANALYZER),
        )
    } else {
        build_packages(&host, "app", &Packages::default(), &Goal::Analyze)
    }
}

fn lint(source: &str, policy: Policy) -> (Output, DiagBuf) {
    let out = checked(
        source,
        &MemoryStore::default(),
        true,
        Executor::Serial(SerialOrder::Priority),
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    let findings = policy.apply(&out.analysis_diags);
    (out, findings)
}

#[test]
fn actual_names_patterns_and_captures_preserve_advice() {
    let source = "fn f() -> void:\n    let (first, second) = (+1, +2)\n    _exempt := +3\n    captured := +4\n    closure := fn() -> i32: captured\n    _ := closure()\n    last := +5\n    pass\n";
    let (out, findings) = lint(source, Policy::default());
    assert!(!out.diags.code.contains(&Code::UnusedLocalBinding));
    assert_eq!(findings.len(), 2, "{}", findings.render_json());
    for (i, name, text) in [(0, "first", "first"), (1, "last", "last")] {
        assert_eq!(findings.code[i], Code::UnusedLocalBinding);
        assert_eq!(
            findings.get_text(findings.message[i]),
            format!("`{name}` is never read; name it `_{name}` to keep it")
        );
        let span = findings.primary[i];
        assert_eq!(&source[span.lo as usize..span.hi as usize], text);
    }
}

#[test]
fn scopes_keep_distinct_bindings_and_ignore_unused_parameters() {
    let source = "fn f(parameter: i32) -> void:\n    item := +1\n    if true:\n        item := +2\n        _ := item\n    pass\n";
    let (_, findings) = lint(source, Policy::default());
    assert_eq!(findings.len(), 1, "{}", findings.render_json());
    assert_eq!(
        findings.get_text(findings.message[0]),
        "`item` is never read; name it `_item` to keep it"
    );
    assert_eq!(
        findings.primary[0].lo as usize,
        source.find("item := +1").unwrap()
    );
}

#[test]
fn unread_optionals_and_results_are_advice_not_suspension_errors() {
    for expression in [
        "let nested: string?? = .None",
        "let result: Result[i32, string] = .Ok(+1)",
    ] {
        let (out, findings) = lint(
            &format!("fn f() -> void:\n    {expression}\n    pass\n"),
            Policy::default(),
        );
        assert!(!out.diags.has_errors());
        assert_eq!(findings.code, [Code::UnusedLocalBinding]);
    }
}

#[test]
fn unused_policy_is_independent_and_never_suppresses_must_use_errors() {
    let source = "fn f() -> void:\n    unused := +1\n    _ := 1u32 < 0\n";
    for (level, severity) in [
        (Level::Allow, None),
        (Level::Warn, Some(Severity::Warning)),
        (Level::Deny, Some(Severity::Error)),
    ] {
        let mut policy = Policy::default();
        policy.set(UNUSED_BINDING, level).unwrap();
        policy
            .set(hd_lint::UNSIGNED_COMPARISON, Level::Allow)
            .unwrap();
        let (_, findings) = lint(source, policy);
        assert_eq!(findings.severity.first().copied(), severity);
    }
    let mut policy = Policy::default();
    policy.set(UNUSED_BINDING, Level::Warn).unwrap();
    assert!(policy.set(UNUSED_BINDING, Level::Deny).is_err());
    for name in ["pending", "_pending"] {
        let source = format!("fn work!() -> void: pass\nfn f() -> void:\n    {name} := work()\n");
        let out = checked(
            &source,
            &MemoryStore::default(),
            true,
            Executor::Serial(SerialOrder::Priority),
        );
        assert_eq!(
            out.diags.code,
            [Code::DiscardedMustUseValue],
            "{}",
            out.render()
        );
        policy = Policy::default();
        policy.set(UNUSED_BINDING, Level::Allow).unwrap();
        assert!(policy.apply(&out.analysis_diags).is_empty());
    }
}

#[test]
fn warm_checks_keep_facts_ephemeral_and_rejected_trials_unique() {
    let source =
        include_str!("../../../spec/conformance/typing/warnings/unused-after-rejected-trials.hd");
    let store = MemoryStore::default();
    let serial = Executor::Serial(SerialOrder::Priority);
    let normal = checked(source, &store, false, serial);
    assert!(normal.analysis_diags.is_empty());
    for _ in 0..2 {
        let out = checked(source, &store, true, serial);
        assert!(!out.diags.has_errors(), "{}", out.render());
        let findings = Policy::default().apply(&out.analysis_diags);
        assert_eq!(findings.len(), 1);
    }
    let normal = checked(source, &store, false, serial);
    assert!(normal.analysis_diags.is_empty());
    assert!(!normal.diags.code.contains(&Code::UnusedLocalBinding));
    assert!(normal.counters.modules_checked.is_empty());
}

#[test]
fn inferred_and_parallel_bodies_keep_each_binding_once() {
    use std::fmt::Write as _;
    let mut source = String::from("fn inferred():\n    extra := +1\n    +2\n");
    for i in 0..64 {
        writeln!(
            source,
            "fn f_{i}() -> void:\n    unused_{i} := +1\n    pass"
        )
        .unwrap();
    }
    for executor in [Executor::Serial(SerialOrder::Priority), Executor::Pool(4)] {
        let out = checked(&source, &MemoryStore::default(), true, executor);
        assert!(!out.diags.has_errors(), "{}", out.render());
        let findings = Policy::default().apply(&out.analysis_diags);
        assert_eq!(
            findings.len(),
            65,
            "{executor:?}: {}",
            findings.render_json()
        );
    }
}

#[test]
fn rejected_closure_trials_leave_only_the_selected_binding() {
    let source = r"trait Parse[T]:
    fn parse(f: fn() -> T) -> bool
struct Item: pass
impl Parse[i32] for Item:
    fn parse(f: fn() -> i32) -> bool: f() >= +0
impl Parse[bool] for Item:
    fn parse(f: fn() -> bool) -> bool: f()
fn choose(value: u32) -> bool:
    Item::parse(fn():
        extra := +1
        value >= 0
    )
";
    let (_, findings) = lint(source, Policy::default());
    assert_eq!(
        findings
            .code
            .iter()
            .filter(|c| **c == Code::UnusedLocalBinding)
            .count(),
        1
    );
    assert_eq!(
        findings
            .code
            .iter()
            .filter(|c| **c == Code::UnsignedComparisonAlways)
            .count(),
        1
    );
}
