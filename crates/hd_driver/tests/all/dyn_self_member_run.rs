//! A method that names `Self` outside its receiver (spec 09
//! trait.dyn.member.self) is unavailable on a `dyn` value and through a
//! requirement key: `dyn-member-unavailable`, once, at the call. It has
//! no vtable slot, so the trait's other members validate and run. Method
//! type parameters and associated types keep a method available.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str, goal: &Goal) -> Output {
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

/// The standard output of a program that must build, validate as Wasm
/// (V8 compiles the whole module) and succeed.
fn output_of(name: &str, main: &str) -> String {
    let out = built(
        main,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("dyn-self-member-{name}.wasm"),
    );
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    assert!(
        ran.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8(ran.stdout).expect("UTF-8")
}

/// The diagnostic codes of a program checked without running.
fn codes_of(main: &str) -> Vec<Code> {
    let out = built(main, &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

const TRAIT: &str = "\
trait Shape:
    fn area(self) -> i32
    fn same_as(self, other: Self) -> bool
    fn twin(self) -> Self
    fn pair(self) -> List[Self]
    fn each(self, f: fn(Self) -> void) -> void
    fn pick[T](self, x: T) -> T
    fn many[T](self, xs: List[T]) -> List[T]

data Sq:
    n: i32

impl Shape for Sq:
    fn area(self) -> i32: self.n * self.n
    fn same_as(self, other: Sq) -> bool: self.n == other.n
    fn twin(self) -> Sq: self
    fn pair(self) -> List[Sq]: [self]
    fn each(self, f: fn(Sq) -> void) -> void: f(self)
    fn pick[T](self, x: T) -> T: x
    fn many[T](self, xs: List[T]) -> List[T]: xs
";

fn dyn_call(call: &str) -> String {
    format!(
        "{TRAIT}
fn use_it(s: dyn Shape) -> void:
    {call}

pub fn main() -> void:
    pass
"
    )
}

fn key_call(call: &str) -> String {
    format!(
        "{TRAIT}
fn use_it() -> void $ Shape:
    {call}

pub fn main() -> void:
    pass
"
    )
}

#[test]
fn self_in_a_parameter_is_unavailable_on_a_dyn_value() {
    let main = dyn_call("s.same_as(s)");
    assert_eq!(codes_of(&main), vec![Code::DynMemberUnavailable]);
}

#[test]
fn self_in_a_result_is_unavailable_on_a_dyn_value() {
    let main = dyn_call("s.twin()");
    assert_eq!(codes_of(&main), vec![Code::DynMemberUnavailable]);
}

#[test]
fn self_in_a_list_is_unavailable_on_a_dyn_value() {
    let main = dyn_call("s.pair()");
    assert_eq!(codes_of(&main), vec![Code::DynMemberUnavailable]);
}

#[test]
fn self_in_a_function_type_is_unavailable_on_a_dyn_value() {
    let main = dyn_call("s.each(fn(x): pass)");
    assert_eq!(codes_of(&main), vec![Code::DynMemberUnavailable]);
}

#[test]
fn self_in_a_parameter_is_unavailable_through_a_key() {
    let main = key_call("$.use(Shape).same_as($.use(Shape))");
    assert_eq!(codes_of(&main), vec![Code::DynMemberUnavailable]);
}

#[test]
fn self_in_a_result_is_unavailable_through_a_key() {
    let main = key_call("$.use(Shape).twin()");
    assert_eq!(codes_of(&main), vec![Code::DynMemberUnavailable]);
}

#[test]
fn self_in_a_list_is_unavailable_through_a_key() {
    let main = key_call("$.use(Shape).pair()");
    assert_eq!(codes_of(&main), vec![Code::DynMemberUnavailable]);
}

#[test]
fn self_in_a_function_type_is_unavailable_through_a_key() {
    let main = key_call("$.use(Shape).each(fn(x): pass)");
    assert_eq!(codes_of(&main), vec![Code::DynMemberUnavailable]);
}

#[test]
fn the_available_members_of_a_trait_with_self_members_run() {
    let main = format!(
        "{TRAIT}
fn show(s: dyn Shape) -> string:
    s.area().to_string() + \",\" + s.pick(5).to_string() + \",\" + s.many([1, 2]).len().to_string()

fn keyed() -> i32 $ Shape:
    $.use(Shape).area() + $.use(Shape).pick(1)

pub fn main() -> void $ Console:
    println(show(Sq {{ n: 3 }}))
    $.with(Shape=Sq {{ n: 2 }}):
        println(keyed())
"
    );
    assert_eq!(output_of("run", &main), "9,5,2\n5\n");
}

#[test]
fn a_supertrait_with_a_self_member_stays_usable_through_the_subtrait() {
    let main = "\
trait Base:
    fn id(self) -> i32
    fn merge(self, other: Self) -> Self

trait Sub < Base:
    fn extra(self) -> i32

data D:
    n: i32

impl Base for D:
    fn id(self) -> i32: self.n
    fn merge(self, other: D) -> D: self

impl Sub for D:
    fn extra(self) -> i32: 4

fn show(s: dyn Sub) -> i32:
    s.id() + s.extra()

pub fn main() -> void $ Console:
    println(show(D { n: 1 }))
";
    assert_eq!(output_of("super", main), "5\n");
}

#[test]
fn a_supertrait_self_member_is_unavailable_through_the_subtrait() {
    let main = "\
trait Base:
    fn id(self) -> i32
    fn merge(self, other: Self) -> Self

trait Sub < Base:
    fn extra(self) -> i32

fn show(s: dyn Sub) -> i32:
    s.merge(s).id()

pub fn main() -> void:
    pass
";
    assert_eq!(codes_of(main), vec![Code::DynMemberUnavailable]);
}

#[test]
fn an_associated_type_projection_of_self_keeps_a_method_available() {
    let main = "\
trait Bag:
    type Item
    fn first(self) -> Self::Item

data D:
    n: i32

impl Bag for D:
    type Item = i32
    fn first(self) -> i32: self.n

fn show(b: dyn Bag[Item = i32]) -> i32:
    b.first()

pub fn main() -> void $ Console:
    println(show(D { n: 8 }))
";
    assert_eq!(output_of("assoc", main), "8\n");
}
