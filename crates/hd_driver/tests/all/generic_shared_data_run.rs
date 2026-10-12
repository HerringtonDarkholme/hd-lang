//! Shared constructor data of a generic enum belongs to each variant of the
//! declaration (`data.shared.per-variant`), and runs once during module
//! initialization (`data.shared.module-init`, `data.shared.default.eval-once`)
//! however many instantiations and values exist.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str) -> Output {
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
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("generic-shared-{name}.wasm"),
    );
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    let stderr = String::from_utf8_lossy(&ran.stderr).into_owned();
    assert!(ran.status.success(), "{name}: {stderr}");
    String::from_utf8(ran.stdout).expect("UTF-8")
}

#[test]
fn the_conformance_case_computes_once_per_variant() {
    let main = "\
let calls: i32 = 0

fn bump() -> i32:
    calls = calls + 1
    calls

enum Box[T](count: i32, seen: i32 = bump(), items: List[i32] = [1]):
    Full(value: T) -> Box(1)
    Empty -> Box(0)

fn total() -> i32:
    let first: Box[string] = Box.Full(\"a\")
    let second: Box[string] = Box.Full(\"b\")
    let empty: Box[string] = Box.Empty
    shared := if first.items is second.items: +10 else: 0
    shared + first.seen + second.seen + empty.seen + calls

pub fn main() -> void $ Console:
    println(total())
";
    assert_eq!(output_of("fixture", main), "16\n");
}

#[test]
fn two_instantiations_share_one_evaluation_per_variant() {
    let main = "\
let calls: i32 = 0

fn bump() -> i32:
    calls = calls + 1
    calls

enum Box[T](seen: i32 = bump(), items: List[i32] = []):
    Full(value: T) -> Box()
    Empty -> Box()

pub fn main() -> void $ Console:
    let text: Box[string] = Box.Full(\"a\")
    let number: Box[i32] = Box.Full(+5)
    let empty: Box[i32] = Box.Empty
    println(calls)
    println(text.seen)
    println(number.seen)
    println(empty.seen)
    println(text.items is number.items)
    println(number.items is empty.items)
";
    assert_eq!(output_of("instances", main), "2\n1\n1\n2\ntrue\nfalse\n");
}

#[test]
fn defaults_run_during_module_initialization_not_per_value() {
    let main = "\
let calls: i32 = 0

fn bump() -> i32:
    calls = calls + 1
    calls

enum Cell[T](tag: i32 = bump()):
    Some(value: T) -> Cell()

pub fn main() -> void $ Console:
    println(calls)
    let sum = +0
    for i in 0..5:
        let cell: Cell[usize] = Cell.Some(i)
        sum = sum + cell.tag
    println(calls)
    println(sum)
";
    assert_eq!(output_of("init", main), "1\n1\n5\n");
}

#[test]
fn a_shared_list_keeps_one_identity_across_values() {
    let main = "\
enum Tagged[T](names: List[string] = [\"x\", \"y\"]):
    One(value: T) -> Tagged()
    Two(value: T) -> Tagged()

pub fn main() -> void $ Console:
    let a: Tagged[i32] = Tagged.One(+1)
    let b: Tagged[i32] = Tagged.One(+2)
    let c: Tagged[string] = Tagged.One(\"s\")
    let d: Tagged[i32] = Tagged.Two(+3)
    println(a.names is b.names)
    println(a.names is c.names)
    println(a.names is d.names)
    println(a.names.len())
";
    assert_eq!(output_of("identity", main), "true\ntrue\nfalse\n2\n");
}
