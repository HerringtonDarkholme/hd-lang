//! CLI flag routing and observable Wasmtime limit failures, using raw Wasm.
//! The tests that run Wasmtime need the `wasmtime` feature.
use std::fmt::Write as _;
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
        .env_remove("HD_ENGINE")
        .env_remove("HD_MAX_MEMORY");
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

fn module(init: &[u8]) -> Vec<u8> {
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

#[cfg(feature = "wasmtime")]
#[test]
fn heap_growth_and_initial_memory_fail_as_panics() {
    let dir = work("heap");
    std::fs::write(dir.join("main.wasm"), module(&[0x41, 2, 0x40, 0, 0x1a])).unwrap();
    for limit in ["0", "64K"] {
        let mut command = hd(&dir);
        command.args(["main.wasm", "--engine=wasmtime", "--max-heap", limit]);
        let output = bounded(command);
        assert_eq!(output.status.code(), Some(3), "{output:?}");
        assert_eq!(
            output.stderr,
            b"panic: heap-exhausted: the program heap cannot allocate more memory\n"
        );
    }
    let mut command = hd(&dir);
    command.args(["main.wasm", "--engine=wasmtime"]);
    assert!(bounded(command).status.success());
}

#[test]
fn compiler_memory_reports_once_on_check_json_stream() {
    let dir = work("memory-json");
    let mut command = hd(&dir);
    command.args(["check", "missing.hd", "--max-memory=0", "--format=json"]);
    let output = bounded(command);
    assert_eq!(output.status.code(), Some(101), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        text.matches("\"code\":\"memory-limit\"").count(),
        1,
        "{text}"
    );
    assert!(text.contains("starting compiler"), "{text}");
    assert!(output.stderr.is_empty(), "{output:?}");
}

/// `cli.memory.cap`, `cli.memory.cap.error`: in a workspace `hd test`, the
/// first member's tests disarm the compiler cap for their run; the second
/// member's compile is limited again. Member `a` alone fits in 32 MiB
/// (about 17 MiB); `b`'s 10000 functions need about 68 MiB to compile.
#[test]
fn compiler_memory_cap_limits_each_workspace_members_compile() {
    let dir = work("memory-members");
    let test = "pub fn one() -> i32:\n    1\n\ntests:\n    use std.testing.assert_equal\n\n    it(\"adds\"):\n        assert_equal(one(), 1, reason=\"one\")\n";
    let mut big = String::new();
    for i in 0..10_000 {
        writeln!(big, "pub fn g{i}(x: i32) -> i32:\n    x + {i}\n").unwrap();
    }
    big.push_str(&test[test.find("tests:").unwrap()..]);
    for (path, text) in [
        ("hd.toml", "[workspace]\nmembers = [\"a\", \"b\"]\n"),
        ("a/hd.toml", "[package]\nname = \"a\"\n"),
        ("a/src/lib.hd", test),
        ("b/hd.toml", "[package]\nname = \"b\"\n"),
        ("b/src/lib.hd", &big.replace("one()", "g0(1)")),
    ] {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    // Each run gets a cold cache, so no run reuses another's compile.
    let run = |cache: &str, args: &[&str]| {
        let mut command = hd(&dir);
        command
            .env("HD_CACHE", dir.join(cache))
            .args(["test", "--jobs", "1"])
            .args(args);
        bounded(command)
    };
    let output = run("cache-a", &["-p", "a", "--max-memory=32M"]);
    assert!(output.status.success(), "{output:?}");
    let output = run("cache-tight", &["--max-memory=32M"]);
    assert_eq!(output.status.code(), Some(101), "{output:?}");
    let error = String::from_utf8_lossy(&output.stderr);
    assert_eq!(error.lines().count(), 1, "{error}");
    assert!(
        error.starts_with(
            "error: memory-limit: compiler memory limit of 33554432 bytes exceeded while "
        ),
        "{error}"
    );
    let output = run("cache-generous", &["--max-memory=256M"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "test result: ok. 2 passed; 0 failed; 0 ignored\n"
    );
}

#[cfg(feature = "wasmtime")]
#[test]
fn explicit_compiler_limit_overrides_invalid_environment() {
    let dir = work("env-override");
    std::fs::write(dir.join("main.wasm"), module(&[])).unwrap();
    let mut command = hd(&dir);
    command.env("HD_MAX_MEMORY", "invalid").args([
        "--max-memory=18446744073709551615",
        "main.wasm",
        "--engine=wasmtime",
    ]);
    assert!(bounded(command).status.success());
    let mut command = hd(&dir);
    command
        .env("HD_MAX_MEMORY", "invalid")
        .args(["main.wasm", "--engine=wasmtime"]);
    let output = bounded(command);
    assert_eq!(output.status.code(), Some(101));
    assert!(String::from_utf8_lossy(&output.stderr).contains("HD_MAX_MEMORY"));
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
        "--max-heap=0",
    ]);
    let output = bounded(command);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("panic: heap-exhausted:"));
}

#[cfg(feature = "wasmtime")]
#[test]
fn invalid_runtime_forms_fail_but_program_words_are_not_parsed() {
    let dir = work("separator");
    std::fs::write(dir.join("main.wasm"), module(&[])).unwrap();
    for flag in [
        "--time-limit=1.5s",
        "--max-heap=-1",
        "--time-limit=18446744073709551615h",
    ] {
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

#[test]
fn node_limits_warn_without_enforcement_until_w5() {
    let dir = work("node-warnings");
    std::fs::write(dir.join("main.wasm"), module(&[])).unwrap();
    let mut command = hd(&dir);
    command.args([
        "--engine=node",
        "--max-heap=0",
        "--time-limit=0ms",
        "main.wasm",
    ]);
    let output = bounded(command);
    assert!(output.status.success(), "{output:?}");
    let warning = String::from_utf8_lossy(&output.stderr);
    assert_eq!(warning.matches("warning:").count(), 2, "{warning}");
    assert!(warning.contains("--max-heap is ignored on Node until W5"));
    assert!(warning.contains("--time-limit is ignored on Node until W5"));
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
