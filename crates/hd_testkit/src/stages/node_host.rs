//! The `stdout` view's Node host: one long-lived `node host/stage-host.ts`
//! for the whole test process (#1975), started by the first program run.
//! Each run gets a fresh compile, host, instance and memory there, so its
//! status and output are a lone `node host/run.ts` run's.
//!
//! cost: one Node start per test process, plus one more after each crash
//! or overrun; per run, the Wasm bytes over a pipe and one answer frame.
//! Runs are serialized on the one process: a program run is milliseconds
//! beside its build, which the case's own thread does in parallel.

use std::io::{Read as _, Write as _};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, sync_channel};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

/// A program run's process status (its low 8 bits, as a process exit
/// reports it), standard output and standard error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ran {
    pub exit: u8,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// The process and the channel its reader thread answers on.
struct Process {
    child: Child,
    stdin: ChildStdin,
    answers: Receiver<std::io::Result<Ran>>,
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

static HOST: Mutex<Option<Process>> = Mutex::new(None);
static STARTS: AtomicUsize = AtomicUsize::new(0);
static CRASHES: AtomicUsize = AtomicUsize::new(0);

/// How many host processes this test process started, and how many of
/// them crashed or overran a run's limit. Every start after the first
/// follows a crash: `starts <= 1 + crashes`.
#[must_use]
pub fn starts() -> (usize, usize) {
    (
        STARTS.load(Ordering::SeqCst),
        CRASHES.load(Ordering::SeqCst),
    )
}

/// Runs `wasm` with `args` on the shared host. A run that has not answered
/// within `limit`, or that ends the host, fails alone: the host is killed
/// and the next run starts a new one.
///
/// # Errors
/// The host could not start, crashed, or overran `limit`.
pub fn run(wasm: &[u8], args: &[&str], limit: Duration) -> Result<Ran, String> {
    request(wasm, args, None, limit)
}

/// Runs the cases `tests` (`test:init:kind,...`, as `hd test` lists them)
/// of the test program `wasm` on the shared host, as `host/test.ts` does.
/// Its standard output is one result record per line, without the time
/// and trap offsets that change from run to run.
///
/// # Errors
/// As [`run`].
pub fn run_tests(wasm: &[u8], tests: &str, limit: Duration) -> Result<Ran, String> {
    request(wasm, &[], Some(tests), limit)
}

fn request(
    wasm: &[u8],
    args: &[&str],
    tests: Option<&str>,
    limit: Duration,
) -> Result<Ran, String> {
    let mut slot = HOST.lock().unwrap_or_else(PoisonError::into_inner);
    if slot.is_none() {
        *slot = Some(start().map_err(|e| format!("node host did not start: {e}"))?);
        STARTS.fetch_add(1, Ordering::SeqCst);
    }
    let process = slot.as_mut().expect("started host");
    let answer = send(&mut process.stdin, wasm, args, tests)
        .map_err(|e| format!("node host took no request: {e}"))
        .and_then(|()| match process.answers.recv_timeout(limit) {
            Ok(Ok(ran)) => Ok(ran),
            Ok(Err(e)) => Err(format!("node host crashed: {e}")),
            Err(RecvTimeoutError::Disconnected) => Err("node host crashed".to_owned()),
            Err(RecvTimeoutError::Timeout) => Err(format!(
                "the run did not end within {}s",
                limit.as_secs_f64()
            )),
        });
    if answer.is_err() {
        // Dropping the process kills it; its reader thread then ends.
        *slot = None;
        CRASHES.fetch_add(1, Ordering::SeqCst);
    }
    answer
}

fn start() -> std::io::Result<Process> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/stage-host.ts");
    // cost: one process per test process, plus one per crash or overrun.
    let mut child = crate::node::command()
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        // cost: as above, one per host process.
        .spawn()?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(std::io::Error::other("no pipes"));
    };
    let (tell, answers) = sync_channel(1);
    // cost: one reader thread per host process; it ends with the process.
    std::thread::Builder::new()
        .name("stage node host".to_owned())
        // cost: as above.
        .spawn(move || {
            let mut stdout = stdout;
            loop {
                let answer = receive(&mut stdout);
                let failed = answer.is_err();
                if tell.send(answer).is_err() || failed {
                    break;
                }
            }
        })?;
    Ok(Process {
        child,
        stdin,
        answers,
    })
}

/// Writes one request frame: the header length, `{"wasmLen","args"}` and
/// `tests` when given, the Wasm bytes.
fn send(
    stdin: &mut ChildStdin,
    wasm: &[u8],
    args: &[&str],
    tests: Option<&str>,
) -> std::io::Result<()> {
    let mut header = format!("{{\"wasmLen\":{},\"args\":[", wasm.len());
    for (k, arg) in args.iter().enumerate() {
        if k > 0 {
            header.push(',');
        }
        json_string(arg, &mut header);
    }
    header.push(']');
    if let Some(tests) = tests {
        header.push_str(",\"tests\":");
        json_string(tests, &mut header);
    }
    header.push('}');
    let len = u32::try_from(header.len()).map_err(std::io::Error::other)?;
    stdin.write_all(&len.to_be_bytes())?;
    stdin.write_all(header.as_bytes())?;
    stdin.write_all(wasm)?;
    stdin.flush()
}

/// Reads one answer frame: the header length, `{"exit","stdoutLen",
/// "stderrLen"}`, the output bytes.
fn receive(stdout: &mut ChildStdout) -> std::io::Result<Ran> {
    let mut len = [0; 4];
    stdout.read_exact(&mut len)?;
    let mut header = vec![0; u32::from_be_bytes(len) as usize];
    stdout.read_exact(&mut header)?;
    let header = String::from_utf8(header).map_err(std::io::Error::other)?;
    let number = |key: &str| {
        header_number(&header, key)
            .ok_or_else(|| std::io::Error::other(format!("node host answered {header:?}")))
    };
    let exit = u8::try_from(number("exit")?).map_err(std::io::Error::other)?;
    let mut out = vec![0; number("stdoutLen")?];
    stdout.read_exact(&mut out)?;
    let mut err = vec![0; number("stderrLen")?];
    stdout.read_exact(&mut err)?;
    Ok(Ran {
        exit,
        stdout: out,
        stderr: err,
    })
}

/// The unsigned number after `"key":` in a header of numbers only.
fn header_number(header: &str, key: &str) -> Option<usize> {
    let (_, rest) = header.split_once(&format!("\"{key}\":"))?;
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Appends `text` as a JSON string literal.
fn json_string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = std::fmt::Write::write_fmt(out, format_args!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
