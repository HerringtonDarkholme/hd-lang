//! CLI flag routing and observable Wasmtime limit failures, using raw Wasm.
//! The tests that run Wasmtime need the `wasmtime` feature.
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use hd_testkit::Scratch;

fn work(name: &str) -> Scratch {
    Scratch::new(env!("CARGO_TARGET_TMPDIR"), &format!("m21-{name}"))
}

fn hd(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_hd"));
    command
        .current_dir(dir)
        .env("HD_CACHE", dir.join("cache"))
        .env_remove("HD_ENGINE");
    command
}

fn bounded(mut command: Command) -> Output {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!("CLI deadline regression hung: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(crate) fn module(init: &[u8]) -> Vec<u8> {
    // Two functions: `hd.init` () -> (), `hd.poll` (i32) -> i32; one
    // initial 64 KiB memory.
    let mut module = b"\0asm\x01\0\0\0".to_vec();
    for (id, body) in [
        (1, vec![2, 0x60, 0, 0, 0x60, 1, 0x7f, 1, 0x7f]),
        (3, vec![2, 0, 1]),
        (5, vec![1, 0, 1]),
        (
            7,
            vec![
                3, 7, b'h', b'd', b'.', b'i', b'n', b'i', b't', 0, 0, 7, b'h', b'd', b'.', b'p',
                b'o', b'l', b'l', 0, 1, 4, b'h', b'd', b'.', b'x', 2, 0,
            ],
        ),
        (10, {
            let mut body = vec![2, u8::try_from(init.len() + 2).unwrap(), 0];
            body.extend_from_slice(init);
            body.extend_from_slice(&[0x0b, 4, 0, 0x41, 0, 0x0b]);
            body
        }),
    ] {
        module.extend([id, u8::try_from(body.len()).unwrap()]);
        module.extend(body);
    }
    module
}

#[cfg(feature = "wasmtime")]
#[test]
fn prebuilt_deadline_interrupts_busy_init() {
    let dir = work("busy-init");
    std::fs::write(dir.join("main.wasm"), module(&[0x03, 0x40, 0x0c, 0, 0x0b])).unwrap();
    let mut command = hd(&dir);
    command.args(["--engine", "wasmtime", "--time-limit=30ms", "main.wasm"]);
    let output = bounded(command);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert_eq!(
        output.stderr,
        b"panic: time-limit: the program exceeded its time limit\n"
    );
}

/// `hd` has no heap or compiler memory flag: `--max-heap` and
/// `--max-memory` are usage errors, in each place they used to be taken.
#[test]
fn heap_and_memory_flags_are_usage_errors() {
    let dir = work("no-memory-flags");
    std::fs::write(
        dir.join("x.hd"),
        "pub fn main() -> void $ Console:\n    println(\"hi\")\n",
    )
    .unwrap();
    let mut command = hd(&dir);
    command.args(["check", "x.hd"]);
    assert!(bounded(command).status.success());
    for args in [
        &["run", "--max-heap", "1M", "x.hd"][..],
        &["x.hd", "--max-heap=1M"],
        &["check", "--max-memory", "4G", "x.hd"],
        &["--max-memory=4G", "check", "x.hd"],
        &["test", "--max-heap=1M", "x.hd"],
    ] {
        let mut command = hd(&dir);
        command.args(args);
        let output = bounded(command);
        assert_eq!(output.status.code(), Some(101), "{args:?}: {output:?}");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.starts_with("error: "), "{args:?}: {error}");
    }
}

#[cfg(feature = "wasmtime")]
#[test]
fn source_file_and_package_run_route_program_limits() {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let dir = work("source-routes");
    let mut command = hd(&dir);
    command
        .arg(samples.join("hello/hello.hd"))
        .args(["--engine=wasmtime", "--time-limit", "0ms"]);
    let output = bounded(command);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("panic: time-limit:"));
    let mut command = hd(&samples.join("fib"));
    command.env("HD_CACHE", dir.join("package-cache")).args([
        "run",
        "--engine",
        "wasmtime",
        "--time-limit=0ms",
    ]);
    let output = bounded(command);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("panic: time-limit:"));
}

#[cfg(feature = "wasmtime")]
#[test]
fn invalid_runtime_forms_fail_but_program_words_are_not_parsed() {
    let dir = work("separator");
    std::fs::write(dir.join("main.wasm"), module(&[])).unwrap();
    for flag in ["--time-limit=1.5s", "--time-limit=18446744073709551615h"] {
        let mut command = hd(&dir);
        command.args(["--engine=wasmtime", "main.wasm", flag]);
        assert_eq!(bounded(command).status.code(), Some(101));
    }
    let mut command = hd(&dir);
    command.args([
        "--engine=wasmtime",
        "main.wasm",
        "--",
        "--max-memory=bad",
        "--time-limit=bad",
    ]);
    assert!(bounded(command).status.success());
}

#[cfg(not(feature = "wasmtime"))]
#[test]
fn wasmtime_is_refused_without_the_feature() {
    let dir = work("no-wasmtime");
    std::fs::write(dir.join("main.wasm"), module(&[])).unwrap();
    let mut command = hd(&dir);
    command.args(["--engine", "wasmtime", "main.wasm"]);
    let output = bounded(command);
    let message = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(101), "{output:?}");
    assert!(message.contains("no wasmtime engine"), "{message}");
    assert!(message.contains("--features wasmtime"), "{message}");
    let mut command = hd(&dir);
    command.env("HD_ENGINE", "wasmtime").arg("main.wasm");
    let output = bounded(command);
    let message = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(101), "{output:?}");
    assert!(message.contains("HD_ENGINE: this build of hd has no wasmtime engine"));
    // `hd build` resolves its engine as `hd run` does, so it refuses the
    // same way before it builds anything.
    std::fs::write(dir.join("hd.toml"), "[package]\nname = \"shop\"\n").unwrap();
    let mut command = hd(&dir);
    command.args(["build", "--engine", "wasmtime"]);
    let output = bounded(command);
    let message = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(101), "{output:?}");
    assert!(message.contains("no wasmtime engine"), "{message}");
    assert!(message.contains("--features wasmtime"), "{message}");
    assert!(!dir.join("build").exists());
}

/// `cli.engine.select`: `--engine` wins over `HD_ENGINE`, which is read
/// only when no flag is given.
#[test]
fn an_engine_flag_wins_over_a_bogus_hd_engine() {
    let dir = work("engine-flag-wins");
    std::fs::write(dir.join("main.wasm"), module(&[])).unwrap();
    let mut command = hd(&dir);
    command.env("HD_ENGINE", "bogus");
    command.args(["--engine", "node", "main.wasm"]);
    let output = bounded(command);
    assert!(output.status.success(), "{output:?}");
    let mut command = hd(&dir);
    command.env("HD_ENGINE", "bogus");
    command.args(["main.wasm"]);
    let output = bounded(command);
    assert!(!output.status.success(), "{output:?}");
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(message.contains("HD_ENGINE"), "{message}");
}
