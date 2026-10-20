//! The operand checks of `is` (spec 05 "Identity Operands"): a tuple, a
//! value type, a list, a map or a function has no identity (`expr.is.tuple`,
//! `expr.is.value-operand`, `types.coll.no-identity`,
//! `expr.is.function.operand`), and two reference operands must be
//! compatible (`expr.is.compatible.referent`, `expr.is.incompatible.data`).
//! Each rejection is one error, with no report for the second operand.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const PRELUDE: &str = "\
struct User:
    name: string

struct Order:
    total: i32

struct Box[T]:
    item: T

enum Color:
    Red
    Green

trait Named:
    fn label(self) -> string

trait Entity < Named

trait Handle < AnyRef

impl Named for User:
    fn label(self) -> string: self.name

impl Entity for User:
    pass

";

/// The codes of a program's diagnostics, in source order.
fn codes(body: &str) -> Vec<Code> {
    let mut src = MemorySources::default();
    src.insert("main.hd", &format!("{PRELUDE}{body}"));
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

fn accepted(body: &str) {
    assert_eq!(codes(body), Vec::<Code>::new(), "{body}");
}

fn rejected(body: &str, code: Code) {
    assert_eq!(codes(body), vec![code], "{body}");
}

#[test]
fn reference_operands_of_one_type_compare() {
    accepted(
        "\
fn struct_refs(a: &User, b: &User) -> bool:
    a is b

fn generic_struct(a: &Box[i32], b: &Box[i32]) -> bool:
    a is b

fn negated(a: &User, b: &User) -> bool:
    !(a is b)
",
    );
}

#[test]
fn permissions_do_not_change_compatibility() {
    accepted(
        "\
fn mixed_refs(a: &User, b: &mut User) -> bool:
    a is b

fn inner(a: &Box[&mut User], b: &Box[&User]) -> bool:
    b.item is a.item
",
    );
}

#[test]
fn trait_value_references_of_one_trait_compare() {
    accepted(
        "\
fn same_trait(a: &dyn Named, b: &dyn Named) -> bool:
    a is b
",
    );
}

#[test]
fn generics_bounded_by_a_reference_trait_compare() {
    accepted(
        "fn same[T < AnyRef](a: T, b: T) -> bool:
    a is b

fn against_any[T < AnyRef](a: &T, b: &dyn Any) -> bool:
    let erased: &dyn Any = a
    erased is b

fn through_trait[T < Handle](a: T, b: T) -> bool:
    a is b
",
    );
}

#[test]
fn inference_and_never_do_not_add_errors() {
    accepted(
        "\
fn diverges(a: &User) -> bool:
    a is panic(\"stop\")
",
    );
}

#[test]
fn a_tuple_has_no_identity() {
    rejected(
        "fn f(a: (i32, i32), b: (i32, i32)) -> bool:\n    a is b\n",
        Code::IdentityRequiresReferences,
    );
    rejected(
        "fn f(a: (User, User), b: (User, User)) -> bool:\n    a is b\n",
        Code::IdentityRequiresReferences,
    );
    rejected(
        "fn f(a: (User, User), b: User) -> bool:\n    a is b\n",
        Code::IdentityRequiresReferences,
    );
}

#[test]
fn a_value_type_has_no_identity() {
    for ty in [
        "i32",
        "bool",
        "char",
        "f64",
        "string",
        "Color",
        "User?",
        "i32?",
        "List[User]",
        "Map[string, User]",
    ] {
        rejected(
            &format!("fn f(a: {ty}, b: {ty}) -> bool:\n    a is b\n"),
            Code::IdentityRequiresReferences,
        );
    }
}

#[test]
fn one_value_operand_is_one_error() {
    rejected(
        "fn f(a: User, b: i32) -> bool:\n    a is b\n",
        Code::IdentityRequiresReferences,
    );
    rejected(
        "fn f(a: i32, b: User) -> bool:\n    a is b\n",
        Code::IdentityRequiresReferences,
    );
    rejected(
        "fn f(a: string, b: &dyn Any) -> bool:\n    a is b\n",
        Code::IdentityRequiresReferences,
    );
    rejected(
        "fn f(a: i32, b: Order) -> bool:\n    a is b\n",
        Code::IdentityRequiresReferences,
    );
}

#[test]
fn a_literal_has_no_identity() {
    rejected(
        "fn f() -> bool:\n    1 is 2\n",
        Code::IdentityRequiresReferences,
    );
    rejected(
        "fn f() -> bool:\n    1.5 is 2.5\n",
        Code::IdentityRequiresReferences,
    );
    rejected(
        "fn f(a: User) -> bool:\n    a is 1\n",
        Code::IdentityRequiresReferences,
    );
}

#[test]
fn a_function_has_no_identity() {
    rejected(
        "fn inc(v: i32) -> i32: v + 1\n\nfn f() -> bool:\n    inc is inc\n",
        Code::IdentityRequiresReferences,
    );
    rejected(
        "fn f(a: fn() -> i32, b: &dyn Any) -> bool:\n    a is b\n",
        Code::IdentityRequiresReferences,
    );
    rejected(
        "struct Handler:\n    run: fn() -> i32\n\nfn f(a: Handler, b: Handler) -> bool:\n    a.run is b.run\n",
        Code::IdentityRequiresReferences,
    );
    rejected(
        "fn f() -> bool:\n    cb := fn() -> i32: 1\n    alias := cb\n    cb is alias\n",
        Code::IdentityRequiresReferences,
    );
}

#[test]
fn an_unbounded_type_parameter_has_no_identity() {
    rejected(
        "fn f[T](a: T, b: T) -> bool:\n    a is b\n",
        Code::IdentityRequiresReferences,
    );
    rejected(
        "fn f[T < Named](a: T, b: T) -> bool:\n    a is b\n",
        Code::IdentityRequiresReferences,
    );
}

#[test]
fn unrelated_reference_types_are_incompatible() {
    for (a, b) in [
        ("User", "Order"),
        ("Box[i32]", "Box[string]"),
        ("Box[Box[User]]", "Box[User]"),
        ("User", "Box[User]"),
    ] {
        rejected(
            &format!("fn f(a: &{a}, b: &{b}) -> bool:\n    a is b\n"),
            Code::IncompatibleIdentityOperands,
        );
    }
}

#[test]
fn a_trait_value_needs_the_other_type_to_convert() {
    rejected(
        "fn f(named: &dyn Named, order: &Order) -> bool:\n    named is order\n",
        Code::IncompatibleIdentityOperands,
    );
    rejected(
        "fn f(order: &Order, named: &dyn Named) -> bool:\n    order is named\n",
        Code::IncompatibleIdentityOperands,
    );
    rejected(
        "fn f[T < AnyRef](a: &T, user: &User) -> bool:\n    a is user\n",
        Code::IncompatibleIdentityOperands,
    );
}
