//! The least common type at every site (spec 04 "Least Common Type",
//! `types.lct.*`) and the narrower join of a generic call's arguments
//! (`types.generic.infer.join`). Valid joins build and run on V8
//! (`host/run.ts`); a value that does not join is `no-common-type`, or
//! `no-least-common-type`, reported once at that value.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str, goal: &Goal) -> Output {
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
    build(&host, "app", goal)
}

/// The diagnostic codes of a program checked without running.
fn codes_of(main: &str) -> Vec<Code> {
    let out = built(main, &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

/// The least-common-type work of checking a list literal of `n`
/// integer literals.
fn list_join_work(n: usize) -> u64 {
    let elems: Vec<String> = (0..n).map(|i| format!("+{i}")).collect();
    let main = format!(
        "fn f() -> usize:\n    xs := [{}]\n    xs.len()\n",
        elems.join(", ")
    );
    let (codes, work) = hd_check::work::count_join_work(|| codes_of(&main));
    assert_eq!(codes, vec![], "{n} elements");
    work
}

/// A value that leaves a site's type unchanged costs a constant, so a
/// list literal joins in linear work. Comparing every value with the
/// earlier ones again costs n^2 / 2: 100 times as much for 10 times the
/// elements.
#[test]
fn a_long_list_literal_joins_in_linear_time() {
    let (small, large) = (list_join_work(500), list_join_work(5000));
    assert!(small >= 500, "one unit per joined value: {small}");
    eprintln!("join work: 500 elements {small}, 5000 elements {large}");
    assert!(large <= 12 * small, "500 elements: {small}, 5000: {large}");
}
