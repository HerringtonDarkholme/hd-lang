use hd_cache::MemoryStore;
use hd_diag::{Code, DiagBuf, Severity};
use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, build_packages};
use hd_lint::{Level, Policy, UNSIGNED_COMPARISON, analyze};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn checked(source: &str, store: &MemoryStore, collect: bool, executor: Executor) -> Output {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    build_packages(
        &Host {
            render_tir: &[],
            sources: &sources,
            store,
            clock: &NoClock,
            executor,
        },
        "app",
        &Packages {
            collect_source_facts: collect,
            ..Packages::default()
        },
        &Goal::Analyze,
    )
}

fn lint(source: &str, policy: Policy) -> (Output, DiagBuf) {
    let out = checked(
        source,
        &MemoryStore::default(),
        true,
        Executor::Serial(SerialOrder::Priority),
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    let findings = analyze(&out.source_facts, &out.texts, policy);
    (out, findings)
}

#[test]
fn source_forms_preserve_messages_and_spans() {
    for (expression, form, truth) in [
        ("a >= 0", "a >= 0", "true"),
        ("0 <= a", "0 <= a", "true"),
        ("a < 0", "a < 0", "false"),
        ("0 > a", "0 > a", "false"),
        ("a >= 0x0", "a >= 0", "true"),
        ("a >= 0_0", "a >= 0", "true"),
        ("a >= 0b0", "a >= 0", "true"),
        ("a >= 0o0", "a >= 0", "true"),
    ] {
        let source = format!("fn f(a: u32) -> bool:\n    {expression}\n");
        let (out, findings) = lint(&source, Policy::default());
        assert!(!out.diags.code.contains(&Code::UnsignedComparisonAlways));
        assert_eq!(findings.len(), 1, "{expression}");
        assert_eq!(findings.code[0], Code::UnsignedComparisonAlways);
        assert_eq!(
            findings.get_text(findings.message[0]),
            format!("`a` is unsigned, so `{form}` is always {truth}")
        );
        let span = findings.primary[0];
        assert_eq!(&source[span.lo as usize..span.hi as usize], expression);
    }
}

#[test]
fn a_default_usize_keeps_the_actual_name_and_signed_guidance() {
    let (_, findings) = lint(
        "fn f() -> bool:\n    let remaining = 10\n    remaining >= 0\n",
        Policy::default(),
    );
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings.get_text(findings.message[0]),
        "`remaining` is unsigned, so `remaining >= 0` is always true; consider a signed literal such as `+10` or another condition"
    );
}

#[test]
fn signed_suffixed_and_other_comparisons_are_quiet() {
    for (ty, expression) in [
        ("i32", "a >= 0"),
        ("i64", "a < 0"),
        ("u32", "a >= 0u32"),
        ("u32", "a >= 1"),
        ("u32", "a <= 0"),
        ("u32", "a > 0"),
        ("u32", "a == 0"),
        ("u32", "a != 0"),
        ("u32", "0 >= a"),
        ("u32", "0 < a"),
    ] {
        let (_, findings) = lint(
            &format!("fn f(a: {ty}) -> bool:\n    {expression}\n"),
            Policy::default(),
        );
        assert!(
            findings.is_empty(),
            "{ty} {expression}: {}",
            findings.render_json()
        );
    }
}

#[test]
fn allow_and_deny_change_advice_without_changing_acceptance() {
    let source = "fn f(a: u32) -> bool:\n    a < 0\n";
    for (level, expected) in [
        (Level::Allow, None),
        (Level::Warn, Some(Severity::Warning)),
        (Level::Deny, Some(Severity::Error)),
    ] {
        let mut policy = Policy::default();
        policy.set(UNSIGNED_COMPARISON, level).unwrap();
        let (out, findings) = lint(source, policy);
        assert!(!out.diags.has_errors());
        assert_eq!(findings.severity.first().copied(), expected);
    }
    let mut policy = Policy::default();
    assert!(policy.set("not-a-lint", Level::Warn).is_err());
    policy.set(UNSIGNED_COMPARISON, Level::Warn).unwrap();
    policy.set(UNSIGNED_COMPARISON, Level::Warn).unwrap();
    assert!(policy.set(UNSIGNED_COMPARISON, Level::Deny).is_err());
}

#[test]
fn lint_rechecks_warm_normal_entries_and_never_caches_a_finding() {
    let source = "fn f(a: u32) -> bool:\n    a >= 0\nfn inferred(a: u32):\n    a < 0\n";
    let store = MemoryStore::default();
    let serial = Executor::Serial(SerialOrder::Priority);
    let normal = checked(source, &store, false, serial);
    assert!(normal.source_facts.comparisons.is_empty());
    for _ in 0..2 {
        let out = checked(source, &store, true, serial);
        assert_eq!(
            analyze(&out.source_facts, &out.texts, Policy::default()).len(),
            2
        );
    }
    let normal = checked(source, &store, false, serial);
    assert!(normal.source_facts.comparisons.is_empty());
    assert!(!normal.diags.code.contains(&Code::UnsignedComparisonAlways));
    assert!(
        normal.counters.modules_checked.is_empty(),
        "normal checking should use the entry lint populated"
    );
}

#[test]
fn inferred_bodies_and_parallel_batches_keep_their_facts_once() {
    let mut source = String::from("fn inferred(a: u32):\n    a >= 0\n");
    for i in 0..64 {
        use std::fmt::Write as _;
        writeln!(source, "fn probe_{i}(a: u32) -> bool:\n    a >= 0").unwrap();
    }
    for executor in [Executor::Serial(SerialOrder::Priority), Executor::Pool(4)] {
        let out = checked(&source, &MemoryStore::default(), true, executor);
        assert!(!out.diags.has_errors(), "{}", out.render());
        let findings = analyze(&out.source_facts, &out.texts, Policy::default());
        assert_eq!(findings.len(), 65, "{executor:?}");
    }
}

#[test]
fn rejected_callable_trials_leave_no_duplicate_comparison_facts() {
    let source = r"trait Parse[T]:
    fn parse(f: fn() -> T) -> bool
struct Item: pass
impl Parse[i32] for Item:
    fn parse(f: fn() -> i32) -> bool: f() >= +0
impl Parse[bool] for Item:
    fn parse(f: fn() -> bool) -> bool: f()
fn choose(value: u32) -> bool:
    Item::parse(fn(): value >= 0)
";
    let (out, findings) = lint(source, Policy::default());
    assert_eq!(findings.len(), 1);
    let span = findings.primary[0];
    assert_eq!(
        out.source_facts
            .comparisons
            .iter()
            .filter(|fact| fact.span == span)
            .count(),
        1
    );
}
