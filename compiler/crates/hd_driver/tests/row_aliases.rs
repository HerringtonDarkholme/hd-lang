//! Row aliases and context types in headers (resolution-and-interfaces.md,
//! "Row aliases and context types in interfaces"; spec 11 "Row Aliases"
//! and "Reusable Contexts"): every alias use expands first
//! (`req.row.alias.expand.first`), recursively and in any declaration
//! order, so an interface holds only expanded rows; a cycle is
//! `alias-cycle`, reported once; `$.Context[...]` takes its row expanded;
//! two spellings of one row give one interface; and a missing key names
//! the row as written and its expanded keys.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn sources(files: &[(&str, &str)]) -> MemorySources {
    let mut s = MemorySources::default();
    for (path, text) in files {
        s.insert(path, text);
    }
    s
}

fn run(store: &MemoryStore, src: &MemorySources, goal: &Goal) -> Output {
    let host = Host {
        render_tir: &[],
        sources: src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", goal)
}

fn check(files: &[(&str, &str)]) -> Output {
    run(&MemoryStore::default(), &sources(files), &Goal::Analyze)
}

fn codes(out: &Output) -> Vec<Code> {
    out.diags.code.clone()
}

/// The standard output of a one-file program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let goal = Goal::Program {
        entry: "main".into(),
    };
    let out = run(
        &MemoryStore::default(),
        &sources(&[("main.hd", main)]),
        &goal,
    );
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("row-alias-{name}.wasm"));
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

/// A header row names a row alias declared after it; the alias stands for
/// its keys, so a caller that spells the keys out calls it, and the
/// providers reach the body (`req.row.alias.expand`).
#[test]
fn a_row_alias_in_a_header_expands_to_its_keys() {
    let main = "\
fn describe() -> string $ AppRow + Clock:
    \"${$.use(Db).name()} ${$.use(Clock).now()}\"

fn spelled() -> string $ Clock + Db:
    describe()

type AppRow = $ Db

trait Db:
    fn name(self) -> string

trait Clock:
    fn now(self) -> i32

data MemoryDb: pass

impl Db for MemoryDb:
    fn name(self) -> string: \"db\"

data FixedClock: pass

impl Clock for FixedClock:
    fn now(self) -> i32: 3

pub fn main() -> void $ Console:
    $.with(Db=MemoryDb {}, Clock=FixedClock {}):
        println(spelled())
";
    assert_eq!(output_of("expand", main), "db 3\n");
}

/// Aliases nest, in any declaration order and across the modules of a
/// folder, and flatten into one set; a generic row alias extends a row
/// parameter (`req.row.alias.nested`, `req.row.alias.generic.use`).
#[test]
fn nested_and_generic_aliases_flatten_into_one_row() {
    let keys = "\
pub trait Db

pub trait Cache

pub trait Log

pub type Web = $ WebRow

pub type WebRow = $ AppRow + Log

pub type AppRow = $ Db + Cache

pub type WithLog[$R] = $ R + Log
";
    let main = "\
use pkg.keys.{Db, Cache, Log, Web, WithLog}

fn serve() -> void $ Web + Db:
    pass

fn run() -> void $ Log + Cache + Db:
    serve()

fn logged[$R](job: fn() -> void $ R) -> void $ WithLog[$ R]:
    job()

fn quiet() -> void $ Db:
    pass

fn outer() -> void $ Db + Log:
    logged(quiet)
";
    let out = check(&[("keys.hd", keys), ("main.hd", main)]);
    assert!(out.diags.is_empty(), "{}", out.render());
    // Dropping a key the alias brings in is a missing requirement.
    let short = main.replace(
        "fn run() -> void $ Log + Cache + Db:",
        "fn run() -> void $ Log + Db:",
    );
    let out = check(&[("keys.hd", keys), ("main.hd", &short)]);
    assert_eq!(
        codes(&out),
        vec![Code::MissingRequirement],
        "{}",
        out.render()
    );
}

/// Three aliases that expand to one another, declared out of order across
/// two modules, are one cycle: one `alias-cycle`, on the cycle's first
/// declaration in the source (`types.alias.cycle.reported`), and no
/// follow-on error where the aliases are used.
#[test]
fn an_alias_cycle_is_reported_once_on_its_first_declaration() {
    let a = "\
use pkg.b.{Back, Db}

pub type Front = $ Back + Db

fn uses() -> void $ Front:
    pass
";
    let b = "\
use pkg.a.Front

pub trait Db

pub trait Cache

pub type Back = $ Middle + Cache

pub type Middle = $ Front
";
    let out = check(&[("a.hd", a), ("b.hd", b)]);
    assert_eq!(codes(&out), vec![Code::AliasCycle], "{}", out.render());
    let rendered = out.render();
    assert!(rendered.contains("a.hd:"), "{rendered}");
    assert!(rendered.contains("`Front` expands to itself"), "{rendered}");
}

/// `$.Context[...]` in a header takes its row expanded, so two spellings
/// are one type, across a folder's interface too; a row parameter in its
/// row is `row-parameter-in-context` (`req.context.dollar`,
/// `req.row.param.context`).
#[test]
fn a_context_type_in_a_header_takes_its_row_expanded() {
    let lib = "\
pub trait Db

pub trait Cache

pub type AppRow = $ Db + Cache

pub fn pass_on(ctx: $.Context[$ AppRow]) -> $.Context[$ Cache + Db]:
    ctx
";
    let main = "\
use pkg.lib.ctx.{AppRow, Db, Cache, pass_on}

fn again(ctx: $.Context[$ Db + Cache]) -> $.Context[$ AppRow]:
    pass_on(ctx)
";
    let out = check(&[("lib/ctx.hd", lib), ("main.hd", main)]);
    assert!(out.diags.is_empty(), "{}", out.render());
    let other = main.replace("-> $.Context[$ AppRow]:", "-> $.Context[$ Db]:");
    let out = check(&[("lib/ctx.hd", lib), ("main.hd", &other)]);
    assert_eq!(codes(&out), vec![Code::TypeMismatch], "{}", out.render());
    let param = "\
trait Db

fn run[$R](ctx: $.Context[$ R + Db]) -> void:
    pass
";
    let out = check(&[("main.hd", param)]);
    assert_eq!(
        codes(&out),
        vec![Code::RowParameterInContext],
        "{}",
        out.render()
    );
}

/// A header row written through an alias and the same row spelled out
/// give one interface (its rows hash by sorted content), so respelling it
/// leaves the dependent module's check entry valid.
#[test]
fn two_spellings_of_one_row_share_the_interface() {
    let geo = "\
pub trait Db:
    fn id(self) -> i32

pub trait Cache

pub type AppRow = $ Db + Cache

pub fn load() -> i32 $ AppRow:
    $.use(Db).id()
";
    let main = "\
use pkg.geo.shapes.{Db, Cache, load}

data Store: pass

impl Db for Store:
    fn id(self) -> i32: 7

impl Cache for Store

pub fn main() -> void $ Console:
    $.with(Db=Store {}, Cache=Store {}):
        println(load())
";
    let goal = Goal::Program {
        entry: "main".into(),
    };
    let store = MemoryStore::default();
    let cold = run(
        &store,
        &sources(&[("geo/shapes.hd", geo), ("main.hd", main)]),
        &goal,
    );
    assert!(cold.diags.is_empty(), "{}", cold.render());
    let spelled = geo.replace(
        "fn load() -> i32 $ AppRow:",
        "fn load() -> i32 $ Cache + Db:",
    );
    let warm = run(
        &store,
        &sources(&[("geo/shapes.hd", &spelled), ("main.hd", main)]),
        &goal,
    );
    assert!(warm.diags.is_empty(), "{}", warm.render());
    let c = &warm.counters;
    assert_eq!(
        c.deep_hashes["app.geo"], cold.counters.deep_hashes["app.geo"],
        "one row, one interface"
    );
    assert_eq!(c.modules_checked, vec!["app.geo.shapes".to_owned()]);
    assert_eq!(warm.wasm, cold.wasm);
}

/// A missing key under a row written with an alias names the key, the row
/// as written and its expanded keys (`req.row.alias.diagnostics`,
/// `req.row.alias.diagnostics.expanded`).
#[test]
fn a_missing_key_names_the_row_as_written_and_its_expansion() {
    let main = "\
trait Db

trait Cache

trait Clock

trait Metrics

trait Auth

type AppRow = $ Db + Cache

type WebRow = $ AppRow + Clock

fn respond() -> void $ Metrics:
    pass

fn get_order() -> void $ WebRow + Auth:
    respond()
";
    let out = check(&[("main.hd", main)]);
    assert_eq!(
        codes(&out),
        vec![Code::MissingRequirement],
        "{}",
        out.render()
    );
    let rendered = out.render();
    assert!(
        rendered.contains(
            "this needs `$ Metrics`, which the enclosing function's row does not name; `$ WebRow + Auth` is `$ Auth + Cache + Clock + Db`"
        ),
        "{rendered}"
    );
}
