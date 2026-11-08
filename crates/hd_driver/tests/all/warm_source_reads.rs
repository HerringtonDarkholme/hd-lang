//! Cost Model (.agents/skills/shared-rules.md): a warm `hd check` with
//! nothing changed reads no source file through the source provider, at
//! any package size (#1503: today every run reads and skims every file).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_project::{MemorySources, SourceEntry, SourceSet};
use hd_sched::SerialOrder;

/// A source set that counts the files read through it.
struct Counted {
    files: MemorySources,
    reads: AtomicUsize,
}

impl SourceSet for Counted {
    fn list(&self) -> Vec<SourceEntry> {
        self.files.list()
    }
    fn read(&self, path: &str) -> Option<Arc<[u8]>> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.files.read(path)
    }
}

/// A package of `modules` modules besides `main`, each with one function.
fn package(modules: usize) -> Counted {
    let mut files = MemorySources::default();
    files.insert("main.hd", "pub fn main() -> void:\n    pass\n");
    for i in 0..modules {
        files.insert(
            &format!("m{i}.hd"),
            &format!("pub fn value{i}() -> i32: {i}\n"),
        );
    }
    Counted {
        files,
        reads: AtomicUsize::new(0),
    }
}

/// The files a warm `hd check` of `sources` reads, after a cold one.
fn warm_reads(sources: &Counted) -> usize {
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources,
        store: &*store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let cold = build_packages(&host, "app", &Packages::default(), &Goal::Analyze);
    assert!(cold.diags.is_empty(), "{}", cold.render());
    sources.reads.store(0, Ordering::Relaxed);
    let warm = build_packages(&host, "app", &Packages::default(), &Goal::Analyze);
    assert!(warm.diags.is_empty(), "{}", warm.render());
    sources.reads.load(Ordering::Relaxed)
}

#[test]
#[ignore = "#1503"]
fn a_warm_no_change_check_reads_no_source_file_at_ten_times_the_modules() {
    for modules in [20, 200] {
        assert_eq!(warm_reads(&package(modules)), 0, "{modules} modules");
    }
}
