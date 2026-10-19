//! A SIGINT or SIGTERM sent to the `hd` process alone is a Ctrl-C
//! (`cli.cancel.signals`, `cli.cancel.forward`, #534). On the Node engine
//! the program runs in a child process that `hd` must pass the interrupt
//! on to; on wasmtime `hd` cancels the run itself. Both end with
//! `cancelled` on standard error and status 130, after the dispose's wait.

use std::io::{BufRead as _, BufReader, Read as _};
use std::os::unix::process::CommandExt as _;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use hd_testkit::Scratch;
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

/// The entry waits on a timer that never ends, and its dispose waits
/// 300 ms: a cancel that waits for the dispose ends `cancelled`, and one
/// that abandons it ends `cancelled, cleanup abandoned`.
const PROGRAM: &str = "use std.ops.Disposable
use std.time.{Clock, ms, sleep}

struct Conn:
    closing: mut Suspend[void]

impl Disposable for Conn:
    fn dispose!(&mut self) -> void:
        let mut closing = self.closing
        closing!()

pub fn main!() -> void $ Console + Clock:
    using conn = Conn { closing: sleep(ms(300)) }
    println(\"ready\")
    let mut forever = sleep(ms(600000))
    forever!()
    println(\"never\")
";

/// A running `hd`, in a process group of its own, whose program has
/// printed `ready`.
struct Run {
    child: Child,
    _dir: Scratch,
}

impl Run {
    fn start(engine: &str, name: &str, cleanup_limit: &str) -> Self {
        let dir = Scratch::new(env!("CARGO_TARGET_TMPDIR"), &format!("interrupt-{name}"));
        std::fs::write(dir.join("wait.hd"), PROGRAM).expect("write the program");
        let mut child = Command::new(env!("CARGO_BIN_EXE_hd"))
            .current_dir(&dir)
            .env("HD_CACHE", dir.cache())
            .env_remove("HD_ENGINE")
            .args([
                "--engine",
                engine,
                "wait.hd",
                "--cleanup-limit",
                cleanup_limit,
            ])
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("run hd");
        // The program says `ready` just before its host starts to wait.
        let stdout = child.stdout.take().expect("standard output");
        let (said, heard) = mpsc::channel();
        std::thread::spawn(move || {
            let mut lines = BufReader::new(stdout).lines();
            let _ = said.send(lines.next().and_then(Result::ok));
        });
        let line = heard.recv_timeout(Duration::from_mins(2));
        assert_eq!(line, Ok(Some("ready".to_owned())), "the program is running");
        // A signal in the instant between `ready` and the wait would find
        // the program computing, which a Ctrl-C stops (`cli.cancel.computing`).
        std::thread::sleep(Duration::from_millis(200));
        Self { child, _dir: dir }
    }

    /// Sends `signal` to the `hd` process only.
    fn send(&self, signal: Signal) {
        let pid = Pid::from_raw(i32::try_from(self.child.id()).expect("pid"));
        kill(pid, signal).expect("signal hd");
    }

    /// Waits for `hd` to end: its status and standard error.
    fn finish(mut self) -> (Option<i32>, String, Duration) {
        let started = Instant::now();
        let deadline = started + Duration::from_mins(1);
        let status = loop {
            if let Some(status) = self.child.try_wait().expect("wait") {
                break status;
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                panic!("hd did not end after the signal");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let elapsed = started.elapsed();
        let mut stderr = String::new();
        self.child
            .stderr
            .take()
            .expect("standard error")
            .read_to_string(&mut stderr)
            .expect("read standard error");
        (status.code(), stderr, elapsed)
    }
}

/// One signal ends the run `cancelled` with status 130, once the dispose
/// has waited out its 300 ms.
fn cancels(engine: &str, signal: Signal) {
    let run = Run::start(engine, &format!("{engine}-{signal}"), "30s");
    let sent = Instant::now();
    run.send(signal);
    let (code, stderr, _) = run.finish();
    assert_eq!(stderr.trim_end(), "cancelled", "{engine} {signal}");
    assert_eq!(code, Some(130), "{engine} {signal}");
    assert!(
        sent.elapsed() >= Duration::from_millis(250),
        "{engine} {signal}: the cancel ended before the dispose's wait"
    );
}

/// A second signal after the cancel abandons the dispose at once, long
/// before the 30 s bound.
fn second_abandons(engine: &str) {
    let run = Run::start(engine, &format!("{engine}-second"), "30s");
    run.send(Signal::SIGINT);
    // The first signal has cancelled the entry and the dispose waits.
    std::thread::sleep(Duration::from_millis(100));
    run.send(Signal::SIGINT);
    let (code, stderr, elapsed) = run.finish();
    assert_eq!(
        stderr.trim_end(),
        "cancelled, cleanup abandoned",
        "{engine}"
    );
    assert_eq!(code, Some(130), "{engine}");
    assert!(elapsed < Duration::from_secs(20), "{engine}: {elapsed:?}");
}

#[test]
fn node_sigint_to_hd_alone_cancels() {
    cancels("node", Signal::SIGINT);
}

#[test]
fn node_sigterm_to_hd_alone_cancels() {
    cancels("node", Signal::SIGTERM);
}

#[test]
fn node_second_signal_abandons() {
    second_abandons("node");
}

#[cfg(feature = "wasmtime")]
#[test]
fn wasmtime_sigint_to_hd_alone_cancels() {
    cancels("wasmtime", Signal::SIGINT);
}

#[cfg(feature = "wasmtime")]
#[test]
fn wasmtime_sigterm_to_hd_alone_cancels() {
    cancels("wasmtime", Signal::SIGTERM);
}

#[cfg(feature = "wasmtime")]
#[test]
fn wasmtime_second_signal_abandons() {
    second_abandons("wasmtime");
}
