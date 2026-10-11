//! Map literal duplicate keys run (#185a, expr.map.duplicate.last): a later
//! equal key replaces the earlier value, and the map keeps one entry.
//! Programs build through the driver and run on V8 (`host/run.mjs`).

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
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("map-dups-{name}.wasm"));
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
fn a_repeated_string_key_keeps_the_last_value() {
    let main = "\
pub fn main() -> void $ Console:
    m := {\"a\": 20, \"a\": 40}
    println(m.len())
    println(m[\"a\"])
    many := {\"k\": 1, \"j\": 2, \"k\": 3, \"j\": 4, \"k\": 5}
    println(many.len())
    println(many[\"k\"])
    println(many[\"j\"])
";
    assert_eq!(output_of("string", main), "1\n40\n2\n5\n4\n");
}

#[test]
fn a_repeated_integer_key_keeps_the_last_value() {
    let main = "\
pub fn main() -> void $ Console:
    m := {+1: +20, 2: 2, 1: 40, 1: 41}
    println(m.len())
    println(m[1])
    println(m[2])
";
    assert_eq!(output_of("int", main), "2\n41\n2\n");
}
