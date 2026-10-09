//! Assignment through a call (`expr.assign.order.call`,
//! `expr.assign.compound.call-once`, `expr.call.apply.*`): `v() = x` stores
//! through `Update`, `v() op= x` reads through `Apply` and stores through
//! `Update` with the callee evaluated once. Programs build through the
//! driver and run on V8 (`host/run.mjs`), as the conformance runner does.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn program(main: &str) -> Output {
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

/// The codes of the diagnostics `items` produces, with an empty `main`.
fn codes_of(items: &str) -> Vec<Code> {
    program(&format!(
        "{items}\npub fn main() -> void $ Console:\n    println(0)\n"
    ))
    .diags
    .code
    .clone()
}

/// A live cell with both traits, and `Panel` holding one in a field.
const CELL: &str = "\
use std.ops.{Apply, Update}

data Cell:
    value: i32

impl Apply for Cell:
    type Out = i32
    fn apply(self) -> i32: self.value

impl Update[i32] for Cell:
    fn update(mut self, value: i32) -> void:
        self.value = value

fn live(value: i32) -> mut Cell:
    Cell { value: value }

data Panel:
    count: mut Cell
    frozen: Cell
";

fn with_cell(rest: &str) -> String {
    format!("{CELL}\n{rest}")
}

#[test]
fn a_store_and_a_compound_store_check() {
    let ok = with_cell(
        "\
fn run(panel: mut Panel) -> i32:
    let mut count = live(+0)
    count() = 1
    count() += 2
    count() -= 1
    (panel.count)() = 5
    (panel.count)() *= 2
    count() + (panel.count)()
",
    );
    assert_eq!(codes_of(&ok), vec![]);
}

#[test]
fn a_callee_that_is_a_call_is_a_place() {
    let ok = with_cell(
        "\
fn pick(panel: mut Panel) -> mut Cell: panel.count

fn run(panel: mut Panel) -> void:
    pick(panel)() = 3
    pick(panel)() += 1
",
    );
    assert_eq!(codes_of(&ok), vec![]);
}

#[test]
fn a_generic_callee_stores_through_its_bound() {
    let ok = "\
use std.ops.{Apply, Update}

fn bump[C < Apply[Out = i32] & Update[i32]](cell: mut C) -> i32:
    cell() += 1
    cell()
";
    assert_eq!(codes_of(ok), vec![]);
}

#[test]
fn a_call_without_update_is_not_a_place() {
    let apply_only = "\
use std.ops.Apply

data Counter:
    hits: i32

impl Apply for Counter:
    type Out = i32
    fn apply(self) -> i32: self.hits

fn run(counter: mut Counter) -> void:
    counter() = 2
";
    assert_eq!(codes_of(apply_only), vec![Code::InvalidAssignmentTarget]);
    let compound = apply_only.replace("counter() = 2", "counter() += 2");
    assert_eq!(codes_of(&compound), vec![Code::InvalidAssignmentTarget]);
}

#[test]
fn a_function_call_is_not_a_place() {
    let plain = "fn current() -> i64: 1\n\nfn run() -> void:\n    current() = 2\n";
    assert_eq!(codes_of(plain), vec![Code::InvalidAssignmentTarget]);
    let compound = "fn current() -> i64: 1\n\nfn run() -> void:\n    current() += 2\n";
    assert_eq!(codes_of(compound), vec![Code::InvalidAssignmentTarget]);
    let stored = "fn run(f: fn() -> i64) -> void:\n    f() = 2\n";
    assert_eq!(codes_of(stored), vec![Code::InvalidAssignmentTarget]);
}

#[test]
fn a_method_call_is_not_a_place() {
    let src = with_cell(
        "\
impl Cell:
    fn get(self) -> i32: self.value

fn run(cell: mut Cell) -> void:
    cell.get() = 2
",
    );
    assert_eq!(codes_of(&src), vec![Code::InvalidAssignmentTarget]);
}

#[test]
fn a_store_needs_a_mut_cell() {
    let param = with_cell("fn run(cell: Cell) -> void:\n    cell() = 2\n");
    assert_eq!(codes_of(&param), vec![Code::ReadonlyRoot]);
    let param_compound = with_cell("fn run(cell: Cell) -> void:\n    cell() += 2\n");
    assert_eq!(codes_of(&param_compound), vec![Code::ReadonlyRoot]);
    let view = with_cell("fn run() -> void:\n    view := live(1)\n    view() = 2\n");
    assert_eq!(codes_of(&view), vec![Code::ReadonlyRoot]);
    let edge = with_cell("fn run(panel: mut Panel) -> void:\n    (panel.frozen)() = 2\n");
    assert_eq!(codes_of(&edge), vec![Code::ReadonlyEdge]);
    let readonly_holder = with_cell("fn run(panel: Panel) -> void:\n    (panel.count)() = 2\n");
    assert_eq!(codes_of(&readonly_holder), vec![Code::ReadonlyRoot]);
}

#[test]
fn a_callable_value_takes_no_arguments() {
    let read = with_cell("fn run(cell: Cell) -> i32:\n    cell(1)\n");
    assert_eq!(codes_of(&read), vec![Code::ArgumentCount]);
    let store = with_cell("fn run(cell: mut Cell) -> void:\n    cell(1) = 2\n");
    assert_eq!(codes_of(&store), vec![Code::ArgumentCount]);
}

#[test]
fn only_apply_and_update_make_a_value_callable() {
    let list = "fn run(tags: List[string]) -> string:\n    tags(0)\n";
    assert_eq!(codes_of(list), vec![Code::NotCallable]);
    let store = "fn run(tags: mut List[string]) -> void:\n    tags(0) = \"x\"\n";
    assert_eq!(codes_of(store), vec![Code::InvalidAssignmentTarget]);
}

#[test]
fn a_compound_store_reads_through_apply() {
    // `Update` alone stores but cannot be read, so `+=` has no read.
    let update_only = "\
use std.ops.Update

data Sink:
    last: i32

impl Update[i32] for Sink:
    fn update(mut self, value: i32) -> void:
        self.last = value

fn run(sink: mut Sink) -> void:
    sink() = 1
";
    assert_eq!(codes_of(update_only), vec![]);
    let compound = update_only.replace("sink() = 1", "sink() += 1");
    assert_eq!(codes_of(&compound), vec![Code::TypeMismatch]);
}

#[test]
fn the_stored_value_must_fit_the_update_impl() {
    let src = with_cell("fn run(cell: mut Cell) -> void:\n    cell() = \"text\"\n");
    assert!(
        !codes_of(&src).is_empty(),
        "a string cannot be stored in an i32 cell"
    );
}

/// Builds `body` as the body of `main` after `items` and runs it.
fn prints(name: &str, items: &str, body: &str) -> Vec<String> {
    let mut main = format!("{items}\npub fn main() -> void $ Console:\n");
    for line in body.lines() {
        main.push_str("    ");
        main.push_str(line);
        main.push('\n');
    }
    let out = program(&main);
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("call-place-{name}.wasm"));
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
    String::from_utf8(ran.stdout)
        .expect("UTF-8")
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn stores_and_compound_stores_change_the_cell() {
    let body = "\
let mut count = live(+0)
count() = 4
println(count())
count() += 3
println(count())
count() <<= 2
println(count())
let mut panel = Panel { count: live(+10), frozen: live(+0) }
(panel.count)() -= 4
println((panel.count)())
println(count())
";
    assert_eq!(
        prints("values", &with_cell(""), body),
        ["4", "7", "28", "6", "28"]
    );
}

#[test]
fn the_callee_is_evaluated_once_and_before_the_value() {
    let items = with_cell(
        "\
data Pair:
    left: mut Cell
    right: mut Cell

fn pick(pair: mut Pair, name: string) -> mut Cell $ Console:
    println(\"pick ${name}\")
    pair.left

fn val(n: i32) -> i32 $ Console:
    println(\"rhs ${n}\")
    n
",
    );
    let body = "\
let mut pair = Pair { left: live(+1), right: live(+2) }
pick(pair, \"a\")() = val(7)
println((pair.left)())
pick(pair, \"b\")() += val(5)
println((pair.left)())
println((pair.right)())
";
    assert_eq!(
        prints("order", &items, body),
        ["pick a", "rhs 7", "7", "pick b", "rhs 5", "12", "2",]
    );
}

#[test]
fn the_store_runs_the_update_method() {
    let items = "\
use std.ops.{Apply, Update}

data Meter:
    reading: i32
    writes: i32

impl Apply for Meter:
    type Out = i32
    fn apply(self) -> i32: self.reading

impl Update[i32] for Meter:
    fn update(mut self, value: i32) -> void:
        self.writes = self.writes + 1
        self.reading = value * 2

fn fresh() -> mut Meter:
    Meter { reading: 0, writes: 0 }
";
    let body = "\
let mut meter = fresh()
meter() = 3
meter() += 1
println(meter())
println(meter.writes)
";
    assert_eq!(prints("update", items, body), ["14", "2"]);
}
