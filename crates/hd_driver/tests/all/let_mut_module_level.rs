//! `let mut` on a primitive is an error at module level
//! (spec/lang/04-type-system.md, types.bind.let-mut-primitive-top) and, in a
//! body, valid with a warning unless `&mut` of it is taken
//! (types.bind.let-mut-primitive-local, types.bind.let-mut-primitive-unused).
//! On a tuple it is an error in both (types.bind.let-mut-tuple). An annotated
//! list binding is a writable value (types.coll.let-mut).

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
        assert_eq!(local(decl), vec![Code::RedundantLetMut], "{decl} (local)");
    }
}

#[test]
fn local_let_mut_of_a_primitive_with_a_mut_reference_is_quiet() {
    let decls = "let mut n = +0\n    r := &mut n\n    *r += 1";
    assert_eq!(local(decls), Vec::<Code>::new());
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
    let decls = "let mut xs: List[i32] = []\n    r := &mut xs\n    r.push(1)";
    assert_eq!(local(decls), Vec::<Code>::new());
}

#[test]
fn let_mut_with_a_list_value_annotation_is_valid() {
    let decl = "let mut xs: List[i32] = []";
    assert_eq!(module(decl), Vec::<Code>::new());
    assert_eq!(local(decl), Vec::<Code>::new());
}

#[test]
fn module_level_plain_let_of_a_primitive_is_valid() {
    assert_eq!(module("let n = +0"), Vec::<Code>::new());
}
