//! Associated-type bindings on trait values (spec 09 "Bound Associated
//! Types", trait.dyn.binding.*; codegen.md §13.5 "As built (#193)"): a
//! `dyn Tr[Item = T]` value keeps its bindings, so a projection in a
//! method signature denotes the bound type through the value, and the
//! vtable's slots are laid out at the bound types. The checker half checks
//! the programs with `Goal::Analyze`; the runtime half builds them through
//! the driver and runs them on V8 (`host/run.mjs`), as the conformance
//! runner does.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str, goal: &Goal) -> Output {
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

/// The standard output of a program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let out = built(
        main,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("dyn-bindings-{name}.wasm"));
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

/// A lookup table with two associated types, and two implementations
/// that bind them differently.
const LOOKUP: &str = "\
trait Lookup:
    type Key
    type Value
    fn find(self, key: Self::Key) -> Self::Value?
    fn keys(self) -> List[Self::Key]

data PhoneBook:
    names: List[string]
    numbers: List[i32]

impl Lookup for PhoneBook:
    type Key = string
    type Value = i32
    fn find(self, key: string) -> i32?:
        for index in 0..self.names.len():
            if self.names[index] == key:
                return .Some(self.numbers[index])
        .None
    fn keys(self) -> List[string]: self.names

data Squares:
    size: i32

impl Lookup for Squares:
    type Key = i32
    type Value = string
    fn find(self, key: i32) -> string?:
        if key < self.size:
            return .Some(\"${key * key}\")
        .None
    fn keys(self) -> List[i32]:
        let out: mut List[i32] = []
        for k in 0..self.size:
            out.push(k)
        out
";

#[test]
fn the_checker_reads_a_projection_through_a_trait_value_as_its_bound_type() {
    let main = format!(
        "{LOOKUP}
fn number(book: dyn Lookup[Key = string, Value = i32], name: string) -> i32:
    book.find(name).unwrap_or(0)

pub fn main() -> void:
    _ := number(PhoneBook {{ names: [\"ada\"], numbers: [1] }}, \"ada\")
"
    );
    assert_eq!(codes_of(&main), Vec::<Code>::new());
    let wrong = format!(
        "{LOOKUP}
fn number(book: dyn Lookup[Key = string, Value = i32], name: string) -> string:
    book.find(name).unwrap_or(0)
"
    );
    assert_eq!(codes_of(&wrong), vec![Code::TypeMismatch]);
}

#[test]
fn a_slot_returning_a_projection_runs_at_the_bound_type() {
    let main = format!(
        "{LOOKUP}
fn number(book: dyn Lookup[Key = string, Value = i32], name: string) -> i32:
    book.find(name).unwrap_or(-1)

fn square(table: dyn Lookup[Value = string, Key = i32], key: i32) -> string:
    table.find(key).unwrap_or(\"none\")

pub fn main() -> void $ Console:
    book := PhoneBook {{ names: [\"ada\", \"alan\"], numbers: [100, 200] }}
    println(number(book, \"alan\"))
    println(number(book, \"eve\"))
    println(square(Squares {{ size: 4 }}, 3))
    println(square(Squares {{ size: 4 }}, 9))
"
    );
    assert_eq!(output_of("slot", &main), "200\n-1\n9\nnone\n");
}

#[test]
fn trait_values_that_bind_differently_get_their_own_vtables() {
    let main = format!(
        "{LOOKUP}
pub fn main() -> void $ Console:
    let book: dyn Lookup[Key = string, Value = i32] = PhoneBook {{ names: [\"ada\"], numbers: [7] }}
    let table: dyn Lookup[Key = i32, Value = string] = Squares {{ size: 3 }}
    println(book.keys().len())
    println(table.keys().len())
    for k in table.keys():
        println(table.find(k).unwrap_or(\"\"))
"
    );
    assert_eq!(output_of("two-vtables", &main), "1\n3\n0\n1\n4\n");
}

#[test]
fn a_generic_instantiated_at_a_bound_trait_value_normalizes_its_projection() {
    let main = format!(
        "{LOOKUP}
fn first_key[L < Lookup](table: L) -> L::Key?:
    table.keys().first()

pub fn main() -> void $ Console:
    let table: dyn Lookup[Key = i32, Value = string] = Squares {{ size: 3 }}
    println(first_key(table).unwrap_or(-1))
"
    );
    assert_eq!(output_of("generic", &main), "0\n");
}

#[test]
fn a_mut_trait_value_with_a_binding_dispatches_its_mut_slot() {
    let main = "\
trait Feed:
    type Item
    fn next(mut self) -> Self::Item?

data Ticker:
    prices: List[i32]
    at: usize

impl Feed for Ticker:
    type Item = i32
    fn next(mut self) -> i32?:
        if self.at >= self.prices.len():
            return .None
        price := self.prices[self.at]
        self.at = self.at + 1
        .Some(price)

fn total(feed: mut dyn Feed[Item = i32]) -> i32:
    let sum = +0
    while true:
        match feed.next():
            .Some(p) => sum = sum + p
            .None => break
    sum

pub fn main() -> void $ Console:
    let ticker: mut Ticker = Ticker { prices: [3, 9, 4], at: 0 }
    println(total(ticker))
    println(ticker.at)
";
    assert_eq!(output_of("mut-slot", main), "16\n3\n");
}
