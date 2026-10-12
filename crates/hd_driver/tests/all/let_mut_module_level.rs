//! `let mut` on a primitive or tuple is an error at module level as it is in
//! a body (spec/lang/04-type-system.md, types.bind.let-mut-primitive,
//! types.bind.let-mut-tuple). Annotated list bindings follow
//! types.bind.let-mut-annotation.

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

const MAIN: &str = "\npub fn main() -> void $ Console:\n    println(1)\n";

fn module(decl: &str) -> Vec<Code> {
    check(&format!("{decl}\n{MAIN}"))
}

fn local(decl: &str) -> Vec<Code> {
    check(&format!(
        "pub fn main() -> void $ Console:\n    {decl}\n    println(1)\n"
    ))
}

#[test]
fn module_level_let_mut_of_primitives_is_rejected_once() {
    for decl in [
        "let mut n = +0",
        "let mut n: i32 = 0",
        "let mut b = true",
        "let mut s = \"hi\"",
        "let mut f = 1.5",
    ] {
        assert_eq!(module(decl), vec![Code::MutOnPrimitive], "{decl}");
        assert_eq!(local(decl), vec![Code::MutOnPrimitive], "{decl} (local)");
    }
}

#[test]
fn module_level_let_mut_of_a_tuple_is_rejected_once() {
    let decl = "let mut pair = (1, 2)";
    assert_eq!(module(decl), vec![Code::MutOnTuple]);
    assert_eq!(local(decl), vec![Code::MutOnTuple]);
}

#[test]
fn module_level_let_mut_of_a_list_stays_valid() {
    let decl = "let mut xs = [1, 2]";
    assert_eq!(module(decl), Vec::<Code>::new());
    assert_eq!(local(decl), Vec::<Code>::new());
    // The annotation already grants `mut`: only the redundancy warning.
    let decl = "let mut xs: mut List[i32] = []";
    assert_eq!(module(decl), vec![Code::RedundantLetMut]);
    assert_eq!(local(decl), vec![Code::RedundantLetMut]);
}

#[test]
fn module_level_let_mut_with_a_readonly_list_annotation_is_rejected() {
    let decl = "let mut xs: List[i32] = []";
    assert_eq!(module(decl), vec![Code::LetMutReadonlyType]);
    assert_eq!(local(decl), vec![Code::LetMutReadonlyType]);
}

#[test]
fn module_level_plain_let_of_a_primitive_is_valid() {
    assert_eq!(module("let n = +0"), Vec::<Code>::new());
}
