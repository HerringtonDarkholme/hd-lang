//! Random, Sys and Process providers of the Node default host profile.

use std::io::{self, Read, Write};
#[cfg(not(unix))]
use std::process::{Child, Command, Stdio};

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
    match run_process(&name, &argv, input) {
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
fn run_process(name: &str, argv: &[String], input: &[u8]) -> io::Result<(Vec<u8>, Vec<u8>, i64)> {
    run_process_with_limit(name, argv, input, 1 << 30)
}

fn run_process_with_limit(
    name: &str,
    argv: &[String],
    input: &[u8],
    output_limit: usize,
) -> io::Result<(Vec<u8>, Vec<u8>, i64)> {
    let Spawned {
        mut child,
        mut stdin,
        mut stdout,
        mut stderr,
    } = spawn_process(name, argv)?;
    let count = std::sync::atomic::AtomicUsize::new(0);
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::channel();
        let out_tx = tx.clone();
        let err_tx = tx.clone();
        let out_count = &count;
        let err_count = &count;
        let stop = &cancelled;
        let out = scope.spawn(move || drain(&mut *stdout, out_count, &out_tx, output_limit, stop));
        let err = scope.spawn(move || drain(&mut *stderr, err_count, &err_tx, output_limit, stop));
        let writer = scope.spawn(move || feed_input(&mut *stdin, input, stop));
        drop(tx);
        // A reader reports overflow immediately; kill before joining
        // readers, otherwise a child with infinite output would hang.
        // Also stop all three pipe workers: descendants can retain the
        // quiet pipes even after the direct child exits.
        let mut overflow = false;
        for event in rx {
            match event {
                ReadEvent::Done => (),
                ReadEvent::Overflow => {
                    overflow = true;
                    let _ = kill_process(&mut child);
                }
                ReadEvent::Failed => {
                    cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                    let _ = kill_process(&mut child);
                }
            }
        }
        let status = wait_process(&mut child)?;
        let input_result = writer
            .join()
            .map_err(|_| io::Error::other("stdin thread panicked"))?;
        let out = out
            .join()
            .map_err(|_| io::Error::other("stdout thread panicked"))?;
        let err = err
            .join()
            .map_err(|_| io::Error::other("stderr thread panicked"))?;
        if overflow {
            return Err(io::Error::from(io::ErrorKind::FileTooLarge));
        }
        // spawnSync reports EPIPE when a child closes stdin while input
        // remains, even if the child itself exits successfully.
        input_result?;
        Ok((out?, err?, status))
    })
}

#[cfg(unix)]
type Process = nix::unistd::Pid;
#[cfg(not(unix))]
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

#[cfg(not(unix))]
fn spawn_process(name: &str, argv: &[String]) -> io::Result<Spawned> {
    let mut child = Command::new(name)
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
fn spawn_process(name: &str, argv: &[String]) -> io::Result<Spawned> {
    use nix::spawn::{PosixSpawnAttr, PosixSpawnFileActions, PosixSpawnFlags, posix_spawn};
    use nix::sys::signal::SigSet;
    use std::ffi::{CString, OsStr};
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;

    let (stdin_read, stdin) = io::pipe()?;
    let (stdout, stdout_write) = io::pipe()?;
    let (stderr, stderr_write) = io::pipe()?;
    // Only the parent ends are nonblocking. Each pipe's two ends have
    // distinct open-file descriptions, so the child's stdio stays blocking.
    set_nonblocking(&stdin)?;
    set_nonblocking(&stdout)?;
    set_nonblocking(&stderr)?;
    let mut actions = PosixSpawnFileActions::init()?;
    for (source, target) in [
        (stdin_read.as_raw_fd(), 0),
        (stdout_write.as_raw_fd(), 1),
        (stderr_write.as_raw_fd(), 2),
    ] {
        actions.add_dup2(source, target)?;
    }
    let mut attrs = PosixSpawnAttr::init()?;
    attrs.set_sigdefault(&SigSet::all())?;
    attrs.set_sigmask(&SigSet::empty())?;
    let flags = PosixSpawnFlags::POSIX_SPAWN_SETSIGDEF | PosixSpawnFlags::POSIX_SPAWN_SETSIGMASK;
    #[cfg(target_vendor = "apple")]
    let flags = flags | PosixSpawnFlags::from_bits_retain(libc::POSIX_SPAWN_CLOEXEC_DEFAULT);
    attrs.set_flags(flags)?;
    let c_args = std::iter::once(name)
        .chain(argv.iter().map(String::as_str))
        .map(CString::new)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let env = std::env::vars_os()
        .map(|(key, value)| {
            let mut bytes = key.as_bytes().to_vec();
            bytes.push(b'=');
            bytes.extend_from_slice(value.as_bytes());
            CString::new(bytes).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
        })
        .collect::<io::Result<Vec<_>>>()?;
    let spawn = |path: &OsStr| {
        let result = loop {
            match posix_spawn(path, &actions, &attrs, &c_args, &env) {
                Err(nix::errno::Errno::EINTR) => (),
                result => break result,
            }
        };
        #[cfg(any(
            all(target_os = "linux", target_env = "gnu"),
            target_os = "freebsd",
            target_os = "openbsd",
            target_os = "netbsd",
            target_os = "dragonfly"
        ))]
        if result == Err(nix::errno::Errno::ENOEXEC) {
            // Node/libuv 1.52 uses execvp outside Apple platforms. glibc
            // and BSD libc run executable text through /bin/sh; musl
            // instead propagates ENOEXEC, as does Apple's libuv path.
            let file = CString::new(path.as_bytes())
                .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
            let shell_args = std::iter::once(c"/bin/sh")
                .chain(std::iter::once(file.as_c_str()))
                .chain(c_args.iter().skip(1).map(CString::as_c_str))
                .collect::<Vec<_>>();
            return loop {
                match posix_spawn(OsStr::new("/bin/sh"), &actions, &attrs, &shell_args, &env) {
                    Err(nix::errno::Errno::EINTR) => (),
                    result => break result.map_err(io::Error::from),
                }
            };
        }
        result.map_err(io::Error::from)
    };
    // libuv deliberately implements PATH search over posix_spawn, since
    // Apple's posix_spawnp silently executes non-executable-format files
    // through a shell instead of returning ENOEXEC.
    let child = if name.contains('/') {
        spawn(OsStr::new(name))?
    } else {
        // libuv uses NAME_MAX, or 255 where headers omit that constant.
        if name.len() > 255 {
            return Err(io::Error::from_raw_os_error(libc::ENAMETOOLONG));
        }
        let path = env
            .iter()
            .find_map(|entry| entry.as_bytes().strip_prefix(b"PATH="))
            .unwrap_or(b"/usr/bin:/bin");
        let mut last = io::Error::from(io::ErrorKind::NotFound);
        let mut denied = false;
        let mut found = None;
        for entry in path.split(|b| *b == b':') {
            if entry.len() >= usize::try_from(libc::PATH_MAX).unwrap_or(4096) {
                continue;
            }
            let path = std::path::Path::new(OsStr::from_bytes(entry)).join(name);
            match spawn(path.as_os_str()) {
                Ok(pid) => {
                    found = Some(pid);
                    break;
                }
                Err(err) if err.raw_os_error() == Some(libc::EACCES) => {
                    denied = true;
                    last = err;
                }
                Err(err) if matches!(err.raw_os_error(), Some(libc::ENOENT | libc::ENOTDIR)) => {
                    last = err;
                }
                Err(err) => return Err(err),
            }
        }
        found.ok_or_else(|| {
            if denied {
                io::Error::from(io::ErrorKind::PermissionDenied)
            } else {
                last
            }
        })?
    };
    Ok(Spawned {
        child,
        stdin: Box::new(stdin),
        stdout: Box::new(stdout),
        stderr: Box::new(stderr),
    })
}

#[cfg(unix)]
fn kill_process(child: &mut Process) -> io::Result<()> {
    nix::sys::signal::kill(*child, nix::sys::signal::Signal::SIGTERM).map_err(io::Error::from)
}

#[cfg(not(unix))]
fn kill_process(child: &mut Process) -> io::Result<()> {
    child.kill()
}

#[cfg(unix)]
fn wait_process(child: &mut Process) -> io::Result<i64> {
    use nix::sys::wait::{WaitStatus, waitpid};
    loop {
        match waitpid(*child, None) {
            Ok(WaitStatus::Exited(_, status)) => return Ok(i64::from(status)),
            Ok(WaitStatus::Signaled(..)) => return Ok(128),
            Ok(_) | Err(nix::errno::Errno::EINTR) => (),
            Err(err) => return Err(err.into()),
        }
    }
}

#[cfg(not(unix))]
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

    #[cfg(unix)]
    mod unix {
        use super::super::{process_error, run_process, run_process_with_limit};
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
        fn no_shebang_executable_matches_the_platforms_execvp_behavior() {
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
            let result = run_process(path.to_str().unwrap(), &["argument".into()], &[]);
            std::fs::remove_file(&path).unwrap();
            if cfg!(any(
                all(target_os = "linux", target_env = "gnu"),
                target_os = "freebsd",
                target_os = "openbsd",
                target_os = "netbsd",
                target_os = "dragonfly"
            )) {
                let (out, err, status) = result.unwrap();
                assert_eq!(out, b"argument");
                assert!(err.is_empty());
                assert_eq!(status, 6);
            } else {
                assert_eq!(result.unwrap_err().raw_os_error(), Some(libc::ENOEXEC));
            }
        }
    }
}
