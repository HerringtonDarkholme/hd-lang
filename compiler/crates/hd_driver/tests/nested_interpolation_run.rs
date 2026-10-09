//! Nested string literals inside interpolation run (#185a,
//! lex.interp.expression, lex.interp.braces): the inner literal
//! contributes its value once, and braces and quotes stay text.
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
    let path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("nested-interp-{name}.wasm"));
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
fn a_string_literal_inside_an_interpolation_prints_once() {
    let main = "\
pub fn main() -> void $ Console:
    println(\"${\"x\"}\")
    println(\"a${\"b\"}c${\"d\"}e\")
    println(\"[${\"cd\"}]\")
    println(\"nested ${\"\\\"quoted\\\"\"} text\")
    println(\"${\"in ${\"deep\"} out\"}\")
";
    assert_eq!(
        output_of("once", main),
        "x\nabcde\n[cd]\nnested \"quoted\" text\nin deep out\n"
    );
}

#[test]
fn braces_and_literals_in_interpolations_stay_apart() {
    let main = "\
pub fn main() -> void $ Console:
    println(\"map ${{\"a\": 1, \"b\": 2}.len()} {x}\")
    println(\"${\"}\"}|${\"{\"}\")
    println(\"${\"a\" + \"b\"}${1 + 2}\")
";
    assert_eq!(output_of("braces", main), "map 2 {x}\n}|{\nab3\n");
}
