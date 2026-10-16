//! Derivation line drift (spec 14 `annot.line.drift`): derivation blocks
//! for traits of one package on one type whose member lines differ warn
//! `derivation-line-drift` on the later block; agreeing blocks and a lone
//! block stay silent.

use hd_cache::MemoryStore;
use hd_diag::{Code, Severity};
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn build_with(src: &MemorySources, store: &MemoryStore) -> Output {
    let host = Host {
        render_tir: &[],
        sources: src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", &Goal::Analyze)
}

fn analyze(main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    build_with(&src, &MemoryStore::default())
}

fn warnings(out: &Output) -> Vec<Code> {
    out.diags
        .content_order()
        .into_iter()
        .filter(|i| out.diags.severity[*i] == Severity::Warning)
        .map(|i| out.diags.code[i])
        .collect()
}

fn errors(out: &Output) -> Vec<Code> {
    out.diags
        .content_order()
        .into_iter()
        .filter(|i| out.diags.severity[*i] == Severity::Error)
        .map(|i| out.diags.code[i])
        .collect()
}

const PRELUDE: &str =
    "use std.structure.Structure\n\ndata Session:\n    id: i64\n    cache: i64 = 0\n\n";

#[test]
fn blocks_with_different_lines_warn_on_the_later_block() {
    let out = analyze(&format!(
        "{PRELUDE}impl Debug for Session by Structure:\n    cache = pass\n\nimpl Hash for Session by Structure:\n    id = []\n"
    ));
    assert!(errors(&out).is_empty(), "{}", out.render());
    assert_eq!(warnings(&out), vec![Code::DerivationLineDrift]);
}

#[test]
fn blocks_with_the_same_lines_do_not_warn() {
    let out = analyze(&format!(
        "{PRELUDE}impl Debug for Session by Structure:\n    cache = pass\n\nimpl Hash for Session by Structure:\n    cache = pass\n"
    ));
    assert!(errors(&out).is_empty(), "{}", out.render());
    assert!(warnings(&out).is_empty(), "{}", out.render());
}

#[test]
fn a_lone_block_does_not_warn() {
    let out = analyze(&format!(
        "{PRELUDE}impl Debug for Session by Structure:\n    cache = pass\n"
    ));
    assert!(errors(&out).is_empty(), "{}", out.render());
    assert!(warnings(&out).is_empty(), "{}", out.render());
}
