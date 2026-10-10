//! List, deque and list-view iterator invalidation (#192,
//! `flow.for.length`, `flow.for.invalidate.panic`, `flow.for.replace`,
//! `std-collections.deque.invalidate`, `std-collections.view.invalidate`).
//! Programs build through the driver and run on V8 (`host/run.mjs`).

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
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("list-invalidation-run-{name}.wasm"));
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    ran
}

/// `main` running `body`, importing `Deque` when the body names it.
fn program(body: &str) -> String {
    let import = if body.contains("Deque") {
        "use std.collections.Deque\n\n"
    } else {
        ""
    };
    format!("{import}pub fn main() -> void $ Console:\n{body}")
}

/// A program whose `body` runs in `main`; it must exit with the
/// `iterator-invalidated` panic after printing `before`.
fn panics(name: &str, body: &str, before: &str) {
    let main = program(body);
    let r = ran(name, &main);
    assert!(!r.status.success(), "{name} must panic");
    assert_eq!(String::from_utf8_lossy(&r.stdout), before, "{name}");
    let err = String::from_utf8_lossy(&r.stderr);
    assert!(err.contains("panic: iterator-invalidated"), "{name}: {err}");
}

/// A program whose `body` runs in `main` and prints `expected`.
fn prints(name: &str, body: &str, expected: &str) {
    let main = program(body);
    let r = ran(name, &main);
    assert!(
        r.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&r.stdout), expected, "{name}");
}

#[test]
fn pushing_during_a_list_loop_panics() {
    panics(
        "list-push",
        "    let v: mut List[i32] = [1, 2]
    for x in v:
        println(x)
        v.push(x)
",
        "1\n",
    );
}

#[test]
fn removing_during_a_list_loop_panics() {
    panics(
        "list-remove",
        "    let v: mut List[i32] = [1, 2, 3]
    for x in v:
        println(x)
        _ := v.remove_at(2)
",
        "1\n",
    );
}

#[test]
fn replacing_an_element_during_a_list_loop_is_seen_later() {
    prints(
        "list-set",
        "    let v: mut List[i32] = [1, 2, 3]
    for x in v:
        println(x)
        v[2] = 30
",
        "1\n2\n30\n",
    );
}

#[test]
fn a_loop_that_never_changes_the_list_finishes() {
    prints(
        "list-plain",
        "    let v: mut List[i32] = [1, 2, 3]
    let total = +0
    for x in v:
        total = total + x
    v.push(4)
    println(total)
",
        "6\n",
    );
}

#[test]
fn a_list_iterator_panics_after_the_list_changes_length() {
    panics(
        "list-iter-push",
        "    let v: mut List[i32] = [1, 2]
    let it: mut Iterator[i32] = v.iter()
    println(it.next().unwrap_or(-1))
    v.push(3)
    _ := it.next()
",
        "1\n",
    );
}

#[test]
fn an_exhausted_list_iterator_still_panics_after_a_pop() {
    panics(
        "list-iter-pop",
        "    let v: mut List[i32] = [1]
    let it: mut Iterator[i32] = v.iter()
    _ := it.next()
    _ := it.next()
    _ := v.pop()
    _ := it.next()
",
        "",
    );
}

#[test]
fn a_list_iterator_survives_an_element_replacement() {
    prints(
        "list-iter-set",
        "    let v: mut List[i32] = [1, 2]
    let it: mut Iterator[i32] = v.iter()
    println(it.next().unwrap_or(-1))
    v[1] = 20
    println(it.next().unwrap_or(-1))
    println(it.next().unwrap_or(-1))
",
        "1\n20\n-1\n",
    );
}

#[test]
fn pushing_during_a_deque_loop_panics() {
    panics(
        "deque-push",
        "    let q: mut Deque[i32] = Deque::new()
    q.push_back(1)
    q.push_back(2)
    for x in q:
        println(x)
        q.push_back(x)
",
        "1\n",
    );
}

#[test]
fn popping_during_a_deque_loop_panics() {
    panics(
        "deque-pop",
        "    let q: mut Deque[i32] = Deque::new()
    q.push_back(1)
    q.push_back(2)
    for x in q:
        println(x)
        _ := q.pop_front()
",
        "1\n",
    );
}

#[test]
fn a_deque_loop_that_changes_nothing_finishes() {
    prints(
        "deque-plain",
        "    let q: mut Deque[i32] = Deque::new()
    q.push_back(1)
    q.push_front(0)
    for x in q:
        println(x)
    _ := q.pop_back()
    println(q.len())
",
        "0\n1\n1\n",
    );
}

#[test]
fn a_list_view_is_invalid_after_a_push_or_pop() {
    panics(
        "view-push",
        "    let v: mut List[i32] = [1, 2, 3]
    let w = v.view(0, 2)
    v.push(4)
    println(w.len())
",
        "",
    );
    panics(
        "view-pop",
        "    let v: mut List[i32] = [1, 2, 3]
    let w = v.view(0, 2)
    _ := v.pop()
    for x in w:
        println(x)
",
        "",
    );
}

#[test]
fn a_view_iterator_panics_after_a_length_change() {
    panics(
        "view-iter",
        "    let v: mut List[i32] = [1, 2, 3]
    let it: mut Iterator[i32] = v.view(0, 3).iter()
    println(it.next().unwrap_or(-1))
    v.push(4)
    _ := it.next()
",
        "1\n",
    );
}

#[test]
fn a_list_view_survives_an_element_replacement() {
    prints(
        "view-set",
        "    let v: mut List[i32] = [1, 2, 3]
    let w = v.view(0, 2)
    v[1] = 20
    println(w.len())
    println(w[1])
",
        "2\n20\n",
    );
}
