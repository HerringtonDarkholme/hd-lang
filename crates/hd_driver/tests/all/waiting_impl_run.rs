//! Trait method calls whose impl is chosen after inference (#263h; spec 09
//! "Method Resolution", "Instantiations Of One Generic Trait"; spec 04
//! "Open Literal Width"): a receiver whose literal class is still open
//! types the call by its trait's signature, and the impl, the trait's
//! arguments and any `Self::Out` projection are fixed once the class takes
//! its type at the end of the statement. Programs build through the driver
//! and run on V8 (`host/run.mjs`).

use hd_diag::Code;

use crate::type_default_written_run::{codes_of, output_of};

const DESCRIBE: &str = "\
trait Describe:
    fn describe(self) -> string

impl Describe for (i32, string):
    fn describe(self) -> string:
        \"pair \" + self._1

impl Describe for (i32, i32, i32):
    fn describe(self) -> string:
        \"triple\"
";

const PICK: &str = "\
trait Pick[T]:
    fn pick(self) -> T

impl Pick[string] for (i32, i32):
    fn pick(self) -> string:
        \"picked\"
";

/// Tuple targets dispatch by arity once the signed literals are `i32`, in
/// a statement, a function's final value and a closure body.
#[test]
fn a_tuple_receiver_with_open_literals_chooses_its_impl() {
    let main = format!(
        "{DESCRIBE}
fn tail() -> string:
    (+1, \"t\").describe()

pub fn main() -> void $ Console:
    println((+1, \"a\").describe())
    println((+1, +2, +3).describe())
    println(tail())
    f := fn() -> string: (+2, \"c\").describe()
    println(f())
"
    );
    assert_eq!(
        output_of("waiting-impl-tuple", &main),
        "pair a\ntriple\npair t\npair c\n"
    );
}

/// `Iterable.iter` on a range of literals: the range's class takes its
/// default, and the impl it chooses fixes the item type that `sum` and
/// the other literal of the statement then take.
#[test]
fn a_range_of_literals_chooses_its_iterable_impl() {
    let main = "\
pub fn main() -> void $ Console:
    total := (1..=10).iter().sum()
    println(total)
    signed := (+1..=4).iter().sum() - 20
    println(signed)
    for x in (1..=2).iter():
        println(x)
";
    assert_eq!(output_of("waiting-impl-range", main), "55\n-10\n1\n2\n");
}

/// A trait argument that only the impl fixes is learned from it.
#[test]
fn the_impl_fixes_a_trait_argument() {
    let main = format!(
        "{PICK}
pub fn main() -> void $ Console:
    println((+1, +2).pick())
    let s: string = (+3, +4).pick()
    println(s)
"
    );
    assert_eq!(output_of("waiting-impl-pick", &main), "picked\npicked\n");
}

/// A `Self::Out` result on a receiver not yet known is the impl's type
/// once it is chosen: a literal it meets takes that type, and a member
/// asked of it chooses the impl at once.
#[test]
fn a_projection_is_used_before_the_receiver_is_fixed() {
    let main = "\
use std.ops.{Div, Neg}

data Point:
    x: i32

trait Get:
    type Out
    fn get(self) -> Self::Out

impl Get for (i32, string):
    type Out = string
    fn get(self) -> string:
        self._1

impl Get for (i32, bool):
    type Out = Point
    fn get(self) -> Point:
        Point { x: 5 }

pub fn main() -> void $ Console:
    q := Div::[i32]::div(-7, 2) + 1
    println(q)
    let y: f64 = Neg::neg(2.5)
    println(y)
    println((+1, \"abc\").get().len())
    println((+1, true).get().x)
    let t: string = (+1, \"xy\").get()
    println(t)
";
    assert_eq!(
        output_of("waiting-impl-projection", main),
        "-2\n-2.5\n3\n5\nxy\n"
    );
}

/// A call whose receiver, with its defaults in, has no impl is one
/// `unsatisfied-trait-bound`, its projection adding nothing.
#[test]
fn a_call_that_never_finds_an_impl_is_one_error() {
    let main = format!(
        "{DESCRIBE}
pub fn main() -> void $ Console:
    println((1, \"a\").describe())
"
    );
    assert_eq!(codes_of(&main), vec![Code::UnsatisfiedTraitBound]);
    let main = "\
use std.ops.Div

pub fn main() -> void $ Console:
    q := Div::[i32]::div(7, 2) + 1
    println(q)
";
    assert_eq!(codes_of(main), vec![Code::UnsatisfiedTraitBound]);
}

/// A known receiver whose impl still waits at the end of the body has
/// several instantiations that fit: `ambiguous-method`.
#[test]
fn a_call_left_between_instantiations_is_ambiguous() {
    let main = format!(
        "{PICK}
impl Pick[i64] for (i32, i32):
    fn pick(self) -> i64:
        7

pub fn main() -> void $ Console:
    _ := (+1, +2).pick()
"
    );
    assert_eq!(codes_of(&main), vec![Code::AmbiguousMethod]);
}
