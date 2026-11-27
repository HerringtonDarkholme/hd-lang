//! #1987: dyn construction, emission, path lookup and conversion facts walk
//! distinct instantiated supertraits, not the exponentially many diamond paths.

use std::fmt::Write as _;

use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_mono::work::DynWork;
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// A chain of `depth` diamonds plus `width` siblings sharing its top.
/// `Target` is deliberately last, after every route through the diamonds.
/// All nodes are methodless except D0/Target, so empty vtables must keep
/// their parent pointers. Both first-path and last-path calls are reachable.
fn diamond(width: usize, depth: usize) -> String {
    let mut s = String::from("trait D0:\n    fn d(&self) -> i32\n\n");
    for i in 0..depth {
        let _ = writeln!(
            s,
            "trait L{i} < D{i}\n\ntrait R{i} < D{i}\n\ntrait D{} < L{i} & R{i}\n",
            i + 1,
        );
    }
    for i in 0..width {
        let _ = writeln!(s, "trait B{i} < D{depth}\n");
    }
    s.push_str("trait Target:\n    fn t(&self) -> i32\n\ntrait Top < ");
    for i in 0..width {
        let _ = write!(s, "B{i} & ");
    }
    let _ = writeln!(s, "D{depth} & Target\n\nstruct S: pass\n");
    s.push_str("impl D0 for S:\n    fn d(&self) -> i32: 1\n\n");
    for i in 0..depth {
        let _ = writeln!(
            s,
            "impl L{i} for S\n\nimpl R{i} for S\n\nimpl D{} for S\n",
            i + 1
        );
    }
    for i in 0..width {
        let _ = writeln!(s, "impl B{i} for S\n");
    }
    s.push_str(
        "impl Target for S:\n    fn t(&self) -> i32: 2\n\nimpl Top for S\n\n\
         fn base(x: &dyn D0) -> i32: x.d()\n\n\
         fn tail(x: &dyn Target) -> i32: x.t()\n\n\
         fn go(x: &dyn Top) -> i32:\n    x.d() + x.t() + base(x) + tail(x)\n\n\
         pub fn main() -> void $ Console:\n    println(go(&S {}))\n",
    );
    s
}

/// #2415: `width` types coerced to `dyn Wide` before any call names
/// `Wide.called`, and a chain of `depth` traits whose `next` slot each
/// body fills only after it coerces the next type. Every `never_called`
/// slot stays pending; `called` and `next` resolve once per table.
fn late_slots(width: usize, depth: usize) -> String {
    let mut s = String::from(
        "trait Wide:\n    fn called(&self) -> i32\n    fn never_called_wide(&self) -> i32\n\n",
    );
    for i in 0..width {
        let _ = writeln!(
            s,
            "struct W{i}: pass\n\nimpl Wide for W{i}:\n    fn called(&self) -> i32: {i}\n    \
             fn never_called_wide(&self) -> i32: 0\n"
        );
    }
    for i in 0..depth {
        let _ = writeln!(
            s,
            "trait T{i}:\n    fn next(&self) -> i32\n    fn never_called_{i}(&self) -> i32\n\n\
             struct S{i}: pass\n"
        );
        let next = if i + 1 < depth {
            format!(
                "\n        let x: &dyn T{} = &S{} {{}}\n        x.next()",
                i + 1,
                i + 1
            )
        } else {
            " 1".to_owned()
        };
        let _ = writeln!(
            s,
            "impl T{i} for S{i}:\n    fn next(&self) -> i32:{next}\n    \
             fn never_called_{i}(&self) -> i32: 0\n"
        );
    }
    s.push_str("fn sum(all: List[*dyn Wide]) -> i32:\n    let total = +0\n");
    s.push_str("    for w in all:\n        total = total + w.called()\n    total\n\n");
    s.push_str("pub fn main() -> void $ Console:\n    let mut all: List[*dyn Wide] = []\n");
    for i in 0..width {
        let _ = writeln!(s, "    all.push(box(W{i} {{}}))");
    }
    s.push_str("    println(sum(all))\n");
    if depth > 0 {
        s.push_str("    let x: &dyn T0 = &S0 {}\n    println(x.next())\n");
    }
    s
}

fn work(source: &str) -> DynWork {
    work_and_wasm(source).0
}

fn work_and_wasm(source: &str) -> (DynWork, Vec<u8>) {
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
    let goal = Goal::Program {
        entry: "main".into(),
    };
    let (out, work) = hd_mono::work::count_dyn_work(|| build(&host, "app", &goal));
    assert!(!out.diags.has_errors(), "{}", out.render());
    let wasm = out.wasm.expect("no module");
    (work, wasm)
}

fn linear(small: DynWork, large: DynWork) {
    eprintln!("dyn DAG work: {small:?} -> {large:?}");
    for (phase, a, b) in [
        ("collection", small.collected, large.collected),
        ("emission", small.emitted, large.emitted),
        ("paths", small.paths, large.paths),
        ("conversion facts", small.conversions, large.conversions),
    ] {
        assert!(
            a > 0 && b > a,
            "{phase} was not exercised: {small:?}, {large:?}"
        );
        assert!(b <= 10 * a + 100, "{phase}: {a} -> {b}");
    }
}

#[test]
fn dyn_diamond_depth_counts_distinct_nodes_not_paths() {
    linear(work(&diamond(0, 2)), work(&diamond(0, 20)));
}

#[test]
fn dyn_diamond_width_1000_at_depth_20_is_linear() {
    linear(work(&diamond(100, 20)), work(&diamond(1000, 20)));
}

#[test]
fn repeated_dyn_coercions_share_collection_emission_and_conversion_walks() {
    let source = |n| {
        diamond(100, 20).replace(
            "    println(go(&S {}))\n",
            &"    println(go(&S {}))\n".repeat(n),
        )
    };
    let (small, large) = (work(&source(100)), work(&source(1000)));
    eprintln!("repeated coercion work: {small:?} -> {large:?}");
    // Only coercion-site requests grow. The single go body and its path
    // index are unchanged; constructing a graph per site would multiply
    // both width and depth instead.
    for (a, b) in [
        (small.collected, large.collected),
        (small.emitted, large.emitted),
        (small.conversions, large.conversions),
    ] {
        // At most four constant-time requests per added coercion in a
        // phase; traversing even the 100 shared branches again fails this.
        assert!(a > 0 && b <= a + 4 * 900, "{small:?} -> {large:?}");
    }
    assert!(small.paths > 0);
    assert_eq!(small.paths, large.paths);
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[test]
fn late_slots_resolve_once_each_at_width_1000_and_depth_20() {
    let (small, small_wasm) = work_and_wasm(&late_slots(100, 20));
    let (large, large_wasm) = work_and_wasm(&late_slots(1000, 20));
    eprintln!("late slot work: {small:?} -> {large:?}");
    // Each table built before its method's first call resolves that one
    // slot once: `called` per `Wide` table, `next` per chain table. No
    // `never_called` slot resolves, however many tables wait on it.
    assert_eq!(small.late_slots, 100 + 20);
    assert_eq!(large.late_slots, 1000 + 20);
    assert!(
        large.collected <= 10 * small.collected + 100,
        "{small:?} -> {large:?}"
    );
    for wasm in [&small_wasm, &large_wasm] {
        assert!(contains(wasm, "called"), "no called body is linked");
        assert!(
            !contains(wasm, "never_called"),
            "an uncalled body is linked"
        );
    }
    // The fold visits each table, trait and record once: no more than
    // tenfold work for tenfold tables.
    assert!(
        small.folded > 0 && large.folded <= 10 * small.folded + 100,
        "{small:?} -> {large:?}"
    );
}

/// Each root instance's `callee_reps` after building `source` with a
/// fresh store, so collection runs.
fn reps_of(source: &str) -> Vec<(String, hd_base::Hash128)> {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let store = hd_cache::MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let goal = Goal::Program {
        entry: "main".into(),
    };
    let (out, reps) = hd_mono::work::capture_reps(|| build(&host, "app", &goal));
    assert!(!out.diags.has_errors(), "{}", out.render());
    reps
}

fn reps_named(all: &[(String, hd_base::Hash128)], name: &str) -> hd_base::Hash128 {
    let found: Vec<_> = all.iter().filter(|(n, _)| n.ends_with(name)).collect();
    assert_eq!(found.len(), 1, "one instance of {name} in {all:?}");
    found[0].1
}

/// #2415 under the code-key rule (`docs/compiler/tiering.md` §6.4): which
/// slots `T` keeps depends on every caller, so `call_c`'s code, whose slot
/// index for `c` moves when the only call of `b` goes, must have a new
/// `callee_reps`, though its own TIR is unchanged. `plain`, which touches
/// no trait value, keeps its own.
#[test]
fn dropping_another_instances_call_changes_a_dyn_callers_reps() {
    let program = |b: &str| {
        format!(
            "trait T:\n    fn a(&self) -> i32\n    fn b(&self) -> i32\n    fn c(&self) -> i32\n\n\
             struct S: pass\n\n\
             impl T for S:\n    fn a(&self) -> i32: 1\n    fn b(&self) -> i32: 2\n    \
             fn c(&self) -> i32: 3\n\n\
             fn call_a(x: &dyn T) -> i32: x.a()\n\n\
             fn call_b(x: &dyn T) -> i32: x.{b}()\n\n\
             fn call_c(x: &dyn T) -> i32: x.c()\n\n\
             fn plain(n: i32) -> i32: n + 1\n\n\
             pub fn main() -> void $ Console:\n    s := S {{}}\n    \
             println(call_a(&s) + call_b(&s) + call_c(&s) + plain(1))\n"
        )
    };
    let with_b = reps_of(&program("b"));
    let without_b = reps_of(&program("a"));
    assert_ne!(
        reps_named(&with_b, "/call_c"),
        reps_named(&without_b, "/call_c"),
        "call_c's slot index moved, so its code key must change"
    );
    assert_eq!(
        reps_named(&with_b, "/plain"),
        reps_named(&without_b, "/plain"),
        "an instance that touches no trait value keeps its code key"
    );
}
