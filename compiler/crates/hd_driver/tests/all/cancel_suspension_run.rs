//! `Suspend[T].cancel` (spec 11 `req.cancel.steps`, `req.cancel.defer`,
//! `req.check.cancel-idempotent`): cancelling a cold suspension marks it
//! without running it, cancelling a completed one changes nothing,
//! cancelling twice is the same as once, and cancelling a started one
//! runs its registered `defer` suites once. Programs build through the
//! driver and run on V8 (`host/run.mjs`).

use std::path::{Path, PathBuf};
use std::process::Command;

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
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("cancel-{name}.wasm"));
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

/// A program whose `work!` logs its body and its cleanup to `log`, then
/// `body` as `main`'s suite.
fn program(body: &str) -> String {
    format!(
        "\
use std.time.{{Clock, Duration, sleep}}
use std.task.race

data Log:
    text: string

fn work!(log: mut Log) -> i32:
    defer:
        log.text = log.text + \"cleanup;\"
    log.text = log.text + \"body;\"
    7

fn slow!(log: mut Log) -> i32 $ Clock:
    log.text = log.text + \"start;\"
    defer:
        log.text = log.text + \"cleanup;\"
    sleep!(Duration::milliseconds(500))
    log.text = log.text + \"done;\"
    7

fn stopper!(target: mut Suspend[i32], log: mut Log) -> i32:
    target.cancel()
    log.text = log.text + \"stopped;\"
    1

pub fn main!() -> void $ Console + Clock:
    {body}"
    )
}

#[test]
fn cancelling_a_cold_suspension_never_runs_its_body() {
    let main = program(
        "\
    let mut log = Log { text: \"\" }
    let pending: mut Suspend[i32] = work(log)
    pending.cancel()
    println(\"log=[${log.text}]\")
",
    );
    assert_eq!(output_of("cold", &main), "log=[]\n");
}

#[test]
fn cancelling_a_completed_suspension_changes_nothing() {
    let main = program(
        "\
    let mut log = Log { text: \"\" }
    let pending: mut Suspend[i32] = work(log)
    value := pending!()
    pending.cancel()
    println(\"value=$value log=[${log.text}]\")
",
    );
    assert_eq!(
        output_of("completed", &main),
        "value=7 log=[body;cleanup;]\n"
    );
}

#[test]
fn cancelling_twice_is_the_same_as_cancelling_once() {
    let main = program(
        "\
    let mut log = Log { text: \"\" }
    let pending: mut Suspend[i32] = work(log)
    pending.cancel()
    pending.cancel()
    println(\"cold=[${log.text}]\")
    let mut other = Log { text: \"\" }
    let done: mut Suspend[i32] = work(other)
    _ := done!()
    done.cancel()
    done.cancel()
    println(\"completed=[${other.text}]\")
",
    );
    assert_eq!(
        output_of("twice", &main),
        "cold=[]\ncompleted=[body;cleanup;]\n"
    );
}

#[test]
fn cancelling_a_started_suspension_runs_its_defer_once() {
    // `race!` polls `slow` first, which starts and waits; `stopper` then
    // cancels it. `race!` cancels the losing `slow` again, which has no
    // further effect.
    let main = program(
        "\
    let mut log = Log { text: \"\" }
    let target: mut Suspend[i32] = slow(log)
    winner := race!(target, stopper(target, log))
    println(\"winner=$winner log=[${log.text}]\")
",
    );
    assert_eq!(
        output_of("started", &main),
        "winner=1 log=[start;cleanup;stopped;]\n"
    );
}

#[test]
fn a_readonly_suspension_binding_cannot_cancel() {
    // `req.suspend.type.readonly-binding`: `:=` weakens the fresh result
    // to a readonly `Suspend[T]`.
    let main = "\
fn ready!() -> i32: 42

fn main() -> void:
    pending := ready()
    pending.cancel()
";
    let out = built(main);
    assert!(
        out.render().contains("mutable-receiver-required"),
        "{}",
        out.render()
    );
}
