//! The phase-1 exit program (`compiler/samples/exit`) through the driver:
//! std bodies reach emission, a second run is warm, and the Wasm is
//! byte-identical across the serial and pool executors. Its output on V8
//! is checked by `hd_cli`'s `run` tests.

use std::path::Path;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn sources() -> MemorySources {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/exit/main.hd");
    let mut s = MemorySources::default();
    s.insert(
        "main.hd",
        &std::fs::read_to_string(file).expect("exit sample"),
    );
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

#[test]
fn exit_program_collects_std_and_runs_warm() {
    let src = sources();
    let store = MemoryStore::default();
    let cold = run(&store, &src, Executor::Serial(SerialOrder::Priority));
    let wasm = cold.wasm.clone().expect("wasm");
    assert_eq!(&wasm[..4], b"\0asm");
    assert!(
        cold.counters.tir_decoded.iter().any(|m| m == "std.console"),
        "std's println body is decoded for collection: {:?}",
        cold.counters.tir_decoded
    );
    assert!(cold.counters.emitted > 10);

    let again = run(&store, &src, Executor::Serial(SerialOrder::Priority));
    let c = &again.counters;
    assert!(
        c.modules_checked.is_empty(),
        "rechecked: {:?}",
        c.modules_checked
    );
    assert_eq!(c.hit("link"), 1);
    assert_eq!(c.emitted, 0);
    assert!(c.tir_decoded.is_empty());
    assert_eq!(again.wasm.as_deref(), Some(wasm.as_slice()));
}

#[test]
fn exit_program_bytes_match_across_executors() {
    let src = sources();
    let serial = run(
        &MemoryStore::default(),
        &src,
        Executor::Serial(SerialOrder::Fifo),
    );
    let pool = run(&MemoryStore::default(), &src, Executor::Pool(4));
    let lifo = run(
        &MemoryStore::default(),
        &src,
        Executor::Serial(SerialOrder::Priority),
    );
    let w = serial.wasm.expect("serial wasm");
    assert_eq!(pool.wasm.as_deref(), Some(w.as_slice()));
    assert_eq!(lifo.wasm.as_deref(), Some(w.as_slice()));
}
