//! Exact generic-local diagnostics. Fixed-source behavior is covered by conformance.

use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str, goal: &Goal) -> Output {
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
    build(&host, "app", goal)
}

fn program(main: &str) -> Output {
    built(
        main,
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

/// The rendered diagnostics of a program that must not build.
pub(crate) fn errors_of(main: &str) -> String {
    let out = program(main);
    assert!(out.wasm.is_none(), "the program must not build");
    out.render()
}

#[test]
fn a_type_parameter_of_the_enclosing_function_is_not_redeclared() {
    let errors = errors_of(
        "\
fn work[T](value: T) -> i32:
    fn convert[T](other: T) -> i32: 0
    0

pub fn main() -> void:
    pass
",
    );
    assert!(errors.contains("duplicate-binding"), "{errors}");
    assert!(
        errors.contains("`T` is a type parameter in scope"),
        "{errors}"
    );
}

#[test]
fn its_rules_are_a_generic_function_s() {
    let errors = errors_of(
        "\
trait Shape:
    fn area(self) -> i32

fn unsolved() -> i32:
    fn id[U](u: U) -> U: u
    f := id
    0

fn unbounded() -> i32:
    fn area_of[S < Shape](shape: S) -> i32: shape.area()
    area_of(+3)

fn checked_once() -> i32:
    fn size[U](u: U) -> i32: u.area()
    0

pub fn main() -> void:
    pass
",
    );
    // `fn.type.generic.unsolved`, a bound at the use, and the body checked
    // over its parameters, not over a use's types.
    assert!(errors.contains("cannot-infer-type"), "{errors}");
    assert!(
        errors.contains("unsatisfied-trait-bound: i32 does not implement Shape"),
        "{errors}"
    );
    assert!(
        errors.contains("unknown-method: no method `area`"),
        "{errors}"
    );
}
