use std::io::{self, Cursor, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use tokio::sync::oneshot;

use super::engine::TestEnvironment;
use crate::Shared;
use crate::codec::{Dec, Enc};
use crate::host::{Host, HostIo};
use crate::reactor::Done;
use hd_run::{Grants, Instance, Outcome};

/// Child output is bounded across both streams. The flag is checked after
/// each host call and at epoch checks, so an overflowing child is interrupted.
struct ProcessSink {
    bytes: Shared,
    count: Arc<AtomicUsize>,
    overflow: Arc<AtomicBool>,
    limit: usize,
}

impl Write for ProcessSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let start = self.count.fetch_add(bytes.len(), Ordering::Relaxed);
        let kept = bytes.len().min(self.limit.saturating_sub(start));
        self.bytes.write_all(&bytes[..kept])?;
        if kept != bytes.len() {
            self.overflow.store(true, Ordering::Relaxed);
            return Err(io::ErrorKind::FileTooLarge.into());
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The bound on a child's output, across both streams.
const OUTPUT_LIMIT: usize = 1 << 30;

/// `Process.run!` of a named test child: refused at once, or running.
pub(crate) enum Start {
    /// The grant, the name or the thread refused: the encoded result.
    Ready(Enc),
    /// The child runs on its own thread.
    Run(ChildRun),
}

/// A child instance running on its own big-stack thread. A child needs the
/// stack of a test engine's thread, which a tokio task does not have, so
/// the thread delivers its result to a task that waits for it
/// (wasmtime-host-audit.md §5, T8).
pub(crate) struct ChildRun {
    /// Set to interrupt the child: its host checks it at every host call,
    /// in its waits and at epoch ticks, so the thread ends promptly.
    stop: Arc<AtomicBool>,
    result: oneshot::Receiver<Option<Enc>>,
}

/// Interrupts the child when the waiting task ends, finished or aborted.
struct StopOnDrop(Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

impl ChildRun {
    /// The task: waits for the child's result. Aborting it drops the wait,
    /// which interrupts the child, and its late result goes nowhere.
    pub(crate) async fn wait(self) -> Done {
        let _stop = StopOnDrop(self.stop);
        Done::Child(match self.result.await {
            Ok(encoded) => encoded,
            Err(_) => Some(failed("the test child thread panicked")),
        })
    }

    #[cfg(test)]
    pub(crate) fn stop_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }
}

fn failed(message: &str) -> Enc {
    let mut encoded = Enc::default();
    encoded.leb(1).leb(3).str(message);
    encoded
}

/// The result for a child that could not run.
pub(crate) fn not_run(error: &str) -> Enc {
    failed(&format!("cannot run the test child: {error}"))
}

/// `run.start` of a test case's child: the grant and the name are checked
/// here, and an accepted child starts on its thread.
pub(crate) fn start(
    bytes: &[u8],
    grants: &Grants,
    environment: &TestEnvironment,
    deadline: Option<Instant>,
    memory_bytes: u64,
) -> wasmtime::Result<Start> {
    let mut arguments = Dec::new(bytes);
    let name = arguments.str()?;
    let args = arguments.strings()?;
    let input = arguments.bytes()?.to_vec();
    let mut encoded = Enc::default();
    if !grants.covers_name("Process", &name) {
        encoded.leb(1).leb(2);
        return Ok(Start::Ready(encoded));
    }
    let Some(child) = environment.children.iter().find(|child| child.name == name) else {
        encoded.leb(1).leb(0);
        return Ok(Start::Ready(encoded));
    };
    let stop = Arc::new(AtomicBool::new(false));
    let (sender, result) = oneshot::channel();
    let cwd = environment.cwd.clone();
    let engine = Arc::clone(&child.engine);
    let module = Arc::clone(&child.module);
    let child_grants = child.grants.clone();
    let interrupt = Arc::clone(&stop);
    let spawned = std::thread::Builder::new()
        .name("hd-test-process".into())
        .stack_size(crate::thread_stack(&engine.config))
        .spawn(move || {
            let ran = run(
                &engine,
                &module,
                ChildInput {
                    name,
                    args,
                    input,
                    cwd,
                    grants: child_grants,
                },
                deadline,
                memory_bytes,
                &interrupt,
            );
            // The waiting task may be gone: its abort dropped the result.
            let _ = sender.send(ran);
        });
    match spawned {
        Ok(_) => Ok(Start::Run(ChildRun { stop, result })),
        Err(error) => Ok(Start::Ready(not_run(&error.to_string()))),
    }
}

struct ChildInput {
    name: String,
    args: Vec<String>,
    input: Vec<u8>,
    cwd: std::path::PathBuf,
    grants: Grants,
}

/// Runs the child to its end on the current thread: its encoded result, or
/// `None` when the deadline had passed (the time-limit trap).
fn run(
    engine: &crate::WasmtimeEngine,
    module: &crate::Module,
    child: ChildInput,
    deadline: Option<Instant>,
    memory_bytes: u64,
    interrupt: &Arc<AtomicBool>,
) -> Option<Enc> {
    let out = Shared::default();
    let err = Shared::default();
    let count = Arc::new(AtomicUsize::new(0));
    let overflow = Arc::new(AtomicBool::new(false));
    let name = child.name;
    let result = {
        let mut host = Host::new(
            HostIo {
                out: Box::new(ProcessSink {
                    bytes: out.clone(),
                    count: Arc::clone(&count),
                    overflow: Arc::clone(&overflow),
                    limit: OUTPUT_LIMIT,
                }),
                err: Box::new(ProcessSink {
                    bytes: err.clone(),
                    count: Arc::clone(&count),
                    overflow: Arc::clone(&overflow),
                    limit: OUTPUT_LIMIT,
                }),
                input: Some(Box::new(Cursor::new(child.input))),
            },
            child.args,
            name.clone(),
            child.grants,
            None,
            memory_bytes,
        );
        host.environment = Some(TestEnvironment {
            cwd: child.cwd,
            program: name,
            children: Vec::new(),
        });
        // A named child uses the ordinary Process provider, not test names.
        host.set_deadline(deadline);
        host.interrupt = Some(Arc::clone(interrupt));
        host.output_overflow = Some(Arc::clone(&overflow));
        host.output_budget = Some((Arc::clone(&count), OUTPUT_LIMIT));
        match engine
            .instantiate_host(module, host)
            .map_err(|error| format!("{error:?}"))
        {
            Ok(mut instance) => Ok(match instance.init() {
                Outcome::Exit(0) => hd_run::drive(&mut instance, usize::MAX),
                other => other,
            }),
            Err(message) => Err(message),
        }
    };
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return None;
    }
    let mut encoded = Enc::default();
    if overflow.load(Ordering::Relaxed) {
        encoded
            .leb(1)
            .leb(3)
            .str("process output exceeds the host limit");
        return Some(encoded);
    }
    match result {
        Ok(Outcome::Exit(status)) => {
            encoded
                .leb(0)
                .str(&String::from_utf8_lossy(&out.take()))
                .str(&String::from_utf8_lossy(&err.take()))
                .zz(i64::from(status));
        }
        Ok(Outcome::Panic(_)) => {
            encoded
                .leb(0)
                .str(&String::from_utf8_lossy(&out.take()))
                .str(&String::from_utf8_lossy(&err.take()))
                .zz(3);
        }
        Ok(Outcome::Internal(message)) | Err(message) => {
            encoded.leb(1).leb(3).str(&message);
        }
    }
    Some(encoded)
}
