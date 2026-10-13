//! The driver reports a syntax diagnostic with the message the lexer or
//! parser gave it, and an import the module does not give with the module
//! and the name, never the code twice.

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

/// The `code: message` pairs of a run's errors.
fn errors(out: &Output) -> Vec<String> {
    (0..out.diags.len())
        .filter(|&i| out.diags.severity[i] == hd_diag::Severity::Error)
        .map(|i| {
            format!(
                "{}: {}",
                out.diags.code[i].as_str(),
                out.diags.get_text(out.diags.message[i])
            )
        })
        .collect()
}

#[test]
fn a_syntax_error_says_what_was_expected() {
    let out = analyze("fn main():\n    if a and b:\n        pass\n");
    assert!(
        errors(&out).contains(&"syntax-error: expected ':', found 'and'".to_owned()),
        "{:?}",
        errors(&out)
    );
}

#[test]
fn a_rule_diagnostic_says_the_rule() {
    let out = analyze("fn main():\n    x := \"Ada\";\n");
    assert!(
        errors(&out).contains(
            &"reserved-semicolon: semicolon is reserved and cannot separate statements".to_owned()
        ),
        "{:?}",
        errors(&out)
    );
}

#[test]
fn an_unknown_import_names_the_module_and_the_name() {
    let out = analyze("use std.cmp.{NoSuch}\n\nfn main():\n    pass\n");
    assert!(
        errors(&out).contains(&"unknown-import: module 'std.cmp' declares no 'NoSuch'".to_owned()),
        "{:?}",
        errors(&out)
    );
}

/// The first message of a run's errors.
fn first_error(src: &str) -> String {
    errors(&analyze(src)).swap_remove(0)
}

#[test]
fn a_function_type_parameter_prints_as_written() {
    let msg = first_error("fn use_it[T](x: T) -> i32:\n    x\n");
    assert!(msg.ends_with("expected i32, found T"), "{msg}");
}

#[test]
fn an_impl_and_method_parameter_print_as_written() {
    let src =
        "data Box[T]:\n    v: T\n\nimpl[T] Box[T]:\n    fn get[U](self, u: U) -> T:\n        u\n";
    let msg = first_error(src);
    assert!(msg.ends_with("expected T, found U"), "{msg}");
}

#[test]
fn a_trait_self_and_its_associated_type_print_without_ids() {
    let src = "trait Index:\n    type Out\n    fn at(self) -> Self::Out\n    fn me(self) -> i32:\n        self\n";
    let msg = first_error(src);
    assert!(msg.ends_with("expected i32, found Self"), "{msg}");
}

#[test]
fn a_generic_type_from_std_prints_its_arguments_as_written() {
    let msg = first_error("fn pick[K, V](m: Map[K, V]) -> i32:\n    m\n");
    assert!(msg.ends_with("expected i32, found Map[K, V]"), "{msg}");
}

#[test]
fn a_parameter_of_an_enum_prints_as_written() {
    let src = "enum Pair[A, B]:\n    Left(A)\n    Right(B)\n\nfn pr[A, B](p: Pair[A, B]) -> i32:\n    p\n";
    let msg = first_error(src);
    assert!(msg.ends_with("expected i32, found Pair[A, B]"), "{msg}");
    assert!(!msg.contains('#'), "{msg}");
}
