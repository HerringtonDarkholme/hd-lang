//! Exit tests of the walking skeleton: programs run on V8 through Node,
//! incremental behavior through task and cache counters, deterministic bytes.

use std::path::PathBuf;

use hd_driver::node::run_wasm;
use hd_driver::{A1Rule, MemStore, Options, RunResult, SourceFile, run, run_with};

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

const HELLO: &str = "\
fn main():
    println(42)
";

const ARITH: &str = "\
fn fib(n: i32) -> i32:
    a := +0
    b := +1
    i := +0
    while i < n:
        t := a + b
        a = b
        b = t
        i = i + 1
    return a

fn classify(x: i32) -> i32:
    if x < 0:
        return -1
    else if x == 0:
        return 0
    else:
        return 1

fn main():
    println(1 + 2 * 3)
    println((1 + 2) * 3)
    println(10 - 4 - 3)
    println(fib(10))
    println(classify(-5))
    println(classify(0))
    println(classify(7))
    println(17 % 5 + 100 / 7)
    println(-(2 + 3) * 2)
";

const DATA_MAIN: &str = "\
use pkg.geo.shapes.{Point, make, manhattan}

fn main():
    p := make(3, -4)
    println(p.x)
    println(p.y)
    println(manhattan(p))
    q := Point { y: 10, x: 20 }
    println(q.x + q.y)
";

const GENERIC_MAIN: &str = "\
use pkg.geo.shapes.{Point, make, first}

fn main():
    println(first(1, 2))
    p := first(make(5, 6), make(7, 8))
    println(p.y)
    println(first(p, make(0, 0)).x)
";

const TRAIT_MAIN: &str = "\
use pkg.geo.shapes.{Point, Shape, make, first}

fn total[T < Shape](s: T) -> i32:
    return s.area() + 1

fn relay[T < Shape](a: T, b: T) -> i32:
    return total(first(b, a))

fn main():
    p := make(3, 4)
    println(p.area())
    println(total(p))
    println(total(make(10, 10)))
    println(relay(make(1, 1), make(2, 3)))
";

fn program(main: &str, geo: Option<&str>) -> Vec<SourceFile> {
    let mut v = vec![SourceFile { path: "app/main.hd".into(), text: main.into() }];
    if let Some(g) = geo {
        v.push(SourceFile { path: "geo/shapes.hd".into(), text: g.into() });
    }
    v
}

fn build(store: &mut MemStore, files: &[SourceFile]) -> RunResult {
    let r = run(store, files, "pkg.app.main");
    assert!(r.diagnostics.is_empty(), "diagnostics: {:#?}", r.diagnostics);
    r
}

fn run_node(name: &str, wasm: &[u8]) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let path = dir.join(format!("skeleton-{name}.wasm"));
    let out = run_wasm(wasm, &path).expect("run node");
    assert!(
        out.status.success(),
        "node failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    eprintln!("{name}: {}", String::from_utf8_lossy(&out.stderr).trim());
    String::from_utf8(out.stdout).expect("utf8")
}

fn end_to_end(name: &str, files: &[SourceFile], expected: &str) {
    let mut store = MemStore::default();
    let r = build(&mut store, files);
    let wasm = r.wasm.expect("wasm");
    eprintln!("{name}: {} bytes; stage times {:?}", wasm.len(), r.counters.stage_time);
    assert_eq!(run_node(name, &wasm), expected);
    // Byte-identical across two independent runs (fresh store, fresh IDs).
    let mut store2 = MemStore::default();
    let again = build(&mut store2, files).wasm.expect("wasm");
    assert_eq!(wasm, again, "Wasm bytes differ across runs");
}

#[test]
fn hello() {
    end_to_end("hello", &program(HELLO, None), "42\n");
}

#[test]
fn arithmetic_and_control() {
    end_to_end("arith", &program(ARITH, None), "7\n9\n3\n55\n-1\n0\n1\n16\n-10\n");
}

#[test]
fn data_across_folders() {
    end_to_end("data", &program(DATA_MAIN, Some(GEO)), "3\n-4\n7\n30\n");
}

#[test]
fn generic_first() {
    end_to_end("generic", &program(GENERIC_MAIN, Some(GEO)), "1\n6\n5\n");
}

#[test]
fn trait_through_bound() {
    end_to_end("trait", &program(TRAIT_MAIN, Some(GEO)), "12\n13\n101\n7\n");
}

#[test]
fn incremental() {
    let mut store = MemStore::default();
    let base = program(DATA_MAIN, Some(GEO));
    let cold = build(&mut store, &base);
    assert_eq!(cold.counters.modules_checked.len(), 2);
    let cold_wasm = cold.wasm.expect("wasm");

    // Warm, no edit: every boundary hits.
    let warm = build(&mut store, &base);
    assert!(warm.counters.modules_checked.is_empty());
    assert_eq!(warm.counters.hit("link"), 1);
    assert!(warm.counters.tir_decoded.is_empty(), "a prog_key hit decodes no TIR (SK-N16)");
    assert_eq!(warm.counters.ran("Parse"), 0, "a check hit parses nothing (SK-4)");
    assert_eq!(warm.wasm.as_deref(), Some(cold_wasm.as_slice()));

    // 1. Private body edit in B (geo): only B's module is rechecked.
    let edited = GEO.replace("        return -v\n", "        return 0 - v\n");
    let r = build(&mut store, &program(DATA_MAIN, Some(&edited)));
    let c = &r.counters;
    eprintln!("private body edit: {c:#?}");
    assert_eq!(c.modules_checked, vec!["pkg.geo.shapes".to_owned()]);
    assert!(c.ifaces_built.is_empty(), "interface rebuilt: {:?}", c.ifaces_built);
    assert_eq!(c.hit("check"), 1, "A's check entry reused");
    assert_eq!(c.miss("link"), 1, "TIR changed, so the program relinks");
    assert!(c.hit("code") >= 1, "unchanged instances reuse their code");
    assert_eq!(c.emitted, 1, "only the edited function is re-emitted");
    assert_eq!(run_node("inc-private", &r.wasm.expect("wasm")), "3\n-4\n7\n30\n");

    // 2. Comment-only edit in B.
    let commented = GEO.replace("fn abs(v: i32) -> i32:\n", "# absolute value\nfn abs(v: i32) -> i32:\n    # negate when below zero\n");
    let r = build(&mut store, &program(DATA_MAIN, Some(&commented)));
    let c = &r.counters;
    eprintln!("comment edit: {c:#?}");
    assert!(c.ifaces_built.is_empty());
    assert_eq!(c.hit("check"), 1, "A reused");
    // check_key holds source_hash(m), so the commented module is rechecked;
    // its TIR content is equal, so the program key hits.
    assert_eq!(c.modules_checked, vec!["pkg.geo.shapes".to_owned()]);
    assert_eq!(c.hit("link"), 1, "prog_key hits after a comment edit");
    assert_eq!(c.emitted, 0);
    assert_eq!(r.wasm.as_deref(), Some(cold_wasm.as_slice()));

    // 3. Public signature edit in B: B's interface and deep hash change; A rechecks.
    let deep_before = cold.counters.deep_hashes["pkg.geo"];
    let sig = GEO.replace("pub fn manhattan(p: Point) -> i32:", "pub fn manhattan(p: Point, unused: bool) -> i32:");
    let main = DATA_MAIN.replace("manhattan(p)", "manhattan(p, true)");
    let r = build(&mut store, &program(&main, Some(&sig)));
    let c = &r.counters;
    eprintln!("signature edit: {c:#?}");
    // iface_key(A) holds B's deep hash, so A's interface is rebuilt too; A
    // exports nothing that mentions B, so A's deep hash stays the same.
    assert_eq!(c.ifaces_built, vec!["pkg.geo".to_owned(), "pkg.app".to_owned()]);
    assert_ne!(c.deep_hashes["pkg.geo"], deep_before);
    assert_eq!(c.deep_hashes["pkg.app"], cold.counters.deep_hashes["pkg.app"]);
    let mut checked = c.modules_checked.clone();
    checked.sort();
    assert_eq!(checked, vec!["pkg.app.main".to_owned(), "pkg.geo.shapes".to_owned()]);

    // 3b. The same signature edit with A's source untouched still rechecks A
    // (its key holds B's deep hash); A then reports the arity error.
    let r = run(&mut store, &program(DATA_MAIN, Some(&sig)), "pkg.app.main");
    assert!(r.counters.modules_checked.contains(&"pkg.app.main".to_owned()));
    assert!(r.diagnostics.iter().any(|d| d.contains("arity")), "{:?}", r.diagnostics);
}

/// SK-2: the A1 summary as first written (exact only on a trait call in the
/// body) fails at collection when a bounded parameter is passed on to a
/// bounded callee; the bounded rule (a bound makes a parameter exact) runs.
#[test]
fn a1_rule_bounded_parameter_is_exact() {
    let files = program(TRAIT_MAIN, Some(GEO));
    let mut store = MemStore::default();
    let literal = run_with(&mut store, &files, "pkg.app.main", Options { a1_rule: A1Rule::Literal });
    assert!(literal.wasm.is_none());
    assert!(
        literal.diagnostics.iter().any(|d| d.contains("select: no impl")),
        "{:?}",
        literal.diagnostics
    );
    // Same store: the rule is part of the toolchain key, so nothing is reused.
    let bounded = build(&mut store, &files);
    assert_eq!(bounded.counters.hit("check"), 0);
    assert_eq!(run_node("a1-bounded", &bounded.wasm.expect("wasm")), "12\n13\n101\n7\n");
}

/// SK-3: a callee's representation summary is part of its callers' code
/// keys. Bounding `first` turns `first[REF]` into `first[Point]`; `main`'s
/// TIR is unchanged, but its code must be re-emitted to call the new symbol.
#[test]
fn callee_representation_change_re_emits_caller() {
    let main = "\
use pkg.geo.shapes.{Point, make, first}

fn main():
    p := first(make(5, 6), make(7, 8))
    println(p.y)
";
    let mut store = MemStore::default();
    let cold = build(&mut store, &program(main, Some(GEO)));
    assert_eq!(run_node("rep-before", &cold.wasm.expect("wasm")), "6\n");
    let bounded =
        GEO.replace("pub fn first[T](a: T, b: T) -> T:", "pub fn first[T < Shape](a: T, b: T) -> T:");
    let r = build(&mut store, &program(main, Some(&bounded)));
    let c = &r.counters;
    // `make` hits; `first[Point]` is a new instance; `main` misses because
    // its callee's summary changed, though its own TIR did not.
    assert_eq!(c.hit("code"), 1, "{c:#?}");
    assert_eq!(c.emitted, 2, "{c:#?}");
    assert_eq!(run_node("rep-after", &r.wasm.expect("wasm")), "6\n");
}
