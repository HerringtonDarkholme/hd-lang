//! #1540: the emitter decides which enum reads through `&` a match reads
//! in place (`Em::lazy_borrow`, `Em::nested_lazy`) from a per-body use
//! fact, so a body with n matches through `&` costs O(n), not n body
//! scans (.agents/skills/shared-rules.md "Cost Model").

use std::fmt::Write as _;

use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// `sum` matches `a: &A` n times; each match reads `a` through `&`, and
/// its `.Maybe(.Some(x))` arm reads the nested optional in place too.
fn matches(n: usize) -> String {
    let mut s = String::from(
        "enum A:\n    Maybe(i32?)\n    Must(i32)\n\nfn sum(a: &A) -> i32:\n    let s = +0\n",
    );
    for k in 0..n {
        let _ = write!(
            s,
            "    v{k} := match a:\n        .Maybe(.Some(x)) => x\n        \
             .Maybe(.None) => +{k}\n        .Must(y) => y\n    s = s + v{k}\n"
        );
    }
    s.push_str(
        "    s\n\npub fn main() -> void $ Console:\n    a := A.Must(+1)\n    println(sum(&a))\n",
    );
    s
}

/// The use-fact work of building `matches(n)` to Wasm.
fn work(n: usize) -> u64 {
    source_work(&matches(n))
}

fn source_work(source: &str) -> u64 {
    let mut src = MemorySources::default();
    src.insert("main.hd", source);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let goal = Goal::Program {
        entry: "main".into(),
    };
    let (out, work) = hd_wasm::work::count_use_facts(|| build(&host, "app", &goal));
    assert!(!out.diags.has_errors(), "{}", out.render());
    assert!(out.wasm.is_some(), "no wasm");
    work
}

#[test]
fn matches_through_a_reference_cost_linear_use_fact_work() {
    let (base, small, large) = (work(0), work(100), work(1000));
    eprintln!("use-fact work: n=0 {base}, n=100 {small}, n=1000 {large}");
    // The program's other bodies are the same at every size, so the work
    // the matches add is `large - base` and `small - base`. Linear work
    // adds 9 times as much from 100 to 1,000 as from 0 to 100; a body
    // scan per match adds about 99 times as much.
    assert!(small > base, "no use-fact work counted for the matches");
    assert!(
        large - small <= 10 * (small - base),
        "n=0: {base}, n=100: {small}, n=1000: {large}"
    );
}

/// All nested functions belong to one TIR body. Width adds sibling
/// captures/calls; depth adds wrappers without multiplying their source.
fn closures(width: usize, depth: usize, loops: bool) -> String {
    let mut source = String::from("fn run() -> i32:\n");
    let mut indent = "    ".to_owned();
    for level in 0..depth {
        writeln!(source, "{indent}nested{level} := fn() -> i32:").expect("string");
        indent.push_str("    ");
    }
    writeln!(source, "{indent}let total = +0").expect("string");
    for index in 0..width {
        if loops {
            writeln!(source, "{indent}c{index} := fn(x: i32) -> i32:").expect("string");
            writeln!(source, "{indent}    xs := [x, total]").expect("string");
            writeln!(source, "{indent}    let result = +0").expect("string");
            writeln!(source, "{indent}    for i in 0..xs.len():").expect("string");
            writeln!(source, "{indent}        result += xs[i]").expect("string");
            writeln!(source, "{indent}    result").expect("string");
        } else {
            writeln!(source, "{indent}c{index} := fn(x: i32) -> i32: x + total").expect("string");
        }
        writeln!(source, "{indent}total = c{index}(+1)").expect("string");
    }
    writeln!(source, "{indent}total").expect("string");
    for level in (0..depth).rev() {
        indent.truncate(indent.len() - 4);
        writeln!(source, "{indent}nested{level}()").expect("string");
    }
    source.push_str("\npub fn main() -> void $ Console:\n    println(run())\n");
    source
}

#[test]
fn closures_share_one_use_census_at_width_1000_and_depth_20() {
    for depth in [1, 20] {
        let base = source_work(&closures(0, depth, false));
        let small = source_work(&closures(100, depth, false));
        let large = source_work(&closures(1000, depth, false));
        eprintln!("closure use work depth={depth}: zero={base}, n100={small}, n1000={large}");
        assert!(small > base, "the actual census must run");
        assert!(
            large - base <= 11 * (small - base),
            "depth={depth}: {base}, {small}, {large}"
        );
    }
}

/// The preorder added by #1916 is another immutable body analysis: loop
/// queries in sibling closures must share it too, including deep wrappers.
#[test]
fn closures_share_the_loop_preorder_at_width_1000_and_depth_20() {
    for depth in [1, 20] {
        let work = |width| {
            let (_, loops) =
                hd_wasm::work::count_loop_scans(|| source_work(&closures(width, depth, true)));
            loops
        };
        let (base, small, large) = (work(0), work(100), work(1000));
        eprintln!("closure loop work depth={depth}: zero={base}, n100={small}, n1000={large}");
        assert!(small > base, "the real loop proofs must run");
        assert!(
            large - base <= 11 * (small - base),
            "depth={depth}: {base}, {small}, {large}"
        );
    }
}
