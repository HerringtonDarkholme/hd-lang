//! Exact local-function visibility diagnostic. Runtime behavior is in conformance.

use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

/// The rendered diagnostics of a program that must not build.
fn errors_of(main: &str) -> String {
    let out = built(main);
    assert!(out.wasm.is_none(), "the program must not build");
    out.render()
}

#[test]
fn a_local_function_is_not_visible_before_its_declaration() {
    let main = "\
fn shipping(weight: i32) -> i32:
    early := surcharge(weight)
    fn surcharge(w: i32) -> i32: w * 2
    early

pub fn main() -> void $ Console:
    println(shipping(4))
";
    let errors = errors_of(main);
    assert!(errors.contains("unknown-name"), "{errors}");
    assert!(errors.contains("surcharge"), "{errors}");
}
