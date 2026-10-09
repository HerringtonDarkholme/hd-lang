//! Incremental behavior of the one driver through task and cache counters,
//! on the serial and the pool executor; and the build-stopping rule.

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
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

pub fn main() -> void $ Console:
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
        render_tir: &[],
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
    // The program's two modules, and the std modules collection crosses into.
    let own = |c: &hd_driver::Counters| {
        c.modules_checked
            .iter()
            .filter(|m| m.starts_with("demo."))
            .count()
    };
    assert_eq!(own(&cold.counters), 2);
    assert!(cold.counters.modules_checked.len() > 2);
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
    // The package-wide graph parts are computed cold and read warm.
    assert_eq!(
        cold.counters.computed("InitOrder"),
        cold.counters.ran("InitOrder")
    );
    assert_eq!(cold.counters.computed("Coherence"), 1);
    assert_eq!(
        warm.counters.ran("InitOrder"),
        cold.counters.ran("InitOrder")
    );
    assert!(
        warm.counters.parts_computed.is_empty(),
        "a warm run redoes neither InitOrder nor Coherence: {:?}",
        warm.counters.parts_computed
    );

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
    assert!(c.hit("check") >= 1, "main's and std's check entries reused");
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
    assert!(
        c.parts_computed.is_empty(),
        "a comment changes no module fact or interface: {:?}",
        c.parts_computed
    );
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
    // An interface changed, so Coherence reruns; no module's init facts
    // did, so every InitOrder part is read.
    assert_eq!(c.computed("Coherence"), 1);
    assert_eq!(c.computed("InitOrder"), 0);

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
        (0..r.diags.len()).any(|i| r.diags.code[i].as_str() == "argument-count"),
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
        "pub fn main() -> void $ Console:\n    a := b := 1\n    println(a)\n",
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
        "pub fn main() -> void $ Console:\n    println(1)\n    tests:\n        x := 1\n",
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
        "pub fn main() -> void $ Console:\n    println(missing)\n    x := 1\n    x = true\n",
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

/// Interface keys come from API text, which ignores comments and blank
/// lines, so a comment above an item hits the cached interface. Findings
/// from the cached interface still land on the shifted line, because item
/// positions are declaration-relative (resolution-and-interfaces.md §4.10).
const FINDINGS: &str = "\
trait Marker

data Box[T]:
    pub value: T

impl[T] Marker for Box[T]
impl Marker for Box[i32]

data Score:
    value: i32

impl PartialOrd for Score:
    fn partial_cmp(self, other: Score) -> Ordering?:
        .Some(Ordering.Equal)

pub data Needs[T < Hash]:
    pub value: T

pub data Holder[T]:
    ok: i32
    pub bad: Needs[T]

pub fn lookup[T](counts: Needs[T]) -> i32:
    0

impl Display for i32:
    fn to_text(self) -> string:
        \"\"
";

/// The 1-based line of each diagnostic with `code`, for the one-file program.
fn lines_of(r: &Output, text: &str, code: &str) -> Vec<usize> {
    let mut lines: Vec<usize> = (0..r.diags.len())
        .filter(|i| r.diags.code[*i].as_str() == code)
        .map(|i| {
            let lo = r.diags.primary[i].lo as usize;
            text[..lo].matches('\n').count() + 1
        })
        .collect();
    lines.sort_unstable();
    lines
}

fn one_file(text: &str) -> MemorySources {
    let mut s = MemorySources::default();
    s.insert("main.hd", text);
    s
}

fn line_of(text: &str, needle: &str) -> usize {
    text.lines()
        .position(|l| l.contains(needle))
        .expect("needle")
        + 1
}

fn assert_findings(r: &Output, text: &str) {
    let want = |code: &str, needle: &str| {
        assert_eq!(
            lines_of(r, text, code),
            vec![line_of(text, needle)],
            "{code}\n{}",
            r.render()
        );
    };
    want("orphan-impl", "impl Display for i32");
    // The orphan `Display` impl is reported once, as `orphan-impl` only;
    // coherence skips it. The `Marker` overlap is still reported.
    assert_eq!(
        lines_of(r, text, "overlapping-impl"),
        vec![line_of(text, "impl Marker for Box[i32]")],
        "{}",
        r.render()
    );
    want(
        "missing-supertrait-implementation",
        "impl PartialOrd for Score",
    );
    // A field type is reported at the field; a parameter at the signature.
    let bounds = lines_of(r, text, "unsatisfied-trait-bound");
    assert_eq!(
        bounds,
        vec![line_of(text, "bad: Needs"), line_of(text, "fn lookup")],
        "{}",
        r.render()
    );
}

#[test]
fn header_findings_name_their_line_and_survive_a_cached_interface() {
    let store = MemoryStore::default();
    let executor = Executor::Serial(SerialOrder::Priority);
    let cold = run(&store, &one_file(FINDINGS), executor);
    assert_findings(&cold, FINDINGS);

    let shifted = FINDINGS
        .replace("trait Marker", "# the marker\n\ntrait Marker")
        .replace("data Score:", "# scores\ndata Score:")
        .replace("impl Display", "# orphan\n\nimpl Display")
        .replace(
            "pub data Holder[T]:",
            "# holds a map\n\n\npub data Holder[T]:",
        );
    let warm = run(&store, &one_file(&shifted), executor);
    assert!(
        warm.counters.ifaces_built.is_empty(),
        "interface rebuilt: {:?}",
        warm.counters.ifaces_built
    );
    assert_findings(&warm, &shifted);
}

#[test]
fn interface_keys_ignore_a_comment_above_an_item() {
    let store = MemoryStore::default();
    let executor = Executor::Serial(SerialOrder::Priority);
    let base = program(DATA_MAIN, GEO);
    let cold = ok(&store, &base, executor);
    let commented = GEO
        .replace("pub trait Shape:", "# a shape\n\n\npub trait Shape:")
        .replace("pub data Point:", "# a point\npub data Point:");
    let r = ok(&store, &program(DATA_MAIN, &commented), executor);
    let c = &r.counters;
    assert!(c.ifaces_built.is_empty(), "{:?}", c.ifaces_built);
    assert_eq!(
        c.deep_hashes["demo.geo"],
        cold.counters.deep_hashes["demo.geo"]
    );
    assert_eq!(
        c.iface_blobs["demo.geo"],
        cold.counters.iface_blobs["demo.geo"]
    );

    // The same keys on a cold store: the interface bytes and hashes do not
    // depend on where an item sits.
    let fresh = ok(
        &MemoryStore::default(),
        &program(DATA_MAIN, &commented),
        executor,
    );
    assert_eq!(
        fresh.counters.deep_hashes["demo.geo"],
        cold.counters.deep_hashes["demo.geo"]
    );
    assert_eq!(
        fresh.counters.iface_blobs["demo.geo"],
        cold.counters.iface_blobs["demo.geo"]
    );
}
