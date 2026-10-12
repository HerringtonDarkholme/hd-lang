use std::io;
use std::time::{Duration, Instant};

use super::pipes::{
    Process, ReadEvent, ReadPipe, Spawned, WritePipe, drain, feed_input, kill_process,
    spawn_process, try_wait_process, wait_process,
};

/// Drain both pipes while feeding input: a child may write before reading.
/// The host keeps at most 1 GiB across the output streams.
#[cfg(test)]
pub(crate) fn run_process(
    name: &str,
    argv: &[String],
    input: &[u8],
) -> io::Result<(Vec<u8>, Vec<u8>, i64)> {
    run_process_with_limit(name, argv, input, 1 << 30)
}

#[cfg(test)]
pub(crate) fn run_process_with_limit(
    name: &str,
    argv: &[String],
    input: &[u8],
    output_limit: usize,
) -> io::Result<(Vec<u8>, Vec<u8>, i64)> {
    run_process_with_limit_and_deadline(name, argv, input, output_limit, None)
}

#[cfg(test)]
pub(crate) fn run_process_with_limit_and_deadline(
    name: &str,
    argv: &[String],
    input: &[u8],
    output_limit: usize,
    deadline: Option<Instant>,
) -> io::Result<(Vec<u8>, Vec<u8>, i64)> {
    run_process_at(name, argv, input, output_limit, deadline, None)
}

pub(crate) fn run_process_at(
    name: &str,
    argv: &[String],
    input: &[u8],
    output_limit: usize,
    deadline: Option<Instant>,
    cwd: Option<&std::path::Path>,
) -> io::Result<(Vec<u8>, Vec<u8>, i64)> {
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Err(io::ErrorKind::TimedOut.into());
    }
    let Spawned {
        mut child,
        stdin,
        stdout,
        stderr,
    } = spawn_process(name, argv, cwd)?;
    run_process_workers(
        &mut child,
        (stdin, stdout, stderr),
        input,
        output_limit,
        deadline,
        &mut PipeWorkers::default(),
    )
}

type ProcessPipes = (Box<dyn WritePipe>, Box<dyn ReadPipe>, Box<dyn ReadPipe>);

pub(crate) fn run_process_workers(
    child: &mut Process,
    (mut stdin, mut stdout, mut stderr): ProcessPipes,
    input: &[u8],
    output_limit: usize,
    deadline: Option<Instant>,
    workers: &mut PipeWorkers,
) -> io::Result<(Vec<u8>, Vec<u8>, i64)> {
    let count = std::sync::atomic::AtomicUsize::new(0);
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::channel();
        let out_tx = tx.clone();
        let err_tx = tx.clone();
        let input_tx = tx.clone();
        let out_count = &count;
        let err_count = &count;
        let stop = &cancelled;
        let out = workers
            .spawn(scope, "hd-process-stdout", move || {
                drain(&mut *stdout, out_count, &out_tx, output_limit, stop)
            })
            .map_err(|error| cancel_worker_start(error, child, stop))?;
        let err = workers
            .spawn(scope, "hd-process-stderr", move || {
                drain(&mut *stderr, err_count, &err_tx, output_limit, stop)
            })
            .map_err(|error| cancel_worker_start(error, child, stop))?;
        let writer = workers
            .spawn(scope, "hd-process-stdin", move || {
                let result = feed_input(&mut *stdin, input, stop);
                let _ = input_tx.send(ReadEvent::Done);
                result
            })
            .map_err(|error| cancel_worker_start(error, child, stop))?;
        drop(tx);
        // A reader reports overflow immediately; kill before joining
        // readers, otherwise a child with infinite output would hang.
        // Also stop all three pipe workers: descendants can retain the
        // quiet pipes even after the direct child exits.
        let result = wait_process_and_pipes(child, &rx, &cancelled, deadline);
        let input_result = writer
            .join()
            .map_err(|_| io::Error::other("stdin thread panicked"))?;
        let out = out
            .join()
            .map_err(|_| io::Error::other("stdout thread panicked"))?;
        let err = err
            .join()
            .map_err(|_| io::Error::other("stderr thread panicked"))?;
        let (status, overflow) = result?;
        if overflow {
            return Err(io::Error::from(io::ErrorKind::FileTooLarge));
        }
        // A started child may close stdin without consuming every byte.
        // BrokenPipe does not invalidate its captured output or exit status.
        if let Err(error) = input_result
            && error.kind() != io::ErrorKind::BrokenPipe
        {
            return Err(error);
        }
        Ok((out?, err?, status))
    })
}

#[derive(Default)]
pub(crate) struct PipeWorkers {
    #[cfg(test)]
    pub(crate) attempts: usize,
    #[cfg(test)]
    pub(crate) fail_at: Option<usize>,
}

impl PipeWorkers {
    // Production keeps this factory stateless; tests count and fail starts.
    #[cfg_attr(not(test), allow(clippy::unused_self))]
    fn spawn<'scope, 'env, F, T>(
        &mut self,
        scope: &'scope std::thread::Scope<'scope, 'env>,
        name: &str,
        work: F,
    ) -> io::Result<std::thread::ScopedJoinHandle<'scope, T>>
    where
        F: FnOnce() -> T + Send + 'scope,
        T: Send + 'scope,
    {
        #[cfg(test)]
        {
            self.attempts += 1;
            if self.fail_at == Some(self.attempts) {
                return Err(io::Error::other("injected pipe-worker creation failure"));
            }
        }
        std::thread::Builder::new()
            .name(name.into())
            .spawn_scoped(scope, work)
    }
}

fn cancel_worker_start(
    error: io::Error,
    child: &mut Process,
    cancelled: &std::sync::atomic::AtomicBool,
) -> io::Error {
    // A scoped join happens automatically on return. Stop every existing
    // reader/writer and reap the child BEFORE that teardown can begin.
    // Pipe cancellation also handles quiet pipes retained by descendants.
    cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = kill_process(child);
    let _ = wait_process(child);
    error
}

/// Keep checking the child even after every stdio pipe has closed. A child
/// can close its pipes, ignore SIGTERM, and keep running indefinitely.
pub(crate) fn wait_process_and_pipes(
    child: &mut Process,
    rx: &std::sync::mpsc::Receiver<ReadEvent>,
    cancelled: &std::sync::atomic::AtomicBool,
    deadline: Option<Instant>,
) -> io::Result<(i64, bool)> {
    use std::sync::mpsc::RecvTimeoutError;

    let mut status = None;
    let mut disconnected = false;
    let mut overflow = false;
    loop {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
            // Never signal an already-reaped PID, which might have been reused.
            if status.is_none() {
                let _ = kill_process(child);
                wait_process(child)?;
            }
            return Err(io::ErrorKind::TimedOut.into());
        }
        if status.is_none() {
            match try_wait_process(child) {
                Ok(result) => status = result,
                Err(error) => {
                    cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                    let _ = kill_process(child);
                    let _ = wait_process(child);
                    return Err(error);
                }
            }
        }
        if disconnected && let Some(status) = status {
            return Ok((status, overflow));
        }
        let wait = deadline.map_or(Duration::from_millis(50), |deadline| {
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(50))
        });
        if disconnected {
            // recv_timeout returns immediately after disconnection; avoid a
            // busy wait while a child runs with all of its pipes closed.
            std::thread::sleep(wait);
            continue;
        }
        match rx.recv_timeout(wait) {
            Ok(ReadEvent::Done) | Err(RecvTimeoutError::Timeout) => (),
            Ok(ReadEvent::Overflow) => {
                overflow = true;
                cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                if status.is_none() {
                    let _ = kill_process(child);
                }
            }
            Ok(ReadEvent::Failed) => {
                cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                if status.is_none() {
                    let _ = kill_process(child);
                }
            }
            Err(RecvTimeoutError::Disconnected) => disconnected = true,
        }
    }
}
