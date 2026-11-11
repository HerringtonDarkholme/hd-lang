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

/// A store that counts the lookups made through it and the bytes they
/// returned.
#[derive(Default)]
struct CountedStore {
    store: hd_cache::MemoryStore,
    gets: AtomicUsize,
    bytes: AtomicUsize,
}

impl hd_cache::CacheStore for CountedStore {
    fn get(&self, kind: hd_cache::EntryKind, key: &Hash128) -> Option<Arc<[u8]>> {
        let got = self.store.get(kind, key);
        self.gets.fetch_add(1, Ordering::Relaxed);
        self.bytes
            .fetch_add(got.as_ref().map_or(0, |b| b.len()), Ordering::Relaxed);
        got
    }
    fn put(&self, kind: hd_cache::EntryKind, key: &Hash128, bytes: &[u8]) {
        self.store.put(kind, key, bytes);
    }
    fn touch(&self, kind: hd_cache::EntryKind, key: &Hash128) {
        self.store.touch(kind, key);
    }
}

/// The lookups and bytes read of a check of `sources` through `store`.
fn lookups(sources: &Counted, store: &CountedStore) -> (usize, usize, Output) {
    store.gets.store(0, Ordering::Relaxed);
    store.bytes.store(0, Ordering::Relaxed);
    sources.reads.store(0, Ordering::Relaxed);
    let host = Host {
        render_tir: &[],
        sources,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build_packages(&host, "app", &Packages::default(), &Goal::Analyze);
    (
        store.gets.load(Ordering::Relaxed),
        store.bytes.load(Ordering::Relaxed),
        out,
    )
}

/// Cost Model (#1510): a warm `hd check` with nothing changed takes the
/// stored package result, so it makes as many cache lookups and reads as
/// many entry bytes at 1,000 modules as at 100; one edit takes the
/// per-module path again.
#[test]
fn a_warm_no_change_check_does_the_same_lookups_at_ten_times_the_modules() {
    let mut warm = Vec::new();
    for modules in [100, 1_000] {
        let store = CountedStore::default();
        let mut sources = package(&format!("package-result-{modules}"), modules);
        let (cold, _, out) = lookups(&sources, &store);
        assert!(out.diags.is_empty(), "{}", out.render());
        let (gets, bytes, out) = lookups(&sources, &store);
        assert!(out.diags.is_empty(), "{}", out.render());
        assert_eq!(out.counters.hit("package"), 1, "{modules} modules");
        assert_eq!(sources.reads.load(Ordering::Relaxed), 0);
        assert!(gets < cold, "{modules} modules: {gets} of {cold}");
        warm.push((gets, bytes));
        // An edit misses the stored result and is checked.
        sources
            .files
            .insert("m0.hd", "pub fn value0() -> i32: no\n");
        let (_, _, out) = lookups(&sources, &store);
        assert!(out.diags.has_errors(), "the edit is checked");
        assert_eq!(out.counters.hit("package"), 0);
    }
    assert_eq!(warm[0], warm[1], "100 and 1,000 modules");
}

/// `package`, with one diagnostic: `m1` returns a name it never binds.
/// The checker reports no warning of its own (a lint run's findings are
/// `hd_lint`'s tests'), so an error stands for any reported diagnostic.
fn warned(name: &str, modules: usize) -> Counted {
    let mut sources = package(name, modules);
    sources
        .files
        .insert("m1.hd", "pub fn value1() -> i32: missing\n");
    sources
}

/// A run of `goal` with `packages` through `store`: its lookups and bytes
/// read, and its output.
fn lookups_of(
    sources: &Counted,
    store: &CountedStore,
    packages: &Packages<'_>,
    goal: &Goal,
) -> (usize, usize, Output) {
    store.gets.store(0, Ordering::Relaxed);
    store.bytes.store(0, Ordering::Relaxed);
    sources.reads.store(0, Ordering::Relaxed);
    let host = Host {
        render_tir: &[],
        sources,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build_packages(&host, "app", packages, goal);
    (
        store.gets.load(Ordering::Relaxed),
        store.bytes.load(Ordering::Relaxed),
        out,
    )
}

/// Cost Model (#1510): a warm check whose run reported a diagnostic
/// replays the cold run's diagnostics, byte for byte, from the stored
/// package result: as many lookups and entry bytes at 1,000 modules as at
/// 100, and one source read, of the file the diagnostic is located in.
#[test]
fn a_warm_check_with_a_diagnostic_replays_it_with_the_same_lookups_at_ten_times_the_modules() {
    let mut warm = Vec::new();
    for modules in [100, 1_000] {
        let store = CountedStore::default();
        let sources = warned(&format!("warned-{modules}"), modules);
        let (_, _, cold) = lookups(&sources, &store);
        assert!(cold.render().contains("m1.hd"), "{}", cold.render());
        let (gets, bytes, out) = lookups(&sources, &store);
        assert_eq!(out.counters.hit("package"), 1, "{modules} modules");
        // Only the file the diagnostic is located in.
        assert_eq!(sources.reads.load(Ordering::Relaxed), 1);
        assert_eq!(out.render(), cold.render());
        assert_eq!(out.render_located(), cold.render_located());
        assert_eq!(
            hd_driver::diagnostics::from_output(&out, None),
            hd_driver::diagnostics::from_output(&cold, None)
        );
        warm.push((gets, bytes));
    }
    assert_eq!(warm[0], warm[1], "100 and 1,000 modules");
}

/// An edit, an added file and a removed file each miss the stored result
/// of a check that reported a diagnostic; the run after each takes the new
/// one.
#[test]
fn an_edit_an_added_file_and_a_removed_file_each_miss_the_stored_result() {
    let store = CountedStore::default();
    let mut sources = warned("warned-changes", 20);
    let check = |sources: &Counted| lookups(sources, &store).2;
    let cold = check(&sources);
    assert_eq!(check(&sources).counters.hit("package"), 1);
    sources
        .files
        .insert("m0.hd", "pub fn value0() -> i32: no\n");
    let edited = check(&sources);
    assert_eq!(edited.counters.hit("package"), 0, "an edit misses");
    assert!(edited.diags.has_errors(), "{}", edited.render());
    let again = check(&sources);
    assert_eq!(again.counters.hit("package"), 1);
    assert_eq!(again.render(), edited.render());
    sources.files.insert("m0.hd", &module(0, 0));
    sources.files.insert("added/x.hd", "pub fn x() -> i32: y\n");
    let added = check(&sources);
    assert_eq!(added.counters.hit("package"), 0, "an added file misses");
    assert!(added.render().contains("added/x.hd"), "{}", added.render());
    assert_eq!(check(&sources).counters.hit("package"), 1);
    // Back to the first listing: its stored result is the cold run's.
    sources.files.remove("added/x.hd");
    let back = check(&sources);
    assert_eq!(back.counters.hit("package"), 1);
    assert_eq!(back.render(), cold.render());
    sources.files.remove("m1.hd");
    let removed = check(&sources);
    assert_eq!(removed.counters.hit("package"), 0, "a removed file misses");
    assert!(!removed.diags.has_errors(), "{}", removed.render());
    assert_eq!(check(&sources).counters.hit("package"), 1);
}

/// `hd check FILE` (check roots) and `hd check --tests` (`CheckTests`)
/// take their own stored results, with as many lookups at 1,000 modules
/// as at 100; a FILE run's `use` closure is replayed for its caller.
#[test]
fn a_warm_file_check_and_a_warm_test_check_take_their_stored_results() {
    let file = Packages {
        check_roots: Some(vec!["main.hd".to_owned()]),
        ..Packages::default()
    };
    let plain = Packages::default();
    let runs = [(&file, Goal::Analyze), (&plain, Goal::CheckTests)];
    for (packages, goal) in runs {
        let mut warm = Vec::new();
        for modules in [100, 1_000] {
            let store = CountedStore::default();
            let mut sources = warned(&format!("roots-{goal:?}-{modules}"), modules);
            sources.files.insert(
                "main.hd",
                "use pkg.m1.{value1}\n\nfn main() -> void:\n    let v: i32 = value1()\n",
            );
            let (_, _, cold) = lookups_of(&sources, &store, packages, &goal);
            assert!(cold.render().contains("m1.hd"), "{}", cold.render());
            let (gets, bytes, out) = lookups_of(&sources, &store, packages, &goal);
            assert_eq!(
                out.counters.hit("package"),
                1,
                "{goal:?}, {modules} modules"
            );
            assert_eq!(out.render(), cold.render());
            if packages.check_roots.is_some() {
                let main = out.files.iter().position(|f| f == "main.hd").unwrap();
                let m1 = out.files.iter().position(|f| f == "m1.hd").unwrap();
                assert_eq!(out.uses[main], cold.uses[main]);
                assert!(out.uses[main].contains(&m1), "{:?}", out.uses[main]);
            }
            warm.push((gets, bytes));
        }
        assert_eq!(warm[0], warm[1], "{goal:?}: 100 and 1,000 modules");
    }
}
