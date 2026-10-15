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
