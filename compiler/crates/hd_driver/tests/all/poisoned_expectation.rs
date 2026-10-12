//! A poisoned expected type absorbs the expression checked against it: the
//! error that poisoned it is reported once, and no `cannot-infer-type`
//! follows from the value's open types. Unrelated unsolved types in the
//! same body still report.

use hd_diag::Code;

use crate::type_default_written_run::codes_of;

const HEAD: &str = "\
type Both[A, B] = (A, B)

data Pair[K, V]:
    key: K
    value: V

fn id[T](x: T) -> T: x

";

/// Each way to reach a poisoned expectation: the first error's code and a
/// function whose body is `{body}` against that expectation.
fn cases(body: &str) -> Vec<(Code, String)> {
    let indented = body.replace('\n', "\n    ");
    vec![
        (
            Code::UnknownType,
            format!("fn h() -> Nope:\n    {indented}\n"),
        ),
        (
            Code::PartialGenericArguments,
            format!("fn h() -> Both[i32]:\n    {indented}\n"),
        ),
        (
            Code::ArgumentCount,
            format!("fn h() -> Both[i32, i32, i32]:\n    {indented}\n"),
        ),
        (
            Code::UnknownType,
            format!("fn h() -> i32:\n    let a: Nope = {indented}\n    1\n"),
        ),
        (
            Code::PartialGenericArguments,
            format!("fn h() -> i32:\n    let a: Both[i32] = {indented}\n    1\n"),
        ),
    ]
}

fn assert_first_error_only(body: &str) {
    for (code, f) in cases(body) {
        let codes = codes_of(&format!("{HEAD}{f}"));
        let errors: Vec<Code> = codes
            .into_iter()
            .filter(|c| *c != Code::UnusedLocalBinding)
            .collect();
        assert_eq!(errors, vec![code], "{f}");
    }
}

#[test]
fn an_integer_literal_reports_only_the_first_error() {
    assert_first_error_only("5");
}

#[test]
fn an_empty_list_reports_only_the_first_error() {
    assert_first_error_only("[]");
}

#[test]
fn a_panic_reports_only_the_first_error() {
    assert_first_error_only("panic(\"x\")");
}

#[test]
fn an_option_variant_reports_only_the_first_error() {
    assert_first_error_only(".None");
}

#[test]
fn a_closure_reports_only_the_first_error() {
    assert_first_error_only("fn(x: i32) -> i32: x");
}

#[test]
fn a_generic_call_reports_only_the_first_error() {
    assert_first_error_only("id(5)");
    assert_first_error_only("id([])");
}

#[test]
fn a_poisoned_parameter_absorbs_its_argument() {
    let src = format!("{HEAD}fn f(a: Nope) -> i32: 1\n\nfn h() -> i32: f([])\n");
    assert_eq!(codes_of(&src), vec![Code::UnknownType]);
}

#[test]
fn an_unrelated_unsolved_type_in_the_same_body_still_reports() {
    for ret in ["Nope", "Both[i32]"] {
        let src = format!("{HEAD}fn h() -> {ret}:\n    y := []\n    5\n");
        let codes: Vec<Code> = codes_of(&src)
            .into_iter()
            .filter(|c| *c != Code::UnusedLocalBinding)
            .collect();
        assert_eq!(codes.len(), 2, "{ret}: {codes:?}");
        assert_eq!(codes[1], Code::CannotInferType, "{ret}");
    }
}
