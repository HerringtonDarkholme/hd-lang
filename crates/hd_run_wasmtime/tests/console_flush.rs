//! Held Console output reaches the stream before the program waits on the
//! host (`std-console.host.visible`), and once it grows past a bound.
mod support;

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hd_run::{Grants, HostSetup, Limits, Outcome};
use hd_run_wasmtime::{EngineConfig, HostIo};
use support::Program;
use wasm_encoder::{Instruction as I, ValType};

type Writes = Vec<(Instant, Vec<u8>)>;

/// Every write the host makes to standard output, with its time.
#[derive(Clone, Default)]
struct Recording(Arc<Mutex<Writes>>);

impl Write for Recording {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap()
            .push((Instant::now(), bytes.to_vec()));
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn write_line(p: &mut Program, line: &[u8]) {
    let start = p.import(
        "hd:Console",
        "write_line.start",
        &[ValType::I32],
        &[ValType::I32, ValType::I32],
    );
    let block = p.import("hd:rt", "block", &[], &[ValType::I32]);
    let finish = p.import(
        "hd:Console",
        "write_line.finish",
        &[ValType::I32],
        &[ValType::I32],
    );
    p.load_args(line);
    p.instructions(&[
        I::I32Const(i32::try_from(line.len()).unwrap()),
        I::Call(start),
        I::LocalSet(0),
        I::Drop,
        I::Call(block),
        I::Drop,
        I::LocalGet(0),
        I::Call(finish),
        I::Drop,
    ]);
}

fn sleep(p: &mut Program, millis: i64) {
    let start = p.import(
        "hd:Clock",
        "sleep.start",
        &[ValType::I64],
        &[ValType::I32, ValType::I32],
    );
    let block = p.import("hd:rt", "block", &[], &[ValType::I32]);
    let finish = p.import("hd:Clock", "sleep.finish", &[ValType::I32], &[ValType::I32]);
    p.instructions(&[
        I::I64Const(millis),
        I::Call(start),
        I::LocalSet(0),
        I::Drop,
        I::Call(block),
        I::Drop,
        I::LocalGet(0),
        I::Call(finish),
        I::Drop,
    ]);
}

fn run(p: Program) -> Writes {
    let out = Recording::default();
    let io = HostIo {
        out: Box::new(out.clone()),
        err: Box::new(std::io::sink()),
        input: None,
    };
    let setup = HostSetup {
        providers: vec![],
        grants: Grants::default(),
        limits: Limits::default(),
        args: vec![],
        program: String::new(),
    };
    let outcome =
        hd_run_wasmtime::run(EngineConfig::default(), &p.finish(7).0, &setup, io).unwrap();
    assert_eq!(outcome, Outcome::Exit(7));
    out.0.lock().unwrap().clone()
}

fn position(writes: &Writes, text: &str) -> usize {
    writes
        .iter()
        .position(|(_, bytes)| String::from_utf8_lossy(bytes).contains(text))
        .unwrap_or_else(|| panic!("{text} was written"))
}

#[test]
fn printed_line_reaches_the_stream_before_a_sleep() {
    const SLEEP_MS: i64 = 300;
    let mut p = Program::new();
    write_line(&mut p, b"before");
    sleep(&mut p, SLEEP_MS);
    write_line(&mut p, b"after");
    let writes = run(p);
    let before = position(&writes, "before");
    let after = position(&writes, "after");
    assert!(before < after, "`before` is its own earlier write");
    // `before` was written before the sleep began and `after` after it
    // ended, so the gap is at least the sleep: held until exit, it is zero.
    let gap = writes[after].0.duration_since(writes[before].0);
    assert!(
        gap >= Duration::from_millis(u64::try_from(SLEEP_MS).unwrap() * 3 / 4),
        "`before` waited {gap:?} for `after`"
    );
}

#[test]
fn output_past_the_bound_is_written_before_the_program_ends() {
    let mut p = Program::new();
    let line = [b'x'; 1000];
    // 80 lines of 1001 bytes pass the 64 KiB bound without any wait.
    for _ in 0..80 {
        write_line(&mut p, &line);
    }
    let writes = run(p);
    let total: usize = writes.iter().map(|(_, bytes)| bytes.len()).sum();
    assert!(total > 80_000);
    assert!(writes.len() >= 2, "one write at exit holds all the output");
    assert!(
        writes[0].1.len() <= 70_000,
        "the first write is the bounded part"
    );
}
