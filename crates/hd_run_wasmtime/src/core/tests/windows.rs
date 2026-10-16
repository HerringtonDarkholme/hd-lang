use super::{offload, request, run};
use crate::reactor::Reactor;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn fixture_args(name: &str, path: Option<&std::path::Path>) -> Vec<String> {
    let mut args = vec![
        "--exact".into(),
        format!("core::tests::windows::{name}"),
        "--ignored".into(),
        "--nocapture".into(),
    ];
    if let Some(path) = path {
        // libtest accepts arbitrary skip filters. This one never
        // matches the selected fixture and carries its private path
        // without mutating the parent process's environment.
        args.extend([
            "--skip".into(),
            format!("hd-process-fixture:{}", path.display()),
        ]);
    }
    args
}

fn fixture_path() -> PathBuf {
    std::env::args()
        .find_map(|arg| arg.strip_prefix("hd-process-fixture:").map(PathBuf::from))
        .expect("child fixture path")
}

fn fixture_watchdog() {
    // A broken implementation must not leave this test executable
    // alive indefinitely, even if the external assertion fires.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(8));
        std::process::exit(99);
    });
}

fn bounded(test: impl FnOnce() + Send + 'static) {
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(test));
        let _ = tx.send(result);
    });
    let result = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("Windows Process fixture blocked");
    worker.join().unwrap();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[test]
fn no_limit_process_preserves_binary_output_input_and_exit_status() {
    bounded(|| {
        let input = vec![b'x'; 262_144];
        let (out, err, status) = run(request(
            std::env::current_exe().unwrap().to_str().unwrap(),
            &fixture_args("child_echo", None),
            &input,
        ))
        .unwrap();
        // libtest prints a short preamble before running the fixture.
        // The payload itself must remain binary-exact, not normalized.
        let payload = &out[out.len() - 524_288..];
        assert!(payload[..262_144].iter().all(|byte| *byte == 0xff));
        assert_eq!(&payload[262_144..], input);
        assert_eq!(err, vec![0; 262_144]);
        assert_eq!(status, 23);
    });
}

#[test]
fn no_limit_process_preserves_output_when_the_child_closes_stdin() {
    bounded(|| {
        let (out, err, status) = run(request(
            std::env::current_exe().unwrap().to_str().unwrap(),
            &fixture_args("child_exit", None),
            &vec![0; 262_144],
        ))
        .unwrap();
        assert!(out.ends_with(b"early stdout"));
        assert_eq!(err, b"early stderr");
        assert_eq!(status, 17);
    });
}

/// `std-process.cancel`: the child exits while its descendant keeps the
/// quiet pipes open, so the run stays pending. Aborting it closes the
/// child's Job object, which kills the descendant: it never sees the
/// release file and never writes its `closed` marker.
#[test]
fn abort_kills_a_descendant_holding_the_pipes() {
    let path = std::env::temp_dir().join(format!(
        "hd-process-windows-descendant-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let args = fixture_args("child_descendant", Some(&path));
    let mut reactor = Reactor::new(None);
    let h = offload(
        &mut reactor,
        request(
            std::env::current_exe().unwrap().to_str().unwrap(),
            &args,
            &vec![0; 262_144],
        ),
    );
    // Generous setup bound under load.
    let until = Instant::now() + Duration::from_secs(10);
    while !path.with_extension("ready").is_file() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    let ready = path.with_extension("ready").is_file();
    let started = Instant::now();
    reactor.remove(h);
    let aborted = started.elapsed();
    // Release the descendant: a live one writes `closed` within
    // milliseconds; a killed one never does. It also has an independent
    // eight-second watchdog.
    std::fs::write(&path, []).unwrap();
    std::thread::sleep(Duration::from_secs(1));
    let closed = path.with_extension("closed").is_file();
    let _ = std::fs::remove_file(path.with_extension("ready"));
    let _ = std::fs::remove_file(path.with_extension("closed"));
    std::fs::remove_file(path).unwrap();
    assert!(
        ready,
        "fixture failed to create its inherited-pipe descendant"
    );
    assert!(!closed, "the descendant outlived the abort");
    assert!(aborted < Duration::from_secs(5), "{aborted:?}");
}

#[test]
#[ignore = "subprocess fixture, invoked by the Windows Process tests"]
fn child_echo() {
    fixture_watchdog();
    io::stdout().write_all(&vec![0xff; 262_144]).unwrap();
    io::stderr().write_all(&vec![0; 262_144]).unwrap();
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input).unwrap();
    io::stdout().write_all(&input).unwrap();
    std::process::exit(23);
}

#[test]
#[ignore = "subprocess fixture, invoked by the Windows Process tests"]
fn child_exit() {
    io::stdout().write_all(b"early stdout").unwrap();
    io::stderr().write_all(b"early stderr").unwrap();
    std::process::exit(17);
}

#[test]
#[ignore = "subprocess fixture, invoked by the Windows Process tests"]
fn child_descendant() {
    fixture_watchdog();
    let path = fixture_path();
    let _descendant = Command::new(std::env::current_exe().unwrap())
        .args(fixture_args("child_quiet", Some(&path)))
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    std::fs::write(path.with_extension("ready"), []).unwrap();
    std::process::exit(0);
}

#[test]
#[ignore = "subprocess fixture, invoked by the Windows Process tests"]
fn child_quiet() {
    fixture_watchdog();
    let path = fixture_path();
    while !path.is_file() {
        std::thread::sleep(Duration::from_millis(5));
    }
    std::fs::write(path.with_extension("closed"), []).unwrap();
    std::process::exit(0);
}
