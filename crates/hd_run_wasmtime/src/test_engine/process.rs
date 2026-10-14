use std::io::{self, Cursor, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use tokio::sync::oneshot;

use super::engine::TestEnvironment;
use crate::Shared;
use crate::codec::{Dec, Enc};
use crate::host::{Host, HostIo};
use crate::limits::{Ledger, Reservation};
use crate::reactor::Done;
use hd_run::{Grants, Instance, Outcome};

/// One of a child's output streams. What it keeps is charged to the
/// parent's heap budget (`std-host.op.heap`). Output past it is dropped and
/// interrupts the child: its host checks the flag at every host call, in
/// its waits and at epoch ticks.
struct ProcessSink {
    bytes: Shared,
    output: Arc<Output>,
}

/// What both of a child's output streams share.
struct Output {
    held: Mutex<Reservation>,
    exhausted: AtomicBool,
    interrupt: Arc<AtomicBool>,
}

impl Write for ProcessSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let grown = self
            .output
            .held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .grow(bytes.len());
        if let Err(error) = grown {
            self.output.exhausted.store(true, Ordering::Relaxed);
            self.output.interrupt.store(true, Ordering::Relaxed);
            return Err(io::Error::new(io::ErrorKind::OutOfMemory, error));
        }
        self.bytes.write_all(bytes)?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

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
    result: oneshot::Receiver<Done>,
    /// The parent's heap budget.
    ledger: Arc<Ledger>,
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
        match self.result.await {
            Ok(done) => done,
            Err(_) => Done::Child(
                Some(failed("the test child thread panicked")),
                Reservation::empty(&self.ledger),
            ),
        }
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
/// here, and an accepted child starts on its thread. The child's own heap
/// budget is `memory_bytes`; its output is charged to `ledger`, the
/// parent's.
pub(crate) fn start(
    bytes: &[u8],
    grants: &Grants,
    environment: &TestEnvironment,
    deadline: Option<Instant>,
    memory_bytes: u64,
    ledger: &Arc<Ledger>,
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
    // The input, held until the child ends.
    let input_held = ledger.reserve(input.len())?;
    let output = Arc::new(Output {
        held: Mutex::new(Reservation::empty(ledger)),
        exhausted: AtomicBool::new(false),
        interrupt: Arc::clone(&stop),
    });
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
                &output,
            );
            drop(input_held);
            // The waiting task may be gone: its abort dropped the result.
            let _ = sender.send(ran);
        });
    match spawned {
        Ok(_) => Ok(Start::Run(ChildRun {
            stop,
            result,
            ledger: Arc::clone(ledger),
        })),
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
/// `None` when the deadline had passed (the time-limit trap), with the heap
/// its output holds; `Done::Exhausted` when that output would pass the
/// parent's heap budget.
fn run(
    engine: &crate::WasmtimeEngine,
    module: &crate::Module,
    child: ChildInput,
    deadline: Option<Instant>,
    memory_bytes: u64,
    output: &Arc<Output>,
) -> Done {
    let out = Shared::default();
    let err = Shared::default();
    let name = child.name;
    let result = {
        let mut host = Host::new(
            HostIo {
                out: Box::new(ProcessSink {
                    bytes: out.clone(),
                    output: Arc::clone(output),
                }),
                err: Box::new(ProcessSink {
                    bytes: err.clone(),
                    output: Arc::clone(output),
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
        host.interrupt = Some(Arc::clone(&output.interrupt));
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
    // Taken here, so what the output holds goes with the result, or is
    // released before an exhausted run reports.
    let held = output
        .held
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    if output.exhausted.load(Ordering::Relaxed) {
        return Done::Exhausted;
    }
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Done::Child(None, held);
    }
    let mut encoded = Enc::default();
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
    Done::Child(Some(encoded), held)
}
