//! Variant patterns that mix positional and named sub-patterns
//! (spec/lang/08-data-and-enums.md, data.enum.payload.order and
//! data.match.payload.*): positional ones fill fields by position, named
//! ones by field name, in match arms, `let` with `else`, and nested
//! patterns. Programs build through the driver and run on V8
//! (`host/run.mjs`), as the conformance runner does.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

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
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("mixed-variant-patterns-{name}.wasm"),
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

#[test]
fn a_match_arm_mixes_positional_and_named_sub_patterns() {
    let main = "\
enum Shape:
    Rect(width: i32, height: i32)
    Point(x: i32, y: i32, z: i32)
    Empty

fn describe(shape: Shape) -> i32:
    match shape:
        .Rect(w, height = 0) => w
        .Rect(w, height = h) => w * h
        .Point(x, y = 0, z = z) => x + z
        .Point(x, y, z = 9) => x + y + 1000
        .Point(_, _, _) => -1
        .Empty => 0

pub fn main() -> void $ Console:
    println(describe(.Rect(3, 0)))
    println(describe(.Rect(3, 4)))
    println(describe(.Point(1, 0, 5)))
    println(describe(.Point(1, 2, 9)))
    println(describe(.Point(1, 2, 3)))
    println(describe(.Empty))
";
    assert_eq!(output_of("match", main), "3\n12\n6\n1003\n-1\n0\n");
}

#[test]
fn a_named_sub_pattern_may_leave_a_field_out_of_order() {
    let main = "\
enum Shape:
    Box(width: i32, height: i32, depth: i32)

fn volume(shape: Shape) -> i32:
    match shape:
        .Box(w, depth = d, height = h) => w * h * d

pub fn main() -> void $ Console:
    println(volume(.Box(2, 3, 4)))
";
    assert_eq!(output_of("order", main), "24\n");
}

#[test]
fn let_else_and_nested_patterns_mix_the_forms() {
    let main = "\
enum Slot:
    Held(item: i32, qty: i32)
    Free

enum Bin:
    Full(slot: Slot, label: i32)
    Open

fn take(slot: Slot) -> i32:
    let .Held(item, qty = amount) = slot else: return -1
    item + amount

fn inner(bin: Bin) -> i32:
    match bin:
        .Full(.Held(item, qty = 0), label = l) => item + l
        .Full(.Held(item, qty = q), l) => item + q + l + 100
        .Full(.Free, label = l) => l
        .Open => 0

pub fn main() -> void $ Console:
    println(take(.Held(5, 6)))
    println(take(.Free))
    println(inner(.Full(.Held(1, 0), 7)))
    println(inner(.Full(.Held(1, 2), 3)))
    println(inner(.Full(.Free, 8)))
    println(inner(.Open))
";
    assert_eq!(output_of("nested", main), "11\n-1\n8\n106\n8\n0\n");
}

#[test]
fn a_constructor_mixes_positional_and_named_arguments() {
    let main = "\
enum Shape:
    Rect(width: i32, height: i32)

pub fn main() -> void $ Console:
    match Shape.Rect(3, height = 4):
        .Rect(w, height = h) => println(w * h)
";
    assert_eq!(output_of("construct", main), "12\n");
}
