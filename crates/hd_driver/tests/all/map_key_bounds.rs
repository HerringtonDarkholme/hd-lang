//! A `Map` key type meets `Map[K < Eq & Hash, V]` wherever a type is
//! written (`types.map-key.declared-bound`): annotations, parameters,
//! results, fields and nested arguments, one error per bad key. A `mut`
//! key is `invalid-map-key` (`types.map-key.no-mut`).

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The codes of the errors of one analysis, in report order.
fn errors(main: &str) -> Vec<Code> {
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
        .filter(|c| *c != Code::UnusedLocalBinding)
        .collect()
}

const BOUND: &[Code] = &[Code::UnsatisfiedTraitBound];

#[test]
fn hashable_keys_are_accepted_in_every_position() {
    let src = "\
@derive(Eq, Hash)
data Id:
    value: string

data Holder:
    pub by_id: Map[Id, i32]
    pub by_pair: Map[List[string], Map[u8, bool]]
    pub by_option: Map[i32?, string]

fn lookup(m: Map[(i32, string), i32], k: Map[char, Result[bool, string]]) -> Map[Id, List[Id]]:
    {}

fn local() -> void:
    let m: Map[bool, Map[usize, string]] = {}
    pass
";
    assert_eq!(errors(src), Vec::<Code>::new());
}

#[test]
fn a_float_key_fails_in_a_let_annotation() {
    assert_eq!(errors("let m: Map[f64, string] = {}\n"), BOUND);
}

#[test]
fn a_float_key_fails_in_every_float_width() {
    assert_eq!(errors("let m: Map[f32, i32] = {}\n"), BOUND);
}

#[test]
fn a_float_key_fails_in_a_parameter() {
    assert_eq!(errors("fn f(m: Map[f32, i32]) -> i32:\n    0\n"), BOUND);
}

#[test]
fn a_float_key_fails_in_a_result() {
    assert_eq!(errors("fn f() -> Map[f64, i32]:\n    {}\n"), BOUND);
}

#[test]
fn a_float_key_fails_in_a_field() {
    assert_eq!(errors("data Holder:\n    pub m: Map[f64, i32]\n"), BOUND);
}

#[test]
fn a_list_of_floats_has_no_hash_in_a_parameter() {
    assert_eq!(
        errors("fn f(m: Map[List[f64], i32]) -> i32:\n    0\n"),
        BOUND
    );
}

#[test]
fn an_option_of_floats_has_no_hash() {
    assert_eq!(errors("let m: Map[f64?, i32] = {}\n"), BOUND);
}

#[test]
fn a_function_key_has_no_eq() {
    assert_eq!(
        errors("fn f(m: Map[fn() -> void, i32]) -> i32:\n    0\n"),
        BOUND
    );
}

#[test]
fn a_map_key_has_no_hash() {
    assert_eq!(
        errors("fn f(m: Map[Map[string, i32], i32]) -> i32:\n    0\n"),
        BOUND
    );
}

#[test]
fn data_without_the_traits_is_not_a_key() {
    let src = "data Plain:\n    n: i32\n\nfn f(m: Map[Plain, i32]) -> i32:\n    0\n";
    assert_eq!(errors(src), BOUND);
}

#[test]
fn a_bad_key_nested_in_a_value_is_reported_once() {
    let src = "fn f(m: Map[string, List[Map[f64, i32]]]) -> i32:\n    0\n";
    assert_eq!(errors(src), BOUND);
}

#[test]
fn a_bad_key_nested_in_a_local_annotation_is_reported_once() {
    let src = "fn f() -> void:\n    let m: List[Map[f64, i32]] = []\n    pass\n";
    assert_eq!(errors(src), BOUND);
}

#[test]
fn a_map_inside_a_key_and_its_own_bad_key_are_two_errors() {
    // The inner key `f64` misses, and the outer key holds a map, which has
    // no `Hash`.
    let src = "fn f(m: Map[List[Map[f64, i32]], i32]) -> i32:\n    0\n";
    assert_eq!(errors(src), [Code::UnsatisfiedTraitBound; 2]);
}

#[test]
fn two_bad_keys_are_two_errors() {
    let src = "fn f(a: Map[f64, i32], b: Map[f32, i32]) -> i32:\n    0\n";
    assert_eq!(errors(src), [Code::UnsatisfiedTraitBound; 2]);
}

#[test]
fn a_generic_key_needs_its_bounds() {
    let src = "\
fn good[K < Eq & Hash](m: Map[K, i32]) -> i32:
    0

fn bad[K](m: Map[K, i32]) -> i32:
    0
";
    assert_eq!(errors(src), BOUND);
}

#[test]
fn a_mut_key_is_invalid_in_a_let_annotation() {
    let src = "\
@derive(Eq, Hash)
data Session:
    token: string

fn setup() -> void:
    let open: Map[mut Session, i32] = {}
    pass
";
    assert_eq!(errors(src), [Code::InvalidMapKey]);
}

#[test]
fn a_mut_key_is_invalid_in_a_parameter_a_result_and_a_field() {
    let src = "\
@derive(Eq, Hash)
data Session:
    token: string

data Holder:
    pub open: Map[mut Session, i32]

fn f(m: Map[mut Session, i32]) -> Map[mut Session, i32]:
    m
";
    assert_eq!(errors(src), [Code::InvalidMapKey; 3]);
}

#[test]
fn a_mut_key_that_is_also_unhashable_is_one_error() {
    let src = "data Plain:\n    n: i32\n\nfn f(m: Map[mut Plain, i32]) -> i32:\n    0\n";
    assert_eq!(errors(src), [Code::InvalidMapKey]);
}

#[test]
fn a_mut_value_is_not_a_key() {
    let src = "\
data Session:
    token: string

fn f(m: Map[string, mut Session]) -> i32:
    0
";
    assert_eq!(errors(src), Vec::<Code>::new());
}
