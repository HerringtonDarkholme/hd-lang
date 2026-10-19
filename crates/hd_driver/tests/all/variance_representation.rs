//! A variance conversion never changes an element's representation
//! (`types.variance.no-insertion`, `fn.type.variance-repr.excluded`): one
//! that would is `variance-representation-change`, reported once. An
//! element with no conversion at all stays `type-mismatch`.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn codes(main: &str) -> Vec<Code> {
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
    let out = build(&host, "app", &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

const REPR: Code = Code::VarianceRepresentationChange;

#[test]
fn covariant_argument_to_a_trait_value() {
    let src = "struct Producer[+T]:
    value: T

fn widen(value: Producer[i32]) -> Producer[dyn Display]:
    value
";
    assert_eq!(codes(src), vec![REPR]);
}

#[test]
fn contravariant_argument_from_a_trait_value() {
    let src = "struct Consumer[-T]:
    consume: fn(T) -> void

fn narrow(value: Consumer[dyn Display]) -> Consumer[i32]:
    value
";
    assert_eq!(codes(src), vec![REPR]);
}

#[test]
fn function_result_to_a_trait_value() {
    let src = "fn count() -> i32: 1

fn erase() -> fn() -> dyn Display:
    count
";
    assert_eq!(codes(src), vec![REPR]);
}

#[test]
fn function_parameter_from_a_trait_value() {
    let src = "fn show(value: dyn Display) -> void: ()

fn narrow() -> fn(i32) -> void:
    show
";
    assert_eq!(codes(src), vec![REPR]);
}

#[test]
fn function_result_to_an_optional() {
    let src = "fn count() -> i32: 1

fn erase() -> fn() -> i32?:
    count
";
    assert_eq!(codes(src), vec![REPR]);
}

#[test]
fn unrelated_element_stays_a_type_mismatch() {
    let src = "struct Producer[+T]:
    value: T

fn widen(value: Producer[i32]) -> Producer[string]:
    value
";
    assert_eq!(codes(src), vec![Code::TypeMismatch]);
}

#[test]
fn unrelated_function_result_stays_a_type_mismatch() {
    let src = "fn count() -> i32: 1

fn erase() -> fn() -> string:
    count
";
    assert_eq!(codes(src), vec![Code::TypeMismatch]);
}

#[test]
fn child_trait_widening_inside_a_list_names_the_representation_change() {
    // `types.variance.no-insertion`: `dyn Dog` widens to `dyn Animal`,
    // rebuilding each element's trait value.
    let src = "trait Animal:
    fn name(self) -> string

trait Dog < Animal:
    fn bark(self) -> string

fn widen(items: List[dyn Dog]) -> List[dyn Animal]:
    items
";
    assert_eq!(codes(src), vec![REPR]);
}

#[test]
fn unrelated_trait_values_stay_a_type_mismatch() {
    let src = "trait Animal:
    fn name(self) -> string

trait Cat:
    fn meow(self) -> string

fn widen(items: List[dyn Dog]) -> List[dyn Cat]:
    items

trait Dog:
    fn bark(self) -> string
";
    assert_eq!(codes(src), vec![Code::TypeMismatch]);
}
