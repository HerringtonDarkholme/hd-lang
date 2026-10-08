//! `HeaderCheck(F)` asks its goals through the solver (trait-solver.md
//! §1.1, §3.7; resolution-and-interfaces.md §4.10.1): a supertrait that
//! holds through a bounded blanket impl, one whose blanket impl's bound
//! fails, and answers that depend on the folder's closure (§3.2).

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The rendered diagnostics of one analysis, the lines of `file` only.
fn analyze(files: &[(&str, &str)], executor: Executor, file: &str) -> Vec<String> {
    let mut src = MemorySources::default();
    for (path, text) in files {
        src.insert(path, text);
    }
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor,
    };
    build(&host, "app", &Goal::Analyze)
        .render()
        .lines()
        .filter(|l| l.contains(&format!(" {file}:")))
        .map(str::to_owned)
        .collect()
}

const SUPERS: &str = "\
pub trait Plain:
    fn plain(self) -> i32

pub trait Named:
    fn name(self) -> string

pub trait Tagged < Named:
    fn tag(self) -> i32

pub data Box[T]:
    pub v: T

impl Plain for i32:
    fn plain(self) -> i32:
        self

impl[T < Plain] Named for Box[T]:
    fn name(self) -> string:
        \"box\"

impl Tagged for Box[i32]:
    fn tag(self) -> i32:
        1

impl Tagged for Box[string]:
    fn tag(self) -> i32:
        2
";

#[test]
fn a_supertrait_holds_through_a_blanket_impl_and_fails_on_its_bound() {
    let lines = analyze(
        &[("a/defs.hd", SUPERS)],
        Executor::Serial(SerialOrder::Priority),
        "a/defs.hd",
    );
    // `Box[i32]: Named` holds through `impl[T < Plain] Named for Box[T]`;
    // `Box[string]: Named` commits to it and fails on `string: Plain`.
    assert_eq!(lines.len(), 1, "{lines:#?}");
    assert!(
        lines[0].contains(
            "missing-supertrait-implementation: Box[string] implements app/a.defs/Tagged but not Named"
        ),
        "{lines:#?}"
    );
}

const LIB: &str = "\
pub trait Tag:
    fn tag(self) -> i32

pub trait Pick[T]:
    fn pick(self) -> T

pub data Receiver:
    pub n: i32

pub data Tagged[T < Tag]:
    pub v: T
";

/// Owns `Product`, and so `impl Pick[Product] for Receiver` through the
/// trait argument; `impl Tag for i32` is misplaced here, which keeps it
/// among the unowned rows of this folder only.
const PROD: &str = "\
use pkg.lib.defs.{Pick, Receiver, Tag}

pub data Product:
    pub id: i32

impl Pick[Product] for Receiver:
    fn pick(self) -> Product:
        Product { id: self.n }

impl Tag for i32:
    fn tag(self) -> i32:
        self
";

/// Sees `prod`: both impls answer its header goals.
const WITH: &str = "\
use pkg.lib.defs.{Pick, Receiver, Tagged}
use pkg.prod.defs.{Product}

pub data Picks[T < Pick[Product]]:
    pub v: T

pub fn first(p: Picks[Receiver], t: Tagged[i32]) -> i32:
    1
";

/// Does not see `prod`: `i32: Tag` fails here.
const WITHOUT: &str = "\
use pkg.lib.defs.{Tagged}

pub fn second(t: Tagged[i32]) -> i32:
    1
";

#[test]
fn header_answers_follow_the_folder_closure_on_any_executor() {
    let files = [
        ("lib/defs.hd", LIB),
        ("prod/defs.hd", PROD),
        ("with/main.hd", WITH),
        ("without/main.hd", WITHOUT),
    ];
    let reference = Executor::Serial(SerialOrder::Priority);
    let with = analyze(&files, reference, "with/main.hd");
    assert!(with.is_empty(), "{with:#?}");
    let without = analyze(&files, reference, "without/main.hd");
    assert_eq!(without.len(), 1, "{without:#?}");
    assert!(
        without[0].contains("unsatisfied-trait-bound: i32 does not implement Tag"),
        "{without:#?}"
    );
    for executor in [
        Executor::Serial(SerialOrder::Fifo),
        Executor::Serial(SerialOrder::Shuffled(5)),
        Executor::Pool(4),
    ] {
        assert_eq!(
            analyze(&files, executor, "with/main.hd"),
            with,
            "{executor:?}"
        );
        assert_eq!(
            analyze(&files, executor, "without/main.hd"),
            without,
            "{executor:?}"
        );
    }
}

/// One analysis over a shared store: the file's lines and the counters.
fn analyze_in(
    store: &MemoryStore,
    files: &[(&str, &str)],
    file: &str,
) -> (Vec<String>, usize, usize) {
    let mut src = MemorySources::default();
    for (path, text) in files {
        src.insert(path, text);
    }
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(&host, "app", &Goal::Analyze);
    let lines = out
        .render()
        .lines()
        .filter(|l| l.contains(&format!(" {file}:")))
        .map(str::to_owned)
        .collect();
    (lines, out.counters.hit("graph"), out.counters.miss("graph"))
}

#[test]
fn a_dependency_impl_head_edit_invalidates_the_cached_header_findings() {
    let store = MemoryStore::default();
    let files = [
        ("lib/defs.hd", LIB),
        ("prod/defs.hd", PROD),
        ("with/main.hd", WITH),
        ("without/main.hd", WITHOUT),
    ];
    let (cold, hits, _) = analyze_in(&store, &files, "with/main.hd");
    assert!(cold.is_empty(), "{cold:#?}");
    assert_eq!(hits, 0);
    let (warm, hits, misses) = analyze_in(&store, &files, "with/main.hd");
    assert_eq!(warm, cold);
    assert_eq!(misses, 0, "a warm run misses nothing");
    assert!(hits >= 4, "every folder's findings hit: {hits}");
    // `prod` no longer owns `Pick[Product] for Receiver`: its impl head
    // changes, and so does the answer for `with`.
    let edited = PROD.replace(
        "impl Pick[Product] for Receiver",
        "impl Pick[i32] for Receiver",
    );
    let edited = edited.replace(
        "-> Product:\n        Product { id: self.n }",
        "-> i32:\n        self.n",
    );
    let files = [
        ("lib/defs.hd", LIB),
        ("prod/defs.hd", edited.as_str()),
        ("with/main.hd", WITH),
        ("without/main.hd", WITHOUT),
    ];
    let (after, _, _) = analyze_in(&store, &files, "with/main.hd");
    let (fresh, _, _) = analyze_in(&MemoryStore::default(), &files, "with/main.hd");
    assert_eq!(after, fresh, "warm and fresh agree");
    assert_eq!(after.len(), 1, "{after:#?}");
    assert!(after[0].contains("unsatisfied-trait-bound"), "{after:#?}");
}

#[test]
fn a_comment_edit_keeps_the_cached_findings_and_moves_their_positions() {
    let store = MemoryStore::default();
    let files = [("lib/defs.hd", LIB), ("without/main.hd", WITHOUT)];
    let (cold, _, _) = analyze_in(&store, &files, "without/main.hd");
    assert_eq!(cold.len(), 1, "{cold:#?}");
    let (warm, hits, misses) = analyze_in(&store, &files, "without/main.hd");
    assert_eq!(warm, cold);
    assert_eq!(misses, 0);
    assert!(hits >= 2, "{hits}");
    let commented = format!("// note\n{WITHOUT}");
    let files = [
        ("lib/defs.hd", LIB),
        ("without/main.hd", commented.as_str()),
    ];
    let (after, hits, misses) = analyze_in(&store, &files, "without/main.hd");
    let (fresh, _, _) = analyze_in(&MemoryStore::default(), &files, "without/main.hd");
    assert_eq!(after, fresh, "the position follows the edit");
    assert_ne!(after, cold, "the finding moved down a line");
    assert_eq!(misses, 0, "the interface key ignores comments");
    assert!(hits >= 2, "{hits}");
}
