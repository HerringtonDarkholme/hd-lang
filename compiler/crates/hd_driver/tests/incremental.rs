//! Incremental behavior of the one driver through task and cache counters,
//! on the serial and the pool executor; and the build-stopping rule.

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build, messages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const GEO: &str = "\
pub data Point:
    pub x: i32
    pub y: i32

pub trait Shape:
    fn area(self) -> i32

impl Shape for Point:
    fn area(self) -> i32:
        self.x * self.y

pub fn make(x: i32, y: i32) -> Point:
    Point { x: x, y: y }

pub fn manhattan(p: Point) -> i32:
    return abs(p.x) + abs(p.y)

pub fn first[T](a: T, b: T) -> T:
    return a

fn abs(v: i32) -> i32:
    if v < 0:
        return -v
    return v
";

const DATA_MAIN: &str = "\
use pkg.geo.shapes.{Point, make, manhattan}

fn main() -> void $ Console:
    p := make(3, -4)
    println(p.x)
    println(p.y)
    println(manhattan(p))
    q := Point { y: 10, x: 20 }
    println(q.x + q.y)
";

fn program(main: &str, geo: &str) -> MemorySources {
    let mut s = MemorySources::default();
    s.insert("app/main.hd", main);
    s.insert("geo/shapes.hd", geo);
    s
}

fn run(store: &MemoryStore, src: &MemorySources, executor: Executor) -> Output {
    let host = Host {
        sources: src,
        store,
        clock: &NoClock,
        executor,
    };
    build(
        &host,
        "demo",
        &Goal::Program {
            entry: "app.main".into(),
        },
    )
}

fn ok(store: &MemoryStore, src: &MemorySources, executor: Executor) -> Output {
    let r = run(store, src, executor);
    assert!(r.diags.is_empty(), "{}", r.render());
    r
}

fn incremental(executor: Executor) {
    let store = MemoryStore::default();
    let base = program(DATA_MAIN, GEO);
    let cold = ok(&store, &base, executor);
    assert_eq!(cold.counters.modules_checked.len(), 2);
    let cold_wasm = cold.wasm.clone().expect("wasm");

    // Warm, no edit: every boundary hits.
    let warm = ok(&store, &base, executor);
    assert!(warm.counters.modules_checked.is_empty());
    assert_eq!(warm.counters.hit("link"), 1);
    assert!(
        warm.counters.tir_decoded.is_empty(),
        "a prog_key hit decodes no TIR (SK-N16)"
    );
    assert_eq!(
        warm.counters.ran("Parse"),
        0,
        "a check hit parses nothing (SK-4)"
    );
    assert_eq!(warm.wasm.as_deref(), Some(cold_wasm.as_slice()));

    // 1. Private body edit in geo: only its module is rechecked.
    let edited = GEO.replace("        return -v\n", "        return 0 - v\n");
    let r = ok(&store, &program(DATA_MAIN, &edited), executor);
    let c = &r.counters;
    assert_eq!(c.modules_checked, vec!["demo.geo.shapes".to_owned()]);
    assert!(
        c.ifaces_built.is_empty(),
        "interface rebuilt: {:?}",
        c.ifaces_built
    );
    assert_eq!(c.hit("check"), 1, "main's check entry reused");
    assert_eq!(c.miss("link"), 1, "TIR changed, so the program relinks");
    assert!(c.hit("code") >= 1, "unchanged instances reuse their code");
    assert_eq!(c.emitted, 1, "only the edited function is re-emitted");

    // 2. Comment-only edit: rechecked (source hash), same TIR, link hits.
    let commented = GEO.replace(
        "fn abs(v: i32) -> i32:\n",
        "# absolute value\nfn abs(v: i32) -> i32:\n    # negate when below zero\n",
    );
    let r = ok(&store, &program(DATA_MAIN, &commented), executor);
    let c = &r.counters;
    assert!(c.ifaces_built.is_empty());
    assert_eq!(c.modules_checked, vec!["demo.geo.shapes".to_owned()]);
    assert_eq!(c.hit("link"), 1, "prog_key hits after a comment edit");
    assert_eq!(c.emitted, 0);
    assert_eq!(r.wasm.as_deref(), Some(cold_wasm.as_slice()));

    // 3. Public signature edit: geo's interface and deep hash change; main rechecks.
    let sig = GEO.replace(
        "pub fn manhattan(p: Point) -> i32:",
        "pub fn manhattan(p: Point, unused: bool) -> i32:",
    );
    let main = DATA_MAIN.replace("manhattan(p)", "manhattan(p, true)");
    let r = ok(&store, &program(&main, &sig), executor);
    let c = &r.counters;
    assert!(c.ifaces_built.contains(&"demo.geo".to_owned()));
    assert_ne!(
        c.deep_hashes["demo.geo"],
        cold.counters.deep_hashes["demo.geo"]
    );
    assert_eq!(
        c.modules_checked,
        vec!["demo.app.main".to_owned(), "demo.geo.shapes".to_owned()]
    );

    // 3b. The same edit with main untouched rechecks main, which reports
    // the argument count.
    let r = run(&store, &program(DATA_MAIN, &sig), executor);
    assert!(
        r.counters
            .modules_checked
            .contains(&"demo.app.main".to_owned())
    );
    assert!(r.wasm.is_none());
    assert!(
        messages(&r).iter().any(|d| d.contains("argument-count")),
        "{}",
        r.render()
    );
}

#[test]
fn incremental_serial() {
    incremental(Executor::Serial(SerialOrder::Priority));
}

#[test]
fn incremental_pool() {
    incremental(Executor::Pool(4));
}

/// A stage that answers "not implemented" stops a build with an internal
/// `unsupported` diagnostic.
#[test]
fn not_implemented_stops_a_build() {
    let mut s = MemorySources::default();
    s.insert(
        "main.hd",
        "fn f(a: i32 = 1) -> i32:\n    a\n\nfn main() -> void $ Console:\n    println(f())\n",
    );
    let r = run(
        &MemoryStore::default(),
        &s,
        Executor::Serial(SerialOrder::Fifo),
    );
    assert!(r.wasm.is_none());
    assert!(
        r.diags.code.contains(&hd_diag::Code::Unsupported),
        "{}",
        r.render()
    );
    let mut s = MemorySources::default();
    s.insert(
        "main.hd",
        "fn main() -> void $ Console:\n    println(1)\n    tests:\n        x := 1\n",
    );
    let r = run(
        &MemoryStore::default(),
        &s,
        Executor::Serial(SerialOrder::Fifo),
    );
    assert!(r.wasm.is_none() || r.diags.is_empty());
}

#[test]
fn user_errors_are_coded_diagnostics() {
    let mut s = MemorySources::default();
    s.insert(
        "main.hd",
        "fn main() -> void $ Console:\n    println(missing)\n    x := 1\n    x = true\n",
    );
    let r = run(
        &MemoryStore::default(),
        &s,
        Executor::Serial(SerialOrder::Fifo),
    );
    let codes: Vec<&str> = r.diags.code.iter().map(|c| c.as_str()).collect();
    assert!(codes.contains(&"unknown-name"), "{codes:?}");
    assert!(codes.contains(&"type-mismatch"), "{codes:?}");
}
