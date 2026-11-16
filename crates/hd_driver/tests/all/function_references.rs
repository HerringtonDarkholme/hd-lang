//! Function references (spec/lang/07-functions.md, Generic Function
//! Values and Method References), called through their adapters
//! (`crates/hd_mono/DESIGN.md` §13.11), built by the driver and run on V8
//! (`host/run.ts`), and rebuilt warm with nothing re-emitted. Each kind's
//! `ItemRef` type and the errors are stage cases
//! (`hd_check/tests/cases/function-references/`).

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const COUNTER: &str = "struct Counter:
    value: i32

impl Counter:
    fn bump(&mut self, by: i32) -> void:
        self.value = self.value + by

    fn read(self) -> i32:
        self.value

    fn zero() -> Counter:
        Counter { value: 0 }

    fn pick[V](self, value: V) -> V:
        value

trait Named:
    fn label(self) -> string

trait Factory:
    fn create() -> Self

impl Factory for Counter:
    fn create() -> Counter:
        Counter { value: 1 }

struct Store:
    name: string

impl Store:
    fn load!(self, key: i32) -> i32:
        key

fn identity[T](value: T) -> T:
    value

fn apply[A, B](value: A, f: fn(A) -> B) -> B:
    f(value)

fn empty[C = i32]() -> Option[C]:
    .None
";

fn build_program(store: &MemoryStore, main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{}", out.render());
    out
}

/// Runs a built program on V8 and returns its standard output.
fn run(out: &Output, name: &str) -> String {
    let wasm = out.wasm.as_ref().expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("fnref-run-{name}.wasm"),
    );
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.ts"))
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

/// Each kind of unbound reference is called through its adapter: a plain
/// function, a generic function instantiated from the expected type
/// (`fn.type.generic.instantiate-from`), an unbound method taking the
/// receiver first (`fn.ref.unbound.receiver`), an associated function
/// (`fn.ref.associated`), a trait member whose `Self` comes from the
/// expected type (`fn.ref.trait-self`), a suspending method that keeps
/// its suspension (`fn.ref.suspending`), and a function whose row the
/// adapter forwards from the caller's context (`fn.ref.value`). A second
/// build re-emits nothing.
#[test]
fn references_call_through_their_adapters() {
    let main = format!(
        "{COUNTER}
fn double(x: i32) -> i32:
    x * 2

fn shout(x: i32) -> void $ Console:
    println(x + 1)

fn twice(f: fn(i32) -> i32, x: i32) -> i32:
    f(f(x))

fn fetch!(f: fn!(Store, i32) -> i32, store: Store) -> i32:
    f!(store, 5)

pub fn main!() -> void $ Console:
    println(twice(double, 5))
    let echo: fn(string) -> string = identity
    println(echo(\"generic\"))
    bump := Counter::bump
    let mut counter = Counter {{ value: 1 }}
    bump(&mut counter, 4)
    println(counter.value)
    zero := Counter::zero
    println(zero().value)
    let make: fn() -> Counter = Factory::create
    println(make().value)
    println(fetch!(Store::load, Store {{ name: \"cache\" }}))
    let say: fn(i32) -> void $ Console = shout
    say(9)
"
    );
    let store = MemoryStore::default();
    let out = build_program(&store, &main);
    assert_eq!(run(&out, "kinds"), "20\ngeneric\n5\n0\n1\n5\n10\n");
    let again = build_program(&store, &main);
    assert_eq!(again.counters.emitted, 0, "re-emitted");
    assert_eq!(again.wasm, out.wasm);
}
