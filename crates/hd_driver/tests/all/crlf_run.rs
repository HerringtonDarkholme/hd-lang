//! CRLF source runs (lex.line.crlf, lex.multiline.line-feed): a program
//! written with CRLF line endings builds, and a line ending inside a
//! multiline string, plain or prefixed, contributes one line feed.
//! Programs build through the driver and run on V8 (`host/run.mjs`).

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The standard output of a `main` program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
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
    let out = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(env!("CARGO_TARGET_TMPDIR"), &format!("crlf-{name}.wasm"));
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

/// `main` with LF endings rewritten as CRLF.
fn crlf(main: &str) -> String {
    main.replace('\n', "\r\n")
}

#[test]
fn a_crlf_program_runs() {
    let main = crlf(
        "\
pub fn main() -> void $ Console:
    # a comment
    x := 1 + 2
    println(\"${x}\")
",
    );
    assert_eq!(output_of("plain", &main), "3\n");
}

#[test]
fn a_multiline_string_holds_line_feeds_only() {
    let main = crlf(
        "\
pub fn main() -> void $ Console:
    text := \"\"\"a
b
c\"\"\"
    println(text == \"a\\nb\\nc\")
    println(\"${text.len()}\")
",
    );
    assert_eq!(output_of("multiline", &main), "true\n5\n");
}
