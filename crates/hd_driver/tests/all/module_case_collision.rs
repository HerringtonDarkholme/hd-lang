//! Module paths that collide after case folding and NFC normalization are
//! rejected even when the host filesystem could tell them apart
//! (module.path.case-collision).

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn check(files: &[(&str, &str)]) -> Vec<Code> {
    let mut src = MemorySources::default();
    for (path, text) in files {
        src.insert(path, text);
    }
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out: Output = build(
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
        .filter(|c| *c != Code::UnusedLocalBinding)
        .collect()
}

const MAIN: &str = "pub fn main() -> void $ Console:\n    println(1)\n";
const HELPER: &str = "pub fn one() -> i32: 1\n";

#[test]
fn files_differing_by_case_are_a_duplicate_module() {
    let found = check(&[
        ("src/main.hd", MAIN),
        ("src/Foo.hd", HELPER),
        ("src/foo.hd", HELPER),
    ]);
    assert_eq!(
        found
            .iter()
            .filter(|c| **c == Code::DuplicateModuleName)
            .count(),
        1,
        "{found:?}"
    );
}

#[test]
fn distinct_names_are_accepted() {
    let found = check(&[
        ("src/main.hd", MAIN),
        ("src/foo.hd", HELPER),
        ("src/bar.hd", HELPER),
    ]);
    assert!(!found.contains(&Code::DuplicateModuleName), "{found:?}");
}
