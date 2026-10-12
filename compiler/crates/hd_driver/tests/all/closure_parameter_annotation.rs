//! `fn.closure.needs-annotation`: a closure parameter without an
//! annotation needs an expected function type to supply it, else
//! `closure-parameter-needs-annotation` at that parameter, once, with no
//! error in the body.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn codes_of(body: &str) -> Vec<Code> {
    let mut src = MemorySources::default();
    src.insert("main.hd", body);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = hd_driver::build(&host, "app", &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .filter(|&i| out.diags.severity[i] == hd_diag::Severity::Error)
        .map(|i| out.diags.code[i])
        .collect()
}

const NEEDS: Code = Code::ClosureParameterNeedsAnnotation;

#[test]
fn a_bare_binding_has_no_expected_type() {
    let main = "fn run() -> i32:\n    closure := fn(value) -> i32: value\n    closure(1)\n";
    assert_eq!(codes_of(main), vec![NEEDS]);
}

#[test]
fn each_unannotated_parameter_is_reported() {
    let main = "fn run() -> i32:\n    closure := fn(a, b) -> i32: 1\n    closure(1, 2)\n";
    assert_eq!(codes_of(main), vec![NEEDS, NEEDS]);
}

#[test]
fn an_annotated_binding_supplies_the_type() {
    let main = "fn run() -> i32:\n    let f: fn(i32) -> i32 = fn(v) -> i32: v\n    f(1)\n";
    assert!(codes_of(main).is_empty());
}

#[test]
fn a_function_type_parameter_supplies_the_type() {
    let main = "\
fn apply[T](f: fn(T) -> T, x: T) -> T:
    f(x)

fn run() -> i32:
    apply(fn(q: i32): q, 3)

fn run_inferred() -> i32:
    let xs: i32 = 1
    apply(fn(q): q, xs)
";
    assert!(codes_of(main).is_empty());
}

#[test]
fn a_list_map_argument_supplies_the_type() {
    let main = "fn run(xs: List[i32]) -> List[i32]:\n    xs.map(fn(v): v + 1)\n";
    assert!(codes_of(main).is_empty());
}
