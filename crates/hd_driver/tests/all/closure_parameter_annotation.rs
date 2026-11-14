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

// `fn.type.lifetime.call.written.closure`, `ref.lifetime.not-yet.body`: a
// closure parameter's type places the lifetimes it writes on its `&`
// forms in source order, and a form where that order is not its reference
// slots' is not checked yet.
const NOT_YET: Code = Code::ReferencesNotYetChecked;

/// A function `run['a]` whose body binds the closure `closure`.
fn with_closure(closure: &str) -> String {
    format!(
        "struct Point:\n    x: i32\n\ntrait Source:\n    type Item\n\nfn run['a](p: &'a Point) -> i32:\n    f := {closure}\n    0\n"
    )
}

#[test]
fn a_written_lifetime_on_the_annotations_references_is_checked() {
    for closure in [
        "fn(x: &'a Point): x.x",
        "fn(x: (&Point, &'a Point)?, n: i32): n",
        "fn(xs: List[&'a Point]): 0",
        // `i32` is an alias that holds no reference, and a counted
        // reference has no slot of its own.
        "fn(x: (*Point, &'a Point, i32)): 0",
    ] {
        assert!(codes_of(&with_closure(closure)).is_empty(), "{closure}");
    }
}

#[test]
fn a_written_lifetime_the_annotation_cannot_place_is_not_checked_yet() {
    for closure in [
        "fn(g: fn(&'a Point) -> i32): 0",
        "fn(x: &'a Point | i32): 0",
        "fn(x: &'static Point): 0",
        "fn(x: &'b Point): 0",
        // A trait value's bindings hold no reference slot of its type.
        "fn(s: &'a dyn Source[Item = &'a Point]): 0",
    ] {
        let codes = codes_of(&with_closure(closure));
        assert!(codes.contains(&NOT_YET), "{closure}: {codes:?}");
    }
    let local = "struct Point:\n    x: i32\n\nfn run['a](p: &'a Point) -> i32:\n    fn helper(x: &'a Point) -> i32:\n        x.x\n    helper(p)\n";
    assert_eq!(codes_of(local), vec![NOT_YET]);
    // A top-level binding's closure is in no function's body.
    let top = "struct Point:\n    x: i32\n\nf := fn(x: &'a Point): x.x\n";
    assert!(codes_of(top).contains(&NOT_YET), "{:?}", codes_of(top));
}
