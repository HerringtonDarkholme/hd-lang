//! Row aliases and context types in headers (`crates/hd_resolve/DESIGN.md`,
//! "Row aliases and context types in interfaces"; spec 11 "Row Aliases"
//! and "Reusable Contexts"): every alias use expands first
//! (`req.row.alias.expand.first`), recursively and in any declaration
//! order, so an interface holds only expanded rows; a cycle is
//! `alias-cycle`, reported once; `$.Context[...]` takes its row expanded;
//! two spellings of one row give one interface; and a missing key names
//! the row as written and its expanded keys.

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

struct Store: pass

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
