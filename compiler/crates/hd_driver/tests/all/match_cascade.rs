//! A `match` scrutinee that fails to check reports its own error and
//! nothing more: no `unknown-variant` of its arms, no unknown names for the
//! bindings they declare, and no `nonexhaustive-match` (#178). A scrutinee
//! that checks still gets the exhaustiveness check.

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn codes(main: &str) -> Vec<Code> {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    out.diags
        .code
        .iter()
        .copied()
        .filter(|c| *c != Code::Unsupported)
        .collect()
}

#[test]
fn an_unknown_method_scrutinee_reports_once() {
    let src = "fn run(v: i32) -> i32:
    match v.nothing():
        .Some(n) => n
        .None => 0

pub fn main() -> void:
    pass
";
    assert_eq!(codes(src), vec![Code::UnknownMethod]);
}

#[test]
fn an_unknown_name_scrutinee_reports_once() {
    let src = "fn run() -> i32:
    match missing:
        .Some(n) => n + 1
        .None => 0

pub fn main() -> void:
    pass
";
    assert_eq!(codes(src), vec![Code::UnknownName]);
}

#[test]
fn a_failed_use_scrutinee_reports_once() {
    let src = "use pkg.nowhere.thing

fn run() -> i32:
    match thing:
        .Some(n) => n
        .None => 0

pub fn main() -> void:
    pass
";
    assert_eq!(codes(src), vec![Code::UnknownModule]);
}

#[test]
fn a_failed_scrutinee_binds_data_pattern_names() {
    let src = "data Point:
    x: i32
    y: i32

fn run() -> i32:
    match missing:
        Point { x, y } => x + y

pub fn main() -> void:
    pass
";
    assert_eq!(codes(src), vec![Code::UnknownName]);
}

#[test]
fn a_sound_scrutinee_still_checks_exhaustiveness() {
    let src = "fn run(v: i32?) -> i32:
    match v:
        .Some(n) => n

pub fn main() -> void:
    pass
";
    assert_eq!(codes(src), vec![Code::NonexhaustiveMatch]);
}
