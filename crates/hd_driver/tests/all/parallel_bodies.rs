//! `Body(m)` checks a module's function bodies on the pool in batches
//! (`crates/hd_tir/DESIGN.md` §4.13.1). The output does not depend on it:
//! a serial run and pool runs give the same diagnostics, the same Wasm
//! and the same cache entries.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use hd_cache::{CacheStore, EntryKind, MemoryStore};
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// A module with more bodies than one batch, each outside M1: public
/// functions, and a trait implementation's methods.
fn program(errors: bool) -> String {
    let mut s = String::from(
        "pub struct Cell:\n    pub value: i32\n\npub trait Get:\n    fn get(self) -> i32\n    fn twice(self) -> i32\n\nimpl Get for Cell:\n    fn get(self) -> i32: self.value\n    fn twice(self) -> i32: self.value * +2\n\npub fn step0(x: i32) -> i32:\n    x + +1\n\n",
    );
    for i in 1..80 {
        let _ = write!(
            s,
            "pub fn step{i}(x: i32) -> i32:\n    step{}(x) + +{i}\n\n",
            i - 1
        );
        if errors && i == 50 {
            // `cannot-infer-type` only while the module holds no error.
            s.push_str("pub fn early() -> i32:\n    a := []\n    +0\n\n");
        }
        if errors && i == 60 {
            s.push_str("pub fn bad() -> i32:\n    \"no\"\n\npub fn late() -> i32:\n    b := []\n    +0\n\n");
        }
    }
    s.push_str(
        "pub fn main() -> void $ Console:\n    c := Cell { value: step79(+1) }\n    println(\"${c.get() + c.twice()}\")\n",
    );
    s
}

fn run(
    src: &str,
    goal: &Goal,
    executor: Executor,
) -> (Output, BTreeMap<(EntryKind, u128), Vec<u8>>) {
    let mut sources = MemorySources::default();
    sources.insert("app/main.hd", src);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor,
    };
    let out = build(&host, "demo", goal);
    let entries = store
        .drain_new()
        .into_iter()
        .filter_map(|(kind, key)| Some(((kind, key.0), store.get(kind, &key)?.to_vec())))
        .collect();
    (out, entries)
}

const EXECUTORS: [Executor; 2] = [Executor::Pool(2), Executor::Pool(4)];

#[test]
fn an_error_free_module_builds_the_same_on_the_pool() {
    let goal = Goal::Program {
        entry: "app.main".into(),
    };
    let src = program(false);
    let (serial, serial_entries) = run(&src, &goal, Executor::Serial(SerialOrder::Priority));
    assert!(serial.wasm.is_some(), "{}", serial.render());
    for executor in EXECUTORS {
        let (out, entries) = run(&src, &goal, executor);
        assert_eq!(out.render(), serial.render(), "{executor:?}");
        assert_eq!(out.wasm, serial.wasm, "{executor:?}");
        assert!(
            entries == serial_entries,
            "{executor:?}: cache entries differ"
        );
    }
}

#[test]
fn a_module_with_errors_reports_the_same_on_the_pool() {
    let src = program(true);
    let (serial, serial_entries) = run(
        &src,
        &Goal::Analyze,
        Executor::Serial(SerialOrder::Priority),
    );
    let text = serial.render();
    // The first error ends the parallel part; `late` comes after it, so
    // its `[]` is not reported.
    assert_eq!(text.matches("cannot-infer-type").count(), 1, "{text}");
    assert!(text.contains("type-mismatch"), "{text}");
    for executor in EXECUTORS {
        let (out, entries) = run(&src, &Goal::Analyze, executor);
        assert_eq!(out.render(), text, "{executor:?}");
        assert!(
            entries == serial_entries,
            "{executor:?}: cache entries differ"
        );
    }
}
