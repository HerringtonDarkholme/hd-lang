//! Cost Model (#1510): a warm lint run with nothing changed replays its
//! findings from the stored package result, with as many cache lookups at
//! 1,000 modules as at 100.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, build_packages_with_analysis};
use hd_lint::ANALYZER;
use hd_project::MemorySources;
use hd_sched::SerialOrder;
use hd_testkit::CountingStore;

/// A lint run over `sources` through `store`: its lookups, bytes and output.
fn lint(sources: &MemorySources, store: &CountingStore) -> (usize, usize, Output) {
    store.reset();
    let host = Host {
        render_tir: &[],
        sources,
        store,
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
    (store.gets(), store.bytes(), out)
}

#[test]
fn a_warm_lint_run_replays_its_findings_with_the_same_lookups_at_ten_times_the_modules() {
    let mut warm = Vec::new();
    for modules in [100, 1_000] {
        let mut sources = MemorySources::named(&format!("lint-warm-{modules}"));
        sources.insert("main.hd", include_str!("programs/unread_main.hd"));
        for i in 0..modules {
            sources.insert(
                &format!("m{i}.hd"),
                &format!("pub fn value{i}() -> i32: {i}\n"),
            );
        }
        let store = CountingStore::default();
        let (_, _, cold) = lint(&sources, &store);
        assert!(!cold.diags.has_errors(), "{}", cold.render());
        assert!(
            cold.analysis_diags.code.contains(&Code::UnusedLocalBinding),
            "{}",
            cold.analysis_diags.render_json()
        );
        let (gets, bytes, out) = lint(&sources, &store);
        assert_eq!(out.counters.hit("package"), 1, "{modules} modules");
        assert_eq!(
            out.analysis_diags.render_json(),
            cold.analysis_diags.render_json()
        );
        assert_eq!(out.render(), cold.render());
        warm.push((gets, bytes));
        // An edit misses and is linted again.
        sources.insert("main.hd", include_str!("programs/empty_main.hd"));
        let (_, _, edited) = lint(&sources, &store);
        assert_eq!(edited.counters.hit("package"), 0);
        assert!(
            edited.analysis_diags.is_empty(),
            "{}",
            edited.analysis_diags.render_json()
        );
    }
    assert_eq!(warm[0], warm[1], "100 and 1,000 modules");
}
