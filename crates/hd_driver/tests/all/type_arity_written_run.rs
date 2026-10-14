//! Arity of written types (spec/lang/04-type-system.md,
//! types.generic.default.written-missing, types.generic.default.fill): an
//! alias applied to too few arguments with no default is
//! `partial-generic-arguments`, once, in every type position; an omitted
//! slot in explicit type arguments on an associated-function path takes its
//! default with the earlier arguments substituted; an argument and a
//! default filled from it that both miss a bound report once.

use hd_diag::Code;

use crate::type_default_written_run::{codes_of, output_of};

const PARTIAL: Code = Code::PartialGenericArguments;

#[test]
fn an_alias_missing_a_slot_without_a_default_reports_once_in_every_position() {
    let head = "\
type Both[A, B] = (A, B)

trait Show:
    fn show(self) -> string

data Pair[K, V]:
    key: K
    value: V

";
    for bad in [
        "fn f(b: Both[i32]) -> i32: 0",
        "trait Make:\n    fn make() -> Both[i32]",
        "fn f(b: Both) -> i32: 0",
        "data Holder:\n    both: Both[i32]",
        "enum Shape:\n    Dot(Both[i32])",
        "fn f() -> i32:\n    let b: Both[i32] = (1, 2)\n    b._0",
        "fn f(b: List[Both[i32]]) -> i32: 0",
        "fn f(b: Pair[Both[i32], i32]) -> i32: 0",
        "fn f(b: (Both[i32], i32)) -> i32: 0",
        "impl Show for Both[i32]:\n    fn show(self) -> string: \"\"",
    ] {
        assert_eq!(codes_of(&format!("{head}{bad}\n")), vec![PARTIAL], "{bad}");
    }
}

#[test]
fn an_alias_missing_a_defaulted_slot_is_valid() {
    let main = "\
type Twin[A, B = A] = (A, B)

type Both[A, B] = (A, B)

fn twin(x: i32) -> Twin[i32]:
    (x, x + 1)

fn both(x: i32) -> Both[i32, string]:
    (x, \"s\")

pub fn main() -> void $ Console:
    t := twin(1)
    let b: Twin[i32] = (3, 4)
    println(t._0 + t._1 + b._1)
    println(both(2))
";
    assert_eq!(output_of("arity-alias-valid", main), "7\n(2, s)\n");
}

#[test]
fn an_associated_function_path_fills_defaults() {
    let main = "\
data Cell[T, U = List[T]]:
    first: T
    rest: U

impl[T, U] Cell[T, U]:
    fn make[V = T](v: V) -> V: v
    fn rest_of(self) -> U: self.rest
    fn first_of(a: U) -> U: a

pub fn main() -> void $ Console:
    println(Cell::[i32]::make(3))
    println(Cell::[i32]::first_of([1, 2]).len())
    println(Cell::[i32, string]::first_of(\"s\"))
";
    assert_eq!(output_of("arity-assoc-valid", main), "3\n2\ns\n");
}

#[test]
fn an_associated_function_path_takes_the_substituted_default() {
    let head = "\
data Cell[T, U = List[T]]:
    first: T
    rest: U

impl[T, U] Cell[T, U]:
    fn first_of(a: U) -> U: a
    fn gather[A, B = List[A]](a: A) -> B: panic(\"x\")

";
    for bad in [
        "fn f() -> i32:\n    Cell::[i32]::first_of(\"s\")\n    0",
        "fn f() -> i32:\n    x := Cell::[i32]::gather::[string](\"s\")\n    x.len()\n    x + 1\n    0",
    ] {
        let codes = codes_of(&format!("{head}{bad}\n"));
        assert!(
            !codes.is_empty() && codes.iter().all(|c| *c == Code::TypeMismatch),
            "{bad}: {codes:?}"
        );
    }
}

#[test]
fn an_argument_and_the_default_filled_from_it_report_one_bound_error() {
    let signature = "\
data Plain:
    x: i32

data Tagged[T < Hash, H < Hash = T]:
    tag: T
    hash: H

fn use_it(tagged: Tagged[Plain]) -> i32: 0
";
    assert_eq!(codes_of(signature), vec![Code::UnsatisfiedTraitBound]);
    let field = "\
data Plain:
    x: i32

data Tagged[T < Hash, H < Hash = T]:
    tag: T
    hash: H

data Holder:
    inner: Tagged[Plain]
";
    assert_eq!(codes_of(field), vec![Code::UnsatisfiedTraitBound]);
}

#[test]
fn a_default_alone_missing_its_bound_reports_once() {
    let signature = "\
data Plain:
    x: i32

data Tagged[T, H < Hash = Plain]:
    tag: T
    hash: H

fn use_it(tagged: Tagged[i32]) -> i32: 0
";
    assert_eq!(codes_of(signature), vec![Code::UnsatisfiedTraitBound]);
}

/// A default that misses its parameter's bound reports at the
/// declaration, even with no use (`types.generic.default.bound`,
/// `types.generic.default.checked-once`).
#[test]
fn a_default_missing_its_bound_reports_at_the_declaration() {
    let signature = "\
trait Blank:
    fn blank() -> Self

data Plain: pass

fn empty[C < Blank = Plain]() -> C:
    C::blank()
";
    assert_eq!(codes_of(signature), vec![Code::UnsatisfiedTraitBound]);
}
