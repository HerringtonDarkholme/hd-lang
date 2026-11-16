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

fn work(source: &str) -> DynWork {
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
    assert!(out.wasm.is_some(), "no module");
    work
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
