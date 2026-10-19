//! A generic parameter that nothing at a call solves, with no default, is
//! one `cannot-infer-type` at the call (`types.generic.default.unsolved`).

use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn analyze(main: &str) -> Output {
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
    build(&host, "app", &Goal::Analyze)
}

fn codes(out: &Output) -> Vec<&'static str> {
    (0..out.diags.len())
        .filter(|&i| out.diags.severity[i] == hd_diag::Severity::Error)
        .map(|i| out.diags.code[i].as_str())
        .collect()
}

const DECLS: &str = "fn make[T]() -> T:\n    panic(\"no\")\n\nfn missing[T]() -> i32:\n    1\n\nfn dflt[T = i32]() -> i32:\n    1\n\nfn ident[T](v: T) -> T:\n    v\n\nfn once[A, B](f: fn(A) -> B) -> i32:\n    1\n\nstruct Reader:\n    id: i32\n\nimpl Reader:\n    fn size[T](self) -> i32:\n        self.id\n\n";

fn run(body: &str) -> Vec<&'static str> {
    codes(&analyze(&format!("{DECLS}pub fn main() -> void:\n{body}")))
}

#[test]
fn a_parameter_named_nowhere_is_reported_once() {
    assert_eq!(run("    _ := missing()\n"), ["cannot-infer-type"]);
}

#[test]
fn an_unsolved_placeholder_is_reported_once() {
    assert_eq!(run("    _ := make::[_]()\n"), ["cannot-infer-type"]);
}

#[test]
fn an_unsolved_method_parameter_is_reported_once() {
    assert_eq!(
        run("    r := Reader { id: 1 }\n    _ := r.size()\n"),
        ["cannot-infer-type"]
    );
}

#[test]
fn an_unsolved_function_value_argument_is_reported_once() {
    assert_eq!(run("    _ := once(ident)\n"), ["cannot-infer-type"]);
}

#[test]
fn an_unsolved_data_literal_parameter_is_reported() {
    let src = "struct Batch[T]:\n    items: List[T]\n\npub fn main() -> void:\n    _ := Batch { items: [] }\n";
    assert_eq!(codes(&analyze(src)), ["cannot-infer-type"]);
}

#[test]
fn a_parameter_solved_by_the_expected_type_is_fine() {
    assert!(run("    let x: i32 = make()\n    _ := x\n").is_empty());
}

#[test]
fn a_parameter_with_a_default_is_fine() {
    assert!(run("    _ := dflt()\n").is_empty());
}
