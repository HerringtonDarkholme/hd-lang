//! `Process.run!`: `.start` checks the grant
//! and the arguments on the program thread, and a refusal returns at once.
//! Otherwise the child runs as one task on the `hd-io` runtime, which
//! spawns it in a process group of its own, feeds its standard input and
//! reads its standard output and error together, then reaps it. The task
//! delivers the output for `.finish` to encode.
//!
//! Cancelling the operation drops the task, which kills the child and its
//! descendants (`std-process.cancel`) and then reaps the child: on Unix
//! its process group, on Windows its Job object (audit §5.3 Q1). The
//! child is reaped only after both output pipes close, so its process
//! group ID cannot be reused while the group can still be killed.
//!
//! The child's input and the output kept so far count against the
//! program's heap budget (`std-host.op.heap`). Output that would pass it
//! kills the group the same way, and `.finish` panics `heap-exhausted`.

use std::future::{Future, poll_fn};
use std::io;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::task::Poll;

use hd_run::Grants;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, Command};

use crate::codec::{Dec, Enc};
use crate::limits::{HeapExhausted, Ledger, Reservation};
use crate::reactor::Done;

/// The most bytes one read of an output pipe takes.
const PIPE_CHUNK: usize = 8192;

/// What `.start` decides.
pub(crate) enum Start {
    /// The encoded result of a refused start: no child runs.
    Refused(Enc),
    /// A child to run.
    Run(Request),
}

/// A child to run: the program, its arguments, its standard input, and
/// the working directory it starts in (the process's own when `None`).
pub(crate) struct Request {
    pub(crate) name: String,
    pub(crate) argv: Vec<String>,
    pub(crate) input: Vec<u8>,
    pub(crate) cwd: Option<PathBuf>,
    /// The heap budget the output is charged to.
    pub(crate) ledger: Arc<Ledger>,
    /// The heap the input holds.
    pub(crate) held: Reservation,
}

/// A child's standard output, standard error and status.
pub(crate) type Output = (Vec<u8>, Vec<u8>, i64);

/// What the task delivers: the program, for the error text, how its run
/// ended, and the heap its output holds.
pub(crate) struct Ran {
    name: String,
    result: io::Result<Output>,
    held: Reservation,
}

/// `run.start`'s checks, in the Node host's order: the grant, the empty
/// name, then NUL bytes. An input past the heap budget panics.
pub(crate) fn start(
    bytes: &[u8],
    grants: &Grants,
    cwd: Option<&Path>,
    ledger: &Arc<Ledger>,
) -> wasmtime::Result<Start> {
    let mut d = Dec::new(bytes);
    let name = d.str()?;
    let argv = d.strings()?;
    let input = d.bytes()?.to_vec();
    let mut e = Enc::default();
    if !grants.covers_name("Process", &name) {
        e.leb(1).leb(2);
        return Ok(Start::Refused(e));
    }
    if name.is_empty() {
        e.leb(1).leb(0);
        return Ok(Start::Refused(e));
    }
    if let Some(message) = validate_process(&name, &argv) {
        e.leb(1).leb(3).str(&message);
        return Ok(Start::Refused(e));
    }
    let held = ledger.reserve(input.len())?;
    Ok(Start::Run(Request {
        name,
        argv,
        input,
        cwd: cwd.map(Path::to_path_buf),
        ledger: Arc::clone(ledger),
        held,
    }))
}

/// The encoded result of a start that cannot run its task.
pub(crate) fn not_run(name: &str, error: &str) -> Enc {
    let mut e = Enc::default();
    e.leb(1)
        .leb(3)
        .str(&format!("cannot run {name:?}: {error}"));
    e
}

impl Request {
    /// The task: runs the child to its end. `Done::Exhausted` when its
    /// output would pass the heap budget; the group is killed then.
    pub(crate) async fn run(self) -> Done {
        let mut held = Reservation::empty(&self.ledger);
        let result = run_child(&self, &mut held).await;
        // The input's heap: the child has ended, so nothing holds it now.
        drop(self.held);
        if result.as_ref().is_err_and(is_exhausted) {
            return Done::Exhausted;
        }
        Done::Process(Ran {
            name: self.name,
            result,
            held,
        })
    }
}

/// Whether a run failed because its output would pass the heap budget.
fn is_exhausted(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(<dyn std::error::Error + Send + Sync>::is::<HeapExhausted>)
}

/// `run.finish`'s result: the output, or why the program did not run. The
/// output's heap is released first.
pub(crate) fn finish(ran: Ran) -> Enc {
    drop(ran.held);
    let mut e = Enc::default();
    match ran.result {
        Ok((out, err, status)) => {
            e.leb(0)
                .str(&String::from_utf8_lossy(&out))
                .str(&String::from_utf8_lossy(&err))
                .zz(status);
        }
        Err(err) => {
            let variant = match crate::native_io::error_kind(&err) {
                io::ErrorKind::NotFound => 0,
                io::ErrorKind::PermissionDenied => 1,
                _ => 3,
            };
            e.leb(1).leb(variant);
            if variant == 3 {
                e.str(&format!("cannot run {:?}: {err}", ran.name));
            }
        }
    }
    e
}

#[cfg(test)]
impl Ran {
    pub(crate) fn into_result(self) -> io::Result<Output> {
        self.result
    }
}

fn validate_process(name: &str, argv: &[String]) -> Option<String> {
    if name.contains('\0') {
        return Some("program name contains a NUL byte".into());
    }
    argv.iter()
        .position(|arg| arg.contains('\0'))
        .map(|index| format!("process argument {index} contains a NUL byte"))
}

/// Spawns the child, feeds its input while draining both outputs, and
/// reaps it once both outputs close. Output that `held` cannot reserve
/// kills the group.
async fn run_child(request: &Request, held: &mut Reservation) -> io::Result<Output> {
    let mut command = Command::new(&request.name);
    command
        .args(&request.argv)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // A backstop: `Group` kills and reaps the child itself.
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    if let Some(cwd) = &request.cwd {
        command.current_dir(cwd);
    }
    let mut child = command.spawn()?;
    let (stdin, stdout, stderr) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    let mut group = Group::new(child);
    let (mut out_held, mut err_held) = (
        Reservation::empty(&request.ledger),
        Reservation::empty(&request.ledger),
    );
    let piped = pipes(
        feed(stdin, &request.input),
        drain(stdout, &mut out_held),
        drain(stderr, &mut err_held),
    )
    .await;
    held.absorb(out_held);
    held.absorb(err_held);
    match piped {
        Ok((written, out, err)) => {
            let status = group.reap().await?;
            // A child may close its input without reading all of it; that
            // does not void its output or status.
            if let Err(error) = written
                && error.kind() != io::ErrorKind::BrokenPipe
            {
                return Err(error);
            }
            Ok((out, err, status))
        }
        Err(error) => {
            // A descendant may hold the pipes open: end the whole group.
            group.kill();
            group.reap().await?;
            Err(error)
        }
    }
}

/// Writes the child's input, then closes it so the child reads its end.
async fn feed(stdin: Option<ChildStdin>, input: &[u8]) -> io::Result<()> {
    let Some(mut stdin) = stdin else {
        return Ok(());
    };
    stdin.write_all(input).await
}

/// Reads one output to its end, reserving each chunk in `held` before
/// keeping it. A chunk past the heap budget fails the read with
/// `HeapExhausted`.
async fn drain(
    pipe: Option<impl AsyncRead + Unpin>,
    held: &mut Reservation,
) -> io::Result<Vec<u8>> {
    let Some(mut pipe) = pipe else {
        return Ok(Vec::new());
    };
    let mut bytes = Vec::new();
    let mut chunk = vec![0; PIPE_CHUNK];
    loop {
        let n = pipe.read(&mut chunk).await?;
        if n == 0 {
            return Ok(bytes);
        }
        held.grow(n)
            .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
        bytes.extend_from_slice(&chunk[..n]);
    }
}

/// Runs the input writer and both output readers together. A failed
/// read ends them all at once; the writer's result is kept for the caller.
async fn pipes(
    writer: impl Future<Output = io::Result<()>>,
    out: impl Future<Output = io::Result<Vec<u8>>>,
    err: impl Future<Output = io::Result<Vec<u8>>>,
) -> io::Result<(io::Result<()>, Vec<u8>, Vec<u8>)> {
    let (mut writer, mut out, mut err) = (pin!(writer), pin!(out), pin!(err));
    let (mut written, mut out_bytes, mut err_bytes) = (None, None, None);
    poll_fn(|cx| {
        if written.is_none()
            && let Poll::Ready(result) = writer.as_mut().poll(cx)
        {
            written = Some(result);
        }
        if out_bytes.is_none()
            && let Poll::Ready(result) = out.as_mut().poll(cx)
        {
            out_bytes = Some(result?);
        }
        if err_bytes.is_none()
            && let Poll::Ready(result) = err.as_mut().poll(cx)
        {
            err_bytes = Some(result?);
        }
        if written.is_some()
            && out_bytes.is_some()
            && err_bytes.is_some()
            && let (Some(written), Some(out), Some(err)) =
                (written.take(), out_bytes.take(), err_bytes.take())
        {
            return Poll::Ready(Ok((written, out, err)));
        }
        Poll::Pending
    })
    .await
}

/// The child and the tree of processes it starts, until the child is
/// reaped. Dropping it before then, as an abort drops the task, kills the
/// tree and leaves the child to a task that reaps it.
struct Group {
    child: Option<Child>,
    tree: Tree,
}

impl Group {
    fn new(child: Child) -> Group {
        let tree = Tree::of(&child);
        Group {
            child: Some(child),
            tree,
        }
    }

    /// Kills the child and its descendants.
    fn kill(&mut self) {
        if let Some(child) = &mut self.child {
            self.tree.kill(child);
        }
    }

    /// Waits for the child to exit and reaps it: its status, 128 plus the
    /// signal number for a child a signal ended. Descendants still running
    /// are left running.
    async fn reap(&mut self) -> io::Result<i64> {
        let Some(child) = &mut self.child else {
            return Err(io::Error::other("the child was already reaped"));
        };
        let status = child.wait().await?;
        self.child = None;
        self.tree.release();
        Ok(status_value(status))
    }
}

/// The status a program sees: the exit code, or on Unix 128 plus the number
/// of the signal that ended the child (`std-process.child.signal`).
fn status_value(status: ExitStatus) -> i64 {
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        return 128 + i64::from(signal);
    }
    i64::from(status.code().unwrap_or(128))
}

impl Drop for Group {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        self.tree.kill(&mut child);
        // Reap it rather than leave it to tokio's best-effort orphan reaper.
        // The runtime started before this child did.
        if let Ok(runtime) = crate::io_runtime::handle() {
            runtime.spawn(async move {
                let _ = child.wait().await;
            });
        }
    }
}

/// On Unix the child leads a process group of its own; the group's ID is
/// the child's process ID.
#[cfg(unix)]
struct Tree(Option<rustix::process::Pid>);

#[cfg(unix)]
impl Tree {
    fn of(child: &Child) -> Tree {
        Tree(
            child
                .id()
                .and_then(|pid| i32::try_from(pid).ok())
                .and_then(rustix::process::Pid::from_raw),
        )
    }

    /// Kills the group. Only called while its leader is not reaped, so
    /// the group's ID cannot have been reused.
    fn kill(&mut self, _child: &mut Child) {
        if let Some(pgid) = self.0 {
            let _ = rustix::process::kill_process_group(pgid, rustix::process::Signal::KILL);
        }
    }

    /// The leader is reaped: its ID may be reused, so never signal it again.
    fn release(&mut self) {
        self.0 = None;
    }
}

/// On Windows the child is assigned to a Job object that kills every
/// process in it when it closes. A child the job cannot take is killed
/// alone. Processes the child starts before it is assigned are not in
/// the job.
#[cfg(windows)]
struct Tree(Option<win32job::Job>);

#[cfg(windows)]
impl Tree {
    fn of(child: &Child) -> Tree {
        Tree(child.raw_handle().and_then(|handle| {
            let mut limits = win32job::ExtendedLimitInfo::new();
            limits.limit_kill_on_job_close();
            let job = win32job::Job::create_with_limit_info(&limits).ok()?;
            job.assign_process(handle as isize).ok()?;
            Some(job)
        }))
    }

    /// Closing the job kills the child and its descendants.
    fn kill(&mut self, child: &mut Child) {
        if self.0.take().is_none() {
            let _ = child.start_kill();
        }
    }

    /// The child ended on its own: descendants it left keep running, as
    /// on Unix, so the job closes without killing them.
    fn release(&mut self) {
        if let Some(job) = self.0.take() {
            let _ = job.set_extended_limit_info(&win32job::ExtendedLimitInfo::new());
        }
    }
}

/// Elsewhere only the direct child is killed.
#[cfg(not(any(unix, windows)))]
struct Tree;

#[cfg(not(any(unix, windows)))]
impl Tree {
    fn of(_child: &Child) -> Tree {
        Tree
    }

    fn kill(&mut self, child: &mut Child) {
        let _ = child.start_kill();
    }

    fn release(&mut self) {}
}
