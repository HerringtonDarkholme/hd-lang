//! Observe real pipe output before releasing input to either host engine.
mod support;

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use hd_run::{Grants, HostSetup, Limits, Outcome};
use hd_run_wasmtime::{EngineConfig, HostIo};
use support::{MEM, Program, Scratch};
use wasm_encoder::{BlockType, Instruction as I, ValType};

const CHILD_PROGRAM: &str = "HD_M20_CONSOLE_CHILD_PROGRAM";
const TEST_NAME: &str = "console_input_flushes_each_prompt_before_waiting_for_input";
const DEADLINE: Duration = Duration::from_secs(10);

fn write(p: &mut Program, line: &[u8]) {
    let mut text = line.to_vec();
    text.push(b'\n');
    let line = text.as_slice();
    let start = p.import(
        "hd:Console",
        "write.start",
        &[ValType::I32],
        &[ValType::I32, ValType::I32],
    );
    let block = p.import("hd:rt", "block", &[], &[ValType::I32]);
    let finish = p.import(
        "hd:Console",
        "write.finish",
        &[ValType::I32],
        &[ValType::I32],
    );
    p.load_args(line);
    p.instructions(&[
        I::I32Const(i32::try_from(line.len()).unwrap()),
        I::Call(start),
        I::LocalSet(0),
        // A write that went straight to the stream is ready at its start.
        I::If(wasm_encoder::BlockType::Empty),
        I::Call(block),
        I::Drop,
        I::LocalGet(0),
        I::Call(finish),
        I::Drop,
        I::End,
    ]);
}

fn read_line(p: &mut Program) {
    let start = p.import(
        "hd:ConsoleInput",
        "read_line.start",
        &[],
        &[ValType::I32, ValType::I32],
    );
    let block = p.import("hd:rt", "block", &[], &[ValType::I32]);
    let finish = p.import(
        "hd:ConsoleInput",
        "read_line.finish",
        &[ValType::I32],
        &[ValType::I32],
    );
    p.instructions(&[
        I::Call(start),
        I::LocalSet(0),
        I::LocalTee(2),
        I::I32Const(1),
        I::I32Eq,
        I::If(BlockType::Empty),
        // Either host may return a pending handle. Finish only after
        // block has delivered it.
        I::Call(block),
        I::Drop,
        I::LocalGet(0),
        I::Call(finish),
        I::LocalSet(0),
        I::Else,
        I::LocalGet(2),
        I::If(BlockType::Empty),
        I::Unreachable,
        I::End,
        I::End,
    ]);
    // Nothing writes stderr before the read completes: stderr would flush
    // stdout itself and hide the prompt-before-input regression.
    p.dump();
    p.instructions(&[
        I::I32Const(0),
        I::LocalGet(2),
        I::I32Store8(MEM),
        I::I32Const(1),
        I::LocalSet(0),
    ]);
    p.dump();
}

fn program() -> Vec<u8> {
    let mut p = Program::new();
    write(&mut p, b"first-prompt");
    read_line(&mut p);
    write(&mut p, b"second-prompt");
    read_line(&mut p);
    write(&mut p, b"done");
    p.finish(7).0
}

struct ChildGuard(Child);

impl ChildGuard {
    fn wait(&mut self) -> ExitStatus {
        let deadline = Instant::now() + DEADLINE;
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "input child exits before its deadline"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn expect_line(lines: &mpsc::Receiver<Vec<u8>>, expected: &[u8], engine: &str) {
    let line = lines.recv_timeout(DEADLINE).unwrap_or_else(|error| {
        panic!(
            "{engine} emits {} before receiving input: {error}",
            String::from_utf8_lossy(expected)
        )
    });
    assert_eq!(line, expected, "{engine} stdout order");
}

fn check_child(mut command: Command, engine: &str, harness_prefix: bool) {
    let mut child = ChildGuard(
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut input = child.0.stdin.take().unwrap();
    let stdout = child.0.stdout.take().unwrap();
    let mut stderr = child.0.stderr.take().unwrap();
    let (send, lines) = mpsc::channel();
    let output = std::thread::spawn(move || {
        let mut stdout = BufReader::new(stdout);
        let mut all = Vec::new();
        loop {
            let mut line = Vec::new();
            if stdout.read_until(b'\n', &mut line).unwrap() == 0 {
                return all;
            }
            all.extend_from_slice(&line);
            let _ = send.send(line);
        }
    });
    let errors = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).unwrap();
        bytes
    });
    if harness_prefix {
        // The same integration-test executable is the native child runner.
        // --quiet --nocapture emits only this known libtest preamble before
        // our test body; process::exit avoids an unrelated result trailer.
        expect_line(&lines, b"\n", engine);
        expect_line(&lines, b"running 1 test\n", engine);
    }
    expect_line(&lines, b"core-parity\n", engine);
    expect_line(&lines, b"first-prompt\n", engine);
    // stdin stays open and empty until the first prompt is observed.
    input.write_all(b"Ada\r\n").unwrap();
    input.flush().unwrap();
    expect_line(&lines, b"second-prompt\n", engine);
    // The second line is also withheld until the second read flushes.
    input.write_all(b"Grace\n").unwrap();
    input.flush().unwrap();
    drop(input);
    assert_eq!(child.wait().code(), Some(7), "{engine} child status");
    let prefix = if harness_prefix {
        "\nrunning 1 test\n"
    } else {
        ""
    };
    assert_eq!(
        output.join().unwrap(),
        format!("{prefix}core-parity\nfirst-prompt\nsecond-prompt\ndone\n").as_bytes(),
        "{engine} complete stdout"
    );
    let stderr = errors.join().unwrap();
    let rows = stderr.split(|byte| *byte == b'\n').collect::<Vec<_>>();
    assert_eq!(rows.len(), 5, "{engine} result/status framing");
    assert_eq!(rows[0], b"000103416461", "{engine} decoded first line");
    assert_eq!(rows[2], b"0001054772616365", "{engine} decoded second line");
    for status in [rows[1], rows[3]] {
        assert!(
            status == b"00" || status == b"01",
            "{engine} Ready/Pending status"
        );
    }
    assert!(rows[4].is_empty());
}

fn native_child(file: &Path, status: u8) -> ! {
    let setup = HostSetup {
        providers: vec![],
        grants: Grants::default(),
        limits: Limits::default(),
        args: vec![],
        program: String::new(),
    };
    let outcome = hd_run_wasmtime::run(
        EngineConfig::default(),
        &fs::read(file).unwrap(),
        &setup,
        HostIo::process(),
    )
    .unwrap();
    assert_eq!(outcome, Outcome::Exit(status));
    std::process::exit(i32::from(status));
}

#[test]
fn console_input_flushes_each_prompt_before_waiting_for_input() {
    if let Some(file) = std::env::var_os(CHILD_PROGRAM) {
        native_child(&PathBuf::from(file), 7);
    }
    let scratch = Scratch::new();
    let file = scratch.at("console-input.wasm");
    fs::write(&file, program()).unwrap();
    let mut node = Command::new("node");
    node.arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
        .arg(&file);
    check_child(node, "Node", false);
    let mut rust = Command::new(std::env::current_exe().unwrap());
    rust.args(["--exact", TEST_NAME, "--nocapture", "--quiet"])
        .env(CHILD_PROGRAM, &file);
    check_child(rust, "wasmtime", true);
}

const SIBLING_TEST: &str = "console_input_keeps_siblings_running_in_both_node_profiles";
const SIBLING_PROGRAM: &str = "HD_813_CONSOLE_SIBLING_PROGRAM";

fn compile_fixture(source: &str) -> Vec<u8> {
    use hd_driver::{Executor, Goal, Host, NoClock};
    use hd_project::MemorySources;
    use hd_sched::SerialOrder;
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let store = hd_cache::MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let built = hd_driver::build(
        &host,
        "console-pending",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(built.diags.is_empty(), "{}", built.render());
    built.wasm.expect("console conformance fixture builds")
}

fn check_sibling(mut command: Command, engine: &str, harness_prefix: bool, all: bool) {
    let mut child = ChildGuard(
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("input child starts"),
    );
    let mut input = child.0.stdin.take().unwrap();
    let stdout = child.0.stdout.take().unwrap();
    let mut stderr = child.0.stderr.take().unwrap();
    let (send, lines) = mpsc::channel();
    let output = std::thread::spawn(move || {
        let mut stdout = BufReader::new(stdout);
        let mut all = Vec::new();
        loop {
            let mut line = Vec::new();
            if stdout.read_until(b'\n', &mut line).unwrap() == 0 {
                return all;
            }
            all.extend_from_slice(&line);
            let _ = send.send(line);
        }
    });
    let errors = std::thread::spawn(move || {
        let mut b = Vec::new();
        stderr.read_to_end(&mut b).unwrap();
        b
    });
    if harness_prefix {
        expect_line(&lines, b"\n", engine);
        expect_line(&lines, b"running 1 test\n", engine);
    }
    // No input byte exists until the sibling task runs. A blocking .start
    // prevents this marker and fails at the bounded observation deadline.
    expect_line(&lines, b"sibling progressed\n", engine);
    input.write_all("Ada\r\nlast".as_bytes()).unwrap();
    input.flush().unwrap();
    drop(input);
    let status = child.wait().code();
    let stderr = errors.join().unwrap();
    assert_eq!(
        status,
        Some(0),
        "{engine} status: {}",
        String::from_utf8_lossy(&stderr)
    );
    let prefix = if harness_prefix {
        "\nrunning 1 test\n"
    } else {
        ""
    };
    let text = if all { "Ada\nlast" } else { "Ada" };
    assert_eq!(
        output.join().unwrap(),
        format!("{prefix}sibling progressed\ninput={text}\n").as_bytes(),
        "{engine} complete output"
    );
    assert!(stderr.is_empty(), "{engine} stderr");
}

#[test]
fn console_input_keeps_siblings_running_in_both_node_profiles() {
    if let Some(file) = std::env::var_os(SIBLING_PROGRAM) {
        native_child(&PathBuf::from(file), 0);
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let scratch = Scratch::new();
    for (name, all) in [("line", false), ("all", true)] {
        let source = fs::read_to_string(
            root.join(format!("spec/conformance/cli/console-pending/{name}.hd")),
        )
        .unwrap();
        let file = scratch.at(&format!("{name}.wasm"));
        fs::write(&file, compile_fixture(&source)).unwrap();
        for supervised in [false, true] {
            let config = scratch.at(if supervised {
                "supervised.json"
            } else {
                "direct.json"
            });
            fs::write(
                &config,
                if supervised {
                    "{\"timeLimitMs\":5000}"
                } else {
                    "{}"
                },
            )
            .unwrap();
            let mut node = Command::new("node");
            node.arg(root.join("host/run.mjs")).arg(&file).arg(config);
            check_sibling(
                node,
                if supervised {
                    "Node supervised"
                } else {
                    "Node direct"
                },
                false,
                all,
            );
        }
        let mut native = Command::new(std::env::current_exe().unwrap());
        native
            .args(["--exact", SIBLING_TEST, "--nocapture", "--quiet"])
            .env(SIBLING_PROGRAM, &file);
        check_sibling(native, "wasmtime", true, all);
    }
}
