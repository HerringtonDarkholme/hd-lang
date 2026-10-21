//! List and list-view iterator invalidation (#192, `flow.for.length`,
//! `flow.for.invalidate.panic`, `flow.for.replace`,
//! `std-collections.view.invalidate`), and the snapshot a `for` over a
//! list (`flow.for.snapshot`) or a deque
//! (`std-collections.deque.snapshot`) iterates.
//! Programs build through the driver and run on V8 (`host/run.mjs`).

use std::path::Path;
use std::process::{Command, Output as Ran};

use hd_testkit::Scratch;

use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// Builds `main` as a program and runs it on V8.
fn ran(name: &str, main: &str) -> Ran {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = crate::warm_store();
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
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("list-invalidation-run-{name}.wasm"),
    );
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

#[test]
fn a_list_iterator_panics_after_the_list_changes_length() {
    panics(
        "list-iter-push",
        "    let mut v: List[i32] = [1, 2]
    let mut it = v.iter()
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
        "    let mut v: List[i32] = [1]
    let mut it = v.iter()
    _ := it.next()
    _ := it.next()
    _ := v.pop()
    _ := it.next()
",
        "",
    );
}

#[test]
fn a_list_view_is_invalid_after_a_push_or_pop() {
    panics(
        "view-push",
        "    let mut v: List[i32] = [1, 2, 3]
    let w = v.view(0, 2)
    v.push(4)
    println(w.len())
",
        "",
    );
    panics(
        "view-pop",
        "    let mut v: List[i32] = [1, 2, 3]
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
        "    let mut v: List[i32] = [1, 2, 3]
    let mut it = v.view(0, 3).iter()
    println(it.next().unwrap_or(-1))
    v.push(4)
    _ := it.next()
",
        "1\n",
    );
}
