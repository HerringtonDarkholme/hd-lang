use hd_cache::MemoryStore;
use hd_driver::{
    Executor, Goal, Host, NoClock, Output, Packages, build_packages, build_packages_with_analysis,
};
use hd_lint::{ANALYZER, Level, Policy, UNSIGNED_COMPARISON, UNUSED_BINDING};
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

fn policy() -> Policy {
    let mut policy = Policy::default();
    policy.set(UNUSED_BINDING, Level::Allow).unwrap();
    policy.set(UNSIGNED_COMPARISON, Level::Allow).unwrap();
    policy
}

#[test]
fn inferred_parallel_and_warm_checks_keep_each_site_once_and_normal_caches_quiet() {
    use std::fmt::Write as _;
    let mut source = String::from("fn inferred():\n    debug(+1)\n    return +2\n    +3\n");
    for i in 0..64 {
        writeln!(
            source,
            "fn f_{i}() -> i32:\n    debug(+1)\n    return +2\n    +3"
        )
        .unwrap();
    }
    for executor in [Executor::Serial(SerialOrder::Priority), Executor::Pool(4)] {
        let store = MemoryStore::default();
        let normal = checked(&source, &store, false, executor);
        assert!(normal.diags.is_empty(), "{}", normal.render());
        assert!(normal.analysis_diags.is_empty());
        for _ in 0..2 {
            let out = checked(&source, &store, true, executor);
            assert!(!out.diags.has_errors(), "{}", out.render());
            let findings = policy().apply(&out.analysis_diags);
            assert_eq!(
                findings.len(),
                130,
                "{executor:?}: {}",
                findings.render_json()
            );
        }
        let normal = checked(&source, &store, false, executor);
        assert!(normal.diags.is_empty());
        assert!(normal.counters.modules_checked.is_empty());
    }
}
