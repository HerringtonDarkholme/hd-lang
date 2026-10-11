//! Float literal patterns run (#185a, flow.match.literal): a negative
//! float literal pattern matches the negative value, as a positive one
//! matches the positive value. Programs build through the driver and run
//! on V8 (`host/run.mjs`).

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The standard output of a `main` program that must build and succeed.
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
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("float-patterns-{name}.wasm"));
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
fn negative_and_positive_float_patterns_match_their_values() {
    let main = "\
fn pick(value: f64) -> i32:
    match value:
        -1.5 => 3
        1.5 => 4
        0.25 => 5
        -0.25 => 6
        _ => 0

pub fn main() -> void $ Console:
    println(pick(-1.5))
    println(pick(1.5))
    println(pick(0.25))
    println(pick(-0.25))
    println(pick(2.5))
    println(pick(-2.5))
";
    assert_eq!(output_of("f64", main), "3\n4\n5\n6\n0\n0\n");
}

#[test]
fn a_negative_float_pattern_on_f32_and_negative_int_patterns_match() {
    let main = "\
fn small(value: f32) -> i32:
    match value:
        -2.5 => 1
        _ => 0

fn whole(value: i32) -> i32:
    match value:
        -1 => 7
        1 => 8
        _ => 0

pub fn main() -> void $ Console:
    println(small(-2.5))
    println(small(2.5))
    println(whole(-1))
    println(whole(1))
    println(whole(+2))
";
    assert_eq!(output_of("mixed", main), "1\n0\n7\n8\n0\n");
}
