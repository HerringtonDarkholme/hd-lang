//! A long method chain checks on the pool's workers. The checker takes a
//! chain's receivers in a loop, not one nested call per link, and a
//! worker's stack is a main thread's (`hd_sched::pool::WORKER_STACK`).
//! Before both, one expression of 2,000 links overflowed a worker's
//! stack (the `pathological` metric's `method-chain` shape). The other
//! syntax walks of checking take a chain with a work stack as well.

use std::fmt::Write as _;

use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;

/// `text` followed by `links` chained string method calls.
fn program(links: usize) -> String {
    let methods = ["trim()", "lower()", "upper()", "trim_start()", "trim_end()"];
    let mut s = String::from("fn run(text: string) -> string:\n    text");
    for i in 0..links {
        let _ = write!(s, ".{}", methods[i % methods.len()]);
    }
    s.push('\n');
    s
}

#[test]
fn a_chain_of_six_thousand_links_checks_on_the_pool() {
    let mut src = MemorySources::default();
    src.insert("main.hd", &program(6_000));
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Pool(2),
    };
    let out = build(&host, "app", &Goal::Analyze);
    assert!(!out.diags.has_errors(), "{}", out.render());
}

/// The declaration walk for written local references (`local_refs`) takes
/// the syntax tree with a work stack too, so a dev build checks a chain of
/// 20,000 links (#1180).
#[test]
fn a_chain_of_twenty_thousand_links_checks_on_the_pool() {
    let mut src = MemorySources::default();
    src.insert("main.hd", &program(20_000));
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Pool(2),
    };
    let out = build(&host, "app", &Goal::Analyze);
    assert!(!out.diags.has_errors(), "{}", out.render());
}
