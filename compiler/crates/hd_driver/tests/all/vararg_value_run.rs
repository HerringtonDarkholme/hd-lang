//! Calls through a function value whose inputs end in a rest element
//! (spec/lang/07-functions.md, fn.type.vararg-rest, fn.type.rest-call):
//! the arguments are collected as the direct call collects them, and the
//! vararg stays part of the value's type. Programs build through the
//! driver and run on V8 (`host/run.mjs`), as the conformance runner does.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_diag::Code;
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
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("vararg-value-{name}.wasm"));
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

/// The diagnostic codes of a program that must not build.
fn codes_of(main: &str) -> Vec<Code> {
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
    let out = build(&host, "app", &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

#[test]
fn a_value_call_collects_a_list_vararg() {
    let main = "\
fn label(tag: i32, values...: List[i32]) -> i32: tag * 100 + i32(values.len()) * 10

fn apply(callback: fn(i32, List[i32]...) -> i32) -> i32:
    callback(7, 1, 2, 3)

pub fn main() -> void $ Console:
    f := label
    println(f(1))
    println(f(2, 5, 6))
    xs := [+4, 5, 6, 7]
    println(f(3, xs...))
    println(apply(label))
";
    assert_eq!(output_of("collect", main), "100\n220\n340\n730\n");
}

#[test]
fn a_spelled_function_type_keeps_the_rest_element() {
    let main = "\
use std.function.Fn

fn count(label: string, values...: List[i32]) -> usize: values.len()

fn spread(callback: Fn[(string, List[i32]...), usize, $()]) -> usize:
    callback(\"n\", 1, 2, 3)

pub fn main() -> void $ Console:
    println(spread(count))
";
    assert_eq!(output_of("spelled", main), "3\n");
}

#[test]
fn a_vararg_value_differs_from_a_list_taking_one() {
    let main = "\
fn sum(values...: List[i32]) -> i32: values[0]
fn apply_pair(callback: fn(i32, i32) -> i32) -> i32: callback(1, 2)
fn apply_list(callback: fn(List[i32]) -> i32) -> i32: callback([1, 2])

fn first() -> i32: apply_pair(sum)
fn second() -> i32: apply_list(sum)
";
    assert_eq!(codes_of(main), vec![Code::TypeMismatch, Code::TypeMismatch]);
    let wrong = "\
fn sum(values...: List[i32]) -> i32: values[0]

fn run() -> i32:
    f := sum
    f(1, \"a\")
";
    assert_eq!(codes_of(wrong), vec![Code::TypeMismatch]);
}
