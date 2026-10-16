use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use tokio::sync::oneshot;

use super::engine::TestEnvironment;
use crate::Shared;
use crate::abi::{ProcessError, ProcessOutput};
use crate::codec::{Dec, Enc, Wire, encode};
use crate::core::process::failed;
use crate::host::{Host, HostIo};
use crate::reactor::Done;
use hd_run::{Grants, Instance, Outcome};

/// `Process.run!` of a named test child: refused at once, or running.
pub(crate) enum Start {
    /// The grant, the name or the thread refused: the encoded result.
    Ready(Enc),
    /// The child runs on its own thread.
    Run(ChildRun),
}

/// A child instance running on its own big-stack thread. A child needs the
/// stack of a test engine's thread, which a tokio task does not have, so
/// the thread delivers its result to a task that waits for it.
pub(crate) struct ChildRun {
    /// Set to interrupt the child: its host checks it at every host call,
    /// in its waits and at epoch ticks, so the thread ends promptly.
    stop: Arc<AtomicBool>,
    result: oneshot::Receiver<Done>,
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
            Err(_) => Done::Child(Some(failed(ProcessError::Other {
                message: "the test child thread panicked".into(),
            }))),
        }
    }

    #[cfg(test)]
    pub(crate) fn stop_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }
}

/// The result for a child that could not run.
pub(crate) fn not_run(error: &str) -> Enc {
    failed(ProcessError::Other {
        message: format!("cannot run the test child: {error}"),
    })
}

/// `run.start` of a test case's child: the grant and the name are checked
/// here, and an accepted child starts on its thread.
pub(crate) fn start(
    bytes: &[u8],
    grants: &Grants,
    environment: &TestEnvironment,
    deadline: Option<Instant>,
) -> wasmtime::Result<Start> {
    let mut arguments = Dec::new(bytes);
    let name = String::dec(&mut arguments)?;
    let args = Vec::<String>::dec(&mut arguments)?;
    let input = Vec::<u8>::dec(&mut arguments)?;
    if !grants.covers_name("Process", &name) {
        return Ok(Start::Ready(failed(ProcessError::NotGranted)));
    }
    let Some(child) = environment.children.iter().find(|child| child.name == name) else {
        return Ok(Start::Ready(failed(ProcessError::NotFound)));
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
                interrupt,
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
/// `None` when the deadline had passed (the time-limit trap). `interrupt`
/// stops it: its host checks the flag at every host call, in its waits and
/// at epoch ticks.
fn run(
    engine: &crate::WasmtimeEngine,
    module: &crate::Module,
    child: ChildInput,
    deadline: Option<Instant>,
    interrupt: Arc<AtomicBool>,
) -> Done {
    let out = Shared::default();
    let err = Shared::default();
    let name = child.name;
    let result = {
        let mut host = Host::new(
            HostIo {
                out: Box::new(out.clone()),
                err: Box::new(err.clone()),
                input: Some(Box::new(Cursor::new(child.input))),
            },
            child.args,
            name.clone(),
            child.grants,
            None,
        );
        host.environment = Some(TestEnvironment {
            cwd: child.cwd,
            program: name,
            children: Vec::new(),
        });
        // A named child uses the ordinary Process provider, not test names.
        host.set_deadline(deadline);
        host.interrupt = Some(interrupt);
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
        return Done::Child(None);
    }
    let ran = |status| {
        Ok(ProcessOutput {
            stdout: String::from_utf8_lossy(&out.take()).into_owned(),
            stderr: String::from_utf8_lossy(&err.take()).into_owned(),
            status,
        })
    };
    let encoded = encode(&match result {
        Ok(Outcome::Exit(status)) => ran(i32::from(status)),
        Ok(Outcome::Panic(_)) => ran(3),
        Ok(Outcome::Internal(message)) | Err(message) => Err(ProcessError::Other { message }),
    });
    Done::Child(Some(encoded))
}
