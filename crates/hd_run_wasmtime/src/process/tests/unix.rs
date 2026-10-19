use super::{offload, request, run};
use crate::runtime::reactor::Reactor;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rustix::io::Errno;
use rustix::process::{Pid, Signal, kill_process, test_kill_process};

fn shell(script: &str) -> Vec<String> {
    vec!["-c".into(), script.into()]
}

/// A private temporary path for a fixture's process IDs.
fn pid_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "hd-process-{label}-{}-{}.pid",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

/// The process IDs a fixture wrote to `path`, once it has written them
/// all: within a generous bound for a loaded machine.
fn read_pids(path: &Path, count: usize) -> Vec<Pid> {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let pids: Vec<Pid> = std::fs::read_to_string(path)
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|pid| pid.parse::<i32>().ok())
            .filter(|pid| *pid > 1)
            .filter_map(Pid::from_raw)
            .collect();
        if pids.len() == count {
            return pids;
        }
        assert!(Instant::now() < until, "the fixture wrote no process IDs");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Whether `pid` no longer exists, within a generous bound: a killed
/// process stays a zombie until its parent reaps it, the host's reaper
/// task for the child, init for an orphaned descendant.
fn gone(pid: Pid) -> bool {
    let until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < until {
        if test_kill_process(pid) == Err(Errno::SRCH) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// Kills a fixture process this test started, if it is still running, so
/// a failing test leaves nothing behind.
fn release(pids: &[Pid]) {
    for pid in pids {
        let _ = kill_process(*pid, Signal::KILL);
    }
}

#[test]
fn process_captures_both_streams_exit_code_and_signal() {
    let (out, err, status) = run(request(
        "/bin/sh",
        &shell("printf out; printf err >&2; exit 7"),
        &[],
    ))
    .unwrap();
    assert_eq!(out, b"out");
    assert_eq!(err, b"err");
    assert_eq!(status, 7);
    let (_, _, status) = run(request("/bin/sh", &shell("kill -TERM $$"), &[])).unwrap();
    assert_eq!(status, 143);
    let (_, _, status) = run(request("/bin/sh", &shell("kill -KILL $$"), &[])).unwrap();
    assert_eq!(status, 137);
    let (_, _, status) = run(request("/bin/sh", &shell("exit 3"), &[])).unwrap();
    assert_eq!(status, 3);
}

#[test]
fn process_drains_output_while_writing_input_and_preserves_early_exit() {
    let script = "dd if=/dev/zero bs=65536 count=4 2>/dev/null; \
                  dd if=/dev/zero bs=65536 count=4 >&2 2>/dev/null; \
                  wc -c";
    let (out, err, status) = run(request("/bin/sh", &shell(script), &vec![0; 262_144])).unwrap();
    assert_eq!(status, 0);
    assert!(out[..262_144].iter().all(|b| *b == 0));
    assert_eq!(String::from_utf8_lossy(&out[262_144..]).trim(), "262144");
    assert_eq!(err.len(), 262_144);
    assert!(err.iter().all(|b| *b == 0));
    let (out, err, status) = run(request(
        "/bin/sh",
        &shell("exec 0<&-; printf out; printf err >&2; exit 7"),
        &vec![0; 262_144],
    ))
    .unwrap();
    assert_eq!(out, b"out");
    assert_eq!(err, b"err");
    assert_eq!(status, 7);
}

/// `std-process.cancel`: aborting a pending run kills the child, which
/// ignores SIGTERM, and its descendant, and reaps the child.
#[test]
fn abort_kills_the_process_group_and_reaps_the_child() {
    let path = pid_path("abort-group");
    let script = "trap '' TERM; sleep 30 & \
                  printf '%s %s' \"$$\" \"$!\" > \"$1\"; wait";
    let args = vec![
        "-c".into(),
        script.into(),
        "hd-abort-test".into(),
        path.to_str().unwrap().into(),
    ];
    let mut reactor = Reactor::new(None);
    let h = offload(&mut reactor, request("/bin/sh", &args, &[]));
    let pids = read_pids(&path, 2);
    let started = Instant::now();
    assert!(reactor.remove(h).is_some(), "the abort ends the run");
    let aborted = started.elapsed();
    let (child, descendant) = (gone(pids[0]), gone(pids[1]));
    release(&pids);
    std::fs::remove_file(&path).unwrap();
    assert!(child, "the child is still running or not reaped");
    assert!(descendant, "the descendant outlived the abort");
    assert!(!reactor.waiting(), "nothing pending");
    // Generous: the abort waits only for the task's drop.
    assert!(aborted < Duration::from_secs(5), "{aborted:?}");
}

/// The child exits while its descendant keeps the quiet pipes open, so
/// the run stays pending. The child is not reaped yet, so the abort can
/// still kill its group.
#[test]
fn abort_after_the_child_exits_kills_a_descendant_holding_the_pipes() {
    let path = pid_path("abort-descendant");
    let script = "exec 3<&0; sleep 30 0<&3 1>/dev/null & \
                  printf '%s' \"$!\" > \"$1\"; exit 0";
    let args = vec![
        "-c".into(),
        script.into(),
        "hd-abort-descendant".into(),
        path.to_str().unwrap().into(),
    ];
    let mut reactor = Reactor::new(None);
    let h = offload(&mut reactor, request("/bin/sh", &args, &vec![0; 262_144]));
    let pids = read_pids(&path, 1);
    let mut out = Vec::new();
    assert!(
        !reactor.due(&mut out).contains(&h),
        "the run waits for the pipes"
    );
    reactor.remove(h);
    let descendant = gone(pids[0]);
    release(&pids);
    std::fs::remove_file(&path).unwrap();
    assert!(descendant, "the descendant outlived the abort");
}

#[test]
fn no_shebang_executable_uses_rust_standard_spawning_behavior() {
    let path = pid_path("no-shebang").with_extension("sh");
    // Other tests fork concurrently. A writer opened in this process can
    // be inherited by their children before exec closes it, making either
    // spawn below fail with ETXTBSY. Only this helper child opens the
    // writer, and it exits before either spawn, so neither can race it.
    let created = std::process::Command::new("/bin/sh")
        .args([
            "-c",
            "umask 077; set -C; printf '%s\\n' \"$2\" > \"$1\" && chmod 700 \"$1\"",
            "hd-no-shebang-fixture",
        ])
        .arg(&path)
        .arg("printf '%s' \"$1\"; exit 6")
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "cannot create fixture: {}",
        String::from_utf8_lossy(&created.stderr)
    );
    let expected = std::process::Command::new(&path).arg("argument").output();
    let result = run(request(path.to_str().unwrap(), &["argument".into()], &[]));
    std::fs::remove_file(&path).unwrap();
    if let Ok(expected) = expected {
        let (out, err, status) = result.unwrap();
        assert_eq!(out, expected.stdout);
        assert_eq!(err, expected.stderr);
        assert_eq!(status, expected.status.code().unwrap_or(128));
    } else {
        assert_eq!(result.unwrap_err().kind(), expected.unwrap_err().kind());
    }
}

#[test]
fn child_cwd_is_instance_local_and_arguments_are_not_shell_syntax() {
    let owner = crate::testing::State::new(crate::testing::TestCase::default());
    let cwd = PathBuf::from(owner.temp_dir().unwrap());
    let parent = std::env::current_dir().unwrap();
    let in_cwd = |argv: &[String]| {
        let mut request = request("/bin/sh", argv, &[]);
        request.cwd = Some(cwd.clone());
        run(request).unwrap()
    };
    let (out, err, status) = in_cwd(&shell("pwd"));
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
    let (out, _, _) = in_cwd(&args);
    assert_eq!(out, argument.as_bytes());
    assert_eq!(std::env::current_dir().unwrap(), parent);
}
