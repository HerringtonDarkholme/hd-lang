//! Fact evaluation at run time (`annot.fact.eval.lazy`,
//! `annot.fact.eval.unread`, `annot.fact.eval.panic-runtime`,
//! `annot.metadata.eval-as-fact`): a facts body runs once, when the program
//! first reads the facts, and a panic in it is `fact-evaluation-failed`,
//! naming the facts and the original panic. Programs build through the
//! driver and run on V8 (`host/run.mjs`), as the conformance runner does.

use std::path::Path;
use std::process::{Command, Output as Ran};

use hd_testkit::Scratch;

use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// Builds `main` as a program and runs it on V8.
fn ran(name: &str, main: &str) -> Ran {
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
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("fact-eval-run-{name}.wasm"),
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
fn panic_of(name: &str, main: &str) -> (String, String) {
    let r = ran(name, main);
    assert!(!r.status.success(), "{name} must panic");
    let err = String::from_utf8(r.stderr).expect("UTF-8");
    let line = err
        .lines()
        .find(|l| l.starts_with("panic: "))
        .unwrap_or_else(|| panic!("{name}: no panic line in {err:?}"))
        .to_owned();
    (String::from_utf8(r.stdout).expect("UTF-8"), line)
}

/// The shared parts of the programs below: a fact type, a failing fact
/// function, and a `Label` derivation that reads the type-level fact
/// through `T::facts()`.
const PRELUDE: &str = "use std.annotation.facts_of
use std.structure.{Structure, Field, Variant, Walker}

struct Style:
    prefix: string = \"\"

fn no_prefix() -> string:
    panic(\"no prefix configured\")

trait Label:
    fn label(self) -> string

struct Sink:
    n: i32

impl[S] Walker[S] for Sink:
    type Error = never

    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F](&mut self, h: Field[S, F], value: F) -> Result[void, never]:
        self.n = self.n + 1
        .Ok(())

impl[T] Label for T by Structure:
    fn label(self) -> string:
        let mut w = Sink { n: 0 }
        _ := Structure::walk(self, &mut w)
        match T::facts().find::[Style]():
            .Some(found) => found.prefix
            .None => \"none\"
";

fn with_prelude(rest: &str) -> String {
    format!("{PRELUDE}\n{rest}")
}

/// The panic of a fact is `fact-evaluation-failed` at the first read. The
/// report names the facts and the original category, and the output before
/// the read is kept.
#[test]
fn a_fact_panic_is_reported_at_the_read_that_started_it() {
    let main = with_prelude(
        "fn style(prefix: string) -> Style:
    Style { prefix: prefix }

@derive(Label)
@style(\"good_\")
struct Good:
    x: i64

@derive(Label)
@style(prefix=no_prefix())
struct Point:
    x: i64

pub fn main() -> void $ Console:
    println(Good { x: 1 }.label())
    println(\"before\")
    println(Point { x: 1 }.label())
    println(\"after\")
",
    );
    let (out, line) = panic_of("type-panic", &main);
    assert_eq!(out, "good_\nbefore\n");
    assert_eq!(
        line,
        "panic: fact-evaluation-failed: the facts of `Point` panicked with explicit-panic: no prefix configured"
    );
}

/// A function's facts name the function.
#[test]
fn a_function_fact_panic_names_the_function() {
    let main = with_prelude(
        "fn style(prefix: string) -> Style:
    Style { prefix: prefix }

@style(prefix=no_prefix())
fn route() -> void:
    pass

pub fn main() -> void $ Console:
    println(\"start\")
    println(facts_of(route).items.len())
",
    );
    let (out, line) = panic_of("fn-panic", &main);
    assert_eq!(out, "start\n");
    assert_eq!(
        line,
        "panic: fact-evaluation-failed: the facts of `route` panicked with explicit-panic: no prefix configured"
    );
}

/// The original category is the panic's own: an integer overflow in a fact
/// expression is reported as such, inside `fact-evaluation-failed`.
#[test]
fn the_report_keeps_the_original_category() {
    let main = with_prelude(
        "fn overflow() -> string:
    let big: i32 = 2147483647
    (big + 1).to_string()

fn style(prefix: string) -> Style:
    Style { prefix: prefix }

@style(prefix=overflow())
fn route() -> void:
    pass

pub fn main() -> void $ Console:
    println(facts_of(route).items.len())
",
    );
    let (_, line) = panic_of("category", &main);
    assert!(
        line.starts_with(
            "panic: fact-evaluation-failed: the facts of `route` panicked with integer-overflow: "
        ),
        "{line}"
    );
}

/// A panic the host raises after a trap, as `stack-exhausted` for
/// recursion that never ends, is wrapped like one the program raises.
#[test]
fn a_host_raised_panic_in_a_fact_is_wrapped() {
    let main = "use std.annotation.facts_of

struct Route:
    path: string

fn deep(n: i32) -> i32:
    deep(n + 1) + 1

fn route(path: string) -> Route:
    Route { path: \"${deep(+0)}\" }

@route(\"/users\")
fn list_users() -> void:
    pass

pub fn main() -> void $ Console:
    println(\"start\")
    _ := facts_of(list_users).find::[Route]()
";
    let (out, line) = panic_of("host-trap", main);
    assert_eq!(out, "start\n");
    assert_eq!(
        line,
        "panic: fact-evaluation-failed: the facts of `list_users` panicked with stack-exhausted: \
         the call stack ran out; check for recursion that never ends"
    );
}

/// A member's metadata is a fact expression too: a panic in it, read through
/// the member's handle, names the member.
#[test]
fn a_member_metadata_panic_names_the_member() {
    let main = "use std.structure.{Structure, Field, Variant, Walker}

struct Style:
    prefix: string = \"\"

fn style(prefix: string) -> Style:
    Style { prefix: prefix }

fn no_prefix() -> string:
    panic(\"no prefix configured\")

struct Names:
    out: string

impl[S] Walker[S] for Names:
    type Error = never

    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F](&mut self, h: Field[S, F], value: F) -> Result[void, never]:
        match h.info.facts.find::[Style]():
            .Some(found) => self.out = self.out + found.prefix
            .None => self.out = self.out + \"-\"
        .Ok(())

trait Label:
    fn label(self) -> string

impl[T] Label for T by Structure:
    fn label(self) -> string:
        let mut w = Names { out: \"\" }
        _ := Structure::walk(self, &mut w)
        w.out

@derive(Label)
struct Point:
    @style(\"x_\")
    x: i64
    @style(prefix=no_prefix())
    y: i64

pub fn main() -> void $ Console:
    println(Point { x: 1, y: 2 }.label())
";
    let (_, line) = panic_of("member", main);
    assert_eq!(
        line,
        "panic: fact-evaluation-failed: the facts of `Point.y` panicked with explicit-panic: no prefix configured"
    );
}

/// Facts read while another fact is evaluated report the fact that
/// panicked. After a fact ends, an ordinary panic is reported plainly.
#[test]
fn the_report_names_the_facts_that_are_running() {
    let main = with_prelude(
        "fn style(prefix: string) -> Style:
    Style { prefix: prefix }

@style(\"inner_\")
fn inner() -> void:
    pass

fn outer_prefix() -> string:
    _ := facts_of(inner).items.len()
    panic(\"outer failed\")

@style(prefix=outer_prefix())
fn outer() -> void:
    pass

pub fn main() -> void $ Console:
    println(facts_of(outer).items.len())
",
    );
    let (_, line) = panic_of("nested", &main);
    assert_eq!(
        line,
        "panic: fact-evaluation-failed: the facts of `outer` panicked with explicit-panic: outer failed"
    );

    let after = with_prelude(
        "fn style(prefix: string) -> Style:
    Style { prefix: prefix }

@style(\"a_\")
fn route() -> void:
    pass

pub fn main() -> void $ Console:
    println(facts_of(route).items.len())
    panic(\"later\")
",
    );
    let (out, line) = panic_of("after", &after);
    assert_eq!(out, "1\n");
    assert_eq!(line, "panic: explicit-panic: later");
}
