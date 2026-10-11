//! Suspending closures run (req.suspend.closure.marker,
//! req.suspend.closure.trailing-only, req.bind.construction; suspension.md
//! §14.1 to §14.6): a `fn!` closure's code is the cold constructor of a
//! frame that holds its parameters, captures and providers, and its body
//! is the same state machine as a named suspending function's. Programs
//! build through the driver and run on V8 (`host/run.mjs`), whose default
//! profile has a host timer, so the closures really wait.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str) -> Output {
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
    let path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("suspending-closure-{name}.wasm"));
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

/// A program: the timer helper `nap!`, then `body` as `main!`'s suite.
/// A `\` continuation strips the first line's indentation, so it is
/// added here.
fn program(body: &str) -> String {
    format!(
        "\
use std.time.{{Clock, Duration, sleep}}
use std.task.{{all, race}}

fn nap!(ms: i64) -> void $ Clock:
    sleep!(Duration::milliseconds(ms))

pub fn main!() -> void $ Console + Clock:
    {body}"
    )
}

#[test]
fn a_closure_awaited_once_keeps_its_capture_across_a_wait() {
    let main = program(
        "\
    base := +40
    once := fn!(offset: i32) -> i32 $ Clock:
        nap!(2)
        base + offset
    println(once!(2))
    let pending: mut Suspend[i32] = once(1)
    println(pending!())
",
    );
    assert_eq!(output_of("once", &main), "42\n41\n");
}

#[test]
fn closures_from_a_list_are_each_awaited_twice() {
    let main = program(
        "\
    double := fn!(x: i32) -> i32 $ Clock:
        nap!(1)
        x * 2
    square := fn!(x: i32) -> i32: x * x
    let fns: List[fn!(i32) -> i32 $ Clock] = [double, square]
    first := fns[0]
    second := fns[1]
    println(first!(3))
    println(first!(4))
    println(second!(3))
    println(second!(4))
",
    );
    assert_eq!(output_of("list", &main), "6\n8\n9\n16\n");
}

#[test]
fn a_shared_capture_is_written_on_both_sides_of_a_wait() {
    // Both suspensions are pending at once under `all!`, so each write
    // after the wait sees the other's write before it.
    let main = program(
        "\
    let count: i32 = 0
    bump := fn!() -> void $ Clock:
        count = count + 1
        nap!(1)
        count = count + 10
    _ := all!(bump(), bump())
    println(count)
    bump!()
    println(count)
",
    );
    assert_eq!(output_of("shared", &main), "22\n33\n");
}

#[test]
fn a_trailing_block_for_a_suspending_parameter_waits() {
    let main = format!(
        "\
fn retry_n![$R](times: i32, attempt: fn!() -> bool $ R) -> i32 $ R:
    ok := attempt!()
    if ok || times <= 1:
        return 1
    1 + retry_n!(times - 1, attempt)

{}",
        program(
            "\
    let calls: i32 = 0
    took := retry_n!(5):
        calls = calls + 1
        nap!(1)
        calls == 3
    println(\"took $took calls $calls\")
",
        )
    );
    assert_eq!(output_of("trailing", &main), "took 3 calls 3\n");
}

#[test]
fn a_defer_in_a_closure_runs_when_its_body_completes() {
    let main = program(
        "\
    let log: mut List[string] = []
    guarded := fn!(n: i32) -> i32 $ Clock:
        log.push(\"start $n\")
        defer:
            log.push(\"cleanup $n\")
        nap!(1)
        log.push(\"done $n\")
        n
    println(guarded!(1))
    for line in log:
        println(line)
",
    );
    assert_eq!(output_of("defer", &main), "1\nstart 1\ndone 1\ncleanup 1\n");
}

#[test]
fn race_cancels_a_losing_closure_and_runs_its_defer() {
    let main = program(
        "\
    let log: mut List[string] = []
    guarded := fn!(n: i32, ms: i64) -> i32 $ Clock:
        log.push(\"start $n\")
        defer:
            log.push(\"cleanup $n\")
        nap!(ms)
        log.push(\"done $n\")
        n
    w := race!(guarded(2, 2), guarded(3, 500))
    println(\"race $w\")
    for line in log:
        println(line)
",
    );
    assert_eq!(
        output_of("race", &main),
        "race 2\nstart 2\nstart 3\ndone 2\ncleanup 2\ncleanup 3\n"
    );
}

#[test]
fn a_closure_that_only_creates_another_still_captures_for_it() {
    // `make_inner` never reads `count` itself; the closure it returns
    // does, so `make_inner` must capture the shared cell to pass it on
    // (fn.capture.locals).
    let main = "\
fn child!(value: i32) -> i32: value

fn make() -> fn!() -> i32:
    let count: i32 = 40
    make_inner := fn() -> fn!() -> i32:
        fn!() -> i32:
            count = count + 1
            child!(count)
    next := make_inner()
    count = count + 1
    next

pub fn main!() -> void $ Console:
    next := make()
    println(next!())
    println(next!())
";
    assert_eq!(output_of("nested", main), "42\n43\n");
}
