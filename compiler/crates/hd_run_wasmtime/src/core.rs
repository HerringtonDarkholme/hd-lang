//! Random, Sys and Process providers of the Node default host profile.

use std::io::{self, Read, Write};
#[cfg(not(any(unix, windows)))]
use std::process::Stdio;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use hd_run::Grants;

use crate::codec::{Dec, Enc};
use crate::host::Stop;

pub fn random(count: usize) -> wasmtime::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(count)
        .map_err(|_| Stop::HeapExhausted)?;
    bytes.resize(count, 0);
    getrandom::fill(&mut bytes).map_err(|e| Stop::Internal(e.to_string()))?;
    Ok(bytes)
}

pub fn sys(name: &str, grants: &Grants) -> Enc {
    sys_read(name, grants, |e| {
        match name {
            "os" => {
                e.str(match std::env::consts::OS {
                    "solaris" | "illumos" => "sunos",
                    other => other,
                });
            }
            "arch" => {
                e.str(match std::env::consts::ARCH {
                    "powerpc" => "ppc",
                    "powerpc64" => "ppc64",
                    "loongarch64" => "loong64",
                    other => other,
                });
            }
            "hostname" => match hostname::get() {
                Ok(name) => {
                    e.str(&name.to_string_lossy());
                }
                Err(_) => return false,
            },
            "cpu_count" => match std::thread::available_parallelism() {
                Ok(n) => {
                    e.leb(n.get() as u64);
                }
                Err(_) => return false,
            },
            _ => unreachable!("registered Sys method"),
        }
        true
    })
}

fn sys_read(name: &str, grants: &Grants, read: impl FnOnce(&mut Enc) -> bool) -> Enc {
    let mut e = Enc::default();
    if !grants.covers_name("Sys", name) {
        e.leb(1).leb(0).str(name);
        return e;
    }
    e.leb(0);
    if !read(&mut e) {
        e = Enc::default();
        e.leb(1).leb(1).str(name);
    }
    e
}

pub fn process(bytes: &[u8], grants: &Grants) -> wasmtime::Result<Enc> {
    process_impl(bytes, grants, None, None)
}

pub fn process_with_deadline(
    bytes: &[u8],
    grants: &Grants,
    deadline: Option<Instant>,
) -> wasmtime::Result<Enc> {
    match deadline {
        Some(deadline) => process_impl(bytes, grants, Some(deadline), None),
        None => process(bytes, grants),
    }
}

pub(crate) fn process_with_deadline_at(
    bytes: &[u8],
    grants: &Grants,
    deadline: Option<Instant>,
    cwd: &std::path::Path,
) -> wasmtime::Result<Enc> {
    process_impl(bytes, grants, deadline, Some(cwd))
}

fn process_impl(
    bytes: &[u8],
    grants: &Grants,
    deadline: Option<Instant>,
    cwd: Option<&std::path::Path>,
) -> wasmtime::Result<Enc> {
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Err(Stop::TimeLimit.into());
    }
    let mut d = Dec::new(bytes);
    let name = d.str()?;
    let argv = d.strings()?;
    let input = d.bytes()?;
    let mut e = Enc::default();
    if !grants.covers_name("Process", &name) {
        e.leb(1).leb(2);
        return Ok(e);
    }
    if name.is_empty() {
        e.leb(1).leb(0);
        return Ok(e);
    }
    if let Some(message) = validate_process(&name, &argv) {
        e.leb(1).leb(3).str(&message);
        return Ok(e);
    }
    let result = run_process_at(&name, &argv, input, 1 << 30, deadline, cwd);
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Err(Stop::TimeLimit.into());
    }
    match result {
        Ok((out, err, status)) => {
            e.leb(0)
                .str(&String::from_utf8_lossy(&out))
                .str(&String::from_utf8_lossy(&err))
                .zz(status);
        }
        Err(err) => {
            let variant = match err.kind() {
                io::ErrorKind::NotFound => 0,
                io::ErrorKind::PermissionDenied => 1,
                _ => 3,
            };
            e.leb(1).leb(variant);
            if variant == 3 {
                e.str(&format!("spawnSync {name} {}", process_error(&err)));
            }
        }
    }
    Ok(e)
}

fn validate_process(name: &str, argv: &[String]) -> Option<String> {
    for (argument, value) in std::iter::once(("file".into(), name)).chain(
        argv.iter()
            .enumerate()
            .map(|(i, arg)| (format!("args[{i}]"), arg.as_str())),
    ) {
        if value.contains('\0') {
            return Some(format!(
                "The argument '{argument}' must be a string without null bytes. Received {}",
                crate::fs::inspect_string(value)
            ));
        }
    }
    None
}

fn process_error(err: &io::Error) -> String {
    #[cfg(unix)]
    if let Some(code) = err.raw_os_error() {
        // Node uses libuv's symbolic errno name in spawnSync's message.
        let name = match code {
            libc::E2BIG => Some("E2BIG"),
            libc::EAGAIN => Some("EAGAIN"),
            libc::EBADF => Some("EBADF"),
            libc::EIO => Some("EIO"),
            libc::ELOOP => Some("ELOOP"),
            libc::EMFILE => Some("EMFILE"),
            libc::ENAMETOOLONG => Some("ENAMETOOLONG"),
            libc::ENFILE => Some("ENFILE"),
            libc::ENOBUFS => Some("ENOBUFS"),
            libc::ENOEXEC => Some("ENOEXEC"),
            libc::ENOMEM => Some("ENOMEM"),
            libc::ENOSYS => Some("ENOSYS"),
            libc::ENOTDIR => Some("ENOTDIR"),
            libc::ETXTBSY => Some("ETXTBSY"),
            libc::EINVAL => Some("EINVAL"),
            _ => None,
        };
        if let Some(name) = name {
            return name.into();
        }
    }
    match err.kind() {
        io::ErrorKind::FileTooLarge => "ENOBUFS".into(),
        io::ErrorKind::InvalidInput => "EINVAL".into(),
        io::ErrorKind::ArgumentListTooLong => "E2BIG".into(),
        io::ErrorKind::OutOfMemory => "ENOMEM".into(),
        io::ErrorKind::NotADirectory => "ENOTDIR".into(),
        io::ErrorKind::TooManyLinks => "ELOOP".into(),
        io::ErrorKind::InvalidFilename => "ENAMETOOLONG".into(),
        io::ErrorKind::ExecutableFileBusy => "ETXTBSY".into(),
        io::ErrorKind::ResourceBusy => "EBUSY".into(),
        io::ErrorKind::WouldBlock => "EAGAIN".into(),
        io::ErrorKind::Interrupted => "EINTR".into(),
        io::ErrorKind::BrokenPipe => "EPIPE".into(),
        _ => err.to_string(),
    }
}

/// Drain both pipes while feeding input: a child may write before reading.
/// Node's spawnSync keeps at most 1 GiB across the output streams.
#[cfg(test)]
fn run_process(name: &str, argv: &[String], input: &[u8]) -> io::Result<(Vec<u8>, Vec<u8>, i64)> {
    run_process_with_limit(name, argv, input, 1 << 30)
}

#[cfg(test)]
fn run_process_with_limit(
    name: &str,
    argv: &[String],
    input: &[u8],
    output_limit: usize,
) -> io::Result<(Vec<u8>, Vec<u8>, i64)> {
    run_process_with_limit_and_deadline(name, argv, input, output_limit, None)
}

#[cfg(test)]
fn run_process_with_limit_and_deadline(
    name: &str,
    argv: &[String],
    input: &[u8],
    output_limit: usize,
    deadline: Option<Instant>,
) -> io::Result<(Vec<u8>, Vec<u8>, i64)> {
    run_process_at(name, argv, input, output_limit, deadline, None)
}

fn run_process_at(
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
        mut stdin,
        mut stdout,
        mut stderr,
    } = spawn_process(name, argv, cwd)?;
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
        let out = scope.spawn(move || drain(&mut *stdout, out_count, &out_tx, output_limit, stop));
        let err = scope.spawn(move || drain(&mut *stderr, err_count, &err_tx, output_limit, stop));
        let writer = scope.spawn(move || {
            let result = feed_input(&mut *stdin, input, stop);
            let _ = input_tx.send(ReadEvent::Done);
            result
        });
        drop(tx);
        // A reader reports overflow immediately; kill before joining
        // readers, otherwise a child with infinite output would hang.
        // Also stop all three pipe workers: descendants can retain the
        // quiet pipes even after the direct child exits.
        let result = wait_process_and_pipes(&mut child, &rx, &cancelled, deadline);
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
        // spawnSync reports EPIPE when a child closes stdin while input
        // remains, even if the child itself exits successfully.
        input_result?;
        Ok((out?, err?, status))
    })
}

/// Keep checking the child even after every stdio pipe has closed. A child
/// can close its pipes, ignore SIGTERM, and keep running indefinitely.
fn wait_process_and_pipes(
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

type Process = Child;

#[cfg(unix)]
trait ReadPipe: Read + std::os::fd::AsFd + Send {}
#[cfg(unix)]
impl<T: Read + std::os::fd::AsFd + Send> ReadPipe for T {}
#[cfg(not(unix))]
trait ReadPipe: Read + Send {}
#[cfg(not(unix))]
impl<T: Read + Send> ReadPipe for T {}

#[cfg(unix)]
trait WritePipe: Write + std::os::fd::AsFd + Send {}
#[cfg(unix)]
impl<T: Write + std::os::fd::AsFd + Send> WritePipe for T {}
#[cfg(not(unix))]
trait WritePipe: Write + Send {}
#[cfg(not(unix))]
impl<T: Write + Send> WritePipe for T {}

struct Spawned {
    child: Process,
    stdin: Box<dyn WritePipe>,
    stdout: Box<dyn ReadPipe>,
    stderr: Box<dyn ReadPipe>,
}

#[cfg(windows)]
fn spawn_process(
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
fn spawn_process(
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
fn spawn_process(
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

fn kill_process(child: &mut Process) -> io::Result<()> {
    child.kill()
}

fn try_wait_process(child: &mut Process) -> io::Result<Option<i64>> {
    child
        .try_wait()
        .map(|status| status.map(|status| i64::from(status.code().unwrap_or(128))))
}

fn wait_process(child: &mut Process) -> io::Result<i64> {
    Ok(i64::from(child.wait()?.code().unwrap_or(128)))
}

enum ReadEvent {
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
fn wait_pipe(
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

fn feed_input(
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

fn drain(
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

#[cfg(test)]
mod tests {
    use super::{process, random, sys_read};
    use crate::codec::Enc;
    use hd_run::{Grants, Limit};

    #[test]
    fn random_returns_the_requested_number_of_bytes() {
        assert!(random(0).unwrap().is_empty());
        assert_eq!(random(8).unwrap().len(), 8);
        assert_eq!(random(1024).unwrap().len(), 1024);
    }

    #[test]
    fn sys_grant_is_checked_before_reading_and_failure_is_unsupported() {
        let grants = Grants {
            limits: vec![("Sys".into(), Limit::Entries(vec!["os".into()]))],
        };
        let denied = sys_read("hostname", &grants, |_| panic!("denied read ran"));
        let mut expected = Enc::default();
        expected.leb(1).leb(0).str("hostname");
        assert_eq!(denied.done(), expected.done());

        let unsupported = sys_read("hostname", &Grants::default(), |e| {
            e.str("discard this partial answer");
            false
        });
        let mut expected = Enc::default();
        expected.leb(1).leb(1).str("hostname");
        assert_eq!(unsupported.done(), expected.done());
    }

    #[test]
    fn process_grant_precedes_spawn_and_missing_program_is_not_found() {
        let mut args = Enc::default();
        args.str("hd-no-such-program-for-process-test")
            .leb(0)
            .bytes(&[]);
        let grants = Grants {
            limits: vec![("Process".into(), Limit::Entries(vec![]))],
        };
        assert_eq!(process(args.done(), &grants).unwrap().done(), [1, 2]);
        assert_eq!(
            process(args.done(), &Grants::default()).unwrap().done(),
            [1, 0]
        );
    }

    #[test]
    fn process_invalid_names_and_arguments_are_typed_errors() {
        for (name, argv, expected) in [
            (
                "a\0b",
                vec![],
                "The argument 'file' must be a string without null bytes. Received 'a\\x00b'",
            ),
            (
                "sh",
                vec!["first", "a\0b"],
                "The argument 'args[1]' must be a string without null bytes. Received 'a\\x00b'",
            ),
        ] {
            let mut args = Enc::default();
            args.str(name)
                .list(&argv, |e, arg| {
                    e.str(arg);
                })
                .bytes(&[]);
            let mut error = Enc::default();
            error.leb(1).leb(3).str(expected);
            assert_eq!(
                process(args.done(), &Grants::default()).unwrap().done(),
                error.done()
            );
            let denied = Grants {
                limits: vec![("Process".into(), Limit::Deny)],
            };
            assert_eq!(process(args.done(), &denied).unwrap().done(), [1, 2]);
        }
        let mut args = Enc::default();
        args.str("").leb(0).bytes(&[]);
        assert_eq!(
            process(args.done(), &Grants::default()).unwrap().done(),
            [1, 0]
        );
    }

    #[cfg(windows)]
    mod windows {
        use super::super::{process_error, run_process, run_process_with_limit_and_deadline};
        use std::io::{self, Read, Write};
        use std::path::PathBuf;
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};

        fn fixture_args(name: &str, path: Option<&std::path::Path>) -> Vec<String> {
            let mut args = vec![
                "--exact".into(),
                format!("core::tests::windows::{name}"),
                "--ignored".into(),
                "--nocapture".into(),
            ];
            if let Some(path) = path {
                // libtest accepts arbitrary skip filters. This one never
                // matches the selected fixture and carries its private path
                // without mutating the parent process's environment.
                args.extend([
                    "--skip".into(),
                    format!("hd-process-fixture:{}", path.display()),
                ]);
            }
            args
        }

        fn fixture_path() -> PathBuf {
            std::env::args()
                .find_map(|arg| arg.strip_prefix("hd-process-fixture:").map(PathBuf::from))
                .expect("child fixture path")
        }

        fn fixture_watchdog() {
            // A broken implementation must not leave this test executable
            // alive indefinitely, even if the external assertion fires.
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(8));
                std::process::exit(99);
            });
        }

        fn bounded(test: impl FnOnce() + Send + 'static) {
            let (tx, rx) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(test));
                let _ = tx.send(result);
            });
            let result = rx
                .recv_timeout(Duration::from_secs(5))
                .expect("Windows Process fixture blocked");
            worker.join().unwrap();
            if let Err(panic) = result {
                std::panic::resume_unwind(panic);
            }
        }

        #[test]
        fn no_limit_process_preserves_binary_output_input_and_exit_status() {
            bounded(|| {
                let input = vec![b'x'; 262_144];
                let (out, err, status) = run_process(
                    std::env::current_exe().unwrap().to_str().unwrap(),
                    &fixture_args("child_echo", None),
                    &input,
                )
                .unwrap();
                // libtest prints a short preamble before running the fixture.
                // The payload itself must remain binary-exact, not normalized.
                let payload = &out[out.len() - 524_288..];
                assert!(payload[..262_144].iter().all(|byte| *byte == 0xff));
                assert_eq!(&payload[262_144..], input);
                assert_eq!(err, vec![0; 262_144]);
                assert_eq!(status, 23);
            });
        }

        #[test]
        fn no_limit_process_reports_epipe_when_the_child_closes_stdin() {
            bounded(|| {
                let error = run_process(
                    std::env::current_exe().unwrap().to_str().unwrap(),
                    &fixture_args("child_exit", None),
                    &vec![0; 262_144],
                )
                .unwrap_err();
                assert_eq!(process_error(&error), "EPIPE");
            });
        }

        #[test]
        fn deadline_closes_quiet_pipes_inherited_after_the_direct_child_exits() {
            let path = std::env::temp_dir().join(format!(
                "hd-process-windows-descendant-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let args = fixture_args("child_descendant", Some(&path));
            let start = Instant::now();
            let deadline = start + Duration::from_secs(2);
            let (tx, rx) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                let result = run_process_with_limit_and_deadline(
                    std::env::current_exe().unwrap().to_str().unwrap(),
                    &args,
                    &vec![0; 262_144],
                    8192,
                    Some(deadline),
                );
                let _ = tx.send(result);
            });
            let result = rx.recv_timeout(Duration::from_secs(5));
            let elapsed = start.elapsed();
            let ready = path.with_extension("ready").is_file();
            // Release only our own descendant even if cancellation regresses.
            // It polls this file and also has an independent eight-second
            // watchdog. No taskkill quoting or unsafe handle APIs are needed.
            std::fs::write(&path, []).unwrap();
            worker.join().unwrap();
            let cleanup_deadline = Instant::now() + Duration::from_secs(1);
            while !path.with_extension("closed").is_file() && Instant::now() < cleanup_deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            let closed = path.with_extension("closed").is_file();
            let _ = std::fs::remove_file(path.with_extension("ready"));
            let _ = std::fs::remove_file(path.with_extension("closed"));
            std::fs::remove_file(path).unwrap();
            assert!(
                ready,
                "fixture failed to create its inherited-pipe descendant"
            );
            assert_eq!(result.unwrap().unwrap_err().kind(), io::ErrorKind::TimedOut);
            assert!(elapsed < Duration::from_secs(3));
            assert!(closed, "fixture descendant did not acknowledge cleanup");
        }

        #[test]
        #[ignore = "subprocess fixture, invoked by the Windows Process tests"]
        fn child_echo() {
            fixture_watchdog();
            io::stdout().write_all(&vec![0xff; 262_144]).unwrap();
            io::stderr().write_all(&vec![0; 262_144]).unwrap();
            let mut input = Vec::new();
            io::stdin().read_to_end(&mut input).unwrap();
            io::stdout().write_all(&input).unwrap();
            std::process::exit(23);
        }

        #[test]
        #[ignore = "subprocess fixture, invoked by the Windows Process tests"]
        fn child_exit() {
            std::process::exit(0);
        }

        #[test]
        #[ignore = "subprocess fixture, invoked by the Windows Process tests"]
        fn child_descendant() {
            fixture_watchdog();
            let path = fixture_path();
            let _descendant = Command::new(std::env::current_exe().unwrap())
                .args(fixture_args("child_quiet", Some(&path)))
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            std::fs::write(path.with_extension("ready"), []).unwrap();
            std::process::exit(0);
        }

        #[test]
        #[ignore = "subprocess fixture, invoked by the Windows Process tests"]
        fn child_quiet() {
            fixture_watchdog();
            let path = fixture_path();
            while !path.is_file() {
                std::thread::sleep(Duration::from_millis(5));
            }
            std::fs::write(path.with_extension("closed"), []).unwrap();
            std::process::exit(0);
        }
    }

    #[cfg(unix)]
    mod unix {
        use super::super::{
            process_error, run_process, run_process_with_limit, run_process_with_limit_and_deadline,
        };
        use std::io;

        fn shell(script: &str) -> Vec<String> {
            vec!["-c".into(), script.into()]
        }

        #[test]
        fn process_captures_both_streams_exit_code_and_signal() {
            let (out, err, status) =
                run_process("/bin/sh", &shell("printf out; printf err >&2; exit 7"), &[]).unwrap();
            assert_eq!(out, b"out");
            assert_eq!(err, b"err");
            assert_eq!(status, 7);
            let (_, _, status) = run_process("/bin/sh", &shell("kill -TERM $$"), &[]).unwrap();
            assert_eq!(status, 128);
        }

        #[test]
        fn process_drains_output_while_writing_input_and_reports_closed_stdin() {
            let script = "dd if=/dev/zero bs=65536 count=4 2>/dev/null; \
                          dd if=/dev/zero bs=65536 count=4 >&2 2>/dev/null; \
                          wc -c";
            let (out, err, status) =
                run_process("/bin/sh", &shell(script), &vec![0; 262_144]).unwrap();
            assert_eq!(status, 0);
            assert!(out[..262_144].iter().all(|b| *b == 0));
            assert_eq!(String::from_utf8_lossy(&out[262_144..]).trim(), "262144");
            assert_eq!(err.len(), 262_144);
            assert!(err.iter().all(|b| *b == 0));
            let err = run_process("/bin/sh", &shell("exit 0"), &vec![0; 262_144]).unwrap_err();
            assert_eq!(process_error(&err), "EPIPE");
        }

        #[test]
        fn output_cap_is_aggregate_and_stops_an_infinite_writer() {
            let (out, err, _) = run_process_with_limit(
                "/bin/sh",
                &shell("printf 12345; printf 67890 >&2"),
                &[],
                10,
            )
            .unwrap();
            assert_eq!(out, b"12345");
            assert_eq!(err, b"67890");
            let err =
                run_process_with_limit("/bin/sh", &shell("printf 12345; printf 67890 >&2"), &[], 9)
                    .unwrap_err();
            assert_eq!(process_error(&err), "ENOBUFS");
            let err = run_process_with_limit("/bin/sh", &shell("yes"), &[], 8192).unwrap_err();
            assert_eq!(process_error(&err), "ENOBUFS");
        }

        #[test]
        fn output_cap_closes_quiet_pipes_inherited_by_a_descendant() {
            let pid_file = std::env::temp_dir().join(format!(
                "hd-process-cap-descendant-{}-{}.pid",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            // The explicit stdin redirection prevents the shell's background
            // command from replacing stdin with /dev/null. sleep retains both
            // stdin and quiet stderr, but neither consumes nor produces data.
            let script = "exec 3<&0; sleep 30 0<&3 1>/dev/null & \
                          printf '%s' \"$!\" > \"$1\"; \
                          printf 123456789; wait";
            let args = vec![
                "-c".into(),
                script.into(),
                "hd-cap-test".into(),
                pid_file.to_str().unwrap().into(),
            ];
            let (tx, rx) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                let result = run_process_with_limit("/bin/sh", &args, &vec![0; 262_144], 8);
                let _ = tx.send(result);
            });
            let result = rx.recv_timeout(std::time::Duration::from_secs(2));
            // Always release the test-owned descendant before asserting, so
            // a regression fails promptly instead of leaving blocked workers.
            let pid = std::fs::read_to_string(&pid_file)
                .unwrap()
                .parse::<i32>()
                .unwrap();
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGTERM,
            );
            std::fs::remove_file(pid_file).unwrap();
            worker.join().unwrap();
            assert_eq!(process_error(&result.unwrap().unwrap_err()), "ENOBUFS");
        }

        #[test]
        fn overflow_event_itself_cancels_inherited_quiet_pipe_workers() {
            use super::super::{
                ReadEvent, Spawned, drain, feed_input, spawn_process, wait_pipe,
                wait_process_and_pipes,
            };
            use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

            let pid_file = std::env::temp_dir().join(format!(
                "hd-process-overflow-event-{}-{}.pid",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let args = vec![
                "-c".into(),
                "exec 3<&0; sleep 30 0<&3 1>/dev/null & \
                 printf '%s' \"$!\" > \"$1\"; printf 123456789; wait"
                    .into(),
                "hd-overflow-event-test".into(),
                pid_file.to_str().unwrap().into(),
            ];
            let start = std::time::Instant::now();
            let (completed, completion) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                let Spawned {
                    mut child,
                    mut stdin,
                    mut stdout,
                    mut stderr,
                } = spawn_process("/bin/sh", &args, None).unwrap();
                let cancelled = AtomicBool::new(false);
                let count = AtomicUsize::new(0);
                let result = std::thread::scope(|scope| {
                    let (tx, rx) = std::sync::mpsc::channel();
                    let reader_tx = tx.clone();
                    let writer_tx = tx.clone();
                    let stop = &cancelled;
                    let count = &count;
                    let reader =
                        scope.spawn(move || drain(&mut *stderr, count, &reader_tx, 8, stop));
                    let writer = scope.spawn(move || {
                        let result = feed_input(&mut *stdin, &vec![0; 262_144], stop);
                        let _ = writer_tx.send(ReadEvent::Done);
                        result
                    });
                    let mut bytes = [0; 9];
                    let mut length = 0;
                    while length < bytes.len() {
                        match stdout.read(&mut bytes[length..]) {
                            Ok(0) => panic!("child closed stdout before producing overflow"),
                            Ok(n) => length += n,
                            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                                wait_pipe(stdout.as_fd(), nix::poll::PollFlags::POLLIN, stop)
                                    .unwrap();
                            }
                            Err(error) => panic!("reading child overflow: {error}"),
                        }
                    }
                    assert_eq!(&bytes, b"123456789");
                    assert!(!cancelled.load(Ordering::Relaxed));
                    // Deliberately do NOT pre-set cancellation in the producer:
                    // the controller must make an overflow event sufficient.
                    tx.send(ReadEvent::Overflow).unwrap();
                    drop(tx);
                    let result = wait_process_and_pipes(&mut child, &rx, stop, None);
                    reader.join().unwrap().unwrap();
                    writer.join().unwrap().unwrap();
                    result
                });
                completed
                    .send((result, cancelled.load(Ordering::Relaxed)))
                    .unwrap();
            });
            let result = completion.recv_timeout(std::time::Duration::from_secs(2));
            // This external bound also releases the fixture if the controller
            // forgets cancellation, leaving quiet descendant pipes blocked.
            let pid = std::fs::read_to_string(&pid_file)
                .unwrap()
                .parse::<i32>()
                .unwrap();
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            );
            worker.join().unwrap();
            std::fs::remove_file(pid_file).unwrap();
            let (status, cancelled) = result.unwrap();
            assert_eq!(status.unwrap(), (128, true));
            assert!(cancelled);
            assert!(start.elapsed() < std::time::Duration::from_secs(1));
        }

        #[test]
        fn deadline_kills_and_reaps_a_term_ignoring_child_after_all_pipes_close() {
            let pid_file = std::env::temp_dir().join(format!(
                "hd-process-deadline-child-{}-{}.pid",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let script = "trap '' TERM; printf '%s' \"$$\" > \"$1\"; \
                          exec 0<&- 1>&- 2>&-; while :; do :; done";
            let args = vec![
                "-c".into(),
                script.into(),
                "hd-deadline-test".into(),
                pid_file.to_str().unwrap().into(),
            ];
            let start = std::time::Instant::now();
            let deadline = start + std::time::Duration::from_millis(250);
            let (tx, rx) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                let result = run_process_with_limit_and_deadline(
                    "/bin/sh",
                    &args,
                    &[],
                    8192,
                    Some(deadline),
                );
                let _ = tx.send(result);
            });
            let result = rx.recv_timeout(std::time::Duration::from_secs(2));
            let pid = nix::unistd::Pid::from_raw(
                std::fs::read_to_string(&pid_file)
                    .unwrap()
                    .parse::<i32>()
                    .unwrap(),
            );
            // On regression, release this test's child before joining the
            // worker, so failure cannot leave a TERM-ignoring busy loop behind.
            if result.is_err() {
                let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGKILL);
            }
            worker.join().unwrap();
            std::fs::remove_file(pid_file).unwrap();
            assert_eq!(result.unwrap().unwrap_err().kind(), io::ErrorKind::TimedOut);
            assert!(start.elapsed() < std::time::Duration::from_secs(1));
            assert_eq!(
                nix::sys::wait::waitpid(pid, Some(nix::sys::wait::WaitPidFlag::WNOHANG)),
                Err(nix::errno::Errno::ECHILD),
                "the deadline path must reap its direct child"
            );
        }

        #[test]
        fn deadline_cancels_inherited_quiet_pipes_after_the_direct_child_exits() {
            let pid_file = std::env::temp_dir().join(format!(
                "hd-process-deadline-descendant-{}-{}.pid",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let args = vec![
                "-c".into(),
                "exec 3<&0; sleep 30 0<&3 1>/dev/null & \
                 printf '%s' \"$!\" > \"$1\"; exit 0"
                    .into(),
                "hd-deadline-test".into(),
                pid_file.to_str().unwrap().into(),
            ];
            let start = std::time::Instant::now();
            let deadline = start + std::time::Duration::from_millis(250);
            let (tx, rx) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                let result = run_process_with_limit_and_deadline(
                    "/bin/sh",
                    &args,
                    &vec![0; 262_144],
                    8192,
                    Some(deadline),
                );
                let _ = tx.send(result);
            });
            let result = rx.recv_timeout(std::time::Duration::from_secs(2));
            // The descendant is intentionally independent of the direct
            // child. Release the fixture even if cancellation regresses.
            let pid = std::fs::read_to_string(&pid_file)
                .unwrap()
                .parse::<i32>()
                .unwrap();
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            );
            worker.join().unwrap();
            std::fs::remove_file(pid_file).unwrap();
            assert_eq!(result.unwrap().unwrap_err().kind(), io::ErrorKind::TimedOut);
            assert!(start.elapsed() < std::time::Duration::from_secs(1));
        }

        #[test]
        fn spawn_error_text_uses_node_errno_names() {
            assert_eq!(
                process_error(&io::Error::from_raw_os_error(libc::ENOTDIR)),
                "ENOTDIR"
            );
            assert_eq!(
                process_error(&io::Error::from_raw_os_error(libc::E2BIG)),
                "E2BIG"
            );
        }

        #[test]
        fn no_shebang_executable_uses_rust_standard_spawning_behavior() {
            use std::io::Write;
            use std::os::unix::fs::PermissionsExt;
            let path = std::env::temp_dir().join(format!(
                "hd-process-no-shebang-{}-{}.sh",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .unwrap();
            file.write_all(b"printf '%s' \"$1\"; exit 6\n").unwrap();
            file.set_permissions(std::fs::Permissions::from_mode(0o700))
                .unwrap();
            drop(file);
            let expected = std::process::Command::new(&path).arg("argument").output();
            let result = run_process(path.to_str().unwrap(), &["argument".into()], &[]);
            std::fs::remove_file(&path).unwrap();
            if let Ok(expected) = expected {
                let (out, err, status) = result.unwrap();
                assert_eq!(out, expected.stdout);
                assert_eq!(err, expected.stderr);
                assert_eq!(status, i64::from(expected.status.code().unwrap_or(128)));
            } else {
                assert_eq!(result.unwrap_err().kind(), expected.unwrap_err().kind());
            }
        }

        #[test]
        fn child_cwd_is_instance_local_and_arguments_are_not_shell_syntax() {
            let owner = crate::testing::State::new(crate::testing::TestCase::default());
            let cwd = std::path::PathBuf::from(owner.temp_dir().unwrap());
            let parent = std::env::current_dir().unwrap();
            let (out, err, status) =
                super::super::run_process_at("/bin/sh", &shell("pwd"), &[], 1024, None, Some(&cwd))
                    .unwrap();
            assert_eq!(
                String::from_utf8(out).unwrap().trim(),
                cwd.canonicalize().unwrap().to_str().unwrap()
            );
            assert!(err.is_empty());
            assert_eq!(status, 0);
            let argument = "literal; $(echo not-executed)";
            let args = vec![
                "-c".into(),
                "printf '%s' \"$1\"".into(),
                "fixture".into(),
                argument.into(),
            ];
            let (out, _, _) =
                super::super::run_process_at("/bin/sh", &args, &[], 1024, None, Some(&cwd))
                    .unwrap();
            assert_eq!(out, argument.as_bytes());
            assert_eq!(std::env::current_dir().unwrap(), parent);
        }
    }
}
