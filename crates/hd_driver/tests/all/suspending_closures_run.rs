//! Host-clock race ordering and losing-closure cleanup. Other runs are in conformance.

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

fn output_of(name: &str, main: &str) -> String {
    let out = built(main);
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("suspending-closure-{name}.wasm"),
    );
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.ts"))
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

fn program(body: &str) -> String {
    format!(
        "\
use std.time.{{Clock, Duration, sleep}}
use std.task.{{all, race}}
use std.ops.defer

fn nap!(ms: i64) -> void $ Clock:
    sleep!(Duration::milliseconds(ms))

pub fn main!() -> void $ Console + Clock:
    {body}"
    )
}

#[test]
fn race_cancels_a_losing_closure_and_runs_its_defer() {
    let main = program(
        "\
    let mut log: List[string] = []
    guarded := fn!(n: i32, ms: i64) -> i32 $ Clock:
        log.push(\"start $n\")
        defer(fn() -> void: log.push(\"cleanup $n\"))
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
