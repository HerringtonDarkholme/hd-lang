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

/// The standard output of a program that must build and succeed.
fn output_of(name: &str, files: &[(&str, &str)]) -> String {
    let r = ran(name, files);
    assert!(
        r.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    String::from_utf8(r.stdout).expect("UTF-8")
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

fn assert_forbidden(name: &str, files: &[(&str, &str)]) {
    let (_, line) = panic_of(name, files);
    assert!(
        line.starts_with("panic: suspension-forbidden-context"),
        "{name}: {line}"
    );
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

data QuietConsole: pass

impl Console for QuietConsole:
    fn write_line!(mut self, text: string) -> Result[void, ConsoleError]:
        .Ok(())

fn print_now() -> i32:
    let console: mut QuietConsole = QuietConsole {}
    $.with(Console=console):
        println(\"quiet\")
    1
";

fn with_helpers(rest: &str) -> String {
    format!("{HELPERS}\n{rest}")
}

#[test]
fn block_on_through_a_helper_in_a_defer_suite_panics() {
    let main = with_helpers(
        "fn cleanup() -> void:
    defer:
        _ := wait_now()
    pass

pub fn main() -> void:
    cleanup()
",
    );
    assert_forbidden("defer", &[("main.hd", &main)]);
}

#[test]
fn println_through_a_helper_in_a_defer_suite_panics() {
    let main = with_helpers(
        "fn cleanup() -> void:
    defer:
        _ := print_now()
    pass

pub fn main() -> void:
    cleanup()
",
    );
    assert_forbidden("defer-println", &[("main.hd", &main)]);
}

#[test]
fn block_on_in_a_defer_suite_of_a_suspending_function_panics() {
    let main = with_helpers(
        "fn work!() -> void:
    defer:
        _ := wait_now()
    pass

pub fn main!() -> void:
    work!()
",
    );
    assert_forbidden("defer-suspending", &[("main.hd", &main)]);
}

#[test]
fn block_on_through_a_helper_in_a_default_expression_panics() {
    let main = with_helpers(
        "fn count(start: i32 = wait_now()) -> i32:
    start

pub fn main() -> void:
    _ := count()
",
    );
    assert_forbidden("default", &[("main.hd", &main)]);
}

#[test]
fn println_through_a_helper_in_a_default_expression_panics() {
    let main = with_helpers(
        "fn count(start: i32 = print_now()) -> i32:
    start

pub fn main() -> void:
    _ := count()
",
    );
    assert_forbidden("default-println", &[("main.hd", &main)]);
}

#[test]
fn block_on_through_a_helper_in_non_entry_module_init_panics() {
    let helper = with_helpers(
        "let value = wait_now()

pub fn get() -> i32:
    value
",
    );
    let main = "use pkg.helper.get

pub fn main() -> void $ Console:
    println(get())
";
    assert_forbidden("init", &[("main.hd", main), ("helper.hd", &helper)]);
}

#[test]
fn println_through_a_helper_in_non_entry_module_init_panics() {
    let helper = with_helpers(
        "let value = print_now()

pub fn get() -> i32:
    value
",
    );
    let main = "use pkg.helper.get

pub fn main() -> void $ Console:
    println(get())
";
    assert_forbidden("init-println", &[("main.hd", main), ("helper.hd", &helper)]);
}

#[test]
fn block_on_in_a_facts_body_panics() {
    let main = with_helpers(
        "use std.annotation.facts_of

data Style:
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

#[test]
fn block_on_outside_forbidden_contexts_runs() {
    let main = with_helpers(
        "fn quiet() -> void:
    pass

fn cleanup() -> void:
    defer:
        quiet()
    pass

fn count(start: i32 = 7) -> i32:
    start

pub fn main!() -> void $ Console:
    println(wait_now())
    cleanup()
    println(wait_now())
    println(count())
    println(wait_now())
",
    );
    assert_eq!(
        output_of("outside", &[("main.hd", &main)]),
        "42\n42\n7\n42\n"
    );
}

#[test]
fn the_count_is_balanced_after_a_defer_suite_and_a_default_expression() {
    let main = with_helpers(
        "fn plain() -> i32:
    5

fn count(start: i32 = plain()) -> i32:
    start

fn cleanup() -> void:
    defer:
        _ := count()
    pass

pub fn main() -> void $ Console:
    cleanup()
    println(count())
    println(wait_now())
",
    );
    assert_eq!(output_of("balanced", &[("main.hd", &main)]), "5\n42\n");
}

#[test]
fn the_entry_module_init_may_block_on() {
    let main = with_helpers(
        "let value = wait_now()

pub fn main() -> void $ Console:
    println(value)
",
    );
    assert_eq!(output_of("entry-init", &[("main.hd", &main)]), "42\n");
}

#[test]
fn a_non_entry_module_function_called_from_main_may_block_on() {
    let helper = with_helpers(
        "let value = +3

pub fn get() -> i32:
    value + wait_now()
",
    );
    let main = "use pkg.helper.get

pub fn main() -> void $ Console:
    println(get())
";
    assert_eq!(
        output_of(
            "non-entry-call",
            &[("main.hd", main), ("helper.hd", &helper)]
        ),
        "45\n"
    );
}
