//! #1529: lexical depth must not multiply omitted-row body checking.
//! `req.row.omitted.closure-row`, `req.row.omitted.cycle`, and the Cost
//! Model require the same least rows without nested body retries.

use std::fmt::Write as _;

use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The issue's `gen.py nest N`, kept here so the test is self-contained.
fn nested(depth: usize) -> String {
    let mut out = String::from("pub fn main() -> void $ Console:\n");
    for i in 0..depth {
        let indent = "    ".repeat(i + 1);
        let _ = write!(
            out,
            "{indent}fn g{i}(k: i32) -> void:\n{indent}    if k > 0:\n{indent}        g{i}(k - 1)\n"
        );
    }
    let _ = writeln!(out, "{}println(\"deep $k\")", "    ".repeat(depth + 1));
    for i in (1..depth).rev() {
        let _ = writeln!(out, "{}g{i}(1)", "    ".repeat(i + 1));
    }
    out.push_str("    g0(1)\n");
    out
}

fn wide(width: usize) -> String {
    let mut out = String::from("pub fn main() -> void $ Console:\n");
    for i in 0..width {
        let _ = write!(
            out,
            "    fn g{i}(k: i32) -> void:\n        if k > 0:\n            g{i}(k - 1)\n        println(k)\n    g{i}(0)\n"
        );
    }
    out
}

fn check_work(source: &str, bodies: usize) {
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
    let (out, counts) =
        hd_check::work::count_local_body_checks(|| build(&host, "app", &Goal::Analyze));
    assert!(out.diags.is_empty(), "{}", out.render());
    let counts: Vec<_> = counts
        .iter()
        .filter(|c| c.owner.starts_with("app/"))
        .collect();
    assert_eq!(counts.len(), bodies);
    for c in &counts {
        assert!(
            c.checks <= 2,
            "{} {:?}: {} body checks",
            c.owner,
            c.at,
            c.checks
        );
    }
    assert_eq!(counts.iter().map(|c| c.checks).sum::<usize>(), 2 * bodies);
}

#[test]
fn nested_recursive_bodies_are_checked_twice_independent_of_depth() {
    for depth in [12, 20, 40] {
        check_work(&nested(depth), depth);
    }
}

#[test]
fn recursive_body_checks_grow_linearly_with_width() {
    for width in [100, 1000] {
        check_work(&wide(width), width);
    }
}

#[test]
fn generic_ancestor_capture_dependencies_do_not_retry_subtrees() {
    for depth in [12, 20, 40] {
        let mut source = nested(depth);
        for i in (0..depth).rev() {
            source = source.replace(&format!("fn g{i}(k:"), &format!("fn g{i}[T{i}](k:"));
            source = source.replace(&format!("g{i}("), &format!("g{i}::[i32]("));
            if i > 0 {
                let header = format!("{}fn g{i}[T{i}](k: i32) -> void:\n", "    ".repeat(i + 1));
                let use_parent = format!("{}g{}::[i32](0)\n", "    ".repeat(i + 2), i - 1);
                source = source.replace(&header, &format!("{header}{use_parent}"));
            }
        }
        check_work(&source, depth);
    }
}
