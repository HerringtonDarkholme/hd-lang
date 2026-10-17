//! A suspending function value weakens to its lowered constructor type
//! (fn.type.suspend.weaken, fn.type.suspend.ctor,
//! req.suspend.type.conversion): `fn!(A) -> T $ R` converts to
//! `fn(A) -> mut Suspend[T] $ R` at each expected-type site, and never
//! back (fn.type.suspend.no-reverse). The closure keeps its
//! representation: its code already returns the cold suspension. Programs
//! build through the driver and run on V8 (`host/run.mjs`), whose default
//! profile has a host timer, so the suspensions really wait.

use std::path::Path;
use std::process::Command;

use hd_diag::Code;
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

/// The standard output of a program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let out = built(main);
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("suspending-fn-weaken-{name}.wasm"),
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

/// The suspending `plus!`, the suspending `tick!` that really waits, the plain constructor
/// `ticker`, and the driver `drive!` that takes the lowered type; then
/// `rest`.
fn program(rest: &str) -> String {
    format!(
        "\
use std.time.{{Clock, ms, sleep}}
use std.function.{{Fn, SuspendFn}}

fn plus!(by: i32) -> i32:
    by + 1

fn tick!(by: i32) -> i32 $ Clock:
    sleep!(ms(1))
    by + 1

fn ticker(by: i32) -> mut Suspend[i32] $ Clock:
    tick(by + 10)

fn drive!(constructor: fn(i32) -> mut Suspend[i32] $ Clock) -> i32 $ Clock:
    let pending: mut Suspend[i32] = constructor(41)
    pending!()

{rest}"
    )
}

#[test]
fn a_weakened_value_constructs_and_drives_its_suspension() {
    let main = program(
        "\
fn returned() -> SuspendFn[(i32,), i32, $ Clock]:
    tick

fn lowered(op: SuspendFn[(i32,), i32, $ Clock]) -> void $ Clock:
    let constructor: Fn[(i32,), mut Suspend[i32], $ Clock] = op
    let pending: mut Suspend[i32] = constructor(1)
    _ := pending

fn joined!(flag: bool) -> i32 $ Clock:
    let constructor: fn(i32) -> mut Suspend[i32] $ Clock = if flag:
        tick
    else:
        ticker
    drive!(constructor)

fn wider!(constructor: fn(i32) -> mut Suspend[i32] $ Clock + Console) -> i32 $ Clock + Console:
    let pending: mut Suspend[i32] = constructor(1)
    pending!()

pub fn main!() -> void $ Console + Clock:
    println(drive!(tick))
    let constructor: fn(i32) -> mut Suspend[i32] $ Clock = tick
    println(drive!(constructor))
    let pending: mut Suspend[i32] = constructor(1)
    println(pending!())
    println(drive!(returned()))
    lowered(returned())
    println(joined!(true))
    println(joined!(false))
    println(wider!(tick))
",
    );
    assert_eq!(output_of("drive", &main), "42\n42\n2\n42\n42\n52\n2\n");
}

#[test]
fn a_suspending_function_weakens_at_each_expected_type_site() {
    let out = built(&program(
        "\
fn as_result() -> fn(i32) -> mut Suspend[i32]:
    plus

fn as_return() -> Fn[(i32,), mut Suspend[i32], $ Clock]:
    return tick

fn as_branch(flag: bool) -> Fn[(i32,), mut Suspend[i32], $ Clock]:
    if flag:
        tick
    else:
        ticker

fn as_constructor(op: SuspendFn[(i32,), i32, $ Clock]) -> Fn[(i32,), mut Suspend[i32], $ Clock]:
    op

pub fn main() -> void:
    _ := as_result
    _ := as_return
    _ := as_branch
    _ := as_constructor
",
    ));
    assert!(out.diags.is_empty(), "{}", out.render());
}

/// Each case must be rejected with `type-mismatch` alone.
fn mismatch(name: &str, rest: &str) {
    let out = built(&program(rest));
    assert!(
        !out.diags.code.is_empty() && out.diags.code.iter().all(|c| *c == Code::TypeMismatch),
        "{name}: {}",
        out.render()
    );
}

#[test]
fn the_reverse_conversion_is_a_type_mismatch() {
    mismatch(
        "result",
        "\
fn back() -> SuspendFn[(i32,), i32, $ Clock]:
    ticker

pub fn main() -> void:
    _ := back
",
    );
    mismatch(
        "argument",
        "\
fn takes(op: fn!(i32) -> i32 $ Clock) -> void:
    _ := op

pub fn main() -> void:
    takes(ticker)
",
    );
    mismatch(
        "constructor spelling",
        "\
fn back(op: Fn[(i32,), mut Suspend[i32], $ Clock]) -> SuspendFn[(i32,), i32, $ Clock]:
    op

pub fn main() -> void:
    _ := back
",
    );
}

#[test]
fn the_weakening_keeps_the_row_and_the_result() {
    mismatch(
        "row dropped",
        "\
fn lost() -> fn(i32) -> mut Suspend[i32]:
    tick

pub fn main() -> void:
    _ := lost
",
    );
    mismatch(
        "other result",
        "\
fn other() -> Fn[(i32,), mut Suspend[i64], $ Clock]:
    tick

pub fn main() -> void:
    _ := other
",
    );
    mismatch(
        "inside a type argument",
        "\
fn nested(ops: List[fn!(i32) -> i32 $ Clock]) -> List[fn(i32) -> mut Suspend[i32] $ Clock]:
    ops

pub fn main() -> void:
    _ := nested
",
    );
}
