//! M4a's exit: every `lib/std` body checks through `InferTable`, the
//! solver and `TirBuilder` to verified TIR with no diagnostic when std is
//! the root package, and a few bodies' TIR is pinned by golden files. The
//! checker's errors in user programs are stage cases
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

fn analyze_std(render: &[&str]) -> Output {
    let src = std_sources();
    let store = MemoryStore::default();
    let host = Host {
        render_tir: render,
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
    let out = analyze_std(&[]);
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

/// The golden TIR of a few std bodies: `println` (rows, `$.use`, a cold
/// suspending call, `block_on`, interpolation, `match` on a `BlockOn` of a
/// `Result`),
/// `simple_escape` (integer patterns), `Option.map` (an `Option` match with a
/// closure call) and `Iterator.take` (a trait default returning an owned
/// named adapter).
/// `HD_BLESS=1` rewrites the files.
#[test]
fn std_bodies_match_golden_tir() {
    let bodies = [
        ("std/console/println", "println"),
        ("std/text/simple_escape", "simple_escape"),
        ("std/option/impl[T < ?Copy] T?.map", "option_map"),
        ("std/iter/Iterator.take", "iterator_take"),
    ];
    let paths: Vec<&str> = bodies.iter().map(|b| b.0).collect();
    let out = analyze_std(&paths);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tir");
    let update = std::env::var_os("HD_BLESS").is_some();
    for (path, file) in bodies {
        let got = out
            .tir_text
            .get(path)
            .unwrap_or_else(|| panic!("no body {path}; have {:?}", out.tir_text.keys()));
        let golden = dir.join(format!("{file}.tir"));
        if update {
            std::fs::create_dir_all(&dir).expect("dir");
            std::fs::write(&golden, got).expect("write golden");
            continue;
        }
        let want = std::fs::read_to_string(&golden).unwrap_or_default();
        assert_eq!(got, &want, "{path}: TIR differs from {}", golden.display());
    }
}
