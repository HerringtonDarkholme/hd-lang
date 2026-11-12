//! `types.generic.instantiation-depth.error` and `cli.build.depth-error`:
//! collection stops at the instantiation depth limit (`crates/hd_mono/DESIGN.md` §13.4)
//! with `instantiation-too-deep` on the call that would exceed it, and a
//! check builds no program, so it reports no depth limit. A generic call
//! cycle whose type arguments grow is found by checking
//! (`types.generic.polymorphic-recursion`). Instance keys hash the
//! pool's shared types, never their trees, so a type that doubles per call
//! stops as fast as one that grows a level.

use std::fmt::Write as _;

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

struct Box[T]:
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

/// Each round pairs the receiver with itself: the instance at depth `d`
/// has a tree of 2^d leaves, but only `d + 1` distinct types.
const DOUBLING: &str = "\
trait Grow:
    fn grow(self, depth: i32) -> i32

struct Pair[A, B]:
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

/// A chain of `levels` generic calls, each passing a pair of its own
/// argument: the last instance's argument has 2^levels leaves, but the
/// chain has only `levels + 1` distinct types.
fn doubling_chain(levels: usize) -> String {
    let mut s = String::from("struct Pair[A, B]:\n    a: A\n    b: B\n\n");
    for i in 0..levels {
        let _ = writeln!(
            s,
            "fn d{i}[T](value: T) -> i32:\n    d{}(Pair {{ a: value, b: value }})\n",
            i + 1
        );
    }
    let _ = writeln!(
        s,
        "fn d{levels}[T](value: T) -> i32:\n    0\n\npub fn main() -> void:\n    _ := d0(+1)"
    );
    s
}

/// The type nodes collection walks to build `text` with `store`.
fn type_steps(store: &MemoryStore, text: &str) -> (Output, u64) {
    hd_mono::work::count_type_steps(|| {
        run_in(
            store,
            Executor::Serial(SerialOrder::Priority),
            text,
            &whole_program(),
        )
    })
}

#[test]
fn a_doubling_type_stops_without_exponential_work() {
    // Each level adds one type and one instance. Collection's memo walks
    // each type once; hashes that keep a memo per call walk a level's
    // distinct types again, so the steps may grow quadratically: about
    // twice as many from 6 to 10 levels as from 2 to 6. A walk of each
    // type's tree doubles per level: 16 times as many.
    let store = MemoryStore::default();
    let steps = |levels| {
        let (out, steps) = type_steps(&store, &doubling_chain(levels));
        assert!(out.wasm.is_some(), "{}", out.render());
        steps
    };
    // A first build checks std into the store, so the measured ones
    // differ only in their chains.
    steps(0);
    let (base, small, large) = (steps(2), steps(6), steps(10));
    eprintln!("type steps: 2 levels {base}, 6 levels {small}, 10 levels {large}");
    assert!(small > base, "no type steps counted for the chain");
    assert!(
        large - small <= 4 * (small - base),
        "2 levels: {base}, 6 levels: {small}, 10 levels: {large}"
    );
    // Without the depth limit the doubling cycle reaches 2^33 leaves; the
    // limit stops it after 33 levels.
    let (program, _) = type_steps(&store, DOUBLING);
    assert_eq!(
        codes(&program),
        ["instantiation-too-deep"],
        "{}",
        program.render()
    );
    let text = program.render();
    assert!(text.contains("33 deep, past the limit of 32"), "{text}");
    // The chain's first instances in full, the last one's 2^33-leaf
    // argument cut short.
    assert!(
        text.contains("main/deeper[app.main.Pair[i32, i32]]`"),
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
fn a_warm_rebuild_of_shared_types_links_the_same_module() {
    let store = MemoryStore::default();
    let cold = run_in(
        &store,
        Executor::Serial(SerialOrder::Priority),
        SHARED,
        &whole_program(),
    );
    assert!(cold.diags.is_empty(), "{}", cold.render());
    // A new run interns its types in another order: the keys are the same.
    let warm = run_in(
        &store,
        Executor::Serial(SerialOrder::Shuffled(7)),
        SHARED,
        &whole_program(),
    );
    assert!(warm.diags.is_empty(), "{}", warm.render());
    assert_eq!(warm.wasm, cold.wasm);
}

/// A generic call cycle across two files whose type argument grows by
/// one `List` each round: the folder's joined summaries stop it at check
/// time (`types.generic.polymorphic-recursion`).
const CYCLE_A: &str = "\
use pkg.b.{eat}

pub fn bite[T](xs: List[T], depth: i32) -> i32:
    if depth == 0: 0
    else: eat([xs], depth - 1) + 1
";

const CYCLE_B: &str = "\
use pkg.a.{bite}

pub fn eat[T](xs: List[T], depth: i32) -> i32:
    bite(xs, depth)
";

const CYCLE_MAIN: &str = "\
use pkg.a.{bite}

pub fn main() -> void:
    _ := bite([1], 3)
";

fn check_folder_cycle(store: &MemoryStore, a: &str, b: &str, order: SerialOrder) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", CYCLE_MAIN);
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
        CYCLE_A,
        CYCLE_B,
        SerialOrder::Priority,
    );
    growing_folder_call(&out, "a.hd", CYCLE_A, "eat([xs]");
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
    let a = CYCLE_A.replace("eat([xs]", "eat(xs");
    let store = MemoryStore::default();
    let cold = check_folder_cycle(&store, &a, CYCLE_B, SerialOrder::Priority);
    assert!(cold.diags.is_empty(), "{}", cold.render());
    let warm = check_folder_cycle(&store, &a, CYCLE_B, SerialOrder::Shuffled(41));
    assert!(warm.diags.is_empty(), "{}", warm.render());
    assert!(warm.counters.modules_checked.is_empty());
    assert!(warm.counters.tir_decoded.is_empty());
    assert_eq!(warm.counters.bodies_decoded, 0);
}

#[test]
fn a_folder_call_cycle_tracks_edits_without_rechecking_unchanged_bodies() {
    let plain_a = CYCLE_A.replace("eat([xs]", "eat(xs");
    let growing_b = CYCLE_B.replace("bite(xs", "bite([xs]");
    let store = MemoryStore::default();
    let base = check_folder_cycle(&store, &plain_a, CYCLE_B, SerialOrder::Priority);
    assert!(base.diags.is_empty(), "{}", base.render());
    for (a, b, module, file, call) in [
        (CYCLE_A, CYCLE_B, "app.a", "a.hd", "eat([xs]"),
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
        let fixed = check_folder_cycle(&store, &plain_a, CYCLE_B, SerialOrder::Shuffled(17));
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
