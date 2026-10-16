//! The overlap check is near-linear in a trait's impl count
//! (`crates/hd_resolve/DESIGN.md` §4.12.3): a head is unified only with
//! the earlier heads whose outer constructors can match its own, so many
//! impls of one trait for distinct types add a few pairs each, not one
//! per earlier impl.

use std::fmt::Write as _;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// A program with `n` data types, each implementing one trait.
fn program(n: usize) -> String {
    let mut s = String::from("pub trait Size:\n    fn size(self) -> i32\n\n");
    for i in 0..n {
        let _ = write!(
            s,
            "data T{i}:\n    v: i32\n\nimpl Size for T{i}:\n    fn size(self) -> i32:\n        self.v\n\n"
        );
    }
    s.push_str("pub fn main() -> void $ Console:\n    println(T0 { v: 1 }.size())\n");
    s
}

/// The impl head pairs the coherence check unified for `program(n)`.
fn pairs(n: usize) -> usize {
    let mut src = MemorySources::default();
    let main = program(n);
    src.insert("main.hd", &main);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let goal = Goal::Program {
        entry: "main".into(),
    };
    let out = build_packages(&host, "app", &Packages::default(), &goal);
    assert!(out.wasm.is_some(), "{}", out.render());
    out.counters.overlap_pairs
}

#[test]
fn impls_of_one_trait_for_distinct_types_add_few_pairs() {
    let (small, large) = (pairs(100), pairs(200));
    // Pairwise, the hundred more impls would add about 15,000 pairs.
    assert!(large - small <= 200, "{small} then {large}");
}
