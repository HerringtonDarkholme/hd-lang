//! #2031: each emitted function releases its sub-body's exit set at its
//! one exit. A closure is an instance whose TIR body is its enclosing
//! function's, so the exit set it reads is the whole body's. Each
//! sub-body's rows are indexed once per body, and an emitter visits only
//! its own: a function with n closures does O(n) exit work, not O(n^2).

use std::fmt::Write as _;

use hd_driver::{EngineTarget, Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// `main` owns n strings and n closures, each owning its string
/// parameter: the body's exit set has about 2n rows over n + 1 sub-bodies.
fn program(n: usize) -> String {
    let mut text = String::from("pub fn main() -> void $ Console:\n    let t = +0\n");
    for i in 0..n {
        let _ = write!(
            text,
            "    s{i} := \"a$t\"\n    c{i} := fn(x: string) -> i32: i32(x.len()) + t\n    \
             t = c{i}(\"b$t\") + i32(s{i}.len())\n"
        );
    }
    text.push_str("    println(\"$t\")\n");
    text
}

fn build(source: &str) {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        target: EngineTarget::V8,
        ..Packages::default()
    };
    let output = build_packages(
        &host,
        "app",
        &packages,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(!output.diags.has_errors(), "{}", output.render());
    assert!(output.wasm.is_some());
}

#[test]
fn closure_exit_release_visits_its_own_rows() {
    let worker = std::thread::Builder::new()
        .stack_size(hd_sched::pool::WORKER_STACK)
        .spawn(|| {
            let ((), small) = hd_wasm::work::count_exit_rows(|| build(&program(20)));
            let ((), large) = hd_wasm::work::count_exit_rows(|| build(&program(200)));
            eprintln!("exit rows: n=20 {small}, n=200 {large}");
            assert!(small >= 40, "n=20 {small}");
            assert!(large <= small * 12, "n=20 {small}, n=200 {large}");
        });
    assert!(worker.is_ok_and(|w| w.join().is_ok()));
}
