//! Declaration rules (spec 03, 08, 09, 14): each rule has a program that
//! breaks it, which reports exactly the rule's code, and a legal twin that
//! reports nothing.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn analyze_goal(main: &str, goal: &Goal) -> Output {
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
    build(&host, "app", goal)
}

fn analyze(main: &str) -> Output {
    analyze_goal(main, &Goal::Analyze)
}

fn codes_in(main: &str, goal: &Goal) -> Vec<Code> {
    let out = analyze_goal(main, goal);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

fn codes(main: &str) -> Vec<Code> {
    codes_in(main, &Goal::Analyze)
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
        "struct Invoice:
    number: string
    number: i64
",
        Code::DuplicateField,
        "struct Invoice:
    number: string
    total: i64
",
    );
}

#[test]
fn an_embedded_field_name_is_unique() {
    rule(
        "struct Box[T]:
    value: T

struct Storage:
    Box[i32]
    Box[string]
",
        Code::DuplicateEmbeddedField,
        "struct Box[T]:
    value: T

struct Tag:
    label: string

struct Storage:
    Box[i32]
    Tag
",
    );
}

#[test]
fn a_literal_gives_each_field_once() {
    rule(
        "struct User:
    name: string

user := User {
    name: \"Ada\",
    name: \"Grace\",
}
",
        Code::DuplicateField,
        "struct User:
    name: string

user := User {
    name: \"Ada\",
}
",
    );
}

#[test]
fn an_unknown_field_suppresses_the_missing_field_error() {
    rule(
        "struct Point:
    x: i32

fn get() -> i32: Point { y: 1 }.x
",
        Code::UnknownField,
        "struct Point:
    x: i32

fn get() -> i32: Point { x: 1 }.x
",
    );
}

#[test]
fn an_unknown_shorthand_suppresses_the_missing_field_error() {
    rule(
        "struct Point:
    x: i32
    y: i32

fn broken(x: i32) -> Point:
    Point { x, y }
",
        Code::UnknownName,
        "struct Point:
    x: i32
    y: i32

fn fixed(x: i32, y: i32) -> Point:
    Point { x, y }
",
    );
}

#[test]
fn a_shorthand_field_counts_as_given() {
    rule(
        "struct Coordinate:
    lat: i32
    lon: i32

fn pin(lat: i32, lon: i32) -> Coordinate:
    Coordinate { lat, lon, lat: 0 }
",
        Code::DuplicateField,
        "struct Coordinate:
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
        "struct Box[T]:
    value: T

impl[T] Box[T]:
    fn show(self) -> string: \"box\"

impl Box[i32]:
    fn show(self) -> string: \"int box\"
",
        Code::DuplicateInherentMember,
        // Targets that cannot unify may repeat a name.
        "struct Box[T]:
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
        "struct User: pass

impl User:
    fn name(self) -> string: \"first\"
    fn name(self) -> string: \"second\"
",
        Code::DuplicateInherentMember,
        "struct User: pass

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
        "struct Box[+T]:
    value: T

impl[T] Box[T]:
    pub fn echo[T](self, value: T) -> T:
        value
",
        Code::DuplicateBinding,
        "struct Box[+T]:
    value: T

impl[T] Box[T]:
    pub fn echo[U](self, value: U) -> U:
        value
",
    );
}

/// `names.type-param.no-redeclare`: no parameter reuses a type parameter's
/// name; a parameter of another function may.
#[test]
fn a_value_parameter_does_not_reuse_a_type_parameter() {
    rule(
        "fn keep[T](T: i32) -> i32:
    T
",
        Code::DuplicateBinding,
        "fn keep[T](value: T) -> T:
    value

fn other(T: i32) -> i32:
    T
",
    );
}

#[test]
fn a_method_parameter_does_not_reuse_the_implementation_s_type_parameter() {
    rule(
        "struct Box[+T]:
    value: T

impl[T] Box[T]:
    pub fn get(self, T: i32) -> i32:
        T
",
        Code::DuplicateBinding,
        "struct Box[+T]:
    value: T

impl[T] Box[T]:
    pub fn get(self, n: i32) -> i32:
        n

impl Box[i32]:
    pub fn other(self, T: i32) -> i32:
        T
",
    );
}

#[test]
fn a_trait_method_parameter_does_not_reuse_the_trait_s_type_parameter() {
    rule(
        "trait Tr[U]:
    fn m(self, U: i32) -> i32
",
        Code::DuplicateBinding,
        "trait Tr[U]:
    fn m(self, n: i32) -> i32
",
    );
}

#[test]
fn a_local_function_parameter_does_not_reuse_a_type_parameter() {
    rule(
        "fn work[T](value: T) -> i32:
    fn inner(T: i32) -> i32:
        T
    inner(1)
",
        Code::DuplicateBinding,
        "fn work[T](value: T) -> i32:
    fn inner(n: i32) -> i32:
        n
    inner(1)
",
    );
}

#[test]
fn a_closure_parameter_does_not_reuse_a_type_parameter() {
    rule(
        "fn work[T](value: T) -> i32:
    f := fn(T: i32) -> i32: T
    f(1)
",
        Code::DuplicateBinding,
        "fn work[T](value: T) -> i32:
    f := fn(n: i32) -> i32: n
    f(1)
",
    );
}

/// `names.fn.params.scope`: value parameters have distinct names.
#[test]
fn two_function_parameters_do_not_share_a_name() {
    rule(
        "fn two(a: i32, a: i32) -> i32:
    a
",
        Code::DuplicateBinding,
        "fn two(a: i32, b: i32) -> i32:
    a + b
",
    );
}

#[test]
fn two_method_parameters_do_not_share_a_name() {
    rule(
        "struct Box:
    value: i32

impl Box:
    pub fn add(self, n: i32, n: i32) -> i32:
        n
",
        Code::DuplicateBinding,
        "struct Box:
    value: i32

impl Box:
    pub fn add(self, n: i32, m: i32) -> i32:
        n + m
",
    );
}

#[test]
fn two_trait_method_parameters_do_not_share_a_name() {
    rule(
        "trait Tr:
    fn m(self, n: i32, n: i32) -> i32
",
        Code::DuplicateBinding,
        "trait Tr:
    fn m(self, n: i32, m: i32) -> i32
",
    );
}

#[test]
fn two_local_function_parameters_do_not_share_a_name() {
    rule(
        "fn work() -> i32:
    fn inner(a: i32, a: i32) -> i32:
        a
    inner(1, 2)
",
        Code::DuplicateBinding,
        "fn work() -> i32:
    fn inner(a: i32, b: i32) -> i32:
        a + b
    inner(1, 2)
",
    );
}

#[test]
fn two_closure_parameters_do_not_share_a_name() {
    rule(
        "fn work() -> i32:
    f := fn(a: i32, a: i32) -> i32: a
    f(1, 2)
",
        Code::DuplicateBinding,
        "fn work() -> i32:
    f := fn(a: i32, b: i32) -> i32: a + b
    f(1, 2)
",
    );
}

// ---- Embedding shape (group 13) ----

#[test]
fn an_embedded_field_names_a_data_type() {
    rule(
        "enum JobStatus:
    Queued
    Running

struct Job:
    JobStatus
    id: string
",
        Code::EmbeddedNonData,
        "struct JobStatus:
    label: string

struct Job:
    JobStatus
    id: string
",
    );
}

#[test]
fn a_collection_or_parameter_is_not_embedded() {
    let bad = "struct Job:
    List[i32]
    id: string
";
    assert_eq!(
        codes(bad),
        vec![Code::EmbeddedNonData],
        "{}",
        analyze(bad).render()
    );
    let bad = "struct Holder[T]:
    T
    id: string
";
    assert_eq!(
        codes(bad),
        vec![Code::EmbeddedNonData],
        "{}",
        analyze(bad).render()
    );
}

#[test]
fn a_type_embeds_at_most_three_fields() {
    let parts = "struct Created:
    created_at: i32

struct Updated:
    updated_at: i32

struct Owned:
    owner: string

struct Tagged:
    tag: string

";
    rule(
        &format!(
            "{parts}struct Post:
    Created
    Updated
    Owned
    Tagged
    title: string
"
        ),
        Code::TooManyEmbeddedFields,
        &format!(
            "{parts}struct Post:
    Created
    Updated
    Owned
    title: string
"
        ),
    );
}

const CHAIN: &str = "struct Stamp:
    at: i32

struct Audit:
    Stamp
    by: string

struct Record:
    Audit
    id: string

";

#[test]
fn an_embedding_chain_is_at_most_three_deep() {
    rule(
        &format!(
            "{CHAIN}struct Page:
    Record
    title: string

struct Site:
    Page
    host: string
"
        ),
        Code::EmbeddingTooDeep,
        &format!(
            "{CHAIN}struct Page:
    Record
    title: string
"
        ),
    );
}

#[test]
fn the_depth_limit_counts_generic_embedding() {
    rule(
        "struct Stamp:
    at: i32

struct Tag:
    Stamp
    label: string

struct Box[T]:
    Tag
    value: T

struct Shipment[T]:
    Box[T]
    id: string

struct Order:
    note: string
    Shipment[i32]
",
        Code::EmbeddingTooDeep,
        "struct Stamp:
    at: i32

struct Tag:
    Stamp
    label: string

struct Box[T]:
    Tag
    value: T

struct Order:
    note: string
    Box[i32]
",
    );
}

#[test]
fn a_type_does_not_embed_itself() {
    rule(
        "struct Loop:
    Loop
    id: string
",
        Code::EmbeddingCycle,
        "struct Leaf:
    id: string

struct Loop:
    Leaf
    id: string
",
    );
}

#[test]
fn a_cycle_is_reported_once_in_its_first_declared_type() {
    // Both types are in the cycle; only `Node`, declared first, reports it.
    rule(
        "struct Node:
    Edge
    id: string

struct Edge:
    Node
    weight: i32
",
        Code::EmbeddingCycle,
        "struct Node:
    id: string

struct Edge:
    Node
    weight: i32
",
    );
}

#[test]
fn a_cycle_through_a_generic_type_is_a_cycle() {
    rule(
        "struct Holder:
    Wrapper[i32]
    id: string

struct Wrapper[T]:
    Holder
    value: T
",
        Code::EmbeddingCycle,
        "struct Holder:
    id: string

struct Wrapper[T]:
    Holder
    value: T
",
    );
}

const ACCOUNT: &str = "struct User:
    name: string

struct Account:
    owner: User

";

#[test]
fn the_copy_marker_fills_only_an_embedded_field() {
    rule(
        &format!(
            "{ACCOUNT}fn open(user: User) -> Account:
    Account {{ owner: ...user }}
"
        ),
        Code::CopyIntoOrdinaryField,
        &format!(
            "{ACCOUNT}fn open(user: User) -> Account:
    Account {{ owner: user }}
"
        ),
    );
}

#[test]
fn the_copy_assignment_stores_only_an_embedded_field() {
    rule(
        &format!(
            "{ACCOUNT}fn reown(account: &mut Account, user: User) -> void:
    account.owner ...= user
"
        ),
        Code::CopyIntoOrdinaryField,
        &format!(
            "{ACCOUNT}fn reown(account: &mut Account, user: User) -> void:
    account.owner = user
"
        ),
    );
}

// ---- Member lines (group 13) ----

const WALKER: &str = "use std.structure.{Structure, Field, Variant, Walker}

trait Show:
    fn show(self) -> string

struct Shower:
    out: string

impl[S] Walker[S] for Shower:
    type Error = never

    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F](&mut self, h: Field[S, F], value: F) -> Result[void, never]:
        self.out = self.out + h.info.name + \";\"
        .Ok(())

impl[T] Show for T by Structure:
    fn show(self) -> string:
        let mut w = Shower { out: \"\" }
        _ := Structure::walk(self, &mut w)
        w.out

";

#[test]
fn a_whole_variant_cannot_be_omitted() {
    let enum_ = "enum Reply:
    Sent(id: i64)
    Busy

";
    rule(
        &format!(
            "{WALKER}{enum_}impl Show for Reply by Structure:
    Busy = pass
"
        ),
        Code::InvalidMemberLine,
        &format!(
            "{WALKER}{enum_}impl Show for Reply by Structure:
    Sent = []
"
        ),
    );
}

#[test]
fn a_block_with_no_trait_cannot_omit_a_member() {
    let account = "use std.structure.Structure

struct Token:
    text: string = \"\"

struct Account:
    id: i64
    token: Token = Token {}

";
    rule(
        &format!(
            "{account}impl Account by Structure:
    token = pass
"
        ),
        Code::InvalidMemberLine,
        &format!(
            "{account}impl Account by Structure:
    token = []
"
        ),
    );
}

#[test]
fn a_member_line_takes_a_list() {
    let pair = "struct Pair:
    left: i64
    right: i64

";
    rule(
        &format!(
            "{WALKER}{pair}impl Show for Pair by Structure:
    right = 5
"
        ),
        Code::InvalidMemberLine,
        &format!(
            "{WALKER}{pair}impl Show for Pair by Structure:
    right = []
"
        ),
    );
}
