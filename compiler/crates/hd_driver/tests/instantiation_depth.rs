//! `types.generic.instantiation-depth.error` and `cli.build.depth-error`:
//! collection stops at the instantiation depth limit (codegen.md §13.4)
//! with `instantiation-too-deep` on the call that would exceed it, and a
//! check builds no program, so it reports nothing.

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn run(text: &str, goal: &Goal) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", text);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", goal)
}

fn codes(out: &Output) -> Vec<&str> {
    out.diags.code.iter().map(|c| c.as_str()).collect()
}

/// Each call nests `T` one tuple deeper: `i32`, `(i32, i32)`,
/// `((i32, i32), i32)`, and so on, with no end.
const GROWING: &str = "\
fn wrap[T](value: T, depth: i32) -> i32:
    if depth == 0: return depth
    wrap((value, depth), depth - 1)

pub fn main() -> void:
    _ := wrap(+1, +3)
";

#[test]
fn polymorphic_recursion_stops_at_the_depth_limit() {
    let program = run(
        GROWING,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert_eq!(
        codes(&program),
        ["instantiation-too-deep"],
        "{}",
        program.render()
    );
    assert!(program.wasm.is_none());
    // On the growing call, line 3.
    let text = program.render();
    let call = GROWING.find("wrap((value").expect("the call");
    assert!(text.contains(&format!("main.hd:{call}..")), "{text}");
    assert!(text.contains("33 deep, past the limit of 32"), "{text}");
}

#[test]
fn a_check_reports_no_depth_error() {
    let checked = run(GROWING, &Goal::Analyze);
    assert!(!checked.diags.has_errors(), "{}", checked.render());
}
