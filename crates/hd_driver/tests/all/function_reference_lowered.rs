//! Function references to targets the compiler lowers at each call
//! (`crates/hd_mono/DESIGN.md` §13.11): an intrinsic, a built-in impl's method, and a
//! trait method at a trait-value `Self`. The adapter makes the call a
//! direct call at the same types makes, so each reference gives the
//! answer the call gives. Built and run on V8 (`host/run.mjs`).

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

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

/// Builds the program twice and checks the second build re-emits
/// nothing, then runs it on V8 and returns its standard output.
fn run(name: &str, main: &str) -> String {
    let store = MemoryStore::default();
    let out = build_program(&store, main);
    let again = build_program(&store, main);
    assert_eq!(again.counters.emitted, 0, "{name}: re-emitted");
    assert_eq!(again.wasm, out.wasm, "{name}: rebuilt differently");
    let wasm = out.wasm.as_ref().expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("fnref-lowered-{name}.wasm"),
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

/// A reference to an intrinsic method (`i32`'s `Eq.eq`, `Ord.cmp`, `Add.add`,
/// `Neg.neg`) and to a body-less std function (`TypeId::of`): called,
/// stored, and passed to `map` and to a function parameter.
#[test]
fn references_to_intrinsics_call_through_their_adapters() {
    let main = "use std.inspect.TypeId
use std.ops.{Add, Neg}

fn apply(f: fn(i32, i32) -> i32, a: i32, b: i32) -> i32:
    f(a, b)

pub fn main() -> void $ Console:
    eq := i32::eq
    println(eq(1, 1))
    println(eq(1, 2))
    cmp := i32::cmp
    println(cmp(1, 2) == .Less)
    println(apply(i32::add, 3, 4))
    let xs: List[i32] = [3, 1, 2]
    println(xs.map(i32::neg)[0])
    let flag: fn(bool, bool) -> bool = bool::eq
    println(flag(true, false))
    same := f64::eq
    println(same(1.5, 1.5))
    id := TypeId::of::[i32]
    println(id() == TypeId::of::[i32]())
";
    assert_eq!(
        run("intrinsics", main),
        "true\nfalse\ntrue\n7\n-3\nfalse\ntrue\ntrue\n"
    );
}

/// A reference to a built-in impl's method (`Display` of a primitive):
/// stored and called, and passed to `map`.
#[test]
fn references_to_builtin_impls_call_through_their_adapters() {
    let main = "pub fn main() -> void $ Console:
    show := i32::to_string
    println(show(-4))
    let xs: List[i32] = [1, 2, 3]
    ys := xs.map(i32::to_string)
    println(ys.len())
    println(ys[2])
    half := f64::to_string
    println(half(1.5))
    let flag: fn(bool) -> string = Display::to_string
    println(flag(true))
    let text: fn(string) -> string = string::to_string
    println(text(\"x\"))
";
    assert_eq!(run("builtins", main), "-4\n3\n3\n1.5\ntrue\nx\n");
}

/// A member reference at a `dyn` receiver calls the method in the
/// receiver's vtable (`trait.dyn.bound.dispatch`): a plain method, a `mut
/// self` method, a default method, a method of a supertrait, a generic
/// method through its erased body, and a bound reference.
#[test]
fn references_at_a_dyn_receiver_dispatch_through_the_vtable() {
    let main = "trait Named:
    fn label(&self) -> string
    fn pick[V](self, value: V) -> V
    fn rename(&mut self, to: i32) -> void
    fn greet(self, other: string) -> string:
        \"hi \" + other + \" from \" + self.label()

trait Tagged < Named:
    fn tag(self) -> string

struct Item:
    n: i32

impl Named for Item:
    fn label(&self) -> string:
        \"item\" + self.n.to_string()

    fn pick[V](self, value: V) -> V:
        value

    fn rename(&mut self, to: i32) -> void:
        self.n = to

impl Tagged for Item:
    fn tag(self) -> string:
        \"t\"

fn labels[T < Named](items: List[T]) -> List[string]:
    items.map(fn(item: T) -> string: item.label())

pub fn main() -> void $ Console:
    let items: List[&mut dyn Named] = [&mut Item { n: 1 }]
    println(labels(items)[0])
    println(items.map(Named::label)[0])
    let greet: fn(&dyn Named, string) -> string = Named::greet
    println(greet(&Item { n: 2 }, \"bob\"))
    let tag: fn(&dyn Tagged) -> string = Tagged::tag
    println(tag(&Item { n: 3 }))
    let inherited: fn(&dyn Tagged) -> string = Named::label
    println(inherited(&Item { n: 3 }))
    let rename: fn(&mut dyn Named, i32) -> void = Named::rename
    let mutable: &mut dyn Named = &mut Item { n: 5 }
    rename(mutable, 9)
    println(mutable.label())
    let shared: &dyn Named = &Item { n: 9 }
    let bound = shared::label
    println(bound())
    let pick: fn(&dyn Named, i32) -> i32 = Named::pick
    println(pick(&Item { n: 1 }, 5))
    let word: fn(&dyn Named, string) -> string = Named::pick
    println(word(&Item { n: 1 }, \"w\"))
";
    assert_eq!(
        run("dyn", main),
        "item1\nitem1\nhi bob from item2\nt\nitem3\nitem9\nitem9\n5\nw\n"
    );
}
