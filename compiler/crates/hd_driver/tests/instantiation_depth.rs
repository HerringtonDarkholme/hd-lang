//! `types.generic.instantiation-depth.error` and `cli.build.depth-error`:
//! collection stops at the instantiation depth limit (codegen.md §13.4)
//! with `instantiation-too-deep` on the call that would exceed it, and a
//! check builds no program, so it reports nothing. Instance keys hash the
//! pool's shared types, never their trees, so a type that doubles per call
//! stops as fast as one that grows a level.

use std::time::{Duration, Instant};

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn run_in(store: &MemoryStore, executor: Executor, text: &str, goal: &Goal) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", text);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
        clock: &NoClock,
        executor,
    };
    build(&host, "app", goal)
}

fn run(text: &str, goal: &Goal) -> Output {
    let store = MemoryStore::default();
    run_in(&store, Executor::Serial(SerialOrder::Priority), text, goal)
}

fn whole_program() -> Goal {
    Goal::Program {
        entry: "main".into(),
    }
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
    let program = run(GROWING, &whole_program());
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

/// Each call pairs `T` with itself: the instance at depth `d` has a tree
/// of 2^d leaves, but only `d + 1` distinct types (the CLI case
/// `build-instantiation-too-deep`).
const DOUBLING: &str = "\
fn nest[T](value: T, depth: i32) -> i32:
    if depth == 0: return depth
    nest((value, value), depth - 1)

pub fn main() -> void:
    _ := nest(+1, +3)
";

#[test]
fn a_doubling_type_stops_well_under_a_second() {
    let start = Instant::now();
    let program = run(DOUBLING, &whole_program());
    let took = start.elapsed();
    assert_eq!(
        codes(&program),
        ["instantiation-too-deep"],
        "{}",
        program.render()
    );
    assert!(took < Duration::from_secs(1), "took {took:?}");
    let text = program.render();
    assert!(text.contains("33 deep, past the limit of 32"), "{text}");
    // The chain's first instances in full, the last one's 2^33-leaf
    // argument cut short.
    assert!(text.contains("main/nest[(i32, i32)]`"), "{text}");
    assert!(text.contains("((i32, i32), (i32, i32))"), "{text}");
    assert!(text.contains("...]`"), "{text}");
    assert!(text.len() < 1000, "{text}");
}

/// Generic calls at types that share their parts: pairs of pairs, and a
/// function type over one.
const SHARED: &str = "\
fn pair[T](value: T) -> (T, T):
    (value, value)

fn keep[T](value: T) -> T:
    value

fn apply[A, B](value: A, f: fn(A) -> B) -> B:
    f(value)

pub fn main() -> void $ Console:
    p := pair(pair(pair(+1)))
    _ := keep(p)
    _ := keep((p, p))
    _ := apply(p, pair)
    println(42)
";

#[test]
fn a_warm_rebuild_of_shared_types_emits_nothing() {
    let store = MemoryStore::default();
    let cold = run_in(
        &store,
        Executor::Serial(SerialOrder::Priority),
        SHARED,
        &whole_program(),
    );
    assert!(cold.diags.is_empty(), "{}", cold.render());
    assert!(cold.counters.emitted > 0);
    // A new run interns its types in another order: the keys are the same.
    let warm = run_in(
        &store,
        Executor::Serial(SerialOrder::Shuffled(7)),
        SHARED,
        &whole_program(),
    );
    assert!(warm.diags.is_empty(), "{}", warm.render());
    assert_eq!(warm.counters.emitted, 0);
    assert_eq!(warm.wasm, cold.wasm);
}
