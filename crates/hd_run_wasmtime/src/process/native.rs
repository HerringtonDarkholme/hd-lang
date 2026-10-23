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

use std::future::{Future, poll_fn};
use std::io;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::process::{ExitStatus, Stdio};
use std::task::Poll;

use hd_run::Grants;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{ChildStdin, Command};

#[cfg(not(windows))]
use tokio::process::Child;

#[cfg(windows)]
use process_wrap::tokio::{ChildWrapper, CommandWrap, JobObject};

#[cfg(windows)]
type ProcessChild = Box<dyn ChildWrapper>;
#[cfg(not(windows))]
type ProcessChild = Child;

use crate::abi::{Command as Call, ProcessError, ProcessOutput};
use crate::codec::{Dec, Enc, Wire, encode};
use crate::runtime::reactor::Done;

/// The most bytes one read of an output pipe takes.
const PIPE_CHUNK: usize = 8192;

/// What `.start` decides.
pub(crate) enum Start {
    /// The encoded result of a refused start: no child runs.
    Refused(Enc),
    /// A child to run.
    Run(Request),
}

/// A child to run: the program, its arguments, its standard input, the
/// pairs added to its environment, and the working directory it starts in
/// (the process's own when `None`).
pub(crate) struct Request {
    pub(crate) name: String,
    pub(crate) argv: Vec<String>,
    pub(crate) input: Vec<u8>,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) cwd: Option<PathBuf>,
}

/// A child's standard output, standard error and status.
pub(crate) type Output = (Vec<u8>, Vec<u8>, i32);

/// What the task delivers: the program, for the error text, and how its
/// run ended.
pub(crate) struct Ran {
    name: String,
    result: io::Result<Output>,
}

/// `run.start`'s checks, in the Node host's order: the grant, the empty
/// name, then NUL bytes, then the working directory. `cwd` is the directory
/// a relative `current_dir` is from (the process's own when `None`).
pub(crate) fn start(bytes: &[u8], grants: &Grants, cwd: Option<&Path>) -> wasmtime::Result<Start> {
    let call = Call::dec(&mut Dec::new(bytes))?;
    if !grants.covers_name("Process", &call.program) {
        return Ok(Start::Refused(failed(ProcessError::NotGranted)));
    }
    if call.program.is_empty() {
        return Ok(Start::Refused(failed(ProcessError::NotFound {
            program: call.program,
        })));
    }
    if let Some(message) = validate(&call) {
        return Ok(Start::Refused(failed(ProcessError::Other { message })));
    }
    let dir = match resolve_dir(&call, cwd) {
        Ok(dir) => dir,
        Err(message) => return Ok(Start::Refused(failed(ProcessError::Other { message }))),
    };
    Ok(Start::Run(Request {
        name: call.program,
        argv: call.args,
        input: call.stdin.into_bytes(),
        env: call.env,
        cwd: dir,
    }))
}

/// The directory a child starts in: `current_dir`, from `base` when it is
/// relative, or `base` itself. A directory that does not exist is refused
/// here, so a missing directory is not read as a missing program.
pub(crate) fn resolve_dir(call: &Call, base: Option<&Path>) -> Result<Option<PathBuf>, String> {
    let dir = match (&call.current_dir, base) {
        (Some(dir), Some(base)) => Some(base.join(&dir.0)),
        (Some(dir), None) => Some(PathBuf::from(&dir.0)),
        (None, base) => base.map(Path::to_path_buf),
    };
    if let (Some(given), Some(dir)) = (&call.current_dir, &dir)
        && !dir.is_dir()
    {
        return Err(format!("working directory not found: {}", given.0));
    }
    Ok(dir)
}

/// The encoded result of a start that cannot run its task.
pub(crate) fn not_run(name: &str, error: &str) -> Enc {
    failed(ProcessError::Other {
        message: format!("cannot run {name:?}: {error}"),
    })
}

/// `run`'s result for a program that did not run.
pub(crate) fn failed(error: ProcessError) -> Enc {
    encode(&Err::<ProcessOutput, _>(error))
}

impl Request {
    /// The task: runs the child to its end.
    pub(crate) async fn run(self) -> Done {
        let result = run_child(&self).await;
        Done::Process(Ran {
            name: self.name,
            result,
        })
    }
}

/// `run.finish`'s result: the output, or why the program did not run.
pub(crate) fn finish(ran: Ran) -> Enc {
    match ran.result {
        Ok((out, err, status)) => encode(&Ok::<_, ProcessError>(ProcessOutput {
            stdout: String::from_utf8_lossy(&out).into_owned(),
            stderr: String::from_utf8_lossy(&err).into_owned(),
            status,
            program: ran.name,
        })),
        Err(err) => failed(match crate::runtime::native_io::error_kind(&err) {
            io::ErrorKind::NotFound => ProcessError::NotFound { program: ran.name },
            io::ErrorKind::PermissionDenied => ProcessError::PermissionDenied { program: ran.name },
            _ => ProcessError::Other {
                message: format!("cannot run {:?}: {err}", ran.name),
            },
        }),
    }
}

#[cfg(test)]
impl Ran {
    pub(crate) fn into_result(self) -> io::Result<Output> {
        self.result
    }
}

/// The first reason a command cannot start, in the Node host's order.
pub(crate) fn validate(call: &Call) -> Option<String> {
    if call.program.contains('\0') {
        return Some("program name contains a NUL byte".into());
    }
    if let Some(index) = call.args.iter().position(|arg| arg.contains('\0')) {
        return Some(format!("process argument {index} contains a NUL byte"));
    }
    if let Some(index) = call
        .env
        .iter()
        .position(|(name, _)| name.is_empty() || name.contains(['=', '\0']))
    {
        return Some(format!(
            "process environment name {index} is empty or holds = or a NUL byte"
        ));
    }
    if let Some(index) = call.env.iter().position(|(_, value)| value.contains('\0')) {
        return Some(format!(
            "process environment value {index} contains a NUL byte"
        ));
    }
    if call
        .current_dir
        .as_ref()
        .is_some_and(|dir| dir.0.contains('\0'))
    {
        return Some("working directory contains a NUL byte".into());
    }
    None
}

/// Spawns the child, feeds its input while draining both outputs, and
/// reaps it once both outputs close.
async fn run_child(request: &Request) -> io::Result<Output> {
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
    // The program's environment, then the pairs in order: a later pair for a
    // name wins.
    command.envs(request.env.iter().map(|(name, value)| (name, value)));
    // The wrapper creates the child suspended, assigns it to its Job, and
    // resumes it only after assignment. Even its first descendant belongs
    // to that Job. Group owns the wrapper until abort or normal completion.
    #[cfg(windows)]
    let mut child = CommandWrap::from(command).wrap(JobObject).spawn()?;
    #[cfg(not(windows))]
    let mut child = command.spawn()?;
    #[cfg(windows)]
    let (stdin, stdout, stderr) = (
        child.stdin().take(),
        child.stdout().take(),
        child.stderr().take(),
    );
    #[cfg(not(windows))]
    let (stdin, stdout, stderr) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    let mut group = Group::new(child);
    let piped = pipes(feed(stdin, &request.input), drain(stdout), drain(stderr)).await;
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

/// Reads one output to its end.
async fn drain(pipe: Option<impl AsyncRead + Unpin>) -> io::Result<Vec<u8>> {
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
    child: Option<ProcessChild>,
    tree: Tree,
}

impl Group {
    fn new(child: ProcessChild) -> Group {
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
    async fn reap(&mut self) -> io::Result<i32> {
        let Some(child) = &mut self.child else {
            return Err(io::Error::other("the child was already reaped"));
        };
        let status = wait_child(child).await?;
        self.child = None;
        self.tree.release();
        Ok(status_value(status))
    }
}

/// The Job wrapper's wait includes descendants. Normal completion instead
/// waits only for the direct child, after both output pipes have closed.
async fn wait_child(child: &mut ProcessChild) -> io::Result<ExitStatus> {
    #[cfg(windows)]
    return child.inner_mut().wait().await;
    #[cfg(not(windows))]
    child.wait().await
}

/// The status a program sees: the exit code, or on Unix 128 plus the number
/// of the signal that ended the child (`std-process.child.signal`).
fn status_value(status: ExitStatus) -> i32 {
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        return 128 + signal;
    }
    status.code().unwrap_or(128)
}

impl Drop for Group {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        self.tree.kill(&mut child);
        // Reap it rather than leave it to tokio's best-effort orphan reaper.
        // The runtime started before this child did.
        if let Ok(runtime) = crate::runtime::io::handle() {
            runtime.spawn(async move {
                let _ = wait_child(&mut child).await;
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

/// The Windows child wrapper already owns its Job before the child is
/// resumed. Its `start_kill` terminates every member; normal completion
/// releases the Job without kill-on-close so detached descendants survive.
#[cfg(windows)]
struct Tree;

#[cfg(windows)]
impl Tree {
    fn of(_child: &ProcessChild) -> Tree {
        Tree
    }

    #[expect(clippy::unused_self, reason = "shared process-tree interface")]
    fn kill(&mut self, child: &mut ProcessChild) {
        let _ = child.start_kill();
    }

    #[expect(clippy::unused_self, reason = "shared process-tree interface")]
    fn release(&mut self) {}
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
