//! `facts_of(f)` at run time (annot.facts-of.result, annot.decorator.facts-of):
//! the values a function's decorators attach, in source order, evaluated in
//! the function's own module. Programs build through the driver and run on
//! V8 (`host/run.mjs`), as the conformance runner does.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The standard output of a program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
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
    let out = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("facts-of-{name}.wasm"));
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

/// Values of several types come back by type, and the list keeps the
/// source order of the decorators.
#[test]
fn values_are_found_by_type_in_source_order() {
    let main = "use std.annotation.facts_of

data Route:
    path: string

fn route(path: string) -> Route:
    Route { path: path }

data Hidden: pass

fn hidden() -> Hidden:
    Hidden {}

@route(\"/users\")
@\"lists users\"
@hidden
fn list_users() -> string:
    \"[]\"

pub fn main() -> void $ Console:
    facts := facts_of(list_users)
    println(facts.items.len())
    match facts.find::[Route]():
        .Some(found) => println(found.path)
        .None => println(\"no route\")
    match facts.find::[string]():
        .Some(label) => println(label)
        .None => println(\"no label\")
    println(facts.find::[Hidden]().is_some())
    println(facts.find::[i32]().is_some())
";
    assert_eq!(
        output_of("order", main),
        "3\n/users\nlists users\ntrue\nfalse\n"
    );
}

/// A function with no decorators holds no facts.
#[test]
fn an_undecorated_function_holds_no_facts() {
    let main = "use std.annotation.facts_of

fn plain() -> void:
    pass

pub fn main() -> void $ Console:
    println(facts_of(plain).items.len())
";
    assert_eq!(output_of("none", main), "0\n");
}
