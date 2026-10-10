//! Declaration rules (spec 03, 08, 09, 14): each rule has a program that
//! breaks it, which reports exactly the rule's code, and a legal twin that
//! reports nothing.

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn analyze_goal(main: &str, goal: &Goal) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", goal)
}

fn codes_in(main: &str, goal: &Goal) -> Vec<Code> {
    let out = analyze_goal(main, goal);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

/// `bad` reports `code` once and nothing else; `good` reports nothing.
fn rule(bad: &str, code: Code, good: &str) {
    rule_in(&Goal::Analyze, bad, code, good);
}

fn rule_in(goal: &Goal, bad: &str, code: Code, good: &str) {
    let out = analyze_goal(bad, goal);
    assert_eq!(codes_in(bad, goal), vec![code], "{}", out.render());
    let out = analyze_goal(good, goal);
    assert_eq!(codes_in(good, goal), vec![], "{}", out.render());
}

// ---- Duplicate declarations (group 12) ----

#[test]
fn data_field_names_are_unique() {
    rule(
        "data Invoice:
    number: string
    number: i64
",
        Code::DuplicateField,
        "data Invoice:
    number: string
    total: i64
",
    );
}

#[test]
fn an_embedded_field_name_is_unique() {
    rule(
        "data Box[T]:
    value: T

data Storage:
    Box[i32]
    Box[string]
",
        Code::DuplicateEmbeddedField,
        "data Box[T]:
    value: T

data Tag:
    label: string

data Storage:
    Box[i32]
    Tag
",
    );
}

#[test]
fn a_literal_gives_each_field_once() {
    rule(
        "data User:
    name: string

user := User {
    name: \"Ada\",
    name: \"Grace\",
}
",
        Code::DuplicateField,
        "data User:
    name: string

user := User {
    name: \"Ada\",
}
",
    );
}

#[test]
fn a_shorthand_field_counts_as_given() {
    rule(
        "data Coordinate:
    lat: i32
    lon: i32

fn pin(lat: i32, lon: i32) -> Coordinate:
    Coordinate { lat, lon, lat: 0 }
",
        Code::DuplicateField,
        "data Coordinate:
    lat: i32
    lon: i32

fn pin(lat: i32, lon: i32) -> Coordinate:
    Coordinate { lat, lon }
",
    );
}

#[test]
fn variant_names_are_unique() {
    rule(
        "enum Color:
    Red
    Red
    Blue
",
        Code::DuplicateVariant,
        "enum Color:
    Red
    Green
    Blue
",
    );
}

#[test]
fn a_payload_parameter_does_not_repeat_a_shared_one() {
    rule(
        "enum Status(code: i32):
    Ok -> Status(200)
    Moved(code: i32) -> Status(301)
",
        Code::DuplicateField,
        "enum Status(code: i32):
    Ok -> Status(200)
    Moved(target: i32) -> Status(301)
",
    );
}

#[test]
fn a_supertrait_is_listed_once() {
    rule(
        "trait Parent

trait Twice < Parent & Parent
",
        Code::DuplicateSupertrait,
        "trait Parent

trait Other

trait Twice < Parent & Other
",
    );
}

#[test]
fn a_child_trait_declares_no_supertrait_member() {
    rule(
        "trait Greeter:
    fn greet(self) -> string

trait Polite < Greeter:
    fn thank(self) -> string

trait Loud < Polite:
    fn greet(self) -> string:
        \"HELLO\"
",
        Code::DuplicateTraitMember,
        "trait Greeter:
    fn greet(self) -> string

trait Polite < Greeter:
    fn thank(self) -> string

trait Loud < Polite:
    fn shout(self) -> string
",
    );
}

#[test]
fn a_child_trait_declares_no_supertrait_associated_type() {
    rule(
        "trait Feed:
    type Item

trait PriceFeed < Feed:
    type Item
",
        Code::DuplicateTraitMember,
        "trait Feed:
    type Item

trait PriceFeed < Feed:
    type Price
",
    );
}

#[test]
fn inherent_members_with_unifying_targets_are_unique() {
    rule(
        "data Box[T]:
    value: T

impl[T] Box[T]:
    fn show(self) -> string: \"box\"

impl Box[i32]:
    fn show(self) -> string: \"int box\"
",
        Code::DuplicateInherentMember,
        // Targets that cannot unify may repeat a name.
        "data Box[T]:
    value: T

impl Box[string]:
    fn show(self) -> string: \"string box\"

impl Box[i32]:
    fn show(self) -> string: \"int box\"
",
    );
}

#[test]
fn an_implementation_declares_an_inherent_member_once() {
    rule(
        "data User: pass

impl User:
    fn name(self) -> string: \"first\"
    fn name(self) -> string: \"second\"
",
        Code::DuplicateInherentMember,
        "data User: pass

impl User:
    fn name(self) -> string: \"first\"
    fn title(self) -> string: \"second\"
",
    );
}

#[test]
fn a_module_name_is_declared_once() {
    rule(
        "fn value(input: i32) -> i32: input
fn value(input: string) -> string: input
",
        Code::DuplicateModuleName,
        "fn value(input: i32) -> i32: input
fn text(input: string) -> string: input
",
    );
}

#[test]
fn a_tests_block_declares_each_name_once() {
    rule_in(
        &Goal::CheckTests,
        "tests:
    fn sample() -> i32: 1
    fn sample() -> i32: 2

    it(\"uses the sample\"):
        pass
",
        Code::DuplicateModuleName,
        "tests:
    fn sample() -> i32: 1
    fn other() -> i32: 2

    it(\"uses the sample\"):
        pass
",
    );
}

#[test]
fn a_method_type_parameter_does_not_reuse_the_implementation_s() {
    rule(
        "data Box[+T]:
    value: T

impl[T] Box[T]:
    pub fn echo[T](self, value: T) -> T:
        value
",
        Code::DuplicateBinding,
        "data Box[+T]:
    value: T

impl[T] Box[T]:
    pub fn echo[U](self, value: U) -> U:
        value
",
    );
}
