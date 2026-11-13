//! A long method chain checks on the pool's workers. The checker takes a
//! chain's receivers in a loop, not one nested call per link, and a
//! worker's stack is a main thread's (`hd_sched::pool::WORKER_STACK`).
//! Before both, one expression of 2,000 links overflowed a worker's
//! stack (the `pathological` metric's `method-chain` shape). The other
//! syntax walks of checking take a chain with a work stack as well.
//! Past 10,000 links the parser rejects a chain (#1775).

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
/// the syntax tree with a work stack too (#1180). The parser admits 10,000
/// links (`grammar.limit.chain`), and below that a dev build checks, formats
/// and builds the chain on a worker's stack (#1775).
#[test]
fn a_chain_of_ten_thousand_links_checks_formats_and_builds() {
    let text = format!(
        "{}\npub fn main() -> void $ Console:\n    println(run(\" a \"))\n",
        program(10_000)
    );
    let mut src = MemorySources::default();
    src.insert("main.hd", &text);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Pool(2),
    };
    let out = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    assert!(out.wasm.is_some());
    let formatted = std::thread::Builder::new()
        .stack_size(hd_sched::pool::WORKER_STACK)
        .spawn(move || hd_fmt::format(&text).is_ok())
        .expect("thread")
        .join()
        .expect("fmt");
    assert!(formatted);
}

#[test]
fn a_chain_of_ten_thousand_and_one_links_is_too_deep() {
    let mut src = MemorySources::default();
    src.insert("main.hd", &program(10_001));
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Pool(2),
    };
    let out = build(&host, "app", &Goal::Analyze);
    let codes: Vec<&str> = out.diags.code.iter().map(|code| code.as_str()).collect();
    assert_eq!(codes, ["nesting-too-deep"], "{}", out.render());
}
