use hd_cache::MemoryStore;
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

const PAIR: &str = include_str!("programs/pair.hd");

#[test]
fn inferred_parallel_and_cached_bodies_keep_each_site_once() {
    use std::fmt::Write as _;
    let mut source = format!(
        "{PAIR}fn inferred(value: Pair):\n    let mut count = +1\n    count += +1\n    match value:\n        .Values(right, left) => count + left - right\n"
    );
    for i in 0..64 {
        writeln!(source, "fn f_{i}(value: Pair) -> i32:\n    let mut count = +1\n    count += +1\n    match value:\n        .Values(right, left) => count + left - right").unwrap();
    }
    for executor in [Executor::Serial(SerialOrder::Priority), Executor::Pool(4)] {
        let store = MemoryStore::default();
        let normal = checked(&source, &store, false, executor);
        assert!(normal.diags.is_empty(), "{}", normal.render());
        assert!(normal.analysis_diags.is_empty());
        for _ in 0..2 {
            let (out, findings) = {
                let out = checked(&source, &store, true, executor);
                let findings = Policy::default().apply(&out.analysis_diags);
                (out, findings)
            };
            assert!(!out.diags.has_errors(), "{}", out.render());
            assert_eq!(findings.len(), 195, "{executor:?}");
        }
        let normal = checked(&source, &store, false, executor);
        assert!(normal.diags.is_empty());
        assert!(normal.counters.modules_checked.is_empty());
    }
}
