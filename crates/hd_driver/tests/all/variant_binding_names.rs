//! Swapped variant binding names (spec 06 `flow.match.payload.name-mismatch`):
//! a positional bare binding that equals a different payload field's name
//! warns `variant-binding-name-mismatch`; correct-order and fresh names
//! stay silent.

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

const PAIR: &str = "enum Pair:\n    Values(left: i32, right: i32)\n\n";

#[test]
fn swapped_positional_bindings_are_accepted_without_lint() {
    let out = analyze(&format!(
        "{PAIR}fn read(value: Pair) -> i32:\n    match value:\n        Pair.Values(right, left) => left - right\n"
    ));
    assert!(errors(&out).is_empty(), "{}", out.render());
    assert!(warnings(&out).is_empty(), "{}", out.render());
}

#[test]
fn bindings_in_field_order_do_not_warn() {
    let out = analyze(&format!(
        "{PAIR}fn read(value: Pair) -> i32:\n    match value:\n        Pair.Values(left, right) => left - right\n"
    ));
    assert!(errors(&out).is_empty(), "{}", out.render());
    assert!(warnings(&out).is_empty(), "{}", out.render());
}

#[test]
fn fresh_binding_names_do_not_warn() {
    let out = analyze(&format!(
        "{PAIR}fn read(value: Pair) -> i32:\n    match value:\n        Pair.Values(a, b) => a - b\n"
    ));
    assert!(errors(&out).is_empty(), "{}", out.render());
    assert!(warnings(&out).is_empty(), "{}", out.render());
}

#[test]
fn named_sub_patterns_do_not_warn() {
    let out = analyze(&format!(
        "{PAIR}fn read(value: Pair) -> i32:\n    match value:\n        Pair.Values(left=l, right=r) => l - r\n"
    ));
    assert!(errors(&out).is_empty(), "{}", out.render());
    assert!(warnings(&out).is_empty(), "{}", out.render());
}

#[test]
fn a_wildcard_next_to_a_correct_binding_does_not_warn() {
    let out = analyze(&format!(
        "{PAIR}fn read(value: Pair) -> i32:\n    match value:\n        Pair.Values(left, _) => left\n"
    ));
    assert!(errors(&out).is_empty(), "{}", out.render());
    assert!(warnings(&out).is_empty(), "{}", out.render());
}
