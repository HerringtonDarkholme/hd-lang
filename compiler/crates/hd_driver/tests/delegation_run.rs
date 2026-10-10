//! Delegating implementations, `impl Tr for C by E` (spec 09
//! "Delegation", trait.by.*; trait-solver.md §3.10): associated types take
//! the part's bindings, and each method with a receiver that the body does
//! not write forwards to the part's implementation as `Tr::m(self.E,
//! arguments...)`. The checker half checks the programs with
//! `Goal::Analyze`; the runtime half builds them through the driver and
//! runs them on V8 (`host/run.mjs`), as the conformance runner does.

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
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("delegation-{name}.wasm"));
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

/// A part with a required method, a default method and a `mut` method,
/// and a supplier whose item type a delegating implementation takes.
const PARTS: &str = "\
trait Describe:
    fn describe(self) -> string
    fn headline(self) -> string:
        \"* \" + self.describe()

trait Counter:
    fn bump(mut self, by: i32) -> i32
    fn wrap[T](self, value: T) -> List[T]:
        [value]

trait Supplier:
    type Item
    fn get(self) -> Self::Item

data Logger:
    name: string
    hits: i32

impl Describe for Logger:
    fn describe(self) -> string: self.name

impl Counter for Logger:
    fn bump(mut self, by: i32) -> i32:
        self.hits = self.hits + by
        self.hits

impl Supplier for Logger:
    type Item = i32
    fn get(self) -> i32: self.hits

data Service:
    Logger
    port: i32

impl Describe for Service by Logger:
    fn describe(self) -> string: \"service\"

impl Counter for Service by Logger

impl Supplier for Service by Logger
";

#[test]
fn the_checker_takes_the_parts_associated_types() {
    let main = format!(
        "{PARTS}
fn first[S < Supplier](s: S) -> S::Item:
    s.get()

pub fn main() -> void:
    service := Service {{ Logger: ...Logger {{ name: \"api\", hits: 1 }}, port: 80 }}
    _ := service.get() + first(service)
    let erased: dyn Supplier[Item = i32] = service
    _ := erased.get() + 1
"
    );
    assert_eq!(codes_of(&main), Vec::<Code>::new());
    let wrong = format!(
        "{PARTS}
fn text(service: Service) -> string:
    service.get()
"
    );
    assert_eq!(codes_of(&wrong), vec![Code::TypeMismatch]);
}

#[test]
fn the_checker_rejects_what_a_delegating_implementation_cannot_take() {
    let base = "\
trait Named:
    type Label
    fn name(self) -> string
    fn kind() -> string

data Logger:
    label: string

impl Named for Logger:
    type Label = string
    fn name(self) -> string: self.label
    fn kind() -> string: \"logger\"

data Service:
    Logger
";
    assert_eq!(
        codes_of(&format!("{base}\nimpl Named for Service by Logger\n")),
        vec![Code::MissingTraitMethod]
    );
    assert_eq!(
        codes_of(&format!(
            "{base}\nimpl Named for Service by Logger:\n    type Label = string\n    fn kind() -> string: \"service\"\n"
        )),
        vec![Code::InvalidDelegation]
    );
    assert_eq!(
        codes_of(&format!("{base}\nimpl Service by Logger\n")),
        vec![Code::InvalidDelegation]
    );
}

#[test]
fn generated_methods_forward_to_the_parts_implementation() {
    let main = format!(
        "{PARTS}
fn show(value: dyn Describe) -> string:
    value.headline()

pub fn main() -> void $ Console:
    service := Service {{ Logger: ...Logger {{ name: \"api\", hits: 0 }}, port: 80 }}
    println(service.describe())
    println(service.headline())
    println(show(service))
    println(service.wrap(\"x\").len())
"
    );
    // The part's default calls the part's `describe`: no overriding.
    assert_eq!(output_of("forwards", &main), "service\n* api\n* api\n1\n");
}

#[test]
fn a_mut_method_forwards_through_the_part_in_place() {
    let main = format!(
        "{PARTS}
fn twice[C < mut Counter](counter: C) -> i32:
    counter.bump(1)
    counter.bump(1)

pub fn main() -> void $ Console:
    let service: mut Service = Service {{ Logger: ...Logger {{ name: \"api\", hits: 0 }}, port: 80 }}
    println(service.bump(5))
    println(twice(service))
    println(service.Logger.hits)
    println(service.get())
"
    );
    assert_eq!(output_of("mut", &main), "5\n7\n7\n7\n");
}

#[test]
fn a_delegated_associated_type_normalizes_at_every_use() {
    let main = format!(
        "{PARTS}
fn first[S < Supplier](s: S) -> S::Item:
    s.get()

fn read(source: dyn Supplier[Item = i32]) -> i32:
    source.get() * 10

pub fn main() -> void $ Console:
    service := Service {{ Logger: ...Logger {{ name: \"api\", hits: 4 }}, port: 80 }}
    println(service.get() + 1)
    println(first(service))
    println(read(service))
"
    );
    assert_eq!(output_of("assoc", &main), "5\n4\n40\n");
}

#[test]
fn a_generic_part_delegates_at_the_targets_arguments() {
    let main = "\
trait Supplier:
    type Item
    fn get(self) -> Self::Item
    fn twice(self) -> List[Self::Item]:
        [self.get(), self.get()]

data Box[T]:
    value: T

impl[T] Supplier for Box[T]:
    type Item = T
    fn get(self) -> T: self.value

data Shelf[T]:
    Box[T]
    label: string

impl[T] Supplier for Shelf[T] by Box

fn first[S < Supplier](s: S) -> S::Item:
    s.get()

pub fn main() -> void $ Console:
    shelf := Shelf { Box: ...Box { value: 7 }, label: \"a\" }
    println(shelf.get() + 1)
    println(first(shelf))
    println(shelf.twice().len())
    let s: dyn Supplier[Item = string] = Shelf { Box: ...Box { value: \"hi\" }, label: \"b\" }
    println(s.get())
";
    assert_eq!(output_of("generic", main), "8\n7\n2\nhi\n");
}

#[test]
fn a_suspending_method_and_a_row_forward_as_declared() {
    let main = "\
trait Source:
    fn fetch!(self, n: i32) -> i32
    fn show(self) -> void $ Console

data Disk:
    base: i32

impl Source for Disk:
    fn fetch!(self, n: i32) -> i32: self.base + n
    fn show(self) -> void $ Console: println(self.base)

data Cache:
    Disk

impl Source for Cache by Disk

pub fn main!() -> void $ Console:
    c := Cache { Disk: ...Disk { base: 40 } }
    println(c.fetch!(2))
    c.show()
";
    assert_eq!(output_of("suspend", main), "42\n40\n");
}
