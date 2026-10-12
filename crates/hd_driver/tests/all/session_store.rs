//! The session overlay store (repl-plan.md §6, job C3): a session's builds
//! read the inner store but write only to the overlay's memory, and a
//! second build in the same overlay hits what the first one wrote.

use hd_cache::{CacheStore, MemoryStore, OverlayStore};
use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const MAIN: &str = "pub fn main() -> void:\n    pass\n";

fn build(store: &dyn CacheStore) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", MAIN);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build_packages(
        &host,
        "app",
        &Packages::default(),
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{}", out.render());
    out
}

fn hits(out: &Output) -> usize {
    out.counters.hits.values().sum()
}

#[test]
fn session_builds_add_nothing_to_the_inner_store() {
    let inner = MemoryStore::default();
    // The inner store holds what a plain build left, as the disk cache does.
    build(&inner);
    let before = inner.len();
    assert!(before > 0);
    let overlay = OverlayStore::new(&inner);
    let first = build(&overlay);
    assert_eq!(inner.len(), before);
    assert!(hits(&first) > 0, "the session reads the inner store");
}

#[test]
fn a_second_build_in_the_overlay_hits_its_entries() {
    let inner = MemoryStore::default();
    let overlay = OverlayStore::new(&inner);
    let first = build(&overlay);
    assert!(inner.is_empty());
    assert!(!overlay.memory().is_empty());
    let written = overlay.memory().len();
    let second = build(&overlay);
    assert!(hits(&second) > hits(&first));
    assert_eq!(overlay.memory().len(), written);
    assert!(inner.is_empty());
}
