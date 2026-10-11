//! Omitted result types (#80; spec 07 `fn.decl.result-omitted-private`,
//! `fn.decl.result-inferred`, `fn.decl.result-inferred.common`,
//! `fn.decl.result-inferred.void`, `fn.decl.omitted-cycle`;
//! checking-and-tir.md §4.13.1 "Omitted result types (M1)"). A private
//! function or inherent method without `-> T` takes the least common type
//! of its final value and its `return` operands, and every caller sees
//! that result. Programs build through the driver and run on V8
//! (`host/run.mjs`), as the conformance runner does.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_diag::Code;
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
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("inferred-result-{name}.wasm"));
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

/// A private function's result is its body's type, so a caller that
/// wants `i32` takes it; a caller declared first sees the result of a
/// callee declared later, and a generic callee's result is instantiated.
#[test]
fn a_private_function_result_is_its_body_type() {
    let main = "\
fn first(): second() + 1

fn second(): answer() - 1

fn answer(): +42

fn head[T](items: List[T]): items[0]

pub fn main() -> void $ Console:
    let n: i32 = first()
    let s: string = head([\"x\", \"y\"])
    println(\"${n} ${s}\")
";
    assert_eq!(output_of("plain", main), "42 x\n");
}

/// A `return` operand and the final value join by least common type:
/// `i32?` and `i32` make `i32?` (`fn.decl.result-inferred.common`,
/// `types.lct.optional`), and literals on different return paths form one
/// class (`types.literal.local.form.function-return.statements`).
#[test]
fn return_paths_join_to_their_least_common_type() {
    let main = "\
fn find(flag: bool, count: i32, found: i32?):
    if flag:
        return found
    count

fn widths(flag: bool):
    if flag:
        return 1
    +2

pub fn main() -> void $ Console:
    let missing: i32? = find(true, 7, .None)
    let kept: i32? = find(false, 7, .None)
    let w: i32 = widths(true) + widths(false)
    match (missing, kept):
        (.None, .Some(k)) => println(\"${k} ${w}\")
        _ => println(\"wrong\")
";
    assert_eq!(output_of("join", main), "7 3\n");
}

/// Two function values with different rows join to the union row, so the
/// one call through the result passes both providers
/// (`req.row.union.sites`).
#[test]
fn function_values_join_to_the_union_row() {
    let main = "\
trait Db:
    fn name(self) -> string

trait Clock:
    fn now(self) -> string

data MemoryDb: pass

impl Db for MemoryDb:
    fn name(self) -> string: \"db\"

data FixedClock: pass

impl Clock for FixedClock:
    fn now(self) -> string: \"noon\"

fn health() -> string $ Clock:
    $.use(Clock).now()

fn orders() -> string $ Db:
    $.use(Db).name()

fn pick(admin: bool):
    if admin:
        return orders
    health

fn serve(admin: bool) -> string $ Db + Clock:
    handler := pick(admin)
    handler()

pub fn main() -> void $ Console:
    $.with(Db = MemoryDb {}, Clock = FixedClock {}):
        println(serve(true) + \",\" + serve(false))
";
    assert_eq!(output_of("rows", main), "db,noon\n");
}

/// A private inherent method infers its result too, through another
/// method, and a function value of it carries the result.
#[test]
fn a_private_method_infers_its_result() {
    let main = "\
data Counter:
    start: i32

impl Counter:
    fn twice(self): self.next() + self.next()

    fn next(self): self.start + 1

pub fn main() -> void $ Console:
    c := Counter { start: 1 }
    let n: i32 = c.twice()
    step := Counter::next
    let m: i32 = step(c)
    println(\"${n} ${m}\")
";
    assert_eq!(output_of("method", main), "4 2\n");
}

/// A body with no value infers `void` (`fn.decl.result-inferred.void`),
/// and a top-level statement reads an inferred result.
#[test]
fn a_body_without_a_value_is_void() {
    let main = "\
base := +40

total := add_base(2)

fn add_base(n: i32): base + n

fn note(value: i32):
    _ := value
    pass

fn check() -> void:
    note(1)

pub fn main() -> void $ Console:
    check()
    println(total)
";
    assert_eq!(output_of("void", main), "42\n");
}

/// Mutual recursion is valid when one member writes its result type
/// (`fn.decl.omitted-cycle.resolve`).
#[test]
fn recursion_through_an_annotated_function_is_valid() {
    let main = "\
fn is_even(n: i32) -> bool:
    if n == 0: true
    else: is_odd(n - 1)

fn is_odd(n: i32):
    if n == 0: false
    else: is_even(n - 1)

pub fn main() -> void $ Console:
    println(\"${is_odd(3)} ${is_odd(4)}\")
";
    assert_eq!(output_of("annotated", main), "true false\n");
}

/// A cycle of omitted results is one error, at the member first in source
/// order even when M1 entered another member first
/// (`fn.decl.omitted-cycle.report`): `entry` reaches `late` before `odd`.
#[test]
fn a_cycle_of_omitted_results_is_one_error_at_its_first_member() {
    let main = "\
fn entry(): late(4)

fn odd(n: i32):
    if n == 0: false
    else: late(n - 1)

fn late(n: i32):
    if n == 0: true
    else: odd(n - 1)

pub fn main() -> void $ Console:
    println(1)
";
    let out = built(main);
    assert_eq!(
        out.diags.code,
        vec![Code::RecursiveFunctionNeedsResultType],
        "{}",
        out.render()
    );
    let at = main.find("odd(n: i32)").expect("odd");
    assert_eq!(out.diags.primary[0].lo as usize, at, "{}", out.render());
}

/// A function that calls itself without a result type is a cycle of one.
#[test]
fn self_recursion_without_a_result_type_is_an_error() {
    let main = "\
fn count(n: i32):
    if n == 0: 0
    else: count(n - 1)

pub fn main() -> void $ Console:
    println(1)
";
    let out = built(main);
    assert_eq!(
        out.diags.code,
        vec![Code::RecursiveFunctionNeedsResultType],
        "{}",
        out.render()
    );
}

/// An error inside a body checked for its result is reported once, and
/// its callers add nothing on top of it.
#[test]
fn an_error_in_an_inferred_body_is_reported_once() {
    let main = "\
fn broken(): missing_name

fn caller():
    let n: i32 = broken()
    n

pub fn main() -> void $ Console:
    println(caller())
";
    let out = built(main);
    assert_eq!(out.diags.code, vec![Code::UnknownName], "{}", out.render());
}
