//! Inferred rows of private callables (#194; spec 11
//! `req.row.omitted.inferred-private`, `req.row.omitted.cycle`,
//! `req.row.omitted.closure-row`; checking-and-tir.md §4.13.1 "M3:
//! inferred rows"). A private function or inherent method without a `$`
//! clause takes the least row its body needs, a recursive group the least
//! rows that satisfy every member, and each caller sees that row. Programs
//! build through the driver and run on V8 (`host/run.mjs`), as the
//! conformance runner does.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn run(store: &MemoryStore, main: &str, goal: &Goal) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", goal)
}

fn codes(main: &str) -> Vec<Code> {
    run(&MemoryStore::default(), main, &Goal::Analyze)
        .diags
        .code
        .clone()
}

/// The standard output of a one-file program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    output_in(&MemoryStore::default(), name, main)
}

/// [`output_of`], building against `store`, which earlier builds filled.
fn output_in(store: &MemoryStore, name: &str, main: &str) -> String {
    let goal = Goal::Program {
        entry: "main".into(),
    };
    let out = run(store, main, &goal);
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("inferred-row-{name}.wasm"));
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

const TAGS: &str = "
trait Tag:
    fn name(self) -> string

trait Log:
    fn tag(self) -> string

data Named:
    label: string

impl Tag for Named:
    fn name(self) -> string: self.label

impl Log for Named:
    fn tag(self) -> string: self.label
";

/// A private function's row is the keys its body uses, so a caller that
/// names them may call it, and a public caller without them may not
/// (`req.row.omitted.inferred-private`, `req.row.set.call`).
#[test]
fn a_private_function_takes_the_row_its_body_uses() {
    let ok = format!(
        "{TAGS}
fn read() -> string:
    $.use(Tag).name()

pub fn top() -> string $ Tag:
    read()
"
    );
    assert_eq!(codes(&ok), vec![]);
    let missing = format!(
        "{TAGS}
fn read() -> string:
    $.use(Tag).name()

pub fn top() -> string:
    read()
"
    );
    assert_eq!(codes(&missing), vec![Code::MissingRequirement]);
}

/// A chain of private functions passes the row up: each takes its
/// callee's keys that no `$.with` block around the call provides, and an
/// inherent method infers its row alike.
#[test]
fn a_chain_of_private_callers_takes_the_keys_left_unprovided() {
    let main = format!(
        "{TAGS}
fn tagged() -> string:
    $.use(Tag).name() + $.use(Log).tag()

fn logged() -> string:
    $.with(Tag=Named {{ label: \"t\" }}):
        tagged()

data Box:
    label: string

impl Box:
    fn show(self):
        self.label + logged()

pub fn needs_log() -> string $ Log:
    Box {{ label: \"b\" }}.show()

pub fn needs_nothing() -> string:
    Box {{ label: \"b\" }}.show()
"
    );
    assert_eq!(codes(&main), vec![Code::MissingRequirement]);
    let run = format!(
        "{TAGS}
fn tagged() -> string:
    $.use(Tag).name() + $.use(Log).tag()

fn logged() -> string:
    $.with(Tag=Named {{ label: \"t\" }}):
        tagged()

data Box:
    label: string

impl Box:
    fn show(self):
        self.label + logged()

pub fn main() -> void $ Console:
    $.with(Log=Named {{ label: \"l\" }}):
        println(Box {{ label: \"b\" }}.show())
"
    );
    assert_eq!(output_of("chain", &run), "btl\n");
}

/// Functions that call each other take the least rows that satisfy every
/// member (`req.row.omitted.cycle`): `ping` uses `Tag` and calls `pong`,
/// and `pong` calls `ping` inside a `$.with(Tag=...)` block, so `pong`
/// needs nothing.
#[test]
fn mutually_recursive_private_functions_take_the_least_rows() {
    let rows = format!(
        "{TAGS}
fn ping(n: i32) -> string:
    if n == 0:
        return $.use(Tag).name()
    pong(n - 1)

fn pong(n: i32) -> string:
    $.with(Tag=Named {{ label: \"p\" }}):
        ping(n)

pub fn free() -> string:
    pong(3)

pub fn bound() -> string:
    ping(3)
"
    );
    assert_eq!(codes(&rows), vec![Code::MissingRequirement]);
    let main = format!(
        "{TAGS}
fn ping(n: i32) -> string:
    if n == 0:
        return $.use(Tag).name() + $.use(Log).tag()
    $.use(Log).tag() + pong(n - 1)

fn pong(n: i32) -> string:
    $.with(Tag=Named {{ label: \"p\" }}):
        ping(n)

fn count(n: i32) -> i32:
    if n == 0:
        return 0
    _ := $.use(Log)
    1 + count(n - 1)

pub fn main() -> void $ Console:
    $.with(Log=Named {{ label: \"l\" }}):
        println(pong(2))
        println(count(3))
"
    );
    assert_eq!(output_of("cycle", &main), "llpl\n3\n");
}

/// A function value carries its row, so a top-level reference solves the
/// row first, with the rows it waits on: `outer` needs `inner`'s `Tag`,
/// and a call through the value needs it too.
#[test]
fn a_function_value_carries_the_solved_row() {
    let main = format!(
        "{TAGS}
job := outer

fn outer() -> string:
    inner()

fn inner() -> string:
    $.use(Tag).name()

pub fn bound() -> string $ Tag:
    job()

pub fn free() -> string:
    job()
"
    );
    assert_eq!(codes(&main), vec![Code::MissingRequirement]);
}

/// A written row that calls into a recursive group still being inferred
/// is checked once the group's rows are solved.
#[test]
fn a_written_row_in_a_recursive_group_is_checked_after_the_solve() {
    let main = format!(
        "{TAGS}
fn walk(n: i32) -> string:
    if n == 0:
        return $.use(Log).tag()
    step(n - 1)

fn step(n: i32) $ Tag:
    walk(n)

pub fn top() -> string $ Tag + Log:
    walk(2)
"
    );
    assert_eq!(codes(&main), vec![Code::MissingRequirement]);
}

/// A generic key keeps the call's type arguments: `find[User]` needs
/// `Repo[User]`, so a caller that names `Repo[Post]` may not call it
/// (`req.row.entail.generic`), and a bound key keeps its binding.
#[test]
fn an_inferred_generic_key_keeps_its_type_arguments() {
    let decls = "
trait Repo[T]:
    fn count(self) -> i32

trait Store:
    type Item
    fn load(self) -> Self::Item

data User: pass
data Post: pass
data UserRepo: pass

impl Repo[User] for UserRepo:
    fn count(self) -> i32: 20

impl Store for UserRepo:
    type Item = i32
    fn load(self) -> i32: 7

fn find[T]() -> i32:
    $.use(Repo[T]).count()

fn total[T]() -> i32:
    find::[T]() + $.use(Store[Item = i32]).load()
";
    let wrong = format!(
        "{decls}
pub fn posts() -> i32 $ Repo[Post] + Store[Item = i32]:
    total::[Post]() + total::[User]()
"
    );
    assert_eq!(codes(&wrong), vec![Code::MissingRequirement]);
    let main = format!(
        "{decls}
pub fn main() -> void $ Console:
    $.with(Repo[User]=UserRepo {{}}, Store[Item = i32]=UserRepo {{}}):
        println(total::[User]())
"
    );
    assert_eq!(output_of("generic", &main), "27\n");
}

/// A generic function that calls itself at a growing argument grows its
/// row's key without end: the solve stops at the instantiation depth
/// limit (`types.generic.instantiation-depth`).
#[test]
fn a_growing_key_in_a_recursive_row_is_too_deep() {
    let main = "
trait Repo[T]:
    fn count(self) -> i32

fn grow[T](n: i32) -> i32:
    if n == 0:
        return $.use(Repo[T]).count()
    grow::[List[T]](n - 1)
";
    assert_eq!(codes(main), vec![Code::InstantiationTooDeep]);
}

/// An edit that changes a private function's inferred row changes the
/// code of every caller that passes it providers: `read`'s text and
/// `call`'s are equal in both programs, but `inner` needs `Log` only in
/// the second, so `read`'s row and `call`'s call of it change. The two
/// builds share one store and must not share that code.
#[test]
fn an_edit_that_changes_an_inferred_row_misses_its_callers_code() {
    let store = MemoryStore::default();
    let program = |inner: &str| {
        format!(
            "{TAGS}
fn inner() -> string:
    {inner}

fn read() -> string:
    inner()

fn call() -> string $ Tag + Log:
    read()

pub fn main() -> void $ Console:
    $.with(Tag=Named {{ label: \"t\" }}, Log=Named {{ label: \"l\" }}):
        println(call())
"
        )
    };
    let first = program("$.use(Tag).name()");
    assert_eq!(output_in(&store, "edit-first", &first), "t\n");
    let second = program("$.use(Tag).name() + $.use(Log).tag()");
    assert_eq!(output_in(&store, "edit-second", &second), "tl\n");
    assert_eq!(output_in(&store, "edit-again", &first), "t\n");
}
