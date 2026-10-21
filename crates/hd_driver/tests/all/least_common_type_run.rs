//! The least common type at every site (spec 04 "Least Common Type",
//! `types.lct.*`) and the narrower join of a generic call's arguments
//! (`types.generic.infer.join`). Valid joins build and run on V8
//! (`host/run.mjs`); a value that does not join is `no-common-type`, or
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

/// A value that leaves a site's type unchanged costs a constant, so a
/// 20000-element list literal checks in linear time. The bound is generous
/// for the dev profile: it catches a quadratic join, not a slow machine.
#[test]
fn a_long_list_literal_joins_in_linear_time() {
    let elems: Vec<String> = (0..20_000).map(|i| format!("+{i}")).collect();
    let main = format!(
        "fn f() -> usize:\n    xs := [{}]\n    xs.len()\n",
        elems.join(", ")
    );
    let start = std::time::Instant::now();
    assert_eq!(codes_of(&main), vec![]);
    let took = start.elapsed();
    assert!(
        took < std::time::Duration::from_secs(5),
        "20000 elements took {took:?}"
    );
}
