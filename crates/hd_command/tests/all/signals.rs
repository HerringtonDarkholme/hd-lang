//! A real signal sent to `hd` is handled by the waiting program, which
//! finishes normally and runs cleanup instead of cancelling the entry.
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use std::io::{BufRead as _, BufReader, Read as _};
use std::os::unix::process::{CommandExt as _, ExitStatusExt as _};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn handles(engine: &str, supervised: bool) {
    for (kind, signal, handled) in [
        ("Interrupt", Signal::SIGINT, true),
        ("Terminate", Signal::SIGTERM, true),
        ("Hangup", Signal::SIGHUP, true),
        ("Hangup", Signal::SIGINT, false),
        ("Interrupt", Signal::SIGHUP, false),
    ] {
        let dir = hd_testkit::Scratch::new(
            env!("CARGO_TARGET_TMPDIR"),
            &format!("signals-{engine}-{kind}-{supervised}"),
        );
        std::fs::write(
            dir.join("main.hd"),
            include_str!("programs/signals/program.hd"),
        )
        .unwrap();
        let mut command = hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"));
        command
            .current_dir(&dir)
            .env("HD_CACHE", crate::shared_objects(dir.cache()))
            .env_remove("HD_ENGINE")
            .args(["--engine", engine, "main.hd"]);
        if supervised {
            command.args(["--time-limit", "30s"]);
        }
        let mut child = command
            .args(["--", kind])
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sent, heard) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if sent.send(line.unwrap()).is_err() {
                    break;
                }
            }
        });
        let result = || {
            assert_eq!(
                heard.recv_timeout(Duration::from_secs(30)).unwrap(),
                "ready"
            );
            // stdout is flushed before the host enters its wait. Allow that
            // transition before sending to the parent process alone.
            std::thread::sleep(Duration::from_millis(200));
            kill(Pid::from_raw(i32::try_from(child.id()).unwrap()), signal).unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                assert!(
                    Instant::now() < deadline,
                    "signal did not complete {engine} {kind}"
                );
                std::thread::sleep(Duration::from_millis(10));
            };
            let mut stderr = String::new();
            child
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut stderr)
                .unwrap();
            let default = signal == Signal::SIGHUP && kind != "Hangup";
            let code = status
                .code()
                .or_else(|| status.signal().map(|signal| 128 + signal));
            assert_eq!(
                code,
                Some(if handled {
                    0
                } else if default {
                    129
                } else {
                    130
                }),
                "{engine} {kind}: {stderr}"
            );
            assert_eq!(
                stderr.trim_end(),
                if handled || default { "" } else { "cancelled" },
                "{engine} {kind}"
            );
            let expected = if handled {
                vec!["handled", "cleanup"]
            } else if default {
                vec![]
            } else {
                vec!["cleanup"]
            };
            assert_eq!(heard.into_iter().collect::<Vec<_>>(), expected);
        };
        // Keep failures from leaving a child or helper alive.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(result));
        let _ = child.kill();
        let _ = child.wait();
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }
}
#[test]
fn node_handles_all_signal_kinds_and_runs_cleanup() {
    handles("node", false);
}
#[test]
fn node_supervised_handles_all_signal_kinds_and_runs_cleanup() {
    handles("node", true);
}
#[cfg(feature = "wasmtime")]
#[test]
fn wasmtime_handles_all_signal_kinds_and_runs_cleanup() {
    handles("wasmtime", false);
}
