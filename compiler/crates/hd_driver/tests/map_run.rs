//! Maps run (#124, codegen.md §13.12): iteration in entry order, removal
//! by tombstone, growth that keeps the order, keys hashed and compared
//! through their own `Hash` and `Eq`, and the iterator's invalidation
//! check. Programs build through the driver and run on V8
//! (`host/run.mjs`), as the conformance runner does.

use std::path::{Path, PathBuf};
use std::process::{Command, Output as Ran};

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// Builds `main` as a program and runs it on V8.
fn ran(name: &str, main: &str) -> Ran {
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
    let out = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("map-run-{name}.wasm"));
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    ran
}

/// The standard output of a program that must succeed.
fn output_of(name: &str, main: &str) -> String {
    let r = ran(name, main);
    assert!(
        r.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    String::from_utf8(r.stdout).expect("UTF-8")
}

#[test]
fn a_for_loop_and_keys_and_values_follow_entry_order() {
    // A later duplicate key replaces the value in the first entry's place
    // (expr.map.duplicate.last, types.map.replace.in-place).
    let main = "\
pub fn main() -> void $ Console:
    let m: mut Map[string, i32] = {\"b\": 1, \"a\": 2, \"b\": 3, \"c\": 4}
    m[\"a\"] = 20
    m[\"d\"] = 5
    for (k, v) in m:
        println(\"${k}=${v}\")
    let line = \"\"
    for k in m.keys():
        line = line + k
    println(line)
    let sum = +0
    for v in m.values():
        sum = sum * 100 + v
    println(sum)
    println(m.len())
";
    assert_eq!(
        output_of("order", main),
        "b=3\na=20\nc=4\nd=5\nbacd\n3200405\n4\n"
    );
}

#[test]
fn removal_keeps_the_survivors_order_and_reinsertion_goes_last() {
    let main = "\
pub fn main() -> void $ Console:
    let m: mut Map[i32, string] = {1: \"one\", 2: \"two\", 3: \"three\"}
    println(m.remove(2) == .Some(\"two\"))
    println(m.remove(2).is_some())
    println(m.get(2).is_some())
    m[2] = \"again\"
    for (k, v) in m:
        println(\"${k}=${v}\")
    let big: mut Map[i64, i64] = {}
    for i in 0..1000:
        big[i64(i)] = i64(i) * 3
    for i in 0..1000:
        if i % 4 != 0:
            _ := big.remove(i64(i))
    for i in 1000..1100:
        big[i64(i)] = 0
    println(big.len())
    let last: i64 = -1
    let ordered = true
    for (k, _) in big:
        if k <= last:
            ordered = false
        last = k
    println(ordered)
    println(big[996])
    println(big.get(997).is_some())
";
    assert_eq!(
        output_of("remove", main),
        "true\nfalse\nfalse\n1=one\n3=three\n2=again\n350\ntrue\n2988\nfalse\n"
    );
}

#[test]
fn removing_during_iteration_invalidates_the_iterator() {
    // flow.for.invalidate.panic: the live count changed.
    let main = "\
pub fn main() -> void $ Console:
    let m: mut Map[string, i32] = {\"a\": 1, \"b\": 2, \"c\": 3}
    for (k, _) in m:
        println(k)
        _ := m.remove(\"c\")
";
    let r = ran("remove-during", main);
    assert!(!r.status.success(), "the loop must panic");
    assert_eq!(String::from_utf8_lossy(&r.stdout), "a\n");
    let err = String::from_utf8_lossy(&r.stderr);
    assert!(err.contains("iterator-invalidated"), "{err}");
}

#[test]
fn a_removal_and_an_insertion_in_one_step_skip_and_hide_entries() {
    // The live count is restored, so nothing panics (the length-only
    // check of flow.for.invalidate); the removed entry is skipped and the
    // appended one stays unseen (Q-R7: the written count is captured).
    let main = "\
pub fn main() -> void $ Console:
    let m: mut Map[string, i32] = {\"a\": 1, \"b\": 2, \"c\": 3}
    for (k, _) in m:
        if k == \"a\":
            _ := m.remove(\"c\")
            m[\"d\"] = 4
        println(k)
    println(m.keys().len())
";
    assert_eq!(output_of("swap-during", main), "a\nb\n3\n");
}

#[test]
fn a_data_key_hashes_and_compares_through_its_derived_impls() {
    let main = "\
@derive(Eq, Hash)
data Point:
    x: i32
    label: string

pub fn main() -> void $ Console:
    let m: mut Map[Point, i32] = {}
    for i in 0..50:
        m[Point { x: i % 10, label: \"p\" }] = i
    println(m.len())
    println(m[Point { x: 3, label: \"p\" }])
    println(m.get(Point { x: 3, label: \"q\" }).is_some())
    println(m.remove(Point { x: 4, label: \"p\" }) == .Some(44))
    println(m.len())
    let xs = \"\"
    for (p, _) in m:
        xs = xs + \"${p.x}\"
    println(xs)
";
    assert_eq!(
        output_of("data-key", main),
        "10\n43\nfalse\ntrue\n9\n012356789\n"
    );
}

#[test]
fn derived_eq_and_debug_on_an_enum_holding_a_map() {
    let main = "\
@derive(Eq, Debug)
enum Shelf:
    Empty
    Stock(items: Map[string, i32])

pub fn main() -> void $ Console:
    a := Shelf.Stock({\"x\": 1, \"y\": 2})
    println(a == Shelf.Stock({\"y\": 2, \"x\": 1}))
    println(a == Shelf.Stock({\"x\": 1}))
    println(a == Shelf.Empty)
    println(debug(a))
    println(debug(Shelf.Empty))
";
    assert_eq!(
        output_of("enum-map", main),
        "true\nfalse\nfalse\nShelf.Stock(items={\"x\": 1, \"y\": 2})\nShelf.Empty\n"
    );
}
