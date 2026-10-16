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
    // One Emit per folder group with a miss (`crates/hd_sched/DESIGN.md`
    // §6.1): app, geo and the std folders reached, not one per instance.
    let emits = cold.counters.ran("Emit");
    assert!(
        emits >= 2 && emits < cold.counters.emitted,
        "{emits} Emit tasks for {} instances",
        cold.counters.emitted
    );
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
    assert_eq!(c.ran("Emit"), 1, "the one miss is one folder group");

    // 2. Comment-only edit: the check key holds the token hash, so the
    // module is only parsed (the parse proof), not rechecked; link hits.
    let commented = GEO.replace(
        "fn abs(v: i32) -> i32:\n",
        "# absolute value\n\nfn abs(v: i32) -> i32:\n    # negate when below zero\n",
    );
    let r = ok(&store, &program(DATA_MAIN, &commented), executor);
    let c = &r.counters;
    assert!(c.ifaces_built.is_empty());
    assert!(c.modules_checked.is_empty(), "{:?}", c.modules_checked);
    assert_eq!(c.miss("check"), 0);
    assert_eq!(c.parsed, vec!["demo.geo.shapes".to_owned()]);
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

/// A comment edit above a warning keeps the `check` entry, and the warning
/// still lands on its shifted line, as a fresh run reports it.
#[test]
fn a_comment_edit_reuses_the_check_and_moves_its_positions() {
    let serial = Executor::Serial(SerialOrder::Priority);
    let geo = format!("{GEO}\nfn noisy() -> i32:\n    unused := 1\n    return 2\n");
    let store = MemoryStore::default();
    let cold = run(&store, &program(DATA_MAIN, &geo), serial);
    assert_eq!(
        lines_of(&cold, &geo, "unused-local-binding"),
        vec![line_of(&geo, "unused := 1")],
        "{}",
        cold.render()
    );
    let commented = geo
        .replace("fn noisy", "# one\n# two\n\nfn noisy")
        .replace("    unused", "    # three\n\n    unused");
    let warm = run(&store, &program(DATA_MAIN, &commented), serial);
    let c = &warm.counters;
    assert!(c.modules_checked.is_empty(), "{:?}", c.modules_checked);
    assert_eq!(
        lines_of(&warm, &commented, "unused-local-binding"),
        vec![line_of(&commented, "unused := 1")],
        "{}",
        warm.render()
    );
    let fresh = run(
        &MemoryStore::default(),
        &program(DATA_MAIN, &commented),
        serial,
    );
    assert_eq!(warm.render(), fresh.render());
    assert_eq!(warm.wasm, fresh.wasm);
}

/// A documentation-only edit, of an item's or the module's `##` lines,
/// changes no interface hash: comments stay out of the API text
/// (`crates/hd_cli/DESIGN.md` §7.7), so no interface is rebuilt and no module that uses
/// the edited one is rechecked. The edited module's own bodies are, since
/// its `check` key keeps documentation lines (their attachment is checked).
#[test]
fn a_doc_edit_keeps_every_interface_hash() {
    let serial = Executor::Serial(SerialOrder::Priority);
    let geo = format!(
        "## Shapes.\n\n{}",
        GEO.replace("pub fn make(", "## Makes a point.\npub fn make(")
    );
    let store = MemoryStore::default();
    let cold = ok(&store, &program(DATA_MAIN, &geo), serial);
    let edited = geo.replace("## Shapes.", "## Points and shapes.").replace(
        "## Makes a point.",
        "## Makes a point from x and y.\n##\n## ```hd\n## p := make(1, 2)\n## ```",
    );
    let warm = ok(&store, &program(DATA_MAIN, &edited), serial);
    let c = &warm.counters;
    assert!(c.ifaces_built.is_empty(), "{:?}", c.ifaces_built);
    assert_eq!(c.modules_checked, ["demo.geo.shapes"]);
    assert_eq!(c.iface_blobs, cold.counters.iface_blobs);
    assert_eq!(c.deep_hashes, cold.counters.deep_hashes);
    assert_eq!(warm.ifaces, cold.ifaces);
}

/// A comment between a documentation comment and its item detaches it: the
/// `check` key ignores comments, so the parse proof finds the error.
#[test]
fn a_comment_that_detaches_a_doc_comment_is_reported() {
    let serial = Executor::Serial(SerialOrder::Priority);
    let geo = GEO.replace("fn abs(", "## The absolute value.\nfn abs(");
    let store = MemoryStore::default();
    ok(&store, &program(DATA_MAIN, &geo), serial);
    let detached = geo.replace("value.\n", "value.\n# note\n");
    let warm = run(&store, &program(DATA_MAIN, &detached), serial);
    let fresh = run(
        &MemoryStore::default(),
        &program(DATA_MAIN, &detached),
        serial,
    );
    assert!(warm.wasm.is_none());
    assert_eq!(
        warm.counters.miss("check"),
        0,
        "the check key ignores comments"
    );
    assert_eq!(
        lines_of(&warm, &detached, "doc-comment-without-target"),
        vec![line_of(&detached, "## The absolute")],
        "{}",
        warm.render()
    );
    assert_eq!(warm.render(), fresh.render());
}

/// A stage that answers "not implemented" stops a build with an internal
/// `unsupported` diagnostic.
#[test]
fn not_implemented_stops_a_build() {
    let mut s = MemorySources::default();
    s.insert(
        "main.hd",
        "pub fn main() -> void $ Console:\n    println(0xFFFFFFFFFFFFFFFFF)\n",
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
/// positions are declaration-relative (`crates/hd_resolve/DESIGN.md` §4.10).
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

/// A user tuple template that walks a tuple with one of two walkers. The
/// strict walker's `member` needs `Show`, the lax one's needs nothing, and
/// both stay declared in either text: the edit under test changes the
/// template's body only, so no interface changes. Which tuples meet `Show`
/// is read from that body, which no interface carries yet, so a header
/// finding about a tuple depends on this text (`crates/hd_check/DESIGN.md` (trait solver) §3.9).
fn show_module(strict: bool) -> String {
    let walker = if strict { "Strict" } else { "Lax" };
    format!(
        "\
use std.function.Tuple
use std.structure.{{Structure, Field, Variant, Walker}}

pub trait Show:
    fn show(self) -> string

impl Show for i64:
    fn show(self) -> string:
        \"$self\"

data Strict:
    out: string

impl[S] Walker[S] for Strict:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F < Show](mut self, h: Field[S, F], value: F) -> Result[void, never]:
        self.out = self.out + value.show() + \";\"
        .Ok(())

data Lax:
    out: string

impl[S] Walker[S] for Lax:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F](mut self, h: Field[S, F], value: F) -> Result[void, never]:
        self.out = self.out + \";\"
        .Ok(())

impl[T < Tuple] Show for T by Structure:
    fn show(self) -> string:
        let mut w = {walker} {{ out: \"\" }}
        _ := Structure::walk(self, w)
        w.out
"
    )
}

/// A table keyed by `Show`, as `Map` is by `Eq & Hash`, and a holder
/// whose field keys one by a tuple: the header finding under test.
const TUPLE_KEYED: &str = "\
use pkg.tmpl.show.{Show}

pub data Table[K < Show, V]:
    pub key: K
    pub value: V

pub data Holder:
    pub table: Table[(i64, string), i32]

pub fn main() -> void $ Console:
    println(1)
";

/// An unrelated module, for edits that must change nothing the tuple
/// template's users read.
const OTHER: &str = "\
pub fn twice(v: i32) -> i32:
    v + v
";

fn tuple_program(strict: bool, other: &str) -> MemorySources {
    let mut s = MemorySources::default();
    s.insert("app/main.hd", TUPLE_KEYED);
    s.insert("tmpl/show.hd", &show_module(strict));
    s.insert("other/util.hd", other);
    s
}

/// The `unsatisfied-trait-bound` lines of the holder's field, or none.
fn bound_lines(r: &Output) -> Vec<usize> {
    lines_of(r, TUPLE_KEYED, "unsatisfied-trait-bound")
}

#[test]
fn a_tuple_template_edit_reaches_the_header_findings_it_decides() {
    let store = MemoryStore::default();
    let executor = Executor::Serial(SerialOrder::Priority);
    let field = line_of(TUPLE_KEYED, "pub table:");

    // The strict walker needs `Show` of `string`, which has none.
    let cold = run(&store, &tuple_program(true, OTHER), executor);
    assert_eq!(bound_lines(&cold), vec![field], "{}", cold.render());
    assert!(cold.counters.computed("HeaderCheck") >= 1);

    // The same text on the warm store reads the finding.
    let warm = run(&store, &tuple_program(true, OTHER), executor);
    assert_eq!(bound_lines(&warm), vec![field], "{}", warm.render());
    assert_eq!(warm.counters.computed("HeaderCheck"), 0);

    // An edit elsewhere keeps the cached finding: no header is redone.
    let other = "pub fn twice(v: i32) -> i32:\n    2 * v\n";
    let unrelated = run(&store, &tuple_program(true, other), executor);
    assert_eq!(bound_lines(&unrelated), vec![field]);
    assert_eq!(
        unrelated.counters.computed("HeaderCheck"),
        0,
        "{:?}",
        unrelated.counters.parts_computed
    );

    // A comment in the template's module changes its tokens in no way.
    let mut commented = tuple_program(true, OTHER);
    commented.insert(
        "tmpl/show.hd",
        &format!("# the walker asks `Show`\n{}", show_module(true)),
    );
    let comment = run(&store, &commented, executor);
    assert_eq!(bound_lines(&comment), vec![field]);
    assert_eq!(comment.counters.computed("HeaderCheck"), 0);

    // Dropping the walker's bound lets every tuple through: the finding
    // goes on the warm run, as on a cold store.
    let loose = run(&store, &tuple_program(false, OTHER), executor);
    assert_eq!(
        bound_lines(&loose),
        Vec::<usize>::new(),
        "{}",
        loose.render()
    );
    assert!(loose.counters.computed("HeaderCheck") >= 1);
    let fresh = run(
        &MemoryStore::default(),
        &tuple_program(false, OTHER),
        executor,
    );
    assert_eq!(loose.render(), fresh.render());

    // And back: the finding returns, as on a cold store.
    let strict = run(&store, &tuple_program(true, OTHER), executor);
    assert_eq!(bound_lines(&strict), vec![field], "{}", strict.render());
    assert_eq!(strict.render(), cold.render());
}

/// The build of `src`, package `fan`, at `level` on `store`.
fn fan_run(store: &MemoryStore, src: &MemorySources, level: hd_driver::OptLevel) -> Output {
    let host = Host {
        render_tir: &[],
        sources: src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = hd_driver::Packages {
        level,
        ..hd_driver::Packages::default()
    };
    let r = hd_driver::build_packages(
        &host,
        "fan",
        &packages,
        &Goal::Program {
            entry: "app.main".into(),
        },
    );
    assert!(r.wasm.is_some(), "{}", r.render());
    r
}

/// Rebuild scope on the fan-out package (#49), at both levels. A body
/// edit rechecks its module alone, in a leaf or in `core.util`, which
/// every leaf imports, and re-emits only the instances whose code reads
/// the edited body: in the optimized pipeline a callee body that stops
/// calling its closure parameter turns off `closure_spec` at every
/// caller, so those callers re-emit too. A signature edit in
/// `core.util` rebuilds its importers' interfaces and rechecks them,
/// plus `app.main`, whose key lists its whole use closure because it can
/// see the impls of every folder there (`crates/hd_cache/DESIGN.md` §5.3). Every
/// incremental build's Wasm equals a clean build's, and its relink
/// decodes only the TIR bodies collection reaches.
#[test]
fn a_body_edit_rechecks_one_module_and_a_signature_edit_its_importers() {
    use hd_driver::OptLevel;
    use hd_driver::bench::{FANOUT_EDITS, fanout_edited, fanout_sources};
    const N: usize = 4;
    let own = |c: &hd_driver::Counters| -> Vec<String> {
        c.modules_checked
            .iter()
            .filter(|m| m.starts_with("fan."))
            .cloned()
            .collect()
    };
    let core = vec!["fan.core.util".to_owned()];
    for level in [OptLevel::Dev, OptLevel::Optimized] {
        let store = MemoryStore::default();
        let base = fanout_sources(N);
        let cold = fan_run(&store, &base, level);
        let mut runs = Vec::new();
        for (label, file, from, to) in FANOUT_EDITS {
            let src = fanout_edited(N, file, from, to);
            let r = fan_run(&store, &src, level);
            let clean = fan_run(&MemoryStore::default(), &src, level);
            assert_eq!(
                r.wasm, clean.wasm,
                "{label}, {level:?}: incremental != clean"
            );
            runs.push(r.counters);
            // Back to the base, so each edit starts from the same store.
            fan_run(&store, &base, level);
        }
        let [leaf, body, closure, sig] = &runs[..] else {
            unreachable!("four edits")
        };
        // Every relink decodes only the bodies collection reaches, not
        // every module's: no std module the program never calls.
        for c in [leaf, body, closure, sig] {
            assert!(c.bodies_decoded > 0, "{level:?}");
            assert!(
                !c.tir_decoded.iter().any(|m| m == "std.json"),
                "{level:?}: {:?}",
                c.tir_decoded
            );
        }
        // (a) and (b): one module, no interface, only the edited instances.
        assert_eq!(own(leaf), vec!["fan.f1.leaf".to_owned()], "{level:?}");
        assert_eq!(own(body), core, "{level:?}");
        for c in [leaf, body, closure] {
            assert!(c.ifaces_built.is_empty(), "{level:?}: {:?}", c.ifaces_built);
            assert_eq!(c.miss("check"), 1, "{level:?}");
        }
        assert_eq!(leaf.emitted, 2, "{level:?}: `run_1` and its closure");
        assert_eq!(body.emitted, 1, "{level:?}: `scale` alone");
        // (b2): `apply`'s body is a fact its callers' code reads only in
        // the optimized pipeline.
        assert_eq!(own(closure), core, "{level:?}");
        match level {
            OptLevel::Dev => assert_eq!(closure.emitted, 1),
            OptLevel::Optimized => assert!(closure.emitted > 1 + N, "{}", closure.emitted),
        }
        // (c): core's interface changes; the leaves rebuild theirs, which
        // name nothing of core, so their deep hashes stay.
        assert!(sig.ifaces_built.contains(&"fan.core".to_owned()));
        assert_ne!(
            sig.deep_hashes["fan.core"],
            cold.counters.deep_hashes["fan.core"]
        );
        for i in 0..N {
            let f = format!("fan.f{i}");
            assert_eq!(sig.deep_hashes[&f], cold.counters.deep_hashes[&f], "{f}");
        }
        let mut expected: Vec<String> = (0..N).map(|i| format!("fan.f{i}.leaf")).collect();
        expected.extend(["fan.app.main".to_owned(), "fan.core.util".to_owned()]);
        expected.sort();
        assert_eq!(own(sig), expected, "{level:?}");
    }
}

/// Std's skims come from the toolchain's `depfiles` entry (#49): a cold
/// run records it, a warm run reads it instead of skimming std, and a
/// damaged entry is a miss: std is skimmed again, the program is the same
/// and the entry is recorded anew.
#[test]
fn std_skims_come_from_the_toolchain_entry() {
    use hd_cache::{CacheStore, EntryKind};
    for executor in [Executor::Serial(SerialOrder::Priority), Executor::Pool(4)] {
        let store = MemoryStore::default();
        let base = program(DATA_MAIN, GEO);
        let cold = ok(&store, &base, executor);
        assert_eq!(cold.counters.miss("depfiles"), 1);
        let warm = ok(&store, &base, executor);
        assert_eq!(warm.counters.hit("depfiles"), 1);
        assert_eq!(warm.counters.miss("depfiles"), 0);
        assert_eq!(warm.wasm, cold.wasm);

        let damaged = MemoryStore::default();
        let mut found = false;
        for (kind, key) in store.drain_new() {
            let mut bytes = store.get(kind, &key).expect("entry").to_vec();
            if kind == EntryKind::Depfiles {
                found = true;
                let last = bytes.len() - 1;
                bytes[last] ^= 1;
            }
            damaged.put(kind, &key, &bytes);
        }
        assert!(found, "the cold run recorded std's skims");
        let again = ok(&damaged, &base, executor);
        assert_eq!(again.counters.miss("depfiles"), 1);
        assert_eq!(again.wasm, cold.wasm);
        let healed = ok(&damaged, &base, executor);
        assert_eq!(healed.counters.hit("depfiles"), 1);
    }
}

/// A library module whose `tests:` block, and a test module beside it,
/// use `money.coins`, which no library code uses (#238).
const LEDGER: &str = "\
pub fn total() -> i32:
    3

tests:
    use pkg.money.coins.cents
    use std.testing.assert_equal

    it(\"adds cents\"):
        assert_equal(total() + cents(), 103, reason=\"block\")
";

const LEDGER_TEST: &str = "\
use pkg.money.coins.cents
use std.testing.assert_equal

it(\"checks cents\"):
    assert_equal(cents(), 100, reason=\"module\")
";

fn ledger(coins: &str) -> MemorySources {
    let mut s = MemorySources::default();
    s.insert("src/ledger.hd", LEDGER);
    s.insert("src/ledger_test.hd", LEDGER_TEST);
    s.insert("src/money/coins.hd", coins);
    s
}

fn test_run(store: &MemoryStore, src: &MemorySources, executor: Executor) -> Output {
    let host = Host {
        render_tir: &[],
        sources: src,
        store,
        clock: &NoClock,
        executor,
    };
    build(
        &host,
        "shop",
        &Goal::Tests {
            module: None,
            filter: None,
        },
    )
}

/// The test role's `check` keys reach every folder test code reads
/// (#238): a `tests:` block's use and a test module's use of a folder
/// that no library code uses. Warm, both checks hit; a body edit of the
/// used folder rechecks it alone; an edit of its API rechecks both, which
/// report the call, never a stale pass.
#[test]
fn test_code_uses_reach_the_test_check_keys() {
    const COINS: &str = "pub fn cents() -> i32:\n    100\n";
    let own = |r: &Output| -> Vec<String> {
        r.counters
            .modules_checked
            .iter()
            .filter(|m| m.starts_with("shop."))
            .cloned()
            .collect()
    };
    for executor in [Executor::Serial(SerialOrder::Priority), Executor::Pool(4)] {
        let store = MemoryStore::default();
        let cold = test_run(&store, &ledger(COINS), executor);
        assert!(cold.diags.is_empty(), "{}", cold.render());
        assert_eq!(
            own(&cold),
            ["shop.ledger", "shop.ledger_test", "shop.money.coins"],
            "{executor:?}"
        );
        let warm = test_run(&store, &ledger(COINS), executor);
        assert!(warm.diags.is_empty(), "{}", warm.render());
        assert!(own(&warm).is_empty(), "{executor:?}: {:?}", own(&warm));

        let body = test_run(&store, &ledger(&COINS.replace("100", "50 + 50")), executor);
        assert!(body.diags.is_empty(), "{}", body.render());
        assert_eq!(own(&body), ["shop.money.coins"], "{executor:?}");

        let api = COINS.replace("cents()", "cents(n: i32)");
        let r = test_run(&store, &ledger(&api), executor);
        assert_eq!(
            own(&r),
            ["shop.ledger", "shop.ledger_test", "shop.money.coins"],
            "{executor:?}"
        );
        let located = r.render_located();
        assert_eq!(
            located.matches("argument-count").count(),
            2,
            "{executor:?}: {located}"
        );
        assert!(located.contains("src/ledger.hd:9:"), "{located}");
        assert!(located.contains("src/ledger_test.hd:5:"), "{located}");
    }
}

fn analyze_run(store: &MemoryStore, src: &MemorySources, executor: Executor) -> Output {
    let host = Host {
        render_tir: &[],
        sources: src,
        store,
        clock: &NoClock,
        executor,
    };
    build(&host, "shop", &Goal::Analyze)
}

/// A `tests:` block's use makes no folder edge (#276,
/// `module.cycle.test-code`): `ledger`'s block uses `money.coins`, which
/// uses `ledger`, and neither role reports a folder cycle. The use widens
/// only the test role's closure: an API edit of `money.coins` keeps
/// `shop.ledger`'s library `check` key, so `hd check` rechecks only
/// `money.coins`, while the test-role key changes and the test run
/// rechecks `shop.ledger` too.
#[test]
fn a_tests_block_use_widens_only_the_test_role_closure() {
    const COINS: &str = "use pkg.ledger.{total}\n\npub fn cents() -> i32:\n    total() - 3 + 100\n";
    let sources = |coins: &str| {
        let mut s = MemorySources::default();
        s.insert("src/ledger.hd", LEDGER);
        s.insert("src/money/coins.hd", coins);
        s
    };
    let own = |r: &Output| -> Vec<String> {
        r.counters
            .modules_checked
            .iter()
            .filter(|m| m.starts_with("shop."))
            .cloned()
            .collect()
    };
    let key = |r: &Output| r.counters.check_keys.get("shop.ledger").copied();
    let api = format!("{COINS}\npub fn dimes() -> i32:\n    10\n");
    for executor in [Executor::Serial(SerialOrder::Priority), Executor::Pool(4)] {
        let store = MemoryStore::default();
        let lib = analyze_run(&store, &sources(COINS), executor);
        assert!(lib.diags.is_empty(), "{}", lib.render());
        let test = test_run(&store, &sources(COINS), executor);
        assert!(test.diags.is_empty(), "{}", test.render());
        assert_eq!(test.tests.len(), 1, "{executor:?}");
        assert!(
            key(&lib).is_some() && key(&lib) != key(&test),
            "{executor:?}"
        );

        let lib_api = analyze_run(&store, &sources(&api), executor);
        assert!(lib_api.diags.is_empty(), "{}", lib_api.render());
        assert_eq!(key(&lib_api), key(&lib), "{executor:?}");
        assert_eq!(own(&lib_api), ["shop.money.coins"], "{executor:?}");

        let test_api = test_run(&store, &sources(&api), executor);
        assert!(test_api.diags.is_empty(), "{}", test_api.render());
        assert_ne!(key(&test_api), key(&test), "{executor:?}");
        assert_eq!(
            own(&test_api),
            ["shop.ledger", "shop.money.coins"],
            "{executor:?}"
        );
    }
}

/// A `check` entry answers only a goal that checks its module the same
/// way (#282). A script's top level is entry behavior for a program and
/// for `hd check`, but requirement-free initialization for a test run and
/// for `hd check --tests` of a module with a `tests:` block. For each
/// pair of goals, the second build over the first one's store reports
/// what it reports on a cold store.
#[test]
fn a_goal_reads_only_the_check_entries_it_would_write() {
    const SCRIPT: &str = "name := \"Ada\"\nprintln(\"hello, $name\")\n";
    let tested = format!("{SCRIPT}\ntests:\n    it(\"runs\"):\n        pass\n");
    let goals = [
        Goal::Program {
            entry: "s".to_owned(),
        },
        Goal::Analyze,
        Goal::CheckTests,
        Goal::Tests {
            module: None,
            filter: None,
        },
    ];
    for text in [SCRIPT, tested.as_str(), ""] {
        let mut src = MemorySources::default();
        src.insert("s.hd", text);
        let report = |store: &MemoryStore, goal: &Goal| {
            let host = Host {
                render_tir: &[],
                sources: &src,
                store,
                clock: &NoClock,
                executor: Executor::Serial(SerialOrder::Priority),
            };
            build(&host, "shop", goal).render_located()
        };
        let cold: Vec<String> = goals
            .iter()
            .map(|g| report(&MemoryStore::default(), g))
            .collect();
        for first in &goals {
            for (second, alone) in goals.iter().zip(&cold) {
                let store = MemoryStore::default();
                report(&store, first);
                assert_eq!(
                    &report(&store, second),
                    alone,
                    "{text:?}: {first:?}, then {second:?}"
                );
            }
        }
    }
}

/// A `tests:` block solves over its test role's impl universe, which
/// `test_closure(m)` widens with the folders the block uses (#208).
/// `impl From[Cents] for Dollars` is owned through its trait argument by
/// `cents`, a folder only `ledger`'s block uses, so the library role's
/// universe lacks it and the block's `Dollars::from` needs the test one.
#[test]
fn a_tests_block_solves_over_the_impls_of_folders_it_uses() {
    const DOLLARS: &str = "pub data Dollars:\n    pub amount: i32\n";
    const CENTS: &str = "\
use std.convert.{From}
use pkg.money.dollars.{Dollars}

pub data Cents:
    pub n: i32

impl From[Cents] for Dollars:
    fn from(c: Cents) -> Dollars:
        Dollars { amount: c.n / 100 }
";
    const LEDGER: &str = "\
use pkg.money.dollars.{Dollars}

pub fn zero() -> Dollars:
    Dollars { amount: 0 }

tests:
    use pkg.cents.cents.{Cents}
    use std.convert.{From}
    use std.testing.assert_equal

    it(\"converts cents\"):
        d := Dollars::from(Cents { n: 300 })
        assert_equal(d.amount + zero().amount, 3, reason=\"three dollars\")
";
    let mut src = MemorySources::default();
    src.insert("src/ledger.hd", LEDGER);
    src.insert("src/money/dollars.hd", DOLLARS);
    src.insert("src/cents/cents.hd", CENTS);
    for executor in [Executor::Serial(SerialOrder::Priority), Executor::Pool(4)] {
        let store = MemoryStore::default();
        let lib = analyze_run(&store, &src, executor);
        assert!(lib.diags.is_empty(), "{}", lib.render());
        let test = test_run(&store, &src, executor);
        assert!(test.diags.is_empty(), "{}", test.render());
        assert_eq!(test.tests.len(), 1, "{executor:?}");
    }
}
