//! Run-time forbidden contexts (spec 11 `req.drive.block-on.indirect`,
//! `req.drive.block-on.println`, `req.drive.block-on.fact-contexts`, spec 6
//! `flow.defer.block-on.indirect`): `block_on` or `println` reached through
//! another call while a `defer` suite, a default expression, a facts body or
//! non-entry module initialization runs panics with
//! `suspension-forbidden-context`. Outside those contexts it runs. Programs
//! build through the driver and run on V8 (`host/run.mjs`).

use std::path::Path;
use std::process::{Command, Output as Ran};

use hd_testkit::Scratch;

use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// Builds the modules (`main.hd` is the entry) and runs them on V8.
fn ran(name: &str, files: &[(&str, &str)]) -> Ran {
    let mut src = MemorySources::default();
    for (path, text) in files {
        src.insert(path, text);
    }
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
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("forbidden-context-{name}.wasm"),
    );
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    ran
}

/// A program that must exit with a panic: its standard output, and the
/// panic line of its report.
fn panic_of(name: &str, files: &[(&str, &str)]) -> (String, String) {
    let r = ran(name, files);
    assert!(!r.status.success(), "{name} must panic");
    let err = String::from_utf8(r.stderr).expect("UTF-8");
    let line = err
        .lines()
        .find(|l| l.starts_with("panic: "))
        .unwrap_or_else(|| panic!("{name}: no panic line in {err:?}"))
        .to_owned();
    (String::from_utf8(r.stdout).expect("UTF-8"), line)
}

/// A helper that `block_on`s a ready suspension, and a console that
/// discards its lines, shared by the programs below.
const HELPERS: &str = "use std.task.block_on

fn ready!() -> i32:
    42

fn wait_now() -> i32:
    let pending: mut Suspend[i32] = ready()
    match block_on(pending):
        .Completed(value) => value
        _ => 0

struct QuietConsole: pass

impl Console for QuietConsole:
    fn write_line!(&mut self, text: string) -> Result[void, ConsoleError]:
        .Ok(())

fn print_now() -> i32:
    let mut console = QuietConsole {}
    $.with(Console=&mut console):
        println(\"quiet\")
    1
";

fn with_helpers(rest: &str) -> String {
    format!("{HELPERS}\n{rest}")
}

#[test]
fn block_on_in_a_facts_body_panics() {
    let main = with_helpers(
        "use std.annotation.facts_of

struct Style:
    prefix: string = \"\"

fn style() -> Style:
    _ := wait_now()
    Style { prefix: \"p_\" }

@style()
fn route() -> void:
    pass

pub fn main() -> void $ Console:
    println(facts_of(route).items.len())
",
    );
    // A panic in a facts body is reported as `fact-evaluation-failed`, which
    // names the original panic (`annot.fact.eval.panic-runtime`).
    let (_, line) = panic_of("facts", &[("main.hd", &main)]);
    assert!(
        line.starts_with("panic: fact-evaluation-failed")
            && line.contains("panicked with suspension-forbidden-context"),
        "{line}"
    );
}
