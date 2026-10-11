//! Row unions at the least-common-type sites (`types.lct.row-union-every-site`,
//! `req.row.union.literal`, `req.row.union.sites`, #48): function values
//! with different rows that meet at a join take the union of their rows,
//! so a call through the joined value passes every provider any of them
//! needs, and each function looks up only its own keys. With an expected
//! type there is no union: each value is checked against it
//! (`req.row.union.literal.expected`). Programs build through the driver
//! and run on V8 (`host/run.mjs`), as the conformance runner does.

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
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("row-union-{name}.wasm"));
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

/// Three keys and a handler per key, each reading only its own provider.
const HANDLERS: &str = "\
trait Db:
    fn name(self) -> string

trait Clock:
    fn now(self) -> string

trait Log:
    fn tag(self) -> string

data MemoryDb: pass

impl Db for MemoryDb:
    fn name(self) -> string: \"db\"

data FixedClock: pass

impl Clock for FixedClock:
    fn now(self) -> string: \"noon\"

data Tagged: pass

impl Log for Tagged:
    fn tag(self) -> string: \"log\"

fn health() -> string $ Clock:
    $.use(Clock).now()

fn orders() -> string $ Db:
    $.use(Db).name()

fn audit() -> string $ Log:
    $.use(Log).tag()
";

/// Runs `body` (the lines of a `$ Db + Clock + Log` function returning a
/// string) under all three providers and returns what it printed.
fn run_with_providers(name: &str, decls: &str, body: &str) -> String {
    let main = format!(
        "{HANDLERS}
{decls}
fn serve() -> string $ Db + Clock + Log:
{body}

pub fn main() -> void $ Console:
    $.with(Db = MemoryDb {{}}, Clock = FixedClock {{}}, Log = Tagged {{}}):
        println(serve())
"
    );
    output_of(name, &main)
}

/// The two branches of a value `if` join to the union row, so the one call
/// through the chosen handler passes both providers
/// (`req.row.union.sites`).
#[test]
fn an_if_between_two_rows_calls_either() {
    let decls = "\
fn pick(admin: bool) -> string $ Db + Clock:
    handler := if admin: orders else: health
    handler()
";
    let out = run_with_providers("if", decls, "    pick(true) + \",\" + pick(false)");
    assert_eq!(out, "db,noon\n");
}

/// Three arms of a value `match` join to the union of three rows; an
/// `else if` chain joins the same way.
#[test]
fn a_match_over_three_rows_calls_each() {
    let decls = "\
fn by_match(n: i32) -> string $ Db + Clock + Log:
    handler := match n:
        0 => orders
        1 => health
        _ => audit
    handler()

fn by_chain(n: i32) -> string $ Db + Clock + Log:
    handler := if n == 0: audit else if n == 1: health else: orders
    handler()
";
    let out = run_with_providers(
        "match",
        decls,
        "    by_match(0) + by_match(1) + by_match(2) + \";\" + by_chain(0) + by_chain(1) + by_chain(2)",
    );
    assert_eq!(out, "dbnoonlog;lognoondb\n");
}

/// A closure's inferred result joins its `return` operands and its final
/// value (`req.row.union.sites.type`).
#[test]
fn a_closure_result_joins_its_returns() {
    let decls = "\
fn by_closure(admin: bool) -> string $ Db + Clock:
    pick := fn(flag: bool):
        if flag:
            return orders
        health
    handler := pick(admin)
    handler()
";
    let out = run_with_providers("closure", decls, "    by_closure(true) + by_closure(false)");
    assert_eq!(out, "dbnoon\n");
}

/// A list literal with no expected type takes the union of its elements'
/// rows, a nested `if` element included, and so does a map literal's
/// values (`req.row.union.literal`, `req.row.union.literal.map`).
#[test]
fn list_and_map_literals_join_their_elements() {
    let decls = "\
fn from_list(admin: bool) -> string $ Db + Clock + Log:
    handlers := [health, if admin: orders else: audit]
    let text = \"\"
    for handler in handlers:
        text = text + handler() + \";\"
    text

fn from_map() -> string $ Db + Clock:
    routes := {\"orders\": orders, \"health\": health}
    let text = \"\"
    for (name, route) in routes:
        text = text + name + \"=\" + route() + \";\"
    text
";
    let out = run_with_providers(
        "list",
        decls,
        "    from_list(true) + from_list(false) + from_map()",
    );
    assert_eq!(out, "noon;db;noon;log;orders=db;health=noon;\n");
}

/// A generic parameter not yet solved is no expected type: the `if` and
/// the list still join to the union, and the call solves the parameter
/// from the joined type.
#[test]
fn a_generic_argument_joins_before_it_solves() {
    let decls = "\
fn keep[F](f: F) -> F:
    f

fn second[T](items: List[T]) -> T:
    items[1]

fn generic(admin: bool) -> string $ Db + Clock:
    chosen := keep(if admin: orders else: health)
    listed := second([orders, health])
    chosen() + listed()
";
    let out = run_with_providers(
        "generic",
        decls,
        "    generic(true) + \",\" + generic(false)",
    );
    assert_eq!(out, "dbnoon,noonnoon\n");
}

/// With an expected element type there is no union: each element fits the
/// expected row by subsumption, and a call through the list passes the
/// expected row's providers (`req.row.union.literal.expected`).
#[test]
fn an_expected_element_type_takes_no_union() {
    let decls = "\
fn declared() -> string $ Db + Clock + Log:
    let handlers: List[fn() -> string $ Db + Clock + Log] = [health, orders]
    let text = \"\"
    for handler in handlers:
        text = text + handler()
    text
";
    let out = run_with_providers("expected", decls, "    declared()");
    assert_eq!(out, "noondb\n");
}

/// A key outside the expected row is `type-mismatch`
/// (`req.row.subsume.missing`), even though the literal alone would join.
#[test]
fn a_key_outside_the_expected_row_is_a_mismatch() {
    let main = format!(
        "{HANDLERS}
fn declared() -> void $ Db + Clock:
    let handlers: List[fn() -> string $ Clock] = [health, orders]
    for handler in handlers:
        _ := handler()

pub fn main() -> void $ Console:
    println(1)
"
    );
    let out = built(&main);
    assert_eq!(out.diags.code, vec![Code::TypeMismatch], "{}", out.render());
}

/// A call through a joined value needs every key of the union row
/// (`req.row.union.literal.type`).
#[test]
fn a_call_through_the_union_needs_every_key() {
    let main = format!(
        "{HANDLERS}
fn serve(admin: bool) -> string $ Clock:
    handler := if admin: health else: orders
    handler()

pub fn main() -> void $ Console:
    println(1)
"
    );
    let out = built(&main);
    assert_eq!(
        out.diags.code,
        vec![Code::MissingRequirement],
        "{}",
        out.render()
    );
}
