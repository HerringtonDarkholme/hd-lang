//! Sealed traits (spec 09 "Sealed Traits"): user code implements none of
//! the traits of the table, whatever the target, with one
//! `sealed-trait-implementation` on the `impl` or on the member and
//! nothing after it. A bound, a supertrait and a value type may still name
//! one.

use hd_diag::Code;
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

fn codes(main: &str) -> Vec<Code> {
    let out = analyze(main);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

/// `main` reports exactly one sealed-trait error.
fn sealed_once(main: &str) {
    let out = analyze(main);
    assert_eq!(
        codes(main),
        vec![Code::SealedTraitImplementation],
        "{}",
        out.render()
    );
}

/// `main` reports nothing.
fn clean(main: &str) {
    let out = analyze(main);
    assert_eq!(codes(main), vec![], "{}", out.render());
}

#[test]
fn each_trait_of_the_table_rejects_a_user_impl() {
    let cases = [
        ("Any", "use std.core.Any\n", "impl Any for Handle\n"),
        (
            "AnyVal",
            "use std.core.AnyVal\n",
            "impl AnyVal for Handle\n",
        ),
        (
            "AnyRef",
            "use std.core.AnyRef\n",
            "impl AnyRef for Handle\n",
        ),
        (
            "Inspectable",
            "use std.inspect.{Inspectable, TypeId}\n",
            "impl Inspectable for Handle:\n    fn runtime_type(self) -> TypeId: TypeId::of::[string]()\n",
        ),
        (
            "Structure",
            "use std.structure.Structure\n",
            "impl Structure for Handle\n",
        ),
        ("Num", "use std.num.Num\n", "impl Num for Handle\n"),
        (
            "Integer",
            "use std.num.Integer\n",
            "impl Integer for Handle\n",
        ),
        ("Float", "use std.num.Float\n", "impl Float for Handle\n"),
        (
            "Tuple",
            "use std.function.Tuple\n",
            "impl Tuple for Handle\n",
        ),
    ];
    for (name, uses, imp) in cases {
        let main = format!("{uses}\ndata Handle: pass\n\n{imp}");
        let out = analyze(&main);
        assert_eq!(
            codes(&main),
            vec![Code::SealedTraitImplementation],
            "{name}\n{}",
            out.render()
        );
    }
}

#[test]
fn the_error_starts_at_the_impl() {
    let main = "use std.num.Num

type Count(i64)

impl Num for Count
";
    let out = analyze(main);
    let rendered = out.render();
    let start = main.find("impl Num").unwrap_or(0);
    assert!(
        rendered.contains(&format!("main.hd:{start}..")),
        "{rendered}"
    );
    assert_eq!(codes(main), vec![Code::SealedTraitImplementation]);
}

#[test]
fn a_rejected_impl_adds_no_second_error() {
    // Neither the missing supertraits, the overlap with the standard
    // library's blanket impls, nor the written members add an error.
    sealed_once(
        "use std.num.Num
use std.ops.Add

data Cents:
    value: i64

impl Add for Cents:
    type Out = Cents
    fn add(self, rhs: Cents) -> Cents:
        Cents { value: self.value + rhs.value }

impl Eq for Cents:
    fn eq(self, other: Cents) -> bool:
        self.value == other.value

impl Num for Cents:
    fn zero() -> Cents: Cents { value: 0 }
    fn one() -> Cents: Cents { value: 1 }
    fn from_i64(n: i64) -> Cents: Cents { value: n }
",
    );
    sealed_once(
        "use std.num.Integer

type Count(i64)

impl Integer for Count
",
    );
}

#[test]
fn numeric_family_and_tuple_targets_are_rejected_too() {
    sealed_once(
        "use std.num.{Integer, Num}

impl[N < Integer] Num for N
",
    );
    sealed_once(
        "use std.num.Float

impl Float for i32
",
    );
    sealed_once(
        "use std.inspect.Inspectable

impl Inspectable for (i32, string)
",
    );
    sealed_once(
        "use std.core.AnyRef
use std.function.Tuple
use std.structure.Structure

impl[T < Tuple] AnyRef for T by Structure
",
    );
}

#[test]
fn a_sealed_member_is_not_declared_or_written_again() {
    sealed_once(
        "use std.inspect.{Inspectable, TypeId}

trait Plugin < Inspectable:
    fn name(self) -> string
    fn runtime_type(self) -> TypeId
",
    );
    // Through a transitive supertrait.
    sealed_once(
        "use std.inspect.{Inspectable, TypeId}

trait Plugin < Inspectable:
    fn name(self) -> string

trait Loud < Plugin:
    fn runtime_type(self) -> TypeId
",
    );
    sealed_once(
        "use std.inspect.{Inspectable, TypeId}

trait Plugin < Inspectable:
    fn name(self) -> string

data Exporter:
    format: string

impl Plugin for Exporter:
    fn name(self) -> string: self.format

    fn runtime_type(self) -> TypeId:
        TypeId::of::[string]()
",
    );
}

#[test]
fn a_plain_supertrait_member_stays_a_duplicate() {
    assert_eq!(
        codes(
            "trait Greeter:
    fn greet(self) -> string

trait Loud < Greeter:
    fn greet(self) -> string
"
        ),
        vec![Code::DuplicateTraitMember]
    );
}

#[test]
fn naming_a_sealed_trait_stays_legal() {
    clean(
        "use std.inspect.Inspectable
use std.num.{Float, Integer, Num}

fn double[N < Num](n: N) -> N: n + n
fn count[N < Integer](n: N) -> N: n
fn half[F < Float](f: F) -> F: f
fn show[T < Inspectable](value: T) -> string: \"${value.runtime_type()}\"
fn same[T < AnyRef](a: T, b: T) -> bool: a is b
fn boxed(value: i32) -> dyn Any: value
fn inspected(value: i32) -> dyn Inspectable: value

trait Named < Inspectable:
    fn type_name(self) -> string

data User:
    name: string

impl Named for User:
    fn type_name(self) -> string: \"User\"
",
    );
}

#[test]
fn an_inherent_method_may_share_a_sealed_member_name() {
    clean(
        "use std.inspect.TypeId

data User:
    name: string

impl User:
    fn runtime_type(self) -> string: self.name
",
    );
}

#[test]
fn a_bound_naming_no_trait_adds_no_second_error() {
    for body in ["left is right", "left == right"] {
        let main = format!("fn same[T < Reference](left: T, right: T) -> bool: {body}\n");
        let out = analyze(&main);
        assert_eq!(
            codes(&main),
            vec![Code::UnknownTrait],
            "{body}\n{}",
            out.render()
        );
    }
    let main = "fn name[T < Reference](value: T) -> string: value.describe()\n";
    let out = analyze(main);
    assert_eq!(codes(main), vec![Code::UnknownTrait], "{}", out.render());
}

/// `Suspend[T]` is a built-in type, so an `impl` of it names no trait
/// (`req.protocol.builtin`).
#[test]
fn an_impl_of_suspend_is_an_unknown_trait() {
    let main = "data Handle: pass\n\nimpl Suspend[i32] for Handle:\n    fn poll(mut self, context: PollContext) -> Poll[i32]: Poll.Ready(1)\n    fn cancel(mut self) -> void: pass\n";
    let out = analyze(main);
    assert_eq!(codes(main), vec![Code::UnknownTrait], "{}", out.render());
    assert!(out.render().contains("built-in type"), "{}", out.render());
}
