//! Radix literal values (#263e, lex.int.forms, lex.int.hex-case,
//! lex.sep.prefix): both cases of each prefix give the same value.
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
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("radix-literals-{name}.wasm"),
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
fn radix_prefixes_in_both_cases_have_the_same_value() {
    let main = "\
pub fn main() -> void $ Console:
    println(0O755)
    println(0o755 == 0O755)
    println(0B1010 + 0b1010)
    println(0XfF + 0xFf + 0xff)
    println(0b_1010_0110)
    println(0x_FF_FF_00)
    println(0O_7_5_5)
    println(0X_a)
";
    assert_eq!(
        output_of("values", main),
        "493\ntrue\n20\n765\n166\n16776960\n493\n10\n"
    );
}

#[test]
fn a_radix_literal_works_as_a_match_pattern() {
    let main = "\
pub fn main() -> void $ Console:
    n := 0O17
    match n:
        0B1111 => println(\"fifteen\")
        _ => println(\"other\")
";
    assert_eq!(output_of("pattern", main), "fifteen\n");
}
