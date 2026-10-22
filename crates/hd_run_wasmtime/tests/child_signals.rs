//! Child exits must not kill a process that runs Wasm (#160).
//!
//! tokio's process driver installs a SIGCHLD handler for `Process.run!`.
//! A process-directed signal goes to one thread that does not block it,
//! the first such thread in the kernel's list on macOS. wasmtime's
//! Mach-port exception thread waits with `MACH_RCV_INTERRUPT` and aborts
//! when a handled signal interrupts that wait. So any child exit (or
//! Ctrl-C) could abort the process once that thread took the signal.
//!
//! This binary has no test harness, so it owns every thread. The main
//! thread creates the engine first, so a wasmtime helper thread, if any,
//! starts with nothing blocked. Then the main thread blocks SIGCHLD, and
//! every later thread inherits that, except one receiver that unblocks it.
//! A helper thread that can take a signal then gets each SIGCHLD before the
//! receiver does. With the fix there is no such thread: the receiver takes
//! every SIGCHLD and the children are reaped.

#[cfg(unix)]
#[path = "support/mod.rs"]
mod support;

#[cfg(unix)]
fn main() {
    use std::time::Duration;

    use hd_run::{Grants, HostSetup, Limit, Limits};
    use hd_run_wasmtime::{EngineConfig, WasmtimeEngine, run_captured};
    use nix::sys::signal::{SigSet, Signal};

    const CHILDREN: usize = 200;

    WasmtimeEngine::new(EngineConfig::default()).expect("engine");
    let mut sigchld = SigSet::empty();
    sigchld.add(Signal::SIGCHLD);
    sigchld.thread_block().expect("block SIGCHLD");
    std::thread::spawn(move || {
        sigchld.thread_unblock().expect("unblock SIGCHLD");
        loop {
            std::thread::park();
        }
    });
    // The receiver unblocks before any child starts.
    std::thread::sleep(Duration::from_millis(50));

    let mut p = support::Program::new();
    let run_true = support::Enc::default()
        .str("true")
        .leb(0)
        .leb(0)
        .leb(0)
        .bytes(b"")
        .0;
    for _ in 0..CHILDREN {
        // Ok, empty stdout and stderr, status 0, program "true".
        p.call_waiting(
            "hd:Process",
            "run",
            &run_true,
            vec![0, 0, 0, 0, 4, b't', b'r', b'u', b'e'],
        );
    }
    let (wasm, expected) = p.finish(7);
    let setup = HostSetup {
        providers: vec![],
        grants: Grants {
            limits: vec![("Process".into(), Limit::Entries(vec!["true".into()]))],
        },
        limits: Limits::default(),
        args: vec![],
        program: String::new(),
    };
    let ran = run_captured(EngineConfig::default(), &wasm, &setup).expect("run");
    assert_eq!(String::from_utf8_lossy(&ran.stderr), expected);
    assert_eq!(ran.stdout, b"core-parity\n");
    assert_eq!(ran.status, 7);
    println!("child_signals: {CHILDREN} children reaped");
}

#[cfg(not(unix))]
fn main() {}
