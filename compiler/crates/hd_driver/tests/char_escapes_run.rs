//! Character and string escapes run (#185a, lex.escape.simple,
//! lex.escape.unicode): every escape the lexical spec lists decodes to its
//! own character, including the quote that closes a character literal.
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
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("char-escapes-{name}.wasm"));
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
fn an_escaped_single_quote_is_a_character() {
    let main = "\
pub fn main() -> void $ Console:
    println('\\'')
    println('\"')
    println('\\\\')
    println('\\'' == '\\u{27}')
";
    assert_eq!(output_of("quote", main), "'\n\"\n\\\ntrue\n");
}

#[test]
fn every_character_escape_has_its_code() {
    let main = "\
pub fn main() -> void $ Console:
    println('\\n' == '\\u{a}')
    println('\\r' == '\\u{d}')
    println('\\t' == '\\u{9}')
    println('\\0' == '\\u{0}')
    println('\\$' == '$')
    println('\\\"' == '\"')
    println('\\u{1F600}' == '\\u{1f600}')
";
    assert_eq!(output_of("codes", main), "true\n".repeat(7));
}

#[test]
fn every_string_escape_decodes_in_place() {
    let main = "\
pub fn main() -> void $ Console:
    println(\"a\\'b\\\"c\\\\d\\$e\")
    println(\"x\\u{41}y\")
    println(\"\\\"\")
";
    assert_eq!(output_of("string", main), "a'b\"c\\d$e\nxAy\n\"\n");
}
