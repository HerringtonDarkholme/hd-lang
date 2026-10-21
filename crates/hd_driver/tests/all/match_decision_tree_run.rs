//! The TIR of a nested-enum decision tree grows linearly.
//! Runtime arm and guard behavior is covered by conformance.

use std::fmt::Write as _;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// Enums `E0` to `E{depth}`, each `A` wrapping the one below, and a
/// staircase match on `E{depth}`: arm `k` matches `k` levels of `.A`
/// around `1` (the deepest) or `_`.
fn staircase(depth: usize) -> String {
    let mut s = String::from("enum E0:\n    A(i32)\n    B\n\n");
    for d in 1..=depth {
        let _ = write!(s, "enum E{d}:\n    A(E{})\n    B\n\n", d - 1);
    }
    let _ = writeln!(s, "fn f(x: E{depth}) -> i32:\n    match x:");
    let wrap = |inner: &str, levels: usize| {
        let mut p = inner.to_owned();
        for _ in 0..levels {
            p = format!(".A({p})");
        }
        p
    };
    let _ = writeln!(s, "        {} => {}", wrap("1", depth + 1), depth + 1);
    for k in (1..=depth).rev() {
        let _ = writeln!(s, "        {} => {k}", wrap("_", k));
    }
    s.push_str("        _ => 0\n\npub fn main() -> void:\n    pass\n");
    s
}

/// The rendered TIR of `fn f` in a program, from a cold store so the
/// body is checked here.
fn tir_of_f(main: &str) -> String {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &["app/main/f"],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(&host, "app", &Goal::Analyze);
    assert!(out.diags.is_empty(), "{}", out.render());
    out.tir_text
        .get("app/main/f")
        .unwrap_or_else(|| panic!("no body; have {:?}", out.tir_text.keys()))
        .clone()
}

#[test]
fn nested_enum_match_tir_grows_linearly() {
    let sizes: Vec<usize> = [4, 8, 16]
        .iter()
        .map(|&d| tir_of_f(&staircase(d)).lines().count())
        .collect();
    // Doubling the depth at most doubles the TIR, plus a constant.
    assert!(
        sizes[1] <= 2 * sizes[0] + 8 && sizes[2] <= 2 * sizes[1] + 8,
        "TIR lines at depth 4, 8, 16: {sizes:?}"
    );
}
