//! M4a's exit: every `lib/std` body checks through `InferTable`, the
//! solver and `TirBuilder` to verified TIR with no diagnostic when std is
//! the root package. The checker's errors in user programs are stage cases
//! (`tests/cases/checker-programs/`, `hd_check`'s `tests/cases/checker/`).

use hd_base::SliceSort as _;
use std::path::{Path, PathBuf};

use hd_base::Stage;
use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn walk(root: &Path, dir: &Path, out: &mut MemorySources) {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("dir")
        .flatten()
        .map(|e| e.path())
        .collect();
    paths.sort_stable();
    for p in paths {
        if p.is_dir() {
            walk(root, &p, out);
        } else if p.extension().is_some_and(|x| x == "hd") {
            let rel = p
                .strip_prefix(root)
                .expect("root")
                .to_string_lossy()
                .replace('\\', "/");
            out.insert(&rel, &std::fs::read_to_string(&p).expect("read"));
        }
    }
}

fn std_sources() -> MemorySources {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lib/std");
    let mut s = MemorySources::default();
    walk(&root, &root, &mut s);
    s
}

fn analyze_std() -> Output {
    let src = std_sources();
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "std", &Goal::Analyze)
}

/// Every module checks: each `.hd` file of `lib/std`, plus the compiler-supplied
/// `std.rt.entry`, with the trait default bodies among the bodies. The count
/// comes from the files, so adding a std module needs no edit here.
#[test]
fn every_std_body_checks_with_no_diagnostic() {
    let modules = hd_project::SourceSet::list(&std_sources()).len() + 1;
    let out = analyze_std();
    let body = out.report.tally(Stage::Body);
    assert_eq!(
        (body.ok, body.not_implemented, body.blocked),
        (modules, 0, 0),
        "{}\n{:?}",
        out.report.render(),
        out.report.body_failures
    );
    assert_eq!(out.report.tally(Stage::ModuleFinish).ok, modules);
    assert_eq!(out.report.body_failed, 0, "{:?}", out.report.body_failures);
    assert!(out.report.body_ok > 1000, "{}", out.report.body_ok);
    assert_eq!(out.diags.len(), 0, "{}", out.render());
}
