//! Omitted arguments run their default bodies per call (fn.default.eval,
//! fn.default.scope, data.literal.defaulted; codegen.md §13.13): programs
//! build through the driver and run on V8 (`host/run.mjs`), as the
//! conformance runner does.

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

/// The run of a program that must build: its exit success, standard
/// output and standard error.
fn run(name: &str, main: &str) -> (bool, String, String) {
    let out = built(main);
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("default-args-{name}.wasm"));
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

/// The standard output of a program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let (ok, stdout, stderr) = run(name, main);
    assert!(ok, "{name}: {stderr}");
    stdout
}

#[test]
fn a_default_reads_an_earlier_parameter() {
    let main = "\
fn area(width: i32, height: i32 = width * 2) -> i32: width * height

pub fn main() -> void $ Console:
    println(area(3))
    println(area(3, 4))
";
    assert_eq!(output_of("earlier", main), "18\n12\n");
}

#[test]
fn defaults_run_in_declaration_order_after_every_explicit_argument() {
    let main = "\
fn note(log: mut List[string], text: string) -> string:
    log.push(text)
    text

fn join(log: mut List[string], first: string, second: string = note(log, \"second\"), third: string = note(log, \"third\")) -> string:
    \"$first $second $third\"

pub fn main() -> void $ Console:
    let log: mut List[string] = []
    println(join(log, note(log, \"first\")))
    println(join(log, third=note(log, \"named\"), first=note(log, \"last\")))
    for line in log:
        println(line)
";
    assert_eq!(
        output_of("order", main),
        "first second third\nlast second named\n\
         first\nsecond\nthird\nnamed\nlast\nsecond\n"
    );
}

#[test]
fn a_list_default_is_fresh_on_every_call() {
    let main = "\
fn collect(item: i32, into: mut List[i32] = []) -> usize:
    into.push(item)
    into.len()

pub fn main() -> void $ Console:
    println(collect(1))
    println(collect(2))
    let shared: mut List[i32] = []
    println(collect(3, shared))
    println(collect(4, shared))
";
    assert_eq!(output_of("fresh-list", main), "1\n1\n1\n2\n");
}

#[test]
fn a_generic_function_default_runs_at_each_instantiation() {
    let main = "\
fn twice[T](value: T, items: List[T] = [value, value]) -> List[T]: items

pub fn main() -> void $ Console:
    numbers := twice(7)
    words := twice(\"hi\")
    println(numbers.len() + numbers[1])
    println(\"${words[0]}${words[1]}\")
    println(twice(1, [5]).len())
";
    assert_eq!(output_of("generic", main), "9\nhihi\n1\n");
}

#[test]
fn a_method_default_reads_the_receiver_and_earlier_parameters() {
    let main = "\
data Counter:
    base: i32

impl Counter:
    fn add(self, step: i32 = self.base * 10, extra: i32 = step + 1) -> i32:
        self.base + step + extra

pub fn main() -> void $ Console:
    counter := Counter { base: 2 }
    println(counter.add())
    println(counter.add(5))
    println(counter.add(extra=0))
";
    assert_eq!(output_of("method", main), "43\n13\n22\n");
}

#[test]
fn a_data_field_default_runs_per_construction() {
    let main = "\
data Order:
    id: i32
    tags: mut List[string] = []
    priority: i32 = 3

pub fn main() -> void $ Console:
    let mut first = Order { id: 1 }
    second := Order { id: 2, priority: 9 }
    first.tags.push(\"rush\")
    println(\"${first.tags.len()} ${second.tags.len()} ${first.priority} ${second.priority}\")
";
    assert_eq!(output_of("field", main), "1 0 3 9\n");
}

#[test]
fn println_reached_from_a_default_panics() {
    let main = "\
data QuietConsole: pass

impl Console for QuietConsole:
    fn write_line!(mut self, text: string) -> Result[void, ConsoleError]:
        .Ok(())

fn announce() -> i32:
    let console: mut QuietConsole = QuietConsole {}
    $.with(Console=console):
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

#[test]
fn generic_owners_give_their_arguments_to_method_and_field_defaults() {
    let main = "\
data Shelf[T]:
    first: T
    rest: List[T] = []

impl[T] Shelf[T]:
    fn all(self, extra: List[T] = [self.first]) -> usize: extra.len() + self.rest.len()

pub fn main() -> void $ Console:
    words := Shelf { first: \"a\" }
    numbers := Shelf { first: 1, rest: [2, 3] }
    println(\"${words.all()} ${numbers.all()} ${numbers.all([])}\")
";
    assert_eq!(output_of("generic-owner", main), "1 3 2\n");
}
