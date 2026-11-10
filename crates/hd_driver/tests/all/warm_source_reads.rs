//! Cost Model (.agents/skills/shared-rules.md): a warm `hd check` with
//! nothing changed reads no source file through the source provider, at
//! any package size, and one after editing k files reads those k
//! (`crates/hd_cache/DESIGN.md` §5.5, #1503).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use hd_base::Hash128;
use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, build_packages};
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
    fn identity(&self) -> Option<Hash128> {
        self.files.identity()
    }
}

/// A package of `modules` modules besides `main`, each with one function;
/// `name` keeps its last-run record apart from other tests'.
fn package(name: &str, modules: usize) -> Counted {
    let mut files = MemorySources::named(name);
    files.insert("main.hd", "pub fn main() -> void:\n    pass\n");
    for i in 0..modules {
        files.insert(&format!("m{i}.hd"), &module(i, i));
    }
    Counted {
        files,
        reads: AtomicUsize::new(0),
    }
}

fn module(i: usize, value: usize) -> String {
    format!("pub fn value{i}() -> i32: {value}\n")
}

/// `hd check` of `sources`, its reads counted from zero.
fn check(sources: &Counted) -> Output {
    run(sources, &Goal::Analyze)
}

/// A run of `goal` over `sources`, its reads counted from zero.
fn run(sources: &Counted, goal: &Goal) -> Output {
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources,
        store: &*store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    sources.reads.store(0, Ordering::Relaxed);
    build_packages(&host, "app", &Packages::default(), goal)
}

/// The files a clean `hd check` of `sources` reads.
fn reads(sources: &Counted) -> usize {
    let out = check(sources);
    assert!(out.diags.is_empty(), "{}", out.render());
    sources.reads.load(Ordering::Relaxed)
}

#[test]
fn a_warm_no_change_check_reads_no_source_file_at_ten_times_the_modules() {
    for modules in [20, 200] {
        let sources = package(&format!("warm-{modules}"), modules);
        assert_eq!(reads(&sources), modules + 1, "cold, {modules} modules");
        assert_eq!(reads(&sources), 0, "warm, {modules} modules");
    }
}

#[test]
fn a_warm_no_change_build_reads_no_source_file_at_ten_times_the_modules() {
    let build = Goal::Program {
        entry: "main".to_owned(),
    };
    for modules in [20, 200] {
        let sources = package(&format!("build-{modules}"), modules);
        let cold = run(&sources, &build);
        assert!(cold.wasm.is_some(), "{}", cold.render());
        let warm = run(&sources, &build);
        assert!(warm.wasm.is_some(), "{}", warm.render());
        assert_eq!(
            sources.reads.load(Ordering::Relaxed),
            0,
            "warm, {modules} modules"
        );
    }
}

#[test]
fn a_warm_no_change_test_build_reads_no_source_file() {
    let tests = Goal::Tests {
        module: None,
        filter: None,
    };
    let mut sources = package("tests", 20);
    sources.files.insert(
        "m0.hd",
        "pub fn value0() -> i32: 0\n\ntests:\n    it(\"zero\"):\n        assert(value0() == 0)\n",
    );
    let cold = run(&sources, &tests);
    assert!(!cold.diags.has_errors(), "{}", cold.render());
    let warm = run(&sources, &tests);
    assert!(!warm.diags.has_errors(), "{}", warm.render());
    assert_eq!(sources.reads.load(Ordering::Relaxed), 0);
}

#[test]
fn a_check_after_editing_k_files_reads_those_k_at_ten_times_the_modules() {
    for modules in [20, 200] {
        let mut sources = package(&format!("edit-{modules}"), modules);
        sources.files.insert("gone/y.hd", "pub fn y() -> i32: 1\n");
        reads(&sources);
        // Two body edits, an added file and a deleted one. A file in a
        // folder of its own changes no other folder's interface, so no
        // other module is checked again.
        sources.files.insert("m0.hd", &module(0, 7));
        sources.files.insert("m1.hd", &module(1, 8));
        sources.files.insert("added/x.hd", "pub fn x() -> i32: 1\n");
        sources.files.remove("gone/y.hd");
        assert_eq!(reads(&sources), 3, "after 3 edits, {modules} modules");
        assert_eq!(reads(&sources), 0, "warm again, {modules} modules");
    }
}

#[test]
fn a_signature_edit_checks_its_folder_again() {
    let mut sources = package("signature", 3);
    sources.files.insert(
        "main.hd",
        "use pkg.m1.{value1}\n\npub fn main() -> void:\n    let v: i32 = value1()\n",
    );
    reads(&sources);
    sources.files.insert("m1.hd", "pub fn value1() -> i64: 1\n");
    let out = check(&sources);
    assert!(out.render().contains("main.hd"), "{}", out.render());
}

#[test]
fn a_file_with_the_same_size_and_new_bytes_is_read_and_checked_again() {
    let mut sources = package("same-size", 3);
    reads(&sources);
    // Same path and length: only the stamp tells the bytes apart.
    sources.files.insert("m0.hd", &module(0, 9));
    assert_eq!(reads(&sources), 1);
    sources.files.insert("m0.hd", "pub fn value0() -> i32: x\n");
    let out = check(&sources);
    assert!(out.render().contains("m0.hd"), "{}", out.render());
    assert_eq!(sources.reads.load(Ordering::Relaxed), 1);
}

#[test]
fn a_renamed_folder_is_read_under_its_new_paths() {
    let mut sources = package("rename", 2);
    sources.files.insert("a/x.hd", "pub fn x() -> i32: 1\n");
    sources.files.insert(
        "main.hd",
        "use pkg.a.x.{x}\n\npub fn main() -> void:\n    let v = x()\n",
    );
    reads(&sources);
    sources.files.remove("a/x.hd");
    sources.files.insert("b/x.hd", "pub fn x() -> i32: 1\n");
    // `main` still uses `a.x`, now gone: the rename is seen, not a stale
    // record of the old folder.
    let out = check(&sources);
    assert!(!out.diags.is_empty(), "the use of the moved module fails");
}

/// `cli.repl.package.lib`, `cli.repl.uses.other`: a REPL input's build
/// leaves the modules it does not reach unread, and its record keeps no
/// skim of their placeholder text. A later input that reaches one through
/// `use self.util` sees its declarations (#1616). Stamps make this a
/// last-run record test: a CLI case's files are too new to be stamped.
#[test]
fn a_module_an_earlier_repl_input_left_unread_is_read_when_a_later_one_reaches_it() {
    let mut sources = Counted {
        files: MemorySources::named("repl-unread"),
        reads: AtomicUsize::new(0),
    };
    sources
        .files
        .insert("src/lib.hd", "fn bonus() -> i32:\n    +7\n");
    sources
        .files
        .insert("src/util.hd", "pub fn double(x: i32) -> i32:\n    x * +2\n");
    let input = |sources: &mut Counted, n: u32, text: &str| {
        sources.files.insert(&hd_project::repl_file(n), text);
        let session = hd_driver::Session::linked();
        let store = crate::warm_store();
        let host = Host {
            render_tir: &[],
            sources: &*sources,
            store: &*store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
        };
        let packages = Packages {
            session: Some(&session),
            inputs_from: Some(1),
            ..Packages::default()
        };
        build_packages(&host, "app", &packages, &Goal::ReplInput { input: n })
    };
    let first = input(&mut sources, 1, "bonus()\n");
    assert!(!first.diags.has_errors(), "{}", first.render());
    let second = input(&mut sources, 2, "use self.util.{double}\ndouble(bonus())\n");
    assert!(!second.diags.has_errors(), "{}", second.render());
}
