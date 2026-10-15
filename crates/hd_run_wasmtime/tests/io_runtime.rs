//! The `hd-io` runtime starts lazily: a
//! program that makes no offloaded host call starts no thread. Its own
//! test binary, so no other test in the process starts the runtime.

#[path = "support/hd_source.rs"]
mod hd_source;

use hd_driver::{Executor, Goal, Host, NoClock};
use hd_project::MemorySources;
use hd_run::{Grants, HostSetup, Limits};
use hd_run_wasmtime::{EngineConfig, io_started, run_captured};
use hd_sched::SerialOrder;

#[test]
fn hello_starts_no_io_runtime() {
    let ran = run_main("pub fn main() -> void $ Console:\n    println(42)\n");
    assert_eq!((ran.status, ran.stdout.as_slice()), (0, &b"42\n"[..]));
    assert!(!io_started(), "hello started the hd-io runtime");
}

/// `std-fs.special-file`: a regular file and a directory run inline, so
/// an `Fs` program that touches only those spawns no task and starts no
/// `hd-io` thread.
#[test]
fn regular_file_fs_calls_start_no_io_runtime() {
    let dir = std::env::temp_dir().join(format!("hd-io-fs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("makes a directory");
    let file = dir.join("data.txt");
    let source = format!(
        "use std.fs.{{FsRead, FsWrite, read_text, write_text}}\nuse std.path.Path\n\npub fn main!() -> void $ Console + FsRead + FsWrite:\n    _ := write_text!(Path({file}), \"inline\")\n    println(read_text!(Path({file})).unwrap_or(\"failed\"))\n    println(read_text!(Path({dir})).is_ok())\n",
        file = hd_source::hd_path(&file),
        dir = hd_source::hd_path(&dir),
    );
    let ran = run_main(&source);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        (
            ran.status,
            String::from_utf8_lossy(&ran.stdout).into_owned()
        ),
        (0, "inline\nfalse\n".to_owned())
    );
    assert!(
        !io_started(),
        "a regular-file call started the hd-io runtime"
    );
}

fn run_main(source: &str) -> hd_run_wasmtime::Captured {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
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
    run_captured(EngineConfig::default(), &wasm, &setup).expect("the program runs")
}
