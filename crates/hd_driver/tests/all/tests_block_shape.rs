//! The shape rules of `tests:` blocks: a file has at most one
//! (`grammar.tests.once`), and a test module or an integration test has none
//! (`module.test.no-tests-block`). Each offending block is one diagnostic;
//! the cases are stage cases (`hd_check/tests/cases/tests-block-shape/`).
//! An edit that adds and removes a second block updates a warm report.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const LIB: &str = "pub fn double(n: i32) -> i32: n * 2\n";
const BLOCK: &str = "tests:\n    it(\"first\"):\n        pass\n";
const SECOND: &str = "\ntests:\n    it(\"second\"):\n        pass\n";

fn analyze(store: &hd_cache::MemoryStore, files: &[(&str, &str)]) -> Output {
    let mut src = MemorySources::default();
    for (path, text) in files {
        src.insert(path, text);
    }
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", &Goal::Analyze)
}

fn found(out: &Output) -> Vec<(Code, u32)> {
    out.diags
        .content_order()
        .into_iter()
        .map(|i| (out.diags.code[i], out.diags.primary[i].lo))
        .collect()
}

#[test]
fn editing_a_block_in_and_out_updates_the_report() {
    let store = hd_cache::MemoryStore::default();
    let good = format!("{LIB}{BLOCK}");
    let bad = format!("{LIB}{BLOCK}{SECOND}");
    let out = analyze(&store, &[("src/main.hd", &good)]);
    assert_eq!(found(&out), [], "{}", out.render());
    let out = analyze(&store, &[("src/main.hd", &bad)]);
    let codes: Vec<Code> = found(&out).into_iter().map(|f| f.0).collect();
    assert_eq!(codes, [Code::DuplicateTestsBlock], "{}", out.render());
    let out = analyze(&store, &[("src/main.hd", &good)]);
    assert_eq!(found(&out), [], "{}", out.render());
}
