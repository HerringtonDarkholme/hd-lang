//! M4d's samples through the driver (`compiler/samples/init`, `suspend`
//! and `defer`): module initialization, suspension state machines and the
//! `defer` exit ladder build, a second build is warm, and the Wasm is
//! byte-identical across the serial and pool executors. Their output on V8
//! is checked by `hd_cli`'s `run` tests.

use std::path::Path;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn sources(dir: &str, files: &[&str]) -> MemorySources {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples");
    let mut s = MemorySources::default();
    for f in files {
        let text = std::fs::read_to_string(root.join(dir).join(f)).expect("sample");
        s.insert(f, &text);
    }
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

fn check(dir: &str, files: &[&str]) {
    let src = sources(dir, files);
    let serial = run(
        &MemoryStore::default(),
        &src,
        Executor::Serial(SerialOrder::Fifo),
    );
    let w = serial.wasm.expect("serial wasm");
    assert_eq!(&w[..4], b"\0asm", "{dir}");
    let pool = run(&MemoryStore::default(), &src, Executor::Pool(4));
    let lifo = run(
        &MemoryStore::default(),
        &src,
        Executor::Serial(SerialOrder::Priority),
    );
    assert_eq!(pool.wasm.as_deref(), Some(w.as_slice()), "{dir}: pool");
    assert_eq!(lifo.wasm.as_deref(), Some(w.as_slice()), "{dir}: priority");
    // Warm: every code entry, suspending parts included, comes back.
    let store = MemoryStore::default();
    run(&store, &src, Executor::Serial(SerialOrder::Priority));
    let again = run(&store, &src, Executor::Serial(SerialOrder::Priority));
    assert_eq!(again.counters.emitted, 0, "{dir}: re-emitted");
    assert_eq!(again.wasm.as_deref(), Some(w.as_slice()), "{dir}: warm");
}

#[test]
fn init_program_bytes_match_across_executors() {
    check("init", &["main.hd", "prices.hd"]);
}

#[test]
fn suspend_program_bytes_match_across_executors() {
    check("suspend", &["main.hd"]);
}

#[test]
fn defer_program_bytes_match_across_executors() {
    check("defer", &["main.hd"]);
}
