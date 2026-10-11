//! `void` is the empty tuple (spec/lang/04-type-system.md, types.void,
//! types.unit, types.unit.role, types.tuple.empty): the type `void`, the
//! type `()` and the value `()` are one type and its one value. So `void`
//! implements `Tuple` (fn.type.ctor.tuple-trait) and every trait that has a
//! tuple template (trait.target.tuple.templates), and keeps its own
//! `Termination` (module.entry.termination.void). Programs build through
//! the driver and run on V8 (`host/run.mjs`), as the conformance runner
//! does.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str, goal: &Goal) -> Output {
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
    build(&host, "app", goal)
}

/// The standard output of a program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let out = built(
        main,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("void-unit-{name}.wasm"));
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

/// The diagnostic codes of a program checked without running.
fn codes_of(main: &str) -> Vec<Code> {
    let out = built(main, &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

#[test]
fn void_and_the_empty_tuple_are_one_type() {
    let main = r"
fn take(unit: ()) -> void:
    pass

fn log_start() -> void:
    pass

fn same() -> ():
    log_start()

fn pair() -> (void, i32):
    (log_start(), 1)

pub fn main() -> void $ Console:
    take(())
    take(log_start())
    take(same())
    let unit: void = ()
    take(unit)
    println(pair()._1)
";
    assert_eq!(output_of("one-type", main), "1\n");
}

#[test]
fn void_implements_tuple() {
    let main = r"
use std.function.Tuple

fn keep[T < Tuple](value: T) -> T: value

fn log_start() -> void:
    pass

pub fn main() -> void $ Console:
    let kept: void = keep(log_start())
    println(kept == ())
";
    assert_eq!(output_of("tuple", main), "true\n");
}

#[test]
fn void_has_the_tuple_template_impls() {
    let main = r#"
use std.ops.Default

fn log_start() -> void:
    pass

fn fresh[T < Default]() -> T: T::default()

pub fn main() -> void $ Console:
    println(log_start() == ())
    println(() < log_start())
    println("${log_start()}")
    println(debug(log_start()))
    let made: void = fresh()
    println(made == ())
    let counts: Map[void, i32] = {(): 7}
    println(counts[log_start()])
"#;
    assert_eq!(
        output_of("templates", main),
        "true\nfalse\n()\n()\ntrue\n7\n"
    );
}

#[test]
fn void_keeps_its_termination() {
    let main = r"
use std.process.Termination

fn code_of[T < Termination](value: T) -> u8:
    u8(value.report())

pub fn main() -> void $ Console:
    println(code_of(()))
";
    assert_eq!(output_of("termination", main), "0\n");
}

#[test]
fn the_unit_value_is_no_other_type() {
    let main = r"
fn take(value: i32) -> i32: value

fn wrong() -> i32: take(())
";
    assert_eq!(codes_of(main), vec![Code::TypeMismatch]);
}
