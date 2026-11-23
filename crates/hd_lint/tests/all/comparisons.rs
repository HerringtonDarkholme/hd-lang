use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{
    Executor, Goal, Host, NoClock, Output, Packages, build_packages, build_packages_with_analysis,
};
use hd_lint::{ANALYZER, Level, Policy, UNSIGNED_COMPARISON};
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

#[test]
fn policy_rejects_unknown_names_and_conflicting_levels() {
    let mut policy = Policy::default();
    assert!(policy.set("not-a-lint", Level::Warn).is_err());
    policy.set(UNSIGNED_COMPARISON, Level::Warn).unwrap();
    policy.set(UNSIGNED_COMPARISON, Level::Warn).unwrap();
    assert!(policy.set(UNSIGNED_COMPARISON, Level::Deny).is_err());
}

#[test]
fn lint_rechecks_warm_normal_entries_and_never_caches_a_finding() {
    let source = include_str!("programs/unsigned_comparisons.hd");
    let store = MemoryStore::default();
    let serial = Executor::Serial(SerialOrder::Priority);
    let normal = checked(source, &store, false, serial);
    assert!(normal.analysis_diags.is_empty());
    for _ in 0..2 {
        let out = checked(source, &store, true, serial);
        assert_eq!(Policy::default().apply(&out.analysis_diags).len(), 2);
    }
    let normal = checked(source, &store, false, serial);
    assert!(normal.analysis_diags.is_empty());
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
        let findings = Policy::default().apply(&out.analysis_diags);
        assert_eq!(findings.len(), 65, "{executor:?}");
    }
}
