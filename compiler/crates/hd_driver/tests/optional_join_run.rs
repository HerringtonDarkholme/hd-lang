//! Optional injection at a least-common-type site (spec 04
//! `types.lct.optional`, `types.lct.optional.one-layer`): a plain value and
//! an optional of its type join to the optional, whichever branch comes
//! first, so the branch checked first takes `.Some` too. Programs build
//! through the driver and run on V8 (`host/run.mjs`), as the conformance
//! runner does.

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
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("optional-join-{name}.wasm"));
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

/// `if` and `match` with a plain branch before an optional one, and the
/// other way round.
#[test]
fn if_and_match_join_a_value_and_an_optional() {
    let main = "\
fn value_first(flag: bool, count: i32, found: i32?):
    if flag: count else: found

fn optional_first(flag: bool, count: i32, found: i32?):
    if flag: found else: count

fn match_value_first(key: i32, count: i32, found: i32?):
    match key:
        0 => count
        _ => found

fn match_optional_first(key: i32, count: i32, found: i32?):
    match key:
        0 => found
        _ => count

fn show(v: i32?) -> string:
    match v:
        .Some(n) => \"some ${n}\"
        .None => \"none\"

pub fn main() -> void $ Console:
    let none: i32? = .None
    println(show(value_first(true, 3, none)))
    println(show(value_first(false, 3, none)))
    println(show(value_first(false, 3, .Some(4))))
    println(show(optional_first(true, 3, none)))
    println(show(optional_first(false, 3, none)))
    println(show(match_value_first(0, 5, none)))
    println(show(match_value_first(1, 5, none)))
    println(show(match_optional_first(0, 5, none)))
    println(show(match_optional_first(1, 5, none)))
";
    assert_eq!(
        output_of("if-match", main),
        "some 3\nnone\nsome 4\nnone\nsome 3\nsome 5\nnone\nnone\nsome 5\n"
    );
}

/// Three branches, the optional last or in the middle; `else if` chains
/// and nested sites; a value `if` bound to a local.
#[test]
fn three_branches_and_nested_sites_join_to_the_optional() {
    let main = "\
fn show(v: i32?) -> string:
    match v:
        .Some(n) => \"some ${n}\"
        .None => \"none\"

fn last(key: i32, found: i32?):
    if key == 0: 10 else if key == 1: 11 else: found

fn middle(key: i32, found: i32?):
    if key == 0: 10 else if key == 1: found else: 12

fn nested(key: i32, found: i32?):
    if key > 0:
        match key:
            1 => 21
            _ => found
    else:
        22

pub fn main() -> void $ Console:
    let none: i32? = .None
    println(show(last(0, none)))
    println(show(last(1, none)))
    println(show(last(2, none)))
    println(show(middle(0, none)))
    println(show(middle(1, none)))
    println(show(middle(2, none)))
    println(show(nested(1, none)))
    println(show(nested(2, .Some(7))))
    println(show(nested(2, none)))
    println(show(nested(0, none)))
    let local = if none == .None: 1 else: none
    println(show(local))
";
    assert_eq!(
        output_of("three", main),
        "some 10\nsome 11\nnone\nsome 10\nnone\nsome 12\nsome 21\nsome 7\nnone\nsome 22\nsome 1\n"
    );
}

/// A list literal of a value and an optional joins to a list of optionals.
#[test]
fn list_literals_join_a_value_and_an_optional() {
    let main = "\
pub fn main() -> void $ Console:
    let none: i32? = .None
    values := [3, none, 4]
    let listed: List[i32?] = values
    println(\"${listed.len()}\")
    if listed[0] == .Some(3) && listed[1] == .None && listed[2] == .Some(4):
        println(\"ok\")
";
    assert_eq!(output_of("list", main), "3\nok\n");
}

/// Injection adds one layer: an `i32` and an `i32??` have no common type.
#[test]
fn two_layers_still_have_no_common_type() {
    let main = "\
fn nested(count: i32, found: i32??, flag: bool) -> void:
    value := if flag: count else: found

pub fn main() -> void $ Console:
    println(\"x\")
";
    let out = built(main);
    let shown = out.render();
    assert!(
        shown.contains("type-mismatch") || shown.contains("no-common-type"),
        "{shown}"
    );
}
