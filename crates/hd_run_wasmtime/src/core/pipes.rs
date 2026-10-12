use std::io::{self, Read, Write};
#[cfg(not(any(unix, windows)))]
use std::process::Stdio;
use std::process::{Child, Command};
#[cfg(windows)]
use std::time::Duration;

pub(crate) type Process = Child;

#[cfg(unix)]
pub(crate) trait ReadPipe: Read + std::os::fd::AsFd + Send {}
#[cfg(unix)]
impl<T: Read + std::os::fd::AsFd + Send> ReadPipe for T {}
#[cfg(not(unix))]
pub(crate) trait ReadPipe: Read + Send {}
#[cfg(not(unix))]
impl<T: Read + Send> ReadPipe for T {}

#[cfg(unix)]
pub(crate) trait WritePipe: Write + std::os::fd::AsFd + Send {}
#[cfg(unix)]
impl<T: Write + std::os::fd::AsFd + Send> WritePipe for T {}
#[cfg(not(unix))]
pub(crate) trait WritePipe: Write + Send {}
#[cfg(not(unix))]
impl<T: Write + Send> WritePipe for T {}

pub(crate) struct Spawned {
    pub(crate) child: Process,
    pub(crate) stdin: Box<dyn WritePipe>,
    pub(crate) stdout: Box<dyn ReadPipe>,
    pub(crate) stderr: Box<dyn ReadPipe>,
}

#[cfg(windows)]
pub(crate) fn spawn_process(
    name: &str,
    argv: &[String],
    cwd: Option<&std::path::Path>,
) -> io::Result<Spawned> {
    use crate::windows_pipe::Pipe;

    // Parent ends are synchronous nonblocking pipes; the child ends stay
    // ordinary synchronous blocking stdio. Use this transport even without a
    // deadline, so overflow can close quiet inherited pipes without a relay
    // thread or a blocking flush during teardown.
    let (stdin, child_stdin) = Pipe::child_pair(false)?;
    let (stdout, child_stdout) = Pipe::child_pair(true)?;
    let (stderr, child_stderr) = Pipe::child_pair(true)?;
    let mut command = Command::new(name);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let child = command
        .args(argv)
        .stdin(child_stdin)
        .stdout(child_stdout)
        .stderr(child_stderr)
        .spawn()?;
    Ok(Spawned {
        child,
        stdin: Box::new(stdin),
        stdout: Box::new(stdout),
        stderr: Box::new(stderr),
    })
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn spawn_process(
    name: &str,
    argv: &[String],
    cwd: Option<&std::path::Path>,
) -> io::Result<Spawned> {
    let mut command = Command::new(name);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let mut child = command
        .args(argv)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    Ok(Spawned {
        stdin: Box::new(
            child
                .stdin
                .take()
                .ok_or_else(|| io::Error::other("missing stdin"))?,
        ),
        stdout: Box::new(
            child
                .stdout
                .take()
                .ok_or_else(|| io::Error::other("missing stdout"))?,
        ),
        stderr: Box::new(
            child
                .stderr
                .take()
                .ok_or_else(|| io::Error::other("missing stderr"))?,
        ),
        child,
    })
}

#[cfg(unix)]
pub(crate) fn spawn_process(
    name: &str,
    argv: &[String],
    cwd: Option<&std::path::Path>,
) -> io::Result<Spawned> {
    let (stdin_read, stdin) = io::pipe()?;
    let (stdout, stdout_write) = io::pipe()?;
    let (stderr, stderr_write) = io::pipe()?;
    // Parent pipe ends are nonblocking; child ends remain blocking.
    set_nonblocking(&stdin)?;
    set_nonblocking(&stdout)?;
    set_nonblocking(&stderr)?;
    let mut command = Command::new(name);
    command
        .args(argv)
        .stdin(stdin_read)
        .stdout(stdout_write)
        .stderr(stderr_write);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let child = command.spawn()?;
    Ok(Spawned {
        child,
        stdin: Box::new(stdin),
        stdout: Box::new(stdout),
        stderr: Box::new(stderr),
    })
}

pub(crate) fn kill_process(child: &mut Process) -> io::Result<()> {
    child.kill()
}

pub(crate) fn try_wait_process(child: &mut Process) -> io::Result<Option<i64>> {
    child
        .try_wait()
        .map(|status| status.map(|status| i64::from(status.code().unwrap_or(128))))
}

pub(crate) fn wait_process(child: &mut Process) -> io::Result<i64> {
    Ok(i64::from(child.wait()?.code().unwrap_or(128)))
}

pub(crate) enum ReadEvent {
    Done,
    Overflow,
    Failed,
}

#[cfg(unix)]
fn set_nonblocking(fd: &impl std::os::fd::AsFd) -> io::Result<()> {
    use nix::fcntl::{FcntlArg, OFlag, fcntl};
    let flags = OFlag::from_bits_retain(fcntl(fd, FcntlArg::F_GETFL)?);
    fcntl(fd, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))?;
    Ok(())
}

#[cfg(unix)]
pub(crate) fn wait_pipe(
    fd: std::os::fd::BorrowedFd<'_>,
    events: nix::poll::PollFlags,
    cancelled: &std::sync::atomic::AtomicBool,
) -> io::Result<()> {
    let mut fds = [nix::poll::PollFd::new(fd, events)];
    while !cancelled.load(std::sync::atomic::Ordering::Relaxed) {
        // Bounded polling lets every worker close its pipe on cancellation,
        // including quiet inherited pipes without any readiness event.
        match nix::poll::poll(&mut fds, 50_u16) {
            Ok(0) | Err(nix::errno::Errno::EINTR) => (),
            Ok(_) => return Ok(()),
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}

pub(crate) fn feed_input(
    output: &mut dyn WritePipe,
    mut input: &[u8],
    cancelled: &std::sync::atomic::AtomicBool,
) -> io::Result<()> {
    while !input.is_empty() && !cancelled.load(std::sync::atomic::Ordering::Relaxed) {
        match output.write(input) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero)),
            Ok(n) => input = &input[n..],
            Err(err) if err.kind() == io::ErrorKind::Interrupted => (),
            #[cfg(unix)]
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                wait_pipe(output.as_fd(), nix::poll::PollFlags::POLLOUT, cancelled)?;
            }
            #[cfg(windows)]
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                // Named pipes have no safe readiness API here. Poll only on
                // backpressure, never delay successfully flowing bytes.
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

pub(crate) fn drain(
    input: &mut dyn ReadPipe,
    count: &std::sync::atomic::AtomicUsize,
    tx: &std::sync::mpsc::Sender<ReadEvent>,
    output_limit: usize,
    cancelled: &std::sync::atomic::AtomicBool,
) -> io::Result<Vec<u8>> {
    let result = (|| {
        let mut bytes = Vec::new();
        let mut buf = [0; 8192];
        loop {
            if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                return Ok(bytes);
            }
            let n = match input.read(&mut buf) {
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                #[cfg(unix)]
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                    wait_pipe(input.as_fd(), nix::poll::PollFlags::POLLIN, cancelled)?;
                    continue;
                }
                #[cfg(windows)]
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                result => result?,
            };
            if n == 0 {
                return Ok(bytes);
            }
            if count
                .fetch_add(n, std::sync::atomic::Ordering::Relaxed)
                .saturating_add(n)
                > output_limit
            {
                cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                let _ = tx.send(ReadEvent::Overflow);
                return Ok(bytes);
            }
            bytes.extend_from_slice(&buf[..n]);
        }
    })();
    let _ = tx.send(if result.is_ok() {
        ReadEvent::Done
    } else {
        ReadEvent::Failed
    });
    result
}
