//! Call-form rules of the task combinators (spec/lang/11-requirements-and-suspension.md,
//! `req.combinator.race-empty`, `.race-empty-literal`, `.race-empty-run`,
//! `.all-direct`). Each bad form is one error at check time; the valid
//! forms build and run on V8 (`host/run.mjs`).

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_diag::Code;
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

fn codes(main: &str) -> Vec<Code> {
    built(main)
        .diags
        .code
        .iter()
        .copied()
        .filter(|c| *c != Code::UnusedLocalBinding)
        .collect()
}

const HEAD: &str = "use std.task.{all, race}\n\nfn one!() -> i32: 1\n\nfn two!() -> i32: 2\n\n";

/// A program whose `main!` runs `body`.
fn program(body: &str) -> String {
    format!("{HEAD}pub fn main!() -> void $ Console:\n{body}\n")
}

fn run(name: &str, main: &str) -> std::process::Output {
    let out = built(main);
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(env!("CARGO_TARGET_TMPDIR"), &format!("comb-{name}.wasm"));
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    ran
}

#[test]
fn race_without_a_task_is_one_argument_count_error() {
    for body in [
        "    println(race!::[i32]())",
        "    println(race!::[i32](tasks=[]))",
        "    x := race!::[i32]()\n    println(x)",
    ] {
        assert_eq!(codes(&program(body)), vec![Code::ArgumentCount], "{body}");
    }
}

#[test]
fn all_forms_other_than_direct_positional_are_one_type_mismatch() {
    for body in [
        "    println(all!(tasks...))",
        "    println(all!(x=one(), y=two()))",
        "    println(all!::[i32, i32](one(), two()))",
        "    f := all\n    println(f)",
    ] {
        let main = format!(
            "{HEAD}pub fn main!() -> void $ Console:\n    let tasks: List[mut Suspend[i32]] = [one()]\n{body}\n"
        );
        assert_eq!(codes(&main), vec![Code::TypeMismatch], "{body}");
    }
}

#[test]
fn valid_forms_build_and_run() {
    let main = program(
        "    println(race!(one(), two()))\n    println(race!::[i32](one()))\n    \
         println(race!(tasks=[one(), two()]))\n    let list: List[mut Suspend[i32]] = [two(), one()]\n    \
         println(race!(list...))\n    let (a, b) = all!(one(), two())\n    println(\"${a} ${b}\")",
    );
    let ran = run("valid", &main);
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(
        String::from_utf8(ran.stdout).expect("UTF-8"),
        "1\n1\n1\n2\n1 2\n"
    );
}

#[test]
fn empty_task_list_at_run_time_panics() {
    let main = program("    let none: List[mut Suspend[i32]] = []\n    println(race!(none...))");
    let ran = run("empty", &main);
    assert!(!ran.status.success());
    assert!(
        String::from_utf8_lossy(&ran.stderr).contains("explicit-panic"),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
}
