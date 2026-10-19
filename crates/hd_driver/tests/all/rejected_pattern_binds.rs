//! A rejected pattern still binds its names, and its arm takes no part in
//! the reachability check, so one bad pattern is one diagnostic
//! (spec/lang/06-control-flow.md, flow.match.bare-variant).

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn check(main: &str) -> Vec<Code> {
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

fn codes(body: &str) -> Vec<Code> {
    check(&format!(
        "{body}\npub fn main() -> void $ Console:\n    println(1)\n"
    ))
}

#[test]
fn bare_call_pattern_binds_its_names() {
    let got = codes(
        "fn f(r: Result[i32, string]) -> i32:\n    match r:\n        Ok(number) => number\n        .Err(_) => 0\n",
    );
    assert_eq!(got, vec![Code::BareVariantPattern]);
}

#[test]
fn bare_variant_arm_hides_no_neighbour() {
    let got = codes(
        "enum Flag:\n    On(value: i32)\n    Off\n\nfn f(flag: Flag) -> i32:\n    match flag:\n        On => 1\n        .Off => 0\n",
    );
    assert_eq!(got, vec![Code::BareVariantPattern]);
}

#[test]
fn duplicate_field_binds_the_alias() {
    let got = codes(
        "struct User:\n    name: string\n\nfn f(user: User) -> string:\n    match user:\n        User { name, name: alias } => alias\n",
    );
    assert_eq!(got, vec![Code::DuplicateDataPatternField]);
}

#[test]
fn unknown_variant_field_binds_its_value() {
    let got = codes(
        "enum Choice:\n    Pair(left: i32, right: i32)\n\nfn f(choice: Choice) -> i32:\n    match choice:\n        Choice.Pair(missing=value, right=other) => value\n",
    );
    assert_eq!(got, vec![Code::UnknownField]);
}

#[test]
fn correct_pattern_binds_with_its_type() {
    let got = codes(
        "fn f(o: i32?) -> i32:\n    match o:\n        .Some(number) => number\n        .None => 0\n",
    );
    assert_eq!(got, vec![]);
}

#[test]
fn arm_after_catch_all_is_still_unreachable() {
    let got = codes("fn f(o: i32?) -> i32:\n    match o:\n        _ => 0\n        .None => 1\n");
    assert_eq!(got, vec![Code::UnreachableMatchArm]);
}
