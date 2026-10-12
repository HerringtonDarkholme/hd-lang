//! The shape rules of `tests:` blocks: a file has at most one
//! (`grammar.tests.once`), and a test module or an integration test has none
//! (`module.test.no-tests-block`). Each offending block is one diagnostic.

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

fn offset(text: &str, needle: &str, nth: usize) -> u32 {
    let at = text.match_indices(needle).nth(nth).expect("needle").0;
    u32::try_from(at).expect("offset")
}

#[test]
fn a_second_tests_block_is_reported_once_at_the_block() {
    let main = format!("{LIB}{BLOCK}{SECOND}");
    let out = analyze(&crate::warm_store(), &[("src/main.hd", &main)]);
    assert_eq!(
        found(&out),
        [(Code::DuplicateTestsBlock, offset(&main, "tests:", 1))],
        "{}",
        out.render()
    );
    let third = format!("{main}{SECOND}");
    let out = analyze(&crate::warm_store(), &[("src/main.hd", &third)]);
    let codes: Vec<Code> = found(&out).into_iter().map(|f| f.0).collect();
    assert_eq!(codes, [Code::DuplicateTestsBlock; 2], "{}", out.render());
}

#[test]
fn one_tests_block_in_a_normal_module_is_fine() {
    let main = format!("{LIB}{BLOCK}");
    let out = analyze(&crate::warm_store(), &[("src/main.hd", &main)]);
    assert_eq!(found(&out), [], "{}", out.render());
}

#[test]
fn a_test_module_has_no_tests_block() {
    let unit = format!("{LIB}{BLOCK}");
    let files = [("src/main.hd", LIB), ("src/main_test.hd", unit.as_str())];
    let out = analyze(&crate::warm_store(), &files);
    assert_eq!(
        found(&out),
        [(Code::MisplacedTestsBlock, offset(&unit, "tests:", 0))],
        "{}",
        out.render()
    );
}

#[test]
fn an_integration_test_has_no_tests_block() {
    let files = [("src/main.hd", LIB), ("tests/flow.hd", BLOCK)];
    let out = analyze(&crate::warm_store(), &files);
    assert_eq!(
        found(&out),
        [(Code::MisplacedTestsBlock, 0)],
        "{}",
        out.render()
    );
    // Two blocks: each is misplaced, and none is also a duplicate.
    let two = format!("{BLOCK}{SECOND}");
    let files = [("src/main.hd", LIB), ("tests/flow.hd", two.as_str())];
    let out = analyze(&crate::warm_store(), &files);
    let codes: Vec<Code> = found(&out).into_iter().map(|f| f.0).collect();
    assert_eq!(codes, [Code::MisplacedTestsBlock; 2], "{}", out.render());
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
