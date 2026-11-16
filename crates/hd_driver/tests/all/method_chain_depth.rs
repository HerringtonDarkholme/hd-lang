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

/// `fn f(x: T) -> T:` whose body is `body`, and a `main` that prints
/// `f(1)`.
fn with_main(ty: &str, body: &str) -> String {
    format!(
        "fn f(x: {ty}) -> {ty}:\n    {body}\n\npub fn main() -> void $ Console:\n    println(f(1))\n"
    )
}

/// Checks, formats and, with `build`, builds `text` on a worker's stack.
fn checks_formats_builds(text: String, build_it: bool) {
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
    let goal = if build_it {
        Goal::Program {
            entry: "main".into(),
        }
    } else {
        Goal::Analyze
    };
    let out = build(&host, "app", &goal);
    assert!(!out.diags.has_errors(), "{}", out.render());
    assert_eq!(out.wasm.is_some(), build_it);
    let formatted = std::thread::Builder::new()
        .stack_size(hd_sched::pool::WORKER_STACK)
        .spawn(move || hd_fmt::format(&text).is_ok())
        .expect("thread")
        .join()
        .expect("fmt");
    assert!(formatted);
}

/// #1994: count every liveness rewrite sweep as well as persistent-set
/// work; a whole-body sweep per power level would grow quadratically.
#[test]
fn power_chain_liveness_work_is_near_linear() {
    let worker = std::thread::Builder::new()
        .stack_size(hd_sched::pool::WORKER_STACK)
        .spawn(|| {
            let mut measured = Vec::new();
            for n in [1_000, 10_000] {
                let text = with_main("u32", &vec!["x"; n + 1].join(" ** "));
                let mut src = MemorySources::default();
                src.insert("main.hd", &text);
                let store = crate::warm_store();
                let host = Host {
                    render_tir: &[],
                    sources: &src,
                    store: &store,
                    clock: &NoClock,
                    executor: Executor::Serial(hd_sched::SerialOrder::Priority),
                };
                let (out, work) = hd_wasm::work::count_liveness(|| {
                    build(
                        &host,
                        "app",
                        &Goal::Program {
                            entry: "main".into(),
                        },
                    )
                });
                assert!(!out.diags.has_errors(), "{}", out.render());
                assert!(out.wasm.is_some());
                eprintln!("power count {n}: work {work}");
                measured.push(work);
            }
            // Persistent-set paths contribute a logarithmic factor.
            assert!(
                measured[0] > 0 && measured[1] <= measured[0] * 14,
                "{measured:?}"
            );
        });
    assert!(worker.is_ok());
    if let Ok(worker) = worker {
        assert!(worker.join().is_ok());
    }
}

/// A prefix operator is one link of `grammar.limit.chain` (#1955): the
/// parser takes 10,000 of them in a loop, and below that limit a dev
/// build checks, formats and builds them on a worker's stack, `*` and
/// `&` alternating as well as `-` alone.
#[test]
fn a_prefix_chain_of_ten_thousand_operators_checks_formats_and_builds() {
    checks_formats_builds(with_main("i32", &format!("{}x", "- ".repeat(10_000))), true);
    checks_formats_builds(
        with_main("i32", &format!("{}x", "* & ".repeat(5_000))),
        true,
    );
}

/// `**` is a binary operator, one link each (#1955): 10,000 of them
/// check, format and build on a worker's stack. Persistent liveness sets
/// remove the quadratic build cost previously tracked by #1994.
#[test]
fn a_power_chain_of_ten_thousand_operators_checks_formats_and_builds() {
    checks_formats_builds(
        with_main("u32", &format!("x{}", " ** x".repeat(10_000))),
        true,
    );
}

#[test]
fn a_prefix_or_power_chain_one_past_the_limit_is_too_deep() {
    for body in [
        format!("{}x", "* ".repeat(10_001)),
        format!("x{}", " ** x".repeat(10_001)),
    ] {
        let mut src = MemorySources::default();
        src.insert("main.hd", &with_main("u32", &body));
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
}
