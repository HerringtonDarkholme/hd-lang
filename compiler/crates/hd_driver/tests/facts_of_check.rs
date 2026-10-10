//! `facts_of(f)` names a module-level function and returns `Facts`
//! (annot.facts-of.target, annot.facts-of.result, annot.facts-of.target.error).

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn codes(main: &str) -> Vec<Code> {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(
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
        .filter(|c| *c != Code::Unsupported)
        .collect()
}

const HEAD: &str = "use std.annotation.facts_of

@\"label\"
fn tagged() -> void:
    pass

fn plain() -> void:
    pass
";

fn with(tail: &str) -> String {
    format!("{HEAD}\n{tail}\npub fn main() -> void:\n    pass\n")
}

#[test]
fn the_result_is_facts() {
    let src = with(
        "fn count() -> usize:
    facts_of(tagged).items.len()

fn first() -> string?:
    facts_of(plain).find::[string]()",
    );
    assert_eq!(codes(&src), Vec::<Code>::new());
}

#[test]
fn a_parameter_is_not_a_function_declaration() {
    let src = with(
        "fn read(handler: fn() -> void) -> void:
    _ := facts_of(handler)",
    );
    assert_eq!(codes(&src), vec![Code::InvalidFactsOfTarget]);
}

#[test]
fn a_call_is_not_a_function_declaration() {
    let src = with("value := facts_of(plain())");
    assert_eq!(codes(&src), vec![Code::InvalidFactsOfTarget]);
}

#[test]
fn a_field_access_is_not_a_function_declaration() {
    let src = with(
        "data Holder:
    action: fn() -> void

fn read(holder: Holder) -> void:
    _ := facts_of(holder.action)",
    );
    assert_eq!(codes(&src), vec![Code::InvalidFactsOfTarget]);
}

#[test]
fn a_missing_argument_follows_the_call_rules() {
    let src = with("value := facts_of()");
    assert_eq!(codes(&src), vec![Code::ArgumentCount]);
}

#[test]
fn an_extra_argument_follows_the_call_rules() {
    let src = with("value := facts_of(tagged, plain)");
    assert_eq!(codes(&src), vec![Code::ArgumentCount]);
}

#[test]
fn an_unknown_name_is_unknown() {
    let src = with("value := facts_of(missing)");
    assert_eq!(codes(&src), vec![Code::UnknownName]);
}
