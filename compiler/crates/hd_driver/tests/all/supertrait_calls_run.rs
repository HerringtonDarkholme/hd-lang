//! Supertrait methods through a trait value (trait.dyn.value-methods,
//! trait.dyn.supertrait-methods, trait.dyn.widen; codegen.md §13.16): a
//! vtable holds its own slots and one field per direct supertrait, so a
//! supertrait call on a `dyn` value reads the parent vtable, then the
//! slot, and widening reads the parent field. Programs build through the
//! driver and run on V8 (`host/run.mjs`), as the conformance runner does.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str) -> Output {
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
    build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

/// The standard output of a program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let out = built(main);
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("supertrait-call-{name}.wasm"));
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

#[test]
fn to_string_on_a_dyn_error_reads_the_display_vtable() {
    let main = "\
use std.error.Error

@error(\"disk full on $device\")
data DiskError:
    device: string

data Timeout:
    seconds: i32

impl Display for Timeout:
    fn to_string(self) -> string: \"timed out after ${self.seconds}s\"

impl Error for Timeout

fn message(error: dyn Error) -> string:
    error.to_string()

pub fn main() -> void $ Console:
    println(message(DiskError { device: \"sda\" }))
    println(message(Timeout { seconds: 3 }))
";
    assert_eq!(
        output_of("dyn-error", main),
        "disk full on sda\ntimed out after 3s\n"
    );
}

#[test]
fn a_two_level_chain_reaches_the_grandparent_method() {
    let main = "\
trait Named:
    fn name(self) -> string

trait Greeter < Named:
    fn greeting(self) -> string

trait Host < Greeter:
    fn room(self) -> string

data Concierge:
    who: string

impl Named for Concierge:
    fn name(self) -> string: self.who

impl Greeter for Concierge:
    fn greeting(self) -> string: \"welcome\"

impl Host for Concierge:
    fn room(self) -> string: \"lobby\"

fn introduce(host: dyn Host) -> string:
    \"${host.greeting()}, I am ${host.name()} in the ${host.room()}\"

pub fn main() -> void $ Console:
    println(introduce(Concierge { who: \"Ada\" }))
";
    assert_eq!(
        output_of("two-level", main),
        "welcome, I am Ada in the lobby\n"
    );
}

#[test]
fn a_child_value_widens_where_a_supertrait_value_is_expected() {
    let main = "\
trait Named:
    fn name(self) -> string

trait Greeter < Named:
    fn greeting(self) -> string

trait Host < Greeter:
    fn room(self) -> string

data Concierge:
    who: string

impl Named for Concierge:
    fn name(self) -> string: self.who

impl Greeter for Concierge:
    fn greeting(self) -> string: \"welcome\"

impl Host for Concierge:
    fn room(self) -> string: \"lobby\"

fn name_of(named: dyn Named) -> string:
    named.name()

fn greet(greeter: dyn Greeter) -> string:
    \"${greeter.greeting()} from ${name_of(greeter)}\"

pub fn main() -> void $ Console:
    let host: dyn Host = Concierge { who: \"Ada\" }
    println(greet(host))
    println(name_of(host))
";
    assert_eq!(output_of("widen", main), "welcome from Ada\nAda\n");
}

#[test]
fn a_blanket_parent_impl_answers_through_the_child_value() {
    let main = "\
trait Score:
    fn score(self) -> i32

trait Measure:
    fn measure(self) -> i32

trait Scored < Measure:
    fn marker(self) -> i32

data Point:
    value: i32

impl Score for Point:
    fn score(self) -> i32: self.value

impl[Item < Score] Measure for List[Item]:
    fn measure(self) -> i32: self[0].score()

impl[Item < Score] Scored for List[Item]:
    fn marker(self) -> i32: 0

pub fn main() -> void $ Console:
    let points: List[Point] = [Point { value: 42 }]
    let dynamic: dyn Scored = points
    println(dynamic.measure())
";
    assert_eq!(output_of("blanket", main), "42\n");
}

#[test]
fn a_generic_supertrait_takes_the_child_arguments() {
    let main = "\
trait Source[Output]:
    fn source(self) -> Output

trait Wrapped[Item] < Source[List[Item]]:
    fn count(self) -> i32

data Crate[T]:
    item: T

impl[T] Source[List[T]] for Crate[T]:
    fn source(self) -> List[T]: [self.item, self.item]

impl[T] Wrapped[T] for Crate[T]:
    fn count(self) -> i32: 2

fn first(source: dyn Source[List[string]]) -> string:
    source.source()[0]

pub fn main() -> void $ Console:
    let wrapped: dyn Wrapped[string] = Crate::[string] { item: \"apple\" }
    println(wrapped.source().len())
    println(first(wrapped))
";
    assert_eq!(output_of("generic", main), "2\napple\n");
}

#[test]
fn a_diamond_reaches_the_shared_parent_through_both_paths() {
    let main = "\
trait Base:
    fn base(self) -> i32

trait Left < Base:
    fn left(self) -> i32

trait Right < Base:
    fn right(self) -> i32

trait Both < Left & Right:
    fn both(self) -> i32

data Num:
    n: i32

impl Base for Num:
    fn base(self) -> i32: self.n

impl Left for Num:
    fn left(self) -> i32: self.n + 1

impl Right for Num:
    fn right(self) -> i32: self.n + 2

impl Both for Num:
    fn both(self) -> i32: self.n + 3

fn via_left(left: dyn Left) -> i32:
    left.base() + left.left()

fn via_right(right: dyn Right) -> i32:
    right.base() + right.right()

pub fn main() -> void $ Console:
    let both: dyn Both = Num { n: 10 }
    println(both.base() + both.both())
    println(via_left(both))
    println(via_right(both))
";
    assert_eq!(output_of("diamond", main), "23\n21\n22\n");
}
