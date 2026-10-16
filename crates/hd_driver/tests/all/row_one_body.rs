//! A row-polymorphic body is compiled once (`req.poly.one-body`, spec 11;
//! `crates/hd_mono/DESIGN.md` §12.4, #176): an item's own row parameter
//! takes the open row in every instance, so rows never become instance
//! keys, and a caller passes the row argument's keys in one context.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_driver::{Executor, Goal, Host, NoClock, OptLevel, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The Wasm of the program `main` at `level`, for the default engine's
/// target.
fn wasm_of(main: &str, level: OptLevel) -> Vec<u8> {
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
    let packages = Packages {
        level,
        ..Packages::default()
    };
    let out = build_packages(
        &host,
        "app",
        &packages,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{}", out.render());
    out.wasm.expect("wasm")
}

/// The debug names of the functions a module defines.
fn names_in(wasm: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        if let wasmparser::Payload::CustomSection(c) = payload.expect("payload")
            && let wasmparser::KnownCustom::Name(r) = c.as_known()
        {
            for n in r {
                if let wasmparser::Name::Function(m) = n.expect("name") {
                    names.extend(m.into_iter().map(|f| f.expect("name").name.to_owned()));
                }
            }
        }
    }
    names
}

/// How many instances of the item whose path is `item` the module
/// defines: functions named for it, then `#` and the sub-body, without a
/// suspending instance's parts (`$Body`, `$Poll`, `$Cancel`).
fn instances_of(wasm: &[u8], item: &str) -> usize {
    let prefix = format!("{item}#");
    names_in(wasm)
        .iter()
        .filter(|n| n.starts_with(&prefix) && !n.contains('$'))
        .count()
}

/// The standard output of `wasm` on V8.
fn run(name: &str, wasm: &[u8]) -> String {
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("one-body-{name}.wasm"),
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

/// `run` is called with a `$ Console` callback, a `$ Clock` callback and
/// a plain one, and `twice` forwards its row to `run`.
const THREE_ROWS: &str = "\
trait Clock:
    fn now(self) -> i32

data Fixed:
    hour: i32

impl Clock for Fixed:
    fn now(self) -> i32: self.hour

fn run[$R](job: fn() -> void $ R) -> void $ R:
    job()

fn twice[$R](job: fn() -> void $ R) -> void $ R:
    run(job)
    run(job)

fn shout() -> void $ Console:
    println(\"console\")

fn tick() -> void $ Clock + Console:
    println(\"clock ${$.use(Clock).now()}\")

fn quiet() -> void:
    pass

pub fn main() -> void $ Console:
    run(shout)
    $.with(Clock=Fixed { hour: 9 }):
        run(tick)
        twice(tick)
    run(quiet)
    twice(shout)
    println(\"done\")
";

const THREE_ROWS_OUT: &str = "console\nclock 9\nclock 9\nclock 9\nconsole\nconsole\ndone\n";

#[test]
fn a_row_polymorphic_function_is_one_instance_for_every_row() {
    let wasm = wasm_of(THREE_ROWS, OptLevel::Dev);
    assert_eq!(instances_of(&wasm, "app/main/run"), 1, "run");
    assert_eq!(instances_of(&wasm, "app/main/twice"), 1, "twice");
    assert_eq!(run("dev", &wasm), THREE_ROWS_OUT);
    let opt = wasm_of(THREE_ROWS, OptLevel::Optimized);
    assert_eq!(run("optimized", &opt), THREE_ROWS_OUT);
}

/// A local provider in the shared body is nearer than the caller's for a
/// key of the row argument (`req.with.nearest.row-parameter`), and a
/// suspending row-polymorphic function is one instance too.
const NEAREST: &str = "\
trait Tag:
    fn name(self) -> string

data Named:
    label: string

impl Tag for Named:
    fn name(self) -> string: self.label

fn both[$R](callback: fn() -> string $ R) -> string $ R:
    first := callback()
    $.with(Tag=Named { label: \"inner\" }):
        first + \" \" + callback()

fn both_later![$R](callback: fn!() -> string $ R) -> string $ R:
    first := callback!()
    $.with(Tag=Named { label: \"inner\" }):
        first + \" \" + callback!()

fn read_tag() -> string $ Tag:
    $.use(Tag).name()

fn read_tag_later!() -> string $ Tag:
    $.use(Tag).name()

fn plain() -> string: \"plain\"

fn plain_later!() -> string: \"plain\"

fn later!() -> string $ Tag:
    both_later!(read_tag_later) + \" \" + both_later!(plain_later)

pub fn main!() -> void $ Console:
    $.with(Tag=Named { label: \"outer\" }):
        println(both(read_tag))
        println(both(plain))
        println(later!())
";

#[test]
fn a_shared_body_keeps_the_nearest_provider() {
    let wasm = wasm_of(NEAREST, OptLevel::Dev);
    assert_eq!(instances_of(&wasm, "app/main/both"), 1, "both");
    assert_eq!(instances_of(&wasm, "app/main/both_later"), 1, "both_later");
    assert_eq!(
        run("nearest", &wasm),
        "outer inner\nplain plain\nouter inner plain plain\n"
    );
}
