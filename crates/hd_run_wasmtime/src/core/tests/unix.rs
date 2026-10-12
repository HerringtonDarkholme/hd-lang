use super::super::{run_process, run_process_with_limit};
use std::io;

fn shell(script: &str) -> Vec<String> {
    vec!["-c".into(), script.into()]
}

#[test]
fn worker_creation_failures_cancel_and_reap_before_scope_teardown() {
    use super::super::{PipeWorkers, Spawned, run_process_workers, spawn_process};
    use nix::sys::wait::{WaitPidFlag, waitpid};
    use nix::unistd::Pid;

    for fail_at in 1..=3 {
        // Quiet stdout/stderr force existing readers to wait. The
        // independent finite child lifetime bounds a broken teardown,
        // without a program deadline masking the creation failure.
        let Spawned {
            mut child,
            stdin,
            stdout,
            stderr,
        } = spawn_process("/bin/sh", &shell("exec sleep 2"), None).unwrap();
        let pid = Pid::from_raw(i32::try_from(child.id()).unwrap());
        let mut workers = PipeWorkers {
            attempts: 0,
            fail_at: Some(fail_at),
        };
        let result = run_process_workers(
            &mut child,
            (stdin, stdout, stderr),
            &[],
            1024,
            None,
            &mut workers,
        );
        // Observe reaping before cleanup so cleanup cannot make the
        // assertion pass. Bind the observations to locals, release
        // the fixture, and only then assert, so a failure cannot
        // leave the `sleep 2` child unreaped by this test.
        let reaped = waitpid(pid, Some(WaitPidFlag::WNOHANG));
        let observed_attempts = workers.attempts;
        let observed_error = result.unwrap_err().to_string();
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(observed_attempts, fail_at);
        assert_eq!(observed_error, "injected pipe-worker creation failure");
        assert_eq!(
            reaped,
            Err(nix::errno::Errno::ECHILD),
            "worker {fail_at}: setup failure must reap before returning"
        );
    }
}

#[test]
fn process_captures_both_streams_exit_code_and_signal() {
    let (out, err, status) =
        run_process("/bin/sh", &shell("printf out; printf err >&2; exit 7"), &[]).unwrap();
    assert_eq!(out, b"out");
    assert_eq!(err, b"err");
    assert_eq!(status, 7);
    let (_, _, status) = run_process("/bin/sh", &shell("kill -TERM $$"), &[]).unwrap();
    assert_eq!(status, 128);
}

#[test]
fn process_drains_output_while_writing_input_and_preserves_early_exit() {
    let script = "dd if=/dev/zero bs=65536 count=4 2>/dev/null; \
                  dd if=/dev/zero bs=65536 count=4 >&2 2>/dev/null; \
                  wc -c";
    let (out, err, status) = run_process("/bin/sh", &shell(script), &vec![0; 262_144]).unwrap();
    assert_eq!(status, 0);
    assert!(out[..262_144].iter().all(|b| *b == 0));
    assert_eq!(String::from_utf8_lossy(&out[262_144..]).trim(), "262144");
    assert_eq!(err.len(), 262_144);
    assert!(err.iter().all(|b| *b == 0));
    let (out, err, status) = run_process(
        "/bin/sh",
        &shell("exec 0<&-; printf out; printf err >&2; exit 7"),
        &vec![0; 262_144],
    )
    .unwrap();
    assert_eq!(out, b"out");
    assert_eq!(err, b"err");
    assert_eq!(status, 7);
}

#[test]
fn output_cap_is_aggregate_and_stops_an_infinite_writer() {
    let (out, err, _) =
        run_process_with_limit("/bin/sh", &shell("printf 12345; printf 67890 >&2"), &[], 10)
            .unwrap();
    assert_eq!(out, b"12345");
    assert_eq!(err, b"67890");
    let err = run_process_with_limit("/bin/sh", &shell("printf 12345; printf 67890 >&2"), &[], 9)
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::FileTooLarge);
    let err = run_process_with_limit("/bin/sh", &shell("yes"), &[], 8192).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::FileTooLarge);
}

#[test]
fn output_cap_closes_quiet_pipes_inherited_by_a_descendant() {
    let pid_file = std::env::temp_dir().join(format!(
        "hd-process-cap-descendant-{}-{}.pid",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    // The explicit stdin redirection prevents the shell's background
    // command from replacing stdin with /dev/null. sleep retains both
    // stdin and quiet stderr, but neither consumes nor produces data.
    let script = "exec 3<&0; sleep 30 0<&3 1>/dev/null & \
                  printf '%s' \"$!\" > \"$1\"; \
                  printf 123456789; wait";
    let args = vec![
        "-c".into(),
        script.into(),
        "hd-cap-test".into(),
        pid_file.to_str().unwrap().into(),
    ];
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = run_process_with_limit("/bin/sh", &args, &vec![0; 262_144], 8);
        let _ = tx.send(result);
    });
    let result = rx.recv_timeout(std::time::Duration::from_secs(10));
    // Always release the test-owned descendant before asserting, so
    // a regression fails promptly instead of leaving blocked workers.
    // The 10 s bound is generous under load; overflow itself is
    // event-driven (the 9-byte write), not wall-clock.
    let pid = std::fs::read_to_string(&pid_file)
        .unwrap()
        .parse::<i32>()
        .unwrap();
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid),
        nix::sys::signal::Signal::SIGTERM,
    );
    std::fs::remove_file(pid_file).unwrap();
    worker.join().unwrap();
    assert_eq!(
        result.unwrap().unwrap_err().kind(),
        io::ErrorKind::FileTooLarge
    );
}

#[test]
fn overflow_event_itself_cancels_inherited_quiet_pipe_workers() {
    use super::super::{
        ReadEvent, Spawned, drain, feed_input, spawn_process, wait_pipe, wait_process_and_pipes,
    };
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let pid_file = std::env::temp_dir().join(format!(
        "hd-process-overflow-event-{}-{}.pid",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let args = vec![
        "-c".into(),
        "exec 3<&0; sleep 30 0<&3 1>/dev/null & \
         printf '%s' \"$!\" > \"$1\"; printf 123456789; wait"
            .into(),
        "hd-overflow-event-test".into(),
        pid_file.to_str().unwrap().into(),
    ];
    let start = std::time::Instant::now();
    let (completed, completion) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let Spawned {
            mut child,
            mut stdin,
            mut stdout,
            mut stderr,
        } = spawn_process("/bin/sh", &args, None).unwrap();
        let cancelled = AtomicBool::new(false);
        let count = AtomicUsize::new(0);
        let result = std::thread::scope(|scope| {
            let (tx, rx) = std::sync::mpsc::channel();
            let reader_tx = tx.clone();
            let writer_tx = tx.clone();
            let stop = &cancelled;
            let count = &count;
            let reader = scope.spawn(move || drain(&mut *stderr, count, &reader_tx, 8, stop));
            let writer = scope.spawn(move || {
                let result = feed_input(&mut *stdin, &vec![0; 262_144], stop);
                let _ = writer_tx.send(ReadEvent::Done);
                result
            });
            let mut bytes = [0; 9];
            let mut length = 0;
            while length < bytes.len() {
                match stdout.read(&mut bytes[length..]) {
                    Ok(0) => panic!("child closed stdout before producing overflow"),
                    Ok(n) => length += n,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        wait_pipe(stdout.as_fd(), nix::poll::PollFlags::POLLIN, stop).unwrap();
                    }
                    Err(error) => panic!("reading child overflow: {error}"),
                }
            }
            assert_eq!(&bytes, b"123456789");
            assert!(!cancelled.load(Ordering::Relaxed));
            // Deliberately do NOT pre-set cancellation in the producer:
            // the controller must make an overflow event sufficient.
            tx.send(ReadEvent::Overflow).unwrap();
            drop(tx);
            let result = wait_process_and_pipes(&mut child, &rx, stop, None);
            reader.join().unwrap().unwrap();
            writer.join().unwrap().unwrap();
            result
        });
        completed
            .send((result, cancelled.load(Ordering::Relaxed)))
            .unwrap();
    });
    let result = completion.recv_timeout(std::time::Duration::from_secs(10));
    // This external bound also releases the fixture if the controller
    // forgets cancellation, leaving quiet descendant pipes blocked.
    // Generous under load; the overflow itself is event-driven.
    let pid = std::fs::read_to_string(&pid_file)
        .unwrap()
        .parse::<i32>()
        .unwrap();
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid),
        nix::sys::signal::Signal::SIGKILL,
    );
    worker.join().unwrap();
    std::fs::remove_file(pid_file).unwrap();
    let (status, cancelled) = result.unwrap();
    assert_eq!(status.unwrap(), (128, true));
    assert!(cancelled);
    // Generous completion bound: overflow cancellation is prompt via
    // the event channel; the 5 s wall bound only proves no hang.
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
}

fn ready_process<T>(
    args: &[String],
    exited: bool,
    decode: impl FnOnce(&[u8]) -> io::Result<T>,
) -> io::Result<(super::super::Spawned, T)> {
    use super::super::spawn_process;
    use std::io::Read;
    use std::time::{Duration, Instant};

    let mut spawned = spawn_process("/bin/sh", args, None)?;
    // Generous setup bound: under load (e.g. conformance running
    // alongside) child startup plus pipe polling can exceed 2 s.
    // The test proves deadline/overflow cancellation, not fast setup.
    let until = Instant::now() + Duration::from_secs(10);
    let ready = (|| {
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 64];
        loop {
            if Instant::now() >= until {
                return Err(io::ErrorKind::TimedOut.into());
            }
            match spawned.stdout.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    bytes.extend_from_slice(&buffer[..n]);
                    if bytes.len() > 64 {
                        return Err(io::ErrorKind::InvalidData.into());
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error),
            }
        }
        if exited {
            while spawned.child.try_wait()?.is_none() {
                if Instant::now() >= until {
                    return Err(io::ErrorKind::TimedOut.into());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        decode(&bytes)
    })();
    match ready {
        Ok(value) => Ok((spawned, value)),
        Err(error) => {
            // Cleanup precedes every setup assertion and scoped join.
            let _ = spawned.child.kill();
            let _ = spawned.child.wait();
            Err(error)
        }
    }
}

#[test]
fn deadline_kills_and_reaps_a_term_ignoring_child_after_all_pipes_close() {
    use super::super::{PipeWorkers, Spawned, run_process_workers};
    let args = shell(
        "trap '' TERM; exec 0<&- 2>&-; printf 'ready\\n'; \
         exec 1>&-; while :; do :; done",
    );
    // The marker follows stdin/stderr closure; stdout EOF proves its
    // closure too. No deadline runs while the child is starting.
    let (spawned, ()) = ready_process(&args, false, |bytes| {
        if bytes == b"ready\n" {
            Ok(())
        } else {
            Err(io::ErrorKind::InvalidData.into())
        }
    })
    .expect("TERM-ignoring fixture closes all pipes before its setup bound");
    let pid = nix::unistd::Pid::from_raw(i32::try_from(spawned.child.id()).unwrap());
    let start = std::time::Instant::now();
    let deadline = start + std::time::Duration::from_millis(250);
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let Spawned {
            mut child,
            stdin,
            stdout,
            stderr,
        } = spawned;
        let result = run_process_workers(
            &mut child,
            (stdin, stdout, stderr),
            &[],
            8192,
            Some(deadline),
            &mut PipeWorkers::default(),
        );
        let _ = tx.send(result);
    });
    let result = rx.recv_timeout(std::time::Duration::from_secs(10));
    // On regression, release this test's child before joining the
    // worker, so failure cannot leave a TERM-ignoring busy loop behind.
    if result.is_err() {
        let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGKILL);
    }
    worker.join().unwrap();
    assert_eq!(result.unwrap().unwrap_err().kind(), io::ErrorKind::TimedOut);
    // Generous completion bound: the 250 ms program deadline is the
    // behaviour under test; the 5 s wall bound only proves the worker
    // does not hang under load.
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
    assert_eq!(
        nix::sys::wait::waitpid(pid, Some(nix::sys::wait::WaitPidFlag::WNOHANG)),
        Err(nix::errno::Errno::ECHILD),
        "the deadline path must reap its direct child"
    );
}

#[test]
fn deadline_cancels_inherited_quiet_pipes_after_the_direct_child_exits() {
    use super::super::{PipeWorkers, Spawned, run_process_workers};
    let pid_file = std::env::temp_dir().join(format!(
        "hd-process-deadline-descendant-{}-{}.pid",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let args = vec![
        "-c".into(),
        "exec 3<&0; sleep 30 0<&3 1>/dev/null & \
         printf '%s' \"$!\" > \"$1\"; printf '%s\\n' \"$!\"; exit 0"
            .into(),
        "hd-deadline-test".into(),
        pid_file.to_str().unwrap().into(),
    ];
    // The PID travels on stdout. Its EOF plus try_wait proves the
    // direct child has exited while the descendant retains quiet
    // stdin/stderr. The file is only a setup-failure cleanup fallback.
    let setup = ready_process(&args, true, |bytes| {
        let pid = std::str::from_utf8(bytes)
            .ok()
            .and_then(|text| text.trim().parse::<i32>().ok())
            .filter(|pid| *pid > 1)
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
        Ok(nix::unistd::Pid::from_raw(pid))
    });
    if setup.is_err()
        && let Some(pid) = std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|text| text.parse::<i32>().ok())
            .filter(|pid| *pid > 1)
    {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    if setup.is_err() {
        let _ = std::fs::remove_file(&pid_file);
    }
    let (spawned, pid) = setup.expect("inherited-pipe fixture exits before its setup bound");
    let start = std::time::Instant::now();
    let deadline = start + std::time::Duration::from_millis(250);
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let Spawned {
            mut child,
            stdin,
            stdout,
            stderr,
        } = spawned;
        let result = run_process_workers(
            &mut child,
            (stdin, stdout, stderr),
            &vec![0; 262_144],
            8192,
            Some(deadline),
            &mut PipeWorkers::default(),
        );
        let _ = tx.send(result);
    });
    let result = rx.recv_timeout(std::time::Duration::from_secs(10));
    // The descendant is intentionally independent of the direct
    // child. Release the fixture even if cancellation regresses.
    let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGKILL);
    worker.join().unwrap();
    std::fs::remove_file(pid_file).unwrap();
    assert_eq!(result.unwrap().unwrap_err().kind(), io::ErrorKind::TimedOut);
    // Generous completion bound: the 250 ms program deadline is the
    // behaviour under test; the 5 s wall bound only proves the worker
    // does not hang under load.
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn no_shebang_executable_uses_rust_standard_spawning_behavior() {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::temp_dir().join(format!(
        "hd-process-no-shebang-{}-{}.sh",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    file.write_all(b"printf '%s' \"$1\"; exit 6\n").unwrap();
    file.set_permissions(std::fs::Permissions::from_mode(0o700))
        .unwrap();
    drop(file);
    let expected = std::process::Command::new(&path).arg("argument").output();
    let result = run_process(path.to_str().unwrap(), &["argument".into()], &[]);
    std::fs::remove_file(&path).unwrap();
    if let Ok(expected) = expected {
        let (out, err, status) = result.unwrap();
        assert_eq!(out, expected.stdout);
        assert_eq!(err, expected.stderr);
        assert_eq!(status, i64::from(expected.status.code().unwrap_or(128)));
    } else {
        assert_eq!(result.unwrap_err().kind(), expected.unwrap_err().kind());
    }
}

#[test]
fn child_cwd_is_instance_local_and_arguments_are_not_shell_syntax() {
    let owner = crate::testing::State::new(crate::testing::TestCase::default());
    let cwd = std::path::PathBuf::from(owner.temp_dir().unwrap());
    let parent = std::env::current_dir().unwrap();
    let (out, err, status) =
        super::super::run_process_at("/bin/sh", &shell("pwd"), &[], 1024, None, Some(&cwd))
            .unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap().trim(),
        cwd.canonicalize().unwrap().to_str().unwrap()
    );
    assert!(err.is_empty());
    assert_eq!(status, 0);
    let argument = "literal; $(echo not-executed)";
    let args = vec![
        "-c".into(),
        "printf '%s' \"$1\"".into(),
        "fixture".into(),
        argument.into(),
    ];
    let (out, _, _) =
        super::super::run_process_at("/bin/sh", &args, &[], 1024, None, Some(&cwd)).unwrap();
    assert_eq!(out, argument.as_bytes());
    assert_eq!(std::env::current_dir().unwrap(), parent);
}
