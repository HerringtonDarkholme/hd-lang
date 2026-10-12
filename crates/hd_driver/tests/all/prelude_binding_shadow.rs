//! `module.prelude.no-shadow` for bindings: a local binding, a closure
//! parameter, a pattern name, a loop name, a test local, or a top-level
//! binding must not take a prelude name. Each conflict is one
//! `prelude-name-shadow` error at the name, with no error after it: the
//! binding still introduces the name for the rest of the body.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn analyze(text: &str) -> Output {
    run(text, &Goal::Analyze)
}

fn run(text: &str, goal: &Goal) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", text);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build_packages(&host, "app", &Packages::default(), goal)
}

/// The `(code, line, column)` of each error, in source order.
fn errors(text: &str) -> Vec<(Code, usize, usize)> {
    let goal = if text.starts_with("tests:") {
        Goal::CheckTests
    } else {
        Goal::Analyze
    };
    let out = run(text, &goal);
    out.diags
        .content_order()
        .into_iter()
        .filter(|i| out.diags.severity[*i] == hd_diag::Severity::Error)
        .map(|i| {
            let (_, line, column) = out.locate(out.diags.primary[i]);
            (out.diags.code[i], line, column)
        })
        .collect()
}

#[track_caller]
fn shadows(text: &str, at: &[(usize, usize)]) {
    let want: Vec<_> = at
        .iter()
        .map(|&(l, c)| (Code::PreludeNameShadow, l, c))
        .collect();
    assert_eq!(errors(text), want, "{text}");
}

#[track_caller]
fn accepts(text: &str) {
    let out = analyze(text);
    assert!(!out.diags.has_errors(), "{text}\n{}", out.render());
}

#[test]
fn let_binding_of_a_trait_name() {
    shadows(
        "fn main() -> i32:\n    let Hash: i32 = 42\n    Hash\n",
        &[(2, 9)],
    );
}

#[test]
fn let_bindings_of_a_function_and_a_type() {
    shadows(
        "fn main() -> i32:\n    let panic = +1\n    let Option = +2\n    panic + Option\n",
        &[(2, 9), (3, 9)],
    );
}

#[test]
fn let_mut_binding() {
    shadows(
        "fn main() -> void:\n    let mut debug = [+1]\n    debug.push(+2)\n",
        &[(2, 9)],
    );
}

#[test]
fn short_binding() {
    shadows("fn main() -> i32:\n    debug := +1\n    debug\n", &[(2, 5)]);
}

#[test]
fn chained_short_bindings() {
    shadows(
        "fn main() -> i32:\n    a := Option := +1\n    a + Option\n",
        &[(2, 10)],
    );
}

#[test]
fn tuple_pattern_names() {
    shadows(
        "fn main() -> i32:\n    let (Eq, y) = (+1, +2)\n    Eq + y\n",
        &[(2, 10)],
    );
}

#[test]
fn match_arm_name() {
    shadows(
        "enum Shape:\n    Circle(i32)\n    Empty\n\nfn area(s: Shape) -> i32:\n    match s:\n        .Circle(Ord) => Ord\n        .Empty => +0\n",
        &[(7, 17)],
    );
}

#[test]
fn for_loop_name() {
    shadows(
        "fn total(xs: List[i32]) -> i32:\n    total := +0\n    for Display in xs:\n        _ := Display\n    total\n",
        &[(3, 9)],
    );
}

#[test]
fn closure_parameter() {
    shadows(
        "fn main() -> i32:\n    let f = fn(Debug: i32) -> i32: Debug\n    f(+1)\n",
        &[(2, 16)],
    );
}

#[test]
fn closure_bound_by_a_prelude_name() {
    shadows(
        "fn main() -> i32:\n    let dbg = fn(x: i32) -> i32: x\n    dbg(+1)\n",
        &[(2, 9)],
    );
}

#[test]
fn top_level_binding() {
    shadows("debug := +1\n\nfn main() -> i32:\n    debug\n", &[(1, 1)]);
}

#[test]
fn test_local() {
    shadows(
        "tests:\n    it(\"x\"):\n        let Result = +1\n        _ := Result\n",
        &[(3, 13)],
    );
}

#[test]
fn test_local_named_it() {
    shadows(
        "tests:\n    it(\"x\"):\n        it := +1\n        _ := it\n",
        &[(3, 9)],
    );
}

#[test]
fn it_is_free_outside_tests() {
    accepts("fn main() -> i32:\n    it := +1\n    it\n");
}

#[test]
fn ordinary_names_are_accepted() {
    accepts(
        "fn main() -> i32:\n    let hash = +1\n    option := +2\n    let f = fn(debugging: i32) -> i32: debugging\n    f(hash + option)\n",
    );
}

#[test]
fn prelude_names_used_normally_are_accepted() {
    accepts(
        "fn main() -> i32:\n    let x: Option[i32] = .Some(+1)\n    match x:\n        .Some(v) => v\n        .None => +0\n",
    );
}
