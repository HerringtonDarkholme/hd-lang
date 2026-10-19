//! `types.generic.instantiation-depth.error` and `cli.build.depth-error`:
//! collection stops at the instantiation depth limit (`crates/hd_mono/DESIGN.md` §13.4)
//! with `instantiation-too-deep` on the call that would exceed it, and a
//! check builds no program, so it reports no depth limit. A generic call
//! cycle whose type arguments grow is found by checking
//! (`types.generic.polymorphic-recursion`). Instance keys hash the
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

/// Each call nests `T` one tuple deeper: a generic call cycle whose type
/// argument grows (`types.generic.polymorphic-recursion`), which checking
/// finds from the calls alone.
const GROWING: &str = "\
fn wrap[T](value: T, depth: i32) -> i32:
    if depth == 0: return depth
    wrap((value, depth), depth - 1)

pub fn main() -> void:
    _ := wrap(+1, +3)
";

#[test]
fn a_check_reports_a_growing_call_cycle() {
    let checked = run(GROWING, &Goal::Analyze);
    assert_eq!(
        codes(&checked),
        ["instantiation-too-deep"],
        "{}",
        checked.render()
    );
    // On the growing call, line 3.
    let text = checked.render();
    let call = GROWING.find("wrap((value").expect("the call");
    assert!(text.contains(&format!("main.hd:{call}..")), "{text}");
    assert!(text.contains("passes `(T, i32)` for `T`"), "{text}");
    assert!(text.contains("cycle `wrap -> wrap`"), "{text}");
}

#[test]
fn a_build_reports_the_cycle_before_instantiating() {
    let program = run(GROWING, &whole_program());
    assert_eq!(
        codes(&program),
        ["instantiation-too-deep"],
        "{}",
        program.render()
    );
    assert!(program.wasm.is_none());
    assert!(program.render().contains("generic call cycle"));
}

/// Each round boxes the receiver once more through a call with no fixed
/// callee, `value.grow` through the bound: no generic call cycle shows
/// it, so only the depth limit stops it.
const BOUND_GROWTH: &str = "\
trait Grow:
    fn grow(self, depth: i32) -> i32

data Box[T]:
    value: T

impl[T] Grow for Box[T]:
    fn grow(self, depth: i32) -> i32:
        if depth == 0: return depth
        deeper(Box { value: self }, depth - 1)

fn deeper[G < Grow](value: G, depth: i32) -> i32:
    value.grow(depth)

pub fn main() -> void:
    _ := deeper(Box { value: +1 }, +3)
";

#[test]
fn growth_through_a_bound_stops_at_the_depth_limit() {
    let program = run(BOUND_GROWTH, &whole_program());
    assert_eq!(
        codes(&program),
        ["instantiation-too-deep"],
        "{}",
        program.render()
    );
    assert!(program.wasm.is_none());
    // On the growing call, line 10.
    let text = program.render();
    let call = BOUND_GROWTH
        .find("deeper(Box { value: self }")
        .expect("the call");
    assert!(text.contains(&format!("main.hd:{call}..")), "{text}");
    assert!(text.contains("33 deep, past the limit of 32"), "{text}");
}

#[test]
fn a_check_reports_no_depth_error() {
    let checked = run(BOUND_GROWTH, &Goal::Analyze);
    assert!(!checked.diags.has_errors(), "{}", checked.render());
}

/// Each round pairs the receiver with itself: the instance at depth `d`
/// has a tree of 2^d leaves, but only `d + 1` distinct types.
const DOUBLING: &str = "\
trait Grow:
    fn grow(self, depth: i32) -> i32

data Pair[A, B]:
    a: A
    b: B

impl[A, B] Grow for Pair[A, B]:
    fn grow(self, depth: i32) -> i32:
        if depth == 0: return depth
        deeper(Pair { a: self, b: self }, depth - 1)

fn deeper[G < Grow](value: G, depth: i32) -> i32:
    value.grow(depth)

pub fn main() -> void:
    _ := deeper(Pair { a: +1, b: +2 }, +3)
";

#[test]
fn a_doubling_type_stops_without_exponential_work() {
    let start = Instant::now();
    let program = run(DOUBLING, &whole_program());
    let took = start.elapsed();
    assert_eq!(
        codes(&program),
        ["instantiation-too-deep"],
        "{}",
        program.render()
    );
    // The bound catches exponential work (the chain reaches 2^33 leaves
    // without the depth limit), not speed: tests run in the dev profile,
    // where this takes 1-2 s.
    assert!(took < Duration::from_secs(10), "took {took:?}");
    let text = program.render();
    assert!(text.contains("33 deep, past the limit of 32"), "{text}");
    // The chain's first instances in full, the last one's 2^33-leaf
    // argument cut short.
    assert!(
        text.contains("main/deeper[mut app.main.Pair[i32, i32]]`"),
        "{text}"
    );
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

/// Each round wraps `T` in one more legacy `mut List`, through a generic
/// call cycle that spans two files: the folder's joined summaries stop
/// it before collection (`types.generic.polymorphic-recursion`). A list value
/// type argument stays exact (`hd_mono::holds_coll`), so the legacy view
/// keeps the argument erased.
const ERASED_A: &str = "\
use pkg.b.{eat}

pub fn bite[T](xs: mut List[T], depth: i32) -> i32:
    if depth == 0: 0
    else: eat([xs], depth - 1) + 1
";

const ERASED_B: &str = "\
use pkg.a.{bite}

pub fn eat[T](xs: mut List[T], depth: i32) -> i32:
    bite(xs, depth)
";

const ERASED_MAIN: &str = "\
use pkg.a.{bite}

pub fn main() -> void:
    _ := bite([1], 3)
";

#[test]
fn polymorphic_recursion_through_erasure_stops() {
    let mut src = MemorySources::default();
    src.insert("main.hd", ERASED_MAIN);
    src.insert("a.hd", ERASED_A);
    src.insert("b.hd", ERASED_B);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let program = build(&host, "app", &whole_program());
    assert_eq!(
        codes(&program),
        ["instantiation-too-deep"],
        "{}",
        program.render()
    );
    assert!(program.wasm.is_none());
    let text = program.render();
    assert!(text.contains("generic call cycle"), "{text}");
}

fn check_folder_cycle(store: &MemoryStore, a: &str, b: &str, order: SerialOrder) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", ERASED_MAIN);
    src.insert("a.hd", a);
    src.insert("b.hd", b);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(order),
    };
    build(&host, "app", &Goal::Analyze)
}

fn growing_folder_call(out: &Output, file: &str, text: &str, call: &str) {
    assert_eq!(codes(out), ["instantiation-too-deep"], "{}", out.render());
    let at = text.find(call).expect("the growing call");
    let rendered = out.render();
    assert!(rendered.contains(&format!("{file}:{at}..")), "{rendered}");
    assert!(rendered.contains("generic call cycle"), "{rendered}");
    assert_eq!(out.counters.bodies_decoded, 0);
    assert!(out.counters.tir_decoded.is_empty());
}

#[test]
fn a_check_reports_a_growing_call_cycle_across_files() {
    let out = check_folder_cycle(
        &MemoryStore::default(),
        ERASED_A,
        ERASED_B,
        SerialOrder::Priority,
    );
    growing_folder_call(&out, "a.hd", ERASED_A, "eat([xs]");
    // Equal node indices and byte offsets in different files are two calls.
    let a = "use pkg.b.{feed}\n\npub fn bite[T](x: T) -> void:\n    feed([x])\n";
    let b = "use pkg.a.{bite}\n\npub fn feed[T](x: T) -> void:\n    bite([x])\n";
    assert_eq!(a.find("feed([x])"), b.find("bite([x])"));
    let mut src = MemorySources::default();
    src.insert("a.hd", a);
    src.insert("b.hd", b);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let two = build(&host, "app", &Goal::Analyze);
    assert_eq!(
        codes(&two),
        ["instantiation-too-deep", "instantiation-too-deep"],
        "{}",
        two.render()
    );
    for file in ["a.hd", "b.hd"] {
        assert!(
            two.render().contains(&format!("{file}:")),
            "{}",
            two.render()
        );
    }
}

#[test]
fn a_check_accepts_a_same_shape_call_cycle_across_files() {
    let a = ERASED_A.replace("eat([xs]", "eat(xs");
    let store = MemoryStore::default();
    let cold = check_folder_cycle(&store, &a, ERASED_B, SerialOrder::Priority);
    assert!(cold.diags.is_empty(), "{}", cold.render());
    let warm = check_folder_cycle(&store, &a, ERASED_B, SerialOrder::Shuffled(41));
    assert!(warm.diags.is_empty(), "{}", warm.render());
    assert!(warm.counters.modules_checked.is_empty());
    assert!(warm.counters.tir_decoded.is_empty());
    assert_eq!(warm.counters.bodies_decoded, 0);
}

#[test]
fn a_folder_call_cycle_tracks_edits_without_rechecking_unchanged_bodies() {
    let plain_a = ERASED_A.replace("eat([xs]", "eat(xs");
    let growing_b = ERASED_B.replace("bite(xs", "bite([xs]");
    let store = MemoryStore::default();
    let base = check_folder_cycle(&store, &plain_a, ERASED_B, SerialOrder::Priority);
    assert!(base.diags.is_empty(), "{}", base.render());
    for (a, b, module, file, call) in [
        (ERASED_A, ERASED_B, "app.a", "a.hd", "eat([xs]"),
        (
            plain_a.as_str(),
            growing_b.as_str(),
            "app.b",
            "b.hd",
            "bite([xs]",
        ),
    ] {
        let bad = check_folder_cycle(&store, a, b, SerialOrder::Shuffled(29));
        growing_folder_call(&bad, file, if file == "a.hd" { a } else { b }, call);
        assert_eq!(bad.counters.modules_checked, [module]);
        let warm = check_folder_cycle(&store, a, b, SerialOrder::Shuffled(7));
        growing_folder_call(&warm, file, if file == "a.hd" { a } else { b }, call);
        assert!(warm.counters.modules_checked.is_empty());
        // Comments move the diagnostic while the summary and bodies stay hot.
        let shifted_a = format!("# shifted\n\n{a}");
        let shifted_b = format!("# shifted\n\n{b}");
        let shifted = check_folder_cycle(&store, &shifted_a, &shifted_b, SerialOrder::Shuffled(53));
        growing_folder_call(
            &shifted,
            file,
            if file == "a.hd" {
                &shifted_a
            } else {
                &shifted_b
            },
            call,
        );
        assert!(shifted.counters.modules_checked.is_empty());
        let fixed = check_folder_cycle(&store, &plain_a, ERASED_B, SerialOrder::Shuffled(17));
        assert!(fixed.diags.is_empty(), "{}", fixed.render());
        assert!(fixed.counters.modules_checked.is_empty());
        assert!(fixed.counters.tir_decoded.is_empty());
    }
}

/// Recursion that passes its erased parameter through unchanged shares
/// one instance at one concrete shape: no error.
const ERASED_SHARE: &str = "\
fn walk[T](xs: List[T], n: i32) -> i32:
    if n == 0: 0
    else: walk(xs, n - 1) + 1

fn a[T](x: T, n: i32) -> i32:
    if n == 0: 0
    else: b(x, n - 1)

fn b[T](x: T, n: i32) -> i32:
    if n == 0: 0
    else: a(x, n - 1)

pub fn main() -> void:
    _ := walk([1, 2], 2)
    _ := a([1], 2)
";

#[test]
fn same_shape_recursion_through_erasure_builds() {
    let program = run(ERASED_SHARE, &whole_program());
    assert!(program.diags.is_empty(), "{}", program.render());
    assert!(program.wasm.is_some());
}
