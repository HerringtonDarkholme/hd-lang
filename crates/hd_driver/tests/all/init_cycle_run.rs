//! Statements of a module cycle run in dependency order, then by module
//! identity and source position (spec 10 "Order Inside A Cycle":
//! `module.init.cycle.dependency`, `module.init.cycle.step`,
//! `module.init.cycle.earliest`), splitting a module's statements where
//! another module's must run between them; statements that wait on each
//! other are `top-level-read-before-initialization`
//! (`module.init.cycle.stuck`). Programs build through the driver and run
//! on V8 (`host/run.mjs`).

use std::path::Path;
use std::process::Command;

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;
use hd_testkit::Scratch;

/// Records lines in order; `main` prints them.
const LOG: &str = "let lines: mut List[string] = []

pub fn record(line: string) -> void:
    lines.push(line)

pub fn recorded() -> List[string]:
    lines
";

fn sources(files: &[(&str, &str)]) -> MemorySources {
    let mut s = MemorySources::default();
    s.insert("log/mod.hd", LOG);
    for (path, text) in files {
        s.insert(path, text);
    }
    s
}

fn build_in(store: &MemoryStore, src: &MemorySources) -> Output {
    let host = Host {
        render_tir: &[],
        sources: src,
        store,
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

fn run_wasm(name: &str, wasm: &[u8]) -> String {
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("init-cycle-{name}.wasm"),
    );
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

/// Builds cold, then warm from the same store, and runs the program: the
/// warm build reads `InitOrder`'s part and links the same Wasm.
fn run(name: &str, files: &[(&str, &str)]) -> String {
    let store = MemoryStore::default();
    let src = sources(files);
    let cold = build_in(&store, &src);
    assert!(cold.diags.is_empty(), "{name}: {}", cold.render());
    let linked = cold.wasm.expect("wasm");
    let warm = build_in(&store, &src);
    assert!(warm.diags.is_empty(), "{name}: {}", warm.render());
    assert_eq!(warm.counters.computed("InitOrder"), 0, "{name}");
    assert_eq!(warm.wasm.as_deref(), Some(linked.as_slice()), "{name}");
    run_wasm(name, &linked)
}

/// `main` prints `value`, then the recorded lines.
fn main_printing(uses: &str, value: &str) -> String {
    format!(
        "{uses}\nuse pkg.log.{{recorded}}\n\npub fn main() -> void $ Console:\n    println({value})\n    for line in recorded():\n        println(line)\n"
    )
}

#[test]
fn a_two_module_cycle_runs_a_statement_after_the_binding_it_reaches() {
    // `pkg.shop.catalog` orders first, but its statements wait on
    // `markup`, so prices' first statement runs before them and its
    // second after them.
    let catalog = "use pkg.shop.prices.{price_of}
use pkg.log.{record}

let featured = price_of(\"tea\")
record(\"catalog ${featured}\")

pub fn base_price(sku: string) -> i32:
    if sku == \"tea\": 10 else: 20

pub fn featured_price() -> i32:
    featured
";
    let prices = "use pkg.shop.catalog.{base_price}
use pkg.log.{record}

let markup = +5
record(\"prices\")

pub fn price_of(sku: string) -> i32:
    base_price(sku) + markup
";
    let main = main_printing("use pkg.shop.catalog.{featured_price}", "featured_price()");
    let out = run(
        "two",
        &[
            ("main.hd", &main),
            ("shop/catalog.hd", catalog),
            ("shop/prices.hd", prices),
        ],
    );
    assert_eq!(out, "15\ncatalog 15\nprices\n");
}

#[test]
fn a_three_module_cycle_follows_calls_through_every_module() {
    // a's statement reaches c's binding through b, which has no
    // statement; c's statements are ready first.
    let a = "use pkg.ring.b.{from_b}
use pkg.log.{record}

let a_val = from_b() + 1
record(\"a ${a_val}\")

pub fn hello() -> string:
    \"hello\"

pub fn a_value() -> i32:
    a_val
";
    let b = "use pkg.ring.c.{from_c}

pub fn from_b() -> i32:
    from_c() * 10
";
    let c = "use pkg.ring.a.{hello}
use pkg.log.{record}

record(\"c ${hello()}\")
let c_val = +4

pub fn from_c() -> i32:
    c_val
";
    let main = main_printing("use pkg.ring.a.{a_value}", "a_value()");
    let out = run(
        "three",
        &[
            ("main.hd", &main),
            ("ring/a.hd", a),
            ("ring/b.hd", b),
            ("ring/c.hd", c),
        ],
    );
    assert_eq!(out, "41\nc hello\na 41\n");
}

#[test]
fn statements_interleave_with_the_bindings_they_wait_on() {
    // Order: a's second statement (ready, earliest), b's `y`, a's `x`,
    // a's `z`, then b's rest. Both modules' inits split into runs.
    let a = "use pkg.mix.b.{get_y}
use pkg.log.{record}

let x = get_y() + 1
record(\"a second\")
let z = x * 10

pub fn get_z() -> i32:
    z
";
    let b = "use pkg.mix.a.{get_z}
use pkg.log.{record}

let y = +1
record(\"b y ${y}\")
let w = get_z() + 1
record(\"b w ${w}\")

pub fn get_y() -> i32:
    y

pub fn get_w() -> i32:
    w
";
    let main = main_printing("use pkg.mix.b.{get_w}", "get_w()");
    let out = run(
        "interleave",
        &[("main.hd", &main), ("mix/a.hd", a), ("mix/b.hd", b)],
    );
    assert_eq!(out, "21\na second\nb y 1\nb w 21\n");
}

#[test]
fn statements_that_wait_on_each_other_stay_an_error() {
    let a = "use pkg.shop.b.{read_b}

let a = read_b()

pub fn read_a() -> i32:
    a
";
    let b = "use pkg.shop.a.{read_a}

let b = read_a()

pub fn read_b() -> i32:
    b
";
    let main = "use pkg.shop.a.{read_a}

pub fn main() -> void $ Console:
    println(read_a())
";
    let store = MemoryStore::default();
    let src = sources(&[("main.hd", main), ("shop/a.hd", a), ("shop/b.hd", b)]);
    // Cold, then warm: every run reports the cycle, once, at a's statement.
    for _ in 0..2 {
        let out = build_in(&store, &src);
        assert_eq!(
            out.diags.code,
            vec![Code::TopLevelReadBeforeInitialization],
            "{}",
            out.render()
        );
        assert!(out.wasm.is_none());
        let shown = out.render();
        assert!(
            shown.contains("shop/a.hd:25..41") && shown.contains("`b`"),
            "{shown}"
        );
    }
}
