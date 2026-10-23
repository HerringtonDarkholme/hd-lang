//! An inlined callee is part of its caller's code (`docs/compiler/tir-v2.md`,
//! "The inliner"): after an edit to a small std function, a warm build
//! emits what a cold build of the edited sources does, not the caller with
//! the old copy.

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build, std_sources};
use hd_sched::SerialOrder;

/// A build of std as the root package, with `std.inlined.bump` returning
/// `ret` and `main` printing `bump(41)`; `own` renders `main` after the
/// passes.
fn build_std(store: &MemoryStore, ret: &str, own: &[&str]) -> Output {
    let mut sources = std_sources();
    let callee = format!("pub fn bump(x: i64) -> i64:\n    {ret}\n");
    sources.insert("inlined.hd", &callee);
    sources.insert(
        "main.hd",
        "use std.inlined.bump\n\npub fn main() -> void $ Console:\n    println(bump(41))\n",
    );
    let host = Host {
        render_tir: own,
        sources: &sources,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(
        &host,
        "std",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.wasm.is_some(), "{}", out.render());
    out
}

#[test]
fn an_edit_to_an_inlined_callee_rebuilds_its_caller() {
    let store = MemoryStore::default();
    let before = build_std(&store, "x + 1", &["std/main/main"]);
    let own = before.own_text.get("std/main/main").map(|o| o.text.clone());
    let own = own.unwrap_or_else(|| panic!("no own view; have {:?}", before.own_text.keys()));
    assert!(own.contains("Scope") && !own.contains("bump"), "{own}");
    let warm = build_std(&store, "x + 2", &[]);
    let cold = build_std(&MemoryStore::default(), "x + 2", &[]);
    assert_ne!(warm.wasm, before.wasm);
    assert_eq!(warm.wasm, cold.wasm);
}
