//! The `hd-io` runtime starts lazily (wasmtime-host-audit.md §4.2): a
//! program that makes no offloaded host call starts no thread. Its own
//! test binary, so no other test in the process starts the runtime.

use hd_driver::{Executor, Goal, Host, NoClock};
use hd_project::MemorySources;
use hd_run::{Grants, HostSetup, Limits};
use hd_run_wasmtime::{EngineConfig, io_started, run_captured};
use hd_sched::SerialOrder;

#[test]
fn hello_starts_no_io_runtime() {
    let mut sources = MemorySources::default();
    sources.insert(
        "main.hd",
        "pub fn main() -> void $ Console:\n    println(42)\n",
    );
    let store = hd_cache::MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let built = hd_driver::build(
        &host,
        "hello",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(built.diags.is_empty(), "{}", built.render());
    let wasm = built.wasm.expect("hello builds");
    let setup = HostSetup {
        providers: vec![],
        grants: Grants::default(),
        limits: Limits::default(),
        args: vec![],
        program: "hello".into(),
    };
    let ran = run_captured(EngineConfig::default(), &wasm, &setup).expect("hello runs");
    assert_eq!((ran.status, ran.stdout.as_slice()), (0, &b"42\n"[..]));
    assert!(!io_started(), "hello started the hd-io runtime");
}
