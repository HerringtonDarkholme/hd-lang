//! The Node host (`host/run.mjs`) exits once `main` returns, and enforces
//! `--time-limit` (`cli.limit.time`) while the program computes or waits on
//! a host operation (#161).
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use hd_testkit::Scratch;

const TIME_PANIC: &[u8] = b"panic: time-limit: the program exceeded its time limit\n";

fn host() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs")
}

fn hd(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_hd"));
    command
        .current_dir(dir)
        .env("HD_CACHE", dir.join("cache"))
        .env_remove("HD_ENGINE");
    command
}

/// Waits for `child` at most `limit`; a child still running is killed and
/// fails the test.
fn finish(mut child: Child, limit: Duration, what: &str) -> Output {
    let until = Instant::now() + limit;
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!("{what} did not exit within {limit:?}: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn spawn(mut command: Command, stdin: Stdio) -> Child {
    command
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// Builds `source` as the package `name` for V8 and returns its Wasm.
fn build(dir: &Path, name: &str, source: &str) -> PathBuf {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("hd.toml"),
        format!("[package]\nname = \"{name}\"\n"),
    )
    .unwrap();
    std::fs::write(dir.join("src/main.hd"), source).unwrap();
    let mut command = hd(dir);
    command.args(["build", "--engine", "node"]);
    let output = finish(
        spawn(command, Stdio::null()),
        Duration::from_mins(2),
        "hd build",
    );
    assert!(output.status.success(), "{output:?}");
    dir.join(format!("build/debug/{name}.wasm"))
}

#[test]
fn node_host_exits_after_main_returns_every_time() {
    let dir = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "node-exit-loop");
    let wasm = build(
        &dir,
        "exits",
        "pub fn main() -> void $ Console:\n    println(\"done\")\n",
    );
    // With a time limit, the instance runs in a worker thread.
    let limited = dir.join("limited.json");
    std::fs::write(&limited, "{\"timeLimitMs\":60000}").unwrap();
    for round in 0..40 {
        let mut command = Command::new("node");
        command.arg(host()).arg(&wasm);
        if round % 2 == 1 {
            command.arg(&limited);
        }
        let output = finish(
            spawn(command, Stdio::null()),
            Duration::from_secs(30),
            "the Node host",
        );
        assert!(output.status.success(), "round {round}: {output:?}");
        assert_eq!(output.stdout, b"done\n", "round {round}");
        assert_eq!(output.stderr, b"", "round {round}");
    }
}

#[test]
fn node_time_limit_interrupts_busy_init() {
    let dir = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "node-busy-init");
    std::fs::write(
        dir.join("main.wasm"),
        super::resource_limits::module(&[0x03, 0x40, 0x0c, 0, 0x0b]),
    )
    .unwrap();
    let mut command = hd(&dir);
    command.args(["--engine=node", "--time-limit=300ms", "main.wasm"]);
    let output = finish(spawn(command, Stdio::null()), Duration::from_secs(20), "hd");
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert_eq!(output.stderr, TIME_PANIC);
}

#[test]
fn node_time_limit_cuts_a_socket_wait_after_flushing_output() {
    let dir = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "node-accept-wait");
    let wasm = build(
        &dir,
        "accepts",
        "use std.net.Net\nuse std.time.{Clock, sleep, ms}\nuse std.task.race\n\n\
         fn wait_conn!() -> string $ Net:\n    let mut net = $.use(Net)\n    \
         let mut server = match net.listen!(\"127.0.0.1\", 0):\n        .Ok(l) => l\n        \
         .Err(e) => return \"${e}\"\n    match server.accept!():\n        \
         .Ok(_) => \"accepted\"\n        .Err(e) => \"${e}\"\n\n\
         fn nap!() -> string $ Clock:\n    sleep!(ms(500))\n    \"timeout\"\n\n\
         pub fn main!() -> void $ Console + Net + Clock:\n    println(\"before\")\n    \
         println(race!(wait_conn(), nap()))\n",
    );
    let started = Instant::now();
    let mut command = hd(&dir);
    command
        .arg(&wasm)
        .args(["--engine=node", "--cap", "Net=127.0.0.1", "--time-limit=1s"]);
    let output = finish(spawn(command, Stdio::null()), Duration::from_secs(20), "hd");
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert_eq!(output.stdout, b"before\n");
    assert_eq!(output.stderr, TIME_PANIC);
    assert!(started.elapsed() >= Duration::from_secs(1));
}

#[test]
fn node_time_limit_ends_a_read_of_input_that_never_comes() {
    let dir = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "node-input-wait");
    let wasm = build(
        &dir,
        "reads",
        "use std.console.{ConsoleInput, read_line}\n\n\
         pub fn main!() -> void $ Console + ConsoleInput:\n    println(\"prompt\")\n    \
         match read_line!():\n        .Ok(_) => println(\"read\")\n        \
         .Err(_) => println(\"closed\")\n",
    );
    let mut command = hd(&dir);
    command
        .arg(&wasm)
        .args(["--engine=node", "--time-limit=1s"]);
    // The pipe stays open and empty: the process must still exit.
    let mut child = spawn(command, Stdio::piped());
    let input = child.stdin.take().unwrap();
    let output = finish(child, Duration::from_secs(20), "hd");
    drop(input);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert_eq!(output.stdout, b"prompt\n");
    assert_eq!(output.stderr, TIME_PANIC);
}

#[test]
fn node_time_limit_leaves_input_lines_to_the_program() {
    let dir = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "node-input-lines");
    let wasm = build(
        &dir,
        "lines",
        "use std.console.{ConsoleInput, read_line}\n\n\
         pub fn main!() -> void $ Console + ConsoleInput:\n    for _ in 0..3:\n        \
         match read_line!():\n            .Ok(.Some(line)) => println(\"[${line}]\")\n            \
         .Ok(.None) => println(\"end\")\n            .Err(_) => println(\"closed\")\n",
    );
    let mut command = hd(&dir);
    command
        .arg(&wasm)
        .args(["--engine=node", "--time-limit=60s"]);
    let mut child = spawn(command, Stdio::piped());
    let mut input = child.stdin.take().unwrap();
    std::io::Write::write_all(&mut input, b"one\r\ntwo").unwrap();
    drop(input);
    let output = finish(child, Duration::from_secs(30), "hd");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"[one]\n[two]\nend\n");
    assert_eq!(output.stderr, b"");
}

#[cfg(unix)]
#[test]
fn node_time_limit_ends_a_child_process_wait() {
    let dir = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "node-process-wait");
    let wasm = build(
        &dir,
        "waits",
        "use std.process.Process\n\n\
         pub fn main!() -> void $ Console + Process:\n    println(\"started\")\n    \
         match $.use(Process).run!(\"sleep\", [\"30\"], \"\"):\n        .Ok(_) => println(\"ran\")\n        \
         .Err(e) => println(\"${e}\")\n",
    );
    let started = Instant::now();
    let mut command = hd(&dir);
    command
        .arg(&wasm)
        .args(["--engine=node", "--cap", "Process=sleep", "--time-limit=1s"]);
    let output = finish(spawn(command, Stdio::null()), Duration::from_secs(20), "hd");
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert_eq!(output.stdout, b"started\n");
    assert_eq!(output.stderr, TIME_PANIC);
    assert!(started.elapsed() < Duration::from_secs(20));
}

/// A package `probe` with `lib` as its `src/lib.hd`, and `tests` as the
/// integration test file `tests/waits.hd`, run by `hd test` on V8 under
/// `--time-limit=limit`.
fn test_run(name: &str, lib: &str, tests: &str, limit: &str) -> Output {
    let dir = Scratch::new(env!("CARGO_TARGET_TMPDIR"), name);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("tests")).unwrap();
    std::fs::write(dir.join("hd.toml"), "[package]\nname = \"probe\"\n").unwrap();
    std::fs::write(dir.join("src/lib.hd"), lib).unwrap();
    std::fs::write(dir.join("tests/waits.hd"), tests).unwrap();
    let mut command = hd(&dir);
    command.args(["test", "--engine=node", limit]);
    finish(
        spawn(command, Stdio::null()),
        Duration::from_mins(1),
        "hd test",
    )
}

const SPIN: &str = "fn forever(stop: i32) -> i32:
    let total: i32 = 0
    while total != stop:
        total = (total + 1) % 1000
    total

tests:
    use std.testing.{assert, it_each}

    it(\"spins\"):
        _ := forever(-1)

    it(\"fast\"):
        assert(true, reason=\"ok\")

    it_each(\"rows\", [1, -1, 2], body=fn!(value: i32):
        assert(forever(value) == value, reason=\"ends\")
    )
";

/// The report is the one `--engine=wasmtime` prints for the same package.
#[test]
fn node_test_time_limit_fails_a_busy_case_and_row_like_wasmtime() {
    let started = Instant::now();
    let output = test_run("node-test-busy", SPIN, "", "--time-limit=300ms");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "PANIC src/lib.hd:10: spins\n    \
         panic: time-limit: the program exceeded its time limit\n    \
         repro: hd test src/lib.hd --filter \"spins\"\n\
         PANIC src/lib.hd:16: rows[1]\n    \
         panic: time-limit: the program exceeded its time limit\n    \
         repro: hd test src/lib.hd --filter \"rows[1]\"\n\
         test result: FAILED. 3 passed; 2 failed; 0 ignored\n",
        "{output:?}"
    );
    assert_eq!(output.stderr, b"", "{output:?}");
    assert_eq!(output.status.code(), Some(1));
    assert!(started.elapsed() < Duration::from_secs(30));
}

const WAITS: &str = "use std.time.{ms, s, sleep}
use std.net.Net

it(\"sleeps\"):
    println(\"before\")
    sleep!(30s)

it(\"accepts\"):
    let mut net = $.use(Net)
    let mut server = match net.listen!(\"127.0.0.1\", 0):
        .Ok(l) => l
        .Err(_) => return
    match server.accept!():
        .Ok(_) => println(\"accepted\")
        .Err(e) => println(\"${e}\")

it(\"fast\"):
    sleep!(1ms)
";

#[test]
fn node_test_time_limit_cuts_host_waits_at_the_limit() {
    let started = Instant::now();
    let output = test_run(
        "node-test-waits",
        "pub fn one() -> i32:\n    1\n",
        WAITS,
        "--time-limit=500ms",
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "PANIC tests/waits.hd:4: sleeps\n    \
         panic: time-limit: the program exceeded its time limit\n    \
         repro: hd test tests/waits.hd --filter \"sleeps\"\n\
         PANIC tests/waits.hd:8: accepts\n    \
         panic: time-limit: the program exceeded its time limit\n    \
         repro: hd test tests/waits.hd --filter \"accepts\"\n\
         test result: FAILED. 1 passed; 2 failed; 0 ignored\n",
        "{output:?}"
    );
    assert_eq!(output.status.code(), Some(1));
    // Two waits of 30 s and an accept that never ends, cut at 500 ms each.
    assert!(started.elapsed() < Duration::from_secs(30));
}

/// `cli.limit.test`: each case has the whole limit on its own.
#[test]
fn node_test_time_limit_applies_to_each_case_on_its_own() {
    let tests = "use std.time.{ms, sleep}

it(\"first\"):
    sleep!(ms(400))

it(\"second\"):
    sleep!(ms(400))

it(\"third\"):
    sleep!(ms(400))
";
    let output = test_run(
        "node-test-each",
        "pub fn one() -> i32:\n    1\n",
        tests,
        "--time-limit=1s",
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "test result: ok. 3 passed; 0 failed; 0 ignored\n",
        "{output:?}"
    );
    assert_eq!(output.status.code(), Some(0));
}

/// Makes the FIFO `path`, which no process writes to.
#[cfg(unix)]
fn fifo(path: &Path) {
    let status = Command::new("mkfifo").arg(path).status().unwrap();
    assert!(status.success(), "mkfifo {}", path.display());
}

/// A read of a FIFO with no writer blocks a host worker thread in a system
/// call that the time limit cannot cut (#229): the run still ends.
#[cfg(unix)]
#[test]
fn node_time_limit_ends_a_run_blocked_in_a_file_read() {
    let dir = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "node-fifo-read");
    let wasm = build(
        &dir,
        "fifo",
        "use std.fs.{FsRead, read_text}\nuse std.path.Path\n\n\
         pub fn main!() -> void $ Console + FsRead:\n    println(\"before\")\n    \
         match read_text!(Path(\"pipe/ff\")):\n        .Ok(t) => println(t)\n        \
         .Err(e) => println(\"${e}\")\n",
    );
    std::fs::create_dir_all(dir.join("pipe")).unwrap();
    fifo(&dir.join("pipe/ff"));
    let mut command = hd(&dir);
    command
        .arg(&wasm)
        .args(["--engine=node", "--cap", "FsRead=pipe", "--time-limit=1s"]);
    let output = finish(spawn(command, Stdio::null()), Duration::from_secs(20), "hd");
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert_eq!(output.stdout, b"before\n");
    assert_eq!(output.stderr, TIME_PANIC);
}

/// `hd test` ends too once a case blocked in a FIFO read is cut (#229).
#[cfg(unix)]
#[test]
fn node_test_time_limit_ends_a_run_blocked_in_a_file_read() {
    let dir = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "node-test-fifo-read");
    let package = dir.join("probe");
    std::fs::create_dir_all(package.join("src")).unwrap();
    std::fs::create_dir_all(package.join("tests")).unwrap();
    std::fs::write(package.join("hd.toml"), "[package]\nname = \"probe\"\n").unwrap();
    std::fs::write(package.join("src/lib.hd"), "pub fn one() -> i32:\n    1\n").unwrap();
    std::fs::write(
        package.join("tests/reads.hd"),
        "use std.fs.{FsRead, read_text}\nuse std.path.Path\n\n\
         it(\"reads\"):\n    match read_text!(Path(\"../ff\")):\n        \
         .Ok(t) => println(t)\n        .Err(e) => println(\"${e}\")\n\n\
         it(\"fast\"):\n    println(\"fast\")\n",
    )
    .unwrap();
    // Outside the package, whose files `hd test` reads.
    fifo(&dir.join("ff"));
    let mut command = hd(&package);
    command.args([
        "test",
        "--engine=node",
        "--cap",
        "FsRead=../ff",
        "--time-limit=1s",
    ]);
    let output = finish(
        spawn(command, Stdio::null()),
        Duration::from_mins(1),
        "hd test",
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "PANIC tests/reads.hd:4: reads\n    \
         panic: time-limit: the program exceeded its time limit\n    \
         repro: hd test tests/reads.hd --filter \"reads\"\n\
         test result: FAILED. 1 passed; 1 failed; 0 ignored\n",
        "{output:?}"
    );
    assert_eq!(output.status.code(), Some(1));
}
