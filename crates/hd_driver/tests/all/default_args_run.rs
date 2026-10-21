//! Exact output before a forbidden-default panic needs a process oracle.
//! Other default argument cases are covered by conformance (#687).

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str) -> Output {
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
    build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

/// The run of a program that must build: its exit success, standard
/// output and standard error.
fn run(name: &str, main: &str) -> (bool, String, String) {
    let out = built(main);
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("default-args-{name}.wasm"),
    );
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    (
        ran.status.success(),
        String::from_utf8(ran.stdout).expect("UTF-8"),
        String::from_utf8_lossy(&ran.stderr).into_owned(),
    )
}

#[test]
fn println_reached_from_a_default_panics() {
    let main = "\
struct QuietConsole: pass

impl Console for QuietConsole:
    fn write_line!(&mut self, text: string) -> Result[void, ConsoleError]:
        .Ok(())

fn announce() -> i32:
    let mut console = QuietConsole {}
    $.with(Console=&mut console):
        println(\"default\")
    1

fn count(start: i32 = announce()) -> i32: start

fn seven() -> i32: 7

fn level(value: i32 = seven()) -> i32: value

pub fn main() -> void $ Console:
    println(level())
    println(count(5))
    println(announce())
    println(count())
";
    let (ok, stdout, stderr) = run("forbidden", main);
    assert!(!ok, "the default's println must panic");
    assert_eq!(stdout, "7\n5\n1\n");
    assert!(
        stderr.contains("suspension-forbidden-context"),
        "stderr: {stderr}"
    );
}
