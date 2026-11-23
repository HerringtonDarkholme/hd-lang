//! `module.entry.*`: the entry point of an entry module is validated when
//! the module is checked. A public `main` or `main!` takes no parameters
//! and no generic parameters (`entry-point-parameters`), every key of its
//! requirement row is a host capability trait of the runtime profile
//! (`nonhost-entry-requirement`), and its result implements
//! `std.process.Termination` (`unsatisfied-trait-bound`). Each problem is
//! one error at the name, the offending key or the result type, with no
//! error after it. A function that is not an entry point owes nothing.
//! These tests check that a warm store and a changed entry list keep those
//! checks right.

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The `(code, line, column)` of each error, in source order.
fn errors(out: &Output) -> Vec<(Code, usize, usize)> {
    out.diags
        .content_order()
        .into_iter()
        .filter(|i| out.diags.severity[*i] == hd_diag::Severity::Error)
        .map(|i| {
            let (_, line, column) = out.locate(out.diags.primary[i]);
            (out.diags.code[i], line, column)
        })
        .collect()
}

// Caches: the stage cases under `tests/cases/entry-point/` and
// `hd_check/tests/cases/entry-point/` check each rule once.

#[test]
fn a_warm_cache_reports_the_same_errors() {
    let store = MemoryStore::default();
    let mut src = MemorySources::default();
    src.insert("main.hd", include_str!("programs/entry_point/main_i32.hd"));
    let run = |store: &MemoryStore| {
        let host = Host {
            render_tir: &[],
            sources: &src,
            store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
        };
        build_packages(&host, "app", &Packages::default(), &Goal::Analyze)
    };
    let cold = errors(&run(&store));
    let warm = errors(&run(&store));
    assert_eq!(cold, [(Code::UnsatisfiedTraitBound, 1, 18)]);
    assert_eq!(warm, cold);
}

#[test]
fn naming_an_executable_changes_the_cached_check() {
    let store = MemoryStore::default();
    let mut src = MemorySources::default();
    src.insert("src/lib.hd", include_str!("programs/entry_point/lib.hd"));
    src.insert("src/tool.hd", include_str!("programs/entry_point/tool.hd"));
    let run = |entries: Vec<String>| {
        let host = Host {
            render_tir: &[],
            sources: &src,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
        };
        let packages = Packages {
            entries,
            ..Packages::default()
        };
        build_packages(&host, "app", &packages, &Goal::Analyze)
    };
    assert!(errors(&run(Vec::new())).is_empty());
    assert_eq!(errors(&run(vec!["tool".to_owned()])).len(), 1);
    assert!(errors(&run(Vec::new())).is_empty());
}
