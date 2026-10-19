//! Too many type arguments in a written type (spec/lang/04-type-system.md,
//! types.generic.too-long): `argument-count`, once at the type, in every
//! type position, for aliases, data types, enums, newtypes and traits.
//! The type lowers to poison, so nothing follows it.

use hd_diag::Code;

use crate::type_default_written_run::{codes_of, output_of};

const HEAD: &str = "\
type Both[A, B] = (A, B)

trait Show:
    fn show(self) -> string

trait Conv[T]:
    fn conv(self) -> T

struct Pair[K, V]:
    key: K
    value: V

enum Choice[T]:
    Some(T)
    Empty

type Wrap[T](T)

";

fn assert_once(bads: &[&str]) {
    for bad in bads {
        assert_eq!(
            codes_of(&format!("{HEAD}{bad}\n")),
            vec![Code::ArgumentCount],
            "{bad}"
        );
    }
}

#[test]
fn an_alias_written_too_long_reports_once_in_every_position() {
    assert_once(&[
        "fn f(b: Both[i32, string, bool]) -> i32: 0",
        "struct Holder:\n    both: Both[i32, string, bool]",
        "enum Shape:\n    Dot(Both[i32, string, bool])",
        "impl Show for Both[i32, string, bool]:\n    fn show(self) -> string: \"\"",
        "fn f[T < Conv[Both[i32, string, bool]]](t: T) -> i32: 0",
        "fn f() -> i32:\n    let _b: Both[i32, string, bool] = (1, 2)\n    0",
        "fn f(b: List[Both[i32, string, bool]]) -> i32: 0",
    ]);
}

#[test]
fn a_data_type_written_too_long_reports_once() {
    assert_once(&[
        "fn f(p: Pair[i32, i32, i32]) -> i32: 0",
        "fn f() -> i32:\n    let _x: Pair[i32, i32, i32] = panic(\"x\")\n    0",
        "fn f() -> i32:\n    let _x: Choice[i32, i32] = panic(\"x\")\n    0",
        "fn f() -> i32:\n    let _x: Wrap[i32, i32] = panic(\"x\")\n    0",
        "fn f(x: Choice[i32, i32]) -> i32: 0",
        "fn f(x: Wrap[i32, i32]) -> i32: 0",
    ]);
}

#[test]
fn a_trait_reference_written_too_long_reports_once() {
    assert_once(&[
        "fn f[T < Conv[i32, i32]](t: T) -> i32: 0",
        "fn f(x: dyn Conv[i32, i32]) -> i32: 0",
        "impl Conv[i32, i32] for i32:\n    fn conv(self) -> i32: 0",
        "fn f() -> i32:\n    let _x: dyn Conv[i32, i32] = panic(\"x\")\n    0",
    ]);
}

#[test]
fn valid_arities_and_defaults_stay_valid() {
    let main = "\
type Twin[A, B = A] = (A, B)

struct Pair[K, V = K]:
    key: K
    value: V

pub fn main() -> void $ Console:
    let a: Twin[i32] = (1, 2)
    let b: Twin[i32, string] = (1, \"s\")
    let p: Pair[i32] = Pair { key: 3, value: 4 }
    println(a._1)
    println(b._1)
    println(p.value)
";
    assert_eq!(output_of("too-long-valid", main), "2\ns\n4\n");
}
