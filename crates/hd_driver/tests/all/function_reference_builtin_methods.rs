//! Function references to the language-tier built-in methods of `List`
//! and `Map` (spec/lang/10-modules.md#built-in-methods), which have no
//! declaration: an unbound reference takes the receiver first, a bound
//! one captures it (`fn.ref.unbound.receiver`, `fn.ref.bound.capture`).
//! Built and run on V8 (`host/run.mjs`).

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn build_program(store: &MemoryStore, main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
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
    out
}

fn run(name: &str, main: &str) -> String {
    let store = crate::warm_store();
    let out = build_program(&store, main);
    let wasm = out.wasm.as_ref().expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("fnref-builtin-{name}.wasm"),
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

/// Unbound references to `List` and `Map` built-in methods: with the
/// type arguments written, from the expected type, and as a callback.
/// `push` takes the receiver `mut`.
#[test]
fn unbound_references_to_builtin_methods_take_the_receiver_first() {
    let main = "fn length_of(f: fn(List[i32]) -> usize, items: List[i32]) -> usize:
    f(items)

pub fn main() -> void $ Console:
    let xs: List[i32] = [1, 2, 3]
    len := List::[i32]::len
    println(len(xs))
    println(length_of(List::len, xs))
    let groups: List[List[i32]] = [[1], [2, 3], []]
    println(groups.map(List::len)[1])
    let mut acc: List[i32] = []
    let push: fn(&mut List[i32], i32) -> void = List::push
    push(&mut acc, 7)
    push(&mut acc, 8)
    println(acc[1])
    let m: Map[string, i32] = {\"a\": 1}
    get := Map::[string, i32]::get
    println(get(m, \"a\") == .Some(1))
    println(get(m, \"b\") == .None)
    let size: fn(Map[string, i32]) -> usize = Map::len
    println(size(m))
    let mut walk = List::[i32]::iter(xs)
    println(walk.next() == .Some(1))
";
    assert_eq!(run("unbound", main), "3\n3\n2\n8\ntrue\ntrue\n1\ntrue\n");
}

/// A bound reference to a built-in method captures its receiver when the
/// reference is made, and a bound `push` changes that receiver.
#[test]
fn bound_references_to_builtin_methods_capture_the_receiver() {
    let main = "pub fn main() -> void $ Console:
    let xs: List[i32] = [1, 2, 3]
    len := xs::len
    println(len())
    let mut acc: List[i32] = []
    add := acc::push
    add(5)
    add(6)
    println(acc.len())
    println(acc[1])
    let m: Map[string, i32] = {\"a\": 1}
    mlen := m::len
    println(mlen())
";
    assert_eq!(run("bound", main), "3\n2\n6\n1\n");
}
