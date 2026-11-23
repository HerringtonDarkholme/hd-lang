use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{
    Executor, Goal, Host, NoClock, Output, Packages, build_packages, build_packages_with_analysis,
};
use hd_lint::{ANALYZER, Policy};
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
fn warm_checks_keep_facts_ephemeral_and_rejected_trials_unique() {
    // `text` is read by the candidate that wins after a rejected trial; `extra`
    // is never read, so analysis reports exactly one finding.
    let source = include_str!("programs/rejected_trial_binding.hd");
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
