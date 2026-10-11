//! Synchronous filesystem providers, matching `core.mjs`'s `fsOp`.
//! Grants are checked against resolved paths before any filesystem access.
//! Limited Unix calls use nonblocking descriptors and deadline-bounded FIFO
//! polling. Ordinary filesystem syscalls (including network filesystem access)
//! may still block inside the kernel; safe std does not cancel them. Limited
//! Windows named-pipe calls poll immediate I/O without background workers.
//! No mutation is delegated to a worker that could outlive the deadline.

use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use hd_run::Grants;

use crate::codec::{Dec, Enc};

pub const READ: &[&str] = &["read_bytes", "read_text", "list_dir", "stat"];
pub const WRITE: &[&str] = &[
    "write_bytes",
    "write_text",
    "append_text",
    "create_dir_all",
    "remove",
    "rename",
];

pub fn call(key: &str, method: &str, bytes: &[u8], grants: &Grants) -> wasmtime::Result<Enc> {
    call_until(key, method, bytes, grants, None)
}

pub fn call_until(
    key: &str,
    method: &str,
    bytes: &[u8],
    grants: &Grants,
    deadline: Option<Instant>,
) -> wasmtime::Result<Enc> {
    check_deadline(deadline).map_err(|_| crate::host::Stop::TimeLimit)?;
    let cwd = std::env::current_dir().map_err(|e| crate::host::Stop::Internal(e.to_string()))?;
    call_until_at(key, method, bytes, grants, deadline, &cwd)
}

/// Executes relative paths in the case's directory without changing the
/// process-wide working directory. Encoded paths and errors retain the
/// guest's original spelling.
pub fn call_until_at(
    key: &str,
    method: &str,
    bytes: &[u8],
    grants: &Grants,
    deadline: Option<Instant>,
    cwd: &Path,
) -> wasmtime::Result<Enc> {
    check_deadline(deadline).map_err(|_| crate::host::Stop::TimeLimit)?;
    let mut d = Dec::new(bytes);
    let path = d.str()?;
    let to = if method == "rename" {
        Some(d.str()?)
    } else {
        None
    };
    for p in std::iter::once(&path).chain(to.iter()) {
        if !grants.covers_path(key, Path::new(p), cwd) {
            return Ok(error(p, 2));
        }
    }
    // Decode payloads only after grants, as fsOp does on Node.
    let payload = if matches!(method, "write_bytes" | "write_text" | "append_text") {
        Some(d.bytes()?)
    } else {
        None
    };
    // Node validates path arguments after the grant check, before touching
    // the filesystem. In particular, a NUL is not an OS EINVAL error.
    for (p, name) in std::iter::once((
        &path,
        if method == "rename" {
            "oldPath"
        } else {
            "path"
        },
    ))
    .chain(to.iter().map(|p| (p, "newPath")))
    {
        if p.contains('\0') {
            return Ok(error(
                &format!(
                    "The argument '{name}' must be a string, Uint8Array, or URL without null bytes. Received {}",
                    inspect_string(p)
                ),
                7,
            ));
        }
    }
    let mut e = Enc::default();
    let result = run(method, &path, to.as_deref(), payload, &mut e, deadline, cwd);
    check_deadline(deadline).map_err(|_| crate::host::Stop::TimeLimit)?;
    Ok(match result {
        Ok(()) => e,
        Err(Failure::Deadline) => return Err(crate::host::Stop::TimeLimit.into()),
        Err(err) => failure(to.as_deref().unwrap_or(&path), method, &path, &err),
    })
}

fn run(
    method: &str,
    path: &str,
    to: Option<&str>,
    payload: Option<&[u8]>,
    e: &mut Enc,
    deadline: Option<Instant>,
    cwd: &Path,
) -> Result<(), Failure> {
    check_deadline(deadline)?;
    // Keep an empty argument empty: Node rejects it instead of treating it
    // as the working directory. Absolute paths are unchanged by join.
    let at = |p: &str| {
        if p.is_empty() {
            std::path::PathBuf::new()
        } else {
            cwd.join(p)
        }
    };
    let actual = at(path);
    match method {
        "read_bytes" => {
            e.leb(0).bytes(&read(&actual, deadline)?);
        }
        "read_text" => {
            let bytes = read(&actual, deadline)?;
            check_deadline(deadline)?;
            if let Ok(text) = std::str::from_utf8(&bytes) {
                // TextDecoder strips the initial UTF-8 BOM.
                e.leb(0).str(text.strip_prefix('\u{feff}').unwrap_or(text));
            } else {
                *e = error(path, 6);
            }
        }
        "stat" => match fs::symlink_metadata(&actual) {
            Ok(st) => {
                entry(e.leb(0).leb(1), path, &st);
            }
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                e.leb(0).leb(0);
            }
            Err(err) => return Err(Failure::new("lstat", err)),
        },
        "list_dir" => {
            #[cfg(windows)]
            if fs::metadata(&actual).is_ok_and(|st| !st.is_dir()) {
                return Err(Failure::new("scandir", io::ErrorKind::NotADirectory.into()));
            }
            // Incremental heap ordering allows checks during ordering, not just
            // around one potentially large, uncancellable sort operation.
            let mut names = std::collections::BinaryHeap::new();
            for item in fs::read_dir(&actual).map_err(|err| Failure::new("scandir", err))? {
                check_deadline(deadline)?;
                let item = item.map_err(|err| Failure::new("scandir", err))?;
                names.push(std::cmp::Reverse(
                    item.file_name().to_string_lossy().into_owned(),
                ));
            }
            let under = if path.ends_with('/') {
                path.to_owned()
            } else {
                format!("{path}/")
            };
            let mut found = Vec::new();
            while let Some(std::cmp::Reverse(n)) = names.pop() {
                check_deadline(deadline)?;
                let p = format!("{under}{n}");
                if let Ok(st) = fs::symlink_metadata(actual.join(&n)) {
                    found.push((p, st));
                }
            }
            e.leb(0).leb(u64::try_from(found.len()).unwrap_or(u64::MAX));
            for (p, st) in found {
                check_deadline(deadline)?;
                entry(e, &p, &st);
            }
        }
        "write_bytes" | "write_text" => {
            let mut file = open(&actual, true, false, deadline)?;
            write(&mut file, payload.unwrap_or_default(), deadline)?;
            e.leb(0);
        }
        "append_text" => {
            let mut file = open(&actual, true, true, deadline)?;
            write(&mut file, payload.unwrap_or_default(), deadline)?;
            e.leb(0);
        }
        "create_dir_all" => {
            if fs::metadata(&actual).is_ok_and(|st| !st.is_dir()) {
                *e = error(path, 4);
            } else {
                if path.is_empty() {
                    // std accepts the empty path; Node mkdir does not.
                    *e = error(path, 0);
                    return Ok(());
                }
                mkdir(&actual, deadline).map_err(|err| shown_mkdir_failure(err, cwd, path))?;
                e.leb(0);
            }
        }
        "remove" => {
            if fs::symlink_metadata(&actual)
                .map_err(|err| Failure::new("lstat", err))?
                .is_dir()
            {
                check_deadline(deadline)?;
                fs::remove_dir(&actual).map_err(|err| Failure::new("rmdir", err))?;
            } else {
                check_deadline(deadline)?;
                fs::remove_file(&actual).map_err(|err| Failure::new("unlink", err))?;
            }
            e.leb(0);
        }
        "rename" => {
            fs::rename(&actual, at(to.unwrap_or_default()))
                .map_err(|err| Failure::new("rename", err))?;
            e.leb(0);
        }
        _ => unreachable!("registered filesystem method"),
    }
    Ok(())
}

enum Failure {
    Deadline,
    System {
        error: io::Error,
        syscall: &'static str,
        // Recursive mkdir can fail while creating an ancestor. Node's Other
        // message names that ancestor, while typed errors name the input path.
        path: Option<String>,
    },
    Message(String),
}

impl Failure {
    fn new(syscall: &'static str, error: io::Error) -> Self {
        Self::System {
            error,
            syscall,
            path: None,
        }
    }
}

fn check_deadline(deadline: Option<Instant>) -> Result<(), Failure> {
    if deadline.is_some_and(|limit| Instant::now() >= limit) {
        Err(Failure::Deadline)
    } else {
        Ok(())
    }
}

// Runner-owned snapshots/regressions use the same safe, cancellable pipe
// operations as guest filesystem calls, but retain ordinary Rust I/O errors.
pub(crate) fn read_file_until(path: &Path, deadline: Option<Instant>) -> io::Result<Vec<u8>> {
    read(path, deadline).map_err(file_failure)
}

pub(crate) fn write_file_until(
    path: &Path,
    bytes: &[u8],
    deadline: Option<Instant>,
) -> io::Result<()> {
    let result = (|| {
        let mut file = open(path, true, false, deadline)?;
        write(&mut file, bytes, deadline)
    })();
    result.map_err(file_failure)
}

fn file_failure(failure: Failure) -> io::Error {
    match failure {
        Failure::Deadline => io::Error::new(
            io::ErrorKind::TimedOut,
            "the program exceeded its time limit",
        ),
        Failure::System { error, .. } => error,
        Failure::Message(message) => io::Error::other(message),
    }
}

enum OpenedFile {
    Regular(File),
    #[cfg(windows)]
    Pipe(crate::windows_pipe::Pipe),
}

impl OpenedFile {
    fn metadata(&self) -> io::Result<Option<Metadata>> {
        match self {
            Self::Regular(file) => file.metadata().map(Some),
            // NPFS does not provide ordinary-file size metadata.
            #[cfg(windows)]
            Self::Pipe(_) => Ok(None),
        }
    }
}

impl Read for OpenedFile {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Regular(file) => file.read(bytes),
            #[cfg(windows)]
            Self::Pipe(pipe) => pipe.read(bytes),
        }
    }

    fn read_to_end(&mut self, bytes: &mut Vec<u8>) -> io::Result<usize> {
        match self {
            Self::Regular(file) => file.read_to_end(bytes),
            #[cfg(windows)]
            Self::Pipe(pipe) => pipe.read_to_end(bytes),
        }
    }
}

impl Write for OpenedFile {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self {
            Self::Regular(file) => file.write(bytes),
            #[cfg(windows)]
            Self::Pipe(pipe) => pipe.write(bytes),
        }
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        match self {
            Self::Regular(file) => file.write_all(bytes),
            #[cfg(windows)]
            Self::Pipe(pipe) => pipe.write_all(bytes),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Regular(file) => file.flush(),
            #[cfg(windows)]
            Self::Pipe(pipe) => pipe.flush(),
        }
    }
}

fn read(path: &Path, deadline: Option<Instant>) -> Result<Vec<u8>, Failure> {
    let mut file = open(path, false, false, deadline)?;
    check_deadline(deadline)?;
    let st = file.metadata().map_err(|err| Failure::new("fstat", err))?;
    if let Some(st) = st
        .as_ref()
        .filter(|st| st.is_file() && st.len() > 0x7fff_ffff)
    {
        return Err(Failure::Message(format!(
            "File size ({}) is greater than 2 GiB",
            st.len()
        )));
    }
    let mut bytes = Vec::new();
    if deadline.is_none() {
        file.read_to_end(&mut bytes)
            .map_err(|err| Failure::new("read", err))?;
        return Ok(bytes);
    }
    #[cfg(unix)]
    let fifo = {
        use std::os::unix::fs::FileTypeExt;
        st.is_some_and(|st| st.file_type().is_fifo())
    };
    let mut buffer = [0u8; 8 * 1024];
    loop {
        check_deadline(deadline)?;
        #[cfg(unix)]
        if fifo && bytes.is_empty() {
            // With O_NONBLOCK, a FIFO with no writer returns zero immediately.
            // Poll first so zero is EOF only after a writer has connected/closed.
            // Once data proves the writer connected, read directly: some Unix
            // poll implementations do not report FIFO hangup after draining.
            wait(&file, false, deadline)?;
        }
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => bytes.extend_from_slice(&buffer[..n]),
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                wait(&file, false, deadline)?;
            }
            Err(err) => return Err(Failure::new("read", err)),
        }
    }
    check_deadline(deadline)?;
    Ok(bytes)
}

fn write(
    file: &mut OpenedFile,
    mut bytes: &[u8],
    deadline: Option<Instant>,
) -> Result<(), Failure> {
    if deadline.is_none() {
        return file
            .write_all(bytes)
            .map_err(|err| Failure::new("write", err));
    }
    while !bytes.is_empty() {
        check_deadline(deadline)?;
        let chunk = &bytes[..bytes.len().min(64 * 1024)];
        match file.write(chunk) {
            Ok(0) => return Err(Failure::new("write", io::ErrorKind::WriteZero.into())),
            Ok(n) => bytes = &bytes[n..],
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => wait(file, true, deadline)?,
            Err(err) => return Err(Failure::new("write", err)),
        }
    }
    check_deadline(deadline)
}

fn wait(file: &OpenedFile, write: bool, deadline: Option<Instant>) -> Result<(), Failure> {
    match file {
        OpenedFile::Regular(file) => wait_regular(file, write, deadline),
        #[cfg(windows)]
        OpenedFile::Pipe(_) => pause(deadline),
    }
}

#[cfg(unix)]
fn wait_regular(file: &File, write: bool, deadline: Option<Instant>) -> Result<(), Failure> {
    use nix::poll::{PollFd, PollFlags};
    use std::os::fd::AsFd;
    #[cfg(target_vendor = "apple")]
    let duplicate = {
        use std::os::fd::AsRawFd;
        if usize::try_from(file.as_raw_fd()).is_ok_and(|fd| fd >= nix::sys::select::FD_SETSIZE) {
            // dup chooses the lowest free descriptor, often allowing select
            // even when the incoming descriptor's number exceeds its limit.
            Some(file.try_clone().map_err(|err| Failure::new("dup", err))?)
        } else {
            None
        }
    };
    #[cfg(target_vendor = "apple")]
    let file = duplicate.as_ref().unwrap_or(file);
    loop {
        check_deadline(deadline)?;
        let remaining = deadline.map_or(Duration::from_millis(50), |limit| {
            limit.saturating_duration_since(Instant::now())
        });
        // Round up fractional milliseconds; recheck the actual Instant on wake.
        let milliseconds = remaining.as_millis().saturating_add(1).min(50);
        let timeout = u16::try_from(milliseconds).unwrap_or(50);
        let flags = if write {
            PollFlags::POLLOUT
        } else {
            PollFlags::POLLIN
        };
        let mut fds = [PollFd::new(file.as_fd(), flags)];
        #[cfg(target_vendor = "apple")]
        let ready = {
            use nix::sys::select::{FD_SETSIZE, FdSet, select};
            use nix::sys::time::{TimeVal, TimeValLike};
            use std::os::fd::AsRawFd;
            if usize::try_from(file.as_raw_fd()).is_ok_and(|fd| fd < FD_SETSIZE) {
                // Darwin's poll/kqueue FIFO filter reports only byte count,
                // not EOF. select uses FIFO socket readiness, including EOF.
                let mut set = FdSet::new();
                set.insert(file.as_fd());
                let mut duration = TimeVal::milliseconds(i64::from(timeout));
                if write {
                    select(None, None, Some(&mut set), None, Some(&mut duration))
                } else {
                    select(None, Some(&mut set), None, None, Some(&mut duration))
                }
            } else {
                nix::poll::poll(&mut fds, timeout)
            }
        };
        #[cfg(not(target_vendor = "apple"))]
        let ready = nix::poll::poll(&mut fds, timeout);
        match ready {
            Ok(0) => {}
            Ok(_) => {
                check_deadline(deadline)?;
                return Ok(());
            }
            Err(nix::errno::Errno::EINTR) => {}
            Err(err) => return Err(Failure::new("poll", io::Error::from(err))),
        }
    }
}

#[cfg(not(unix))]
fn wait_regular(_: &File, _: bool, deadline: Option<Instant>) -> Result<(), Failure> {
    pause(deadline)
}

#[cfg(not(unix))]
fn pause(deadline: Option<Instant>) -> Result<(), Failure> {
    check_deadline(deadline)?;
    let remaining = deadline.map_or(Duration::from_millis(5), |limit| {
        limit.saturating_duration_since(Instant::now())
    });
    std::thread::sleep(remaining.min(Duration::from_millis(5)));
    check_deadline(deadline)
}

fn open(
    path: &Path,
    write: bool,
    append: bool,
    deadline: Option<Instant>,
) -> Result<OpenedFile, Failure> {
    #[cfg(windows)]
    if deadline.is_some() && named_pipe_path(&path.to_string_lossy()) {
        loop {
            check_deadline(deadline)?;
            match crate::windows_pipe::Pipe::open(&path.to_string_lossy(), write) {
                Ok(pipe) => return Ok(OpenedFile::Pipe(pipe)),
                Err(err)
                    if err.raw_os_error()
                        == i32::try_from(windows_sys::Win32::Foundation::ERROR_PIPE_BUSY).ok() =>
                {
                    pause(deadline)?;
                }
                Err(err) => return Err(Failure::new("open", err)),
            }
        }
    }
    let mut options = OpenOptions::new();
    if write {
        options
            .write(true)
            .create(true)
            .truncate(!append)
            .append(append);
    } else {
        options.read(true);
    }
    #[cfg(unix)]
    if deadline.is_some() {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
        // libuv opens directories too; a read then returns EISDIR, rather
        // than failing the open with Windows' ERROR_ACCESS_DENIED.
        options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
    }
    #[cfg(unix)]
    let file = loop {
        check_deadline(deadline)?;
        match options.open(path) {
            Err(err) if write && deadline.is_some() && err.raw_os_error() == Some(libc::ENXIO) => {
                use std::os::unix::fs::FileTypeExt;
                if !fs::metadata(path).is_ok_and(|st| st.file_type().is_fifo()) {
                    return Err(Failure::new("open", err));
                }
                check_deadline(deadline)?;
                let remaining = deadline.map_or(Duration::from_millis(5), |limit| {
                    limit.saturating_duration_since(Instant::now())
                });
                std::thread::sleep(remaining.min(Duration::from_millis(5)));
            }
            result => break result.map_err(|err| Failure::new("open", err))?,
        }
    };
    #[cfg(not(unix))]
    let file = {
        check_deadline(deadline)?;
        options
            .open(path)
            .map_err(|err| Failure::new("open", err))?
    };
    #[cfg(windows)]
    if deadline.is_some()
        && crate::windows_pipe::is_pipe(&file).map_err(|err| Failure::new("open", err))?
    {
        // Reparse points/device aliases can resolve an ordinary looking path
        // to a pipe. Never enter blocking std I/O if inspecting/configuring
        // its pipe mode fails (e.g. ACLs).
        return crate::windows_pipe::Pipe::from_file(file)
            .map(OpenedFile::Pipe)
            .map_err(|err| Failure::new("open", err));
    }
    Ok(OpenedFile::Regular(file))
}

#[cfg(windows)]
fn named_pipe_path(path: &str) -> bool {
    let normalized = path.replace('/', "\\");
    let path = normalized.to_ascii_lowercase();
    if path.starts_with(r"\\.\pipe\")
        || path.starts_with(r"\\?\pipe\")
        || path.starts_with(r"\\?\globalroot\device\namedpipe\")
    {
        return true;
    }
    // Both ordinary and extended UNC spellings can address a remote pipe.
    let unc = path
        .strip_prefix(r"\\?\unc\")
        .or_else(|| path.strip_prefix(r"\\"));
    unc.and_then(|rest| rest.split_once('\\'))
        .is_some_and(|(server, share)| !server.is_empty() && share.starts_with(r"pipe\"))
}

fn mkdir(path: &Path, deadline: Option<Instant>) -> Result<(), Failure> {
    check_deadline(deadline)?;
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                mkdir(parent, deadline)?;
                check_deadline(deadline)?;
                // Retry after the parent exists, including a race with another creator.
                return mkdir_finish(path, fs::create_dir(path));
            }
            Err(mkdir_failure(path, err))
        }
        result => mkdir_finish(path, result),
    }
}

fn mkdir_finish(path: &Path, result: io::Result<()>) -> Result<(), Failure> {
    match result {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => match fs::metadata(path) {
            Ok(st) if st.is_dir() => Ok(()),
            Ok(_) => Err(mkdir_failure(path, err)),
            // Node's recursive mkdir follows an existing symlink: a dangling
            // target yields ENOENT, and a loop yields ELOOP, rather than EEXIST.
            Err(err) => Err(mkdir_failure(path, err)),
        },
        Err(err) => Err(mkdir_failure(path, err)),
    }
}

fn mkdir_failure(path: &Path, error: io::Error) -> Failure {
    Failure::System {
        error,
        syscall: "mkdir",
        path: Some(path.to_string_lossy().into_owned()),
    }
}

fn shown_mkdir_failure(failure: Failure, cwd: &Path, original: &str) -> Failure {
    match failure {
        Failure::System {
            error,
            syscall,
            path: Some(path),
        } if Path::new(original).is_relative() => {
            let failed = Path::new(&path);
            let shown = if failed == cwd.join(original) {
                original.to_owned()
            } else {
                failed
                    .strip_prefix(cwd)
                    .unwrap_or(failed)
                    .to_string_lossy()
                    .into_owned()
            };
            Failure::System {
                error,
                syscall,
                path: Some(shown),
            }
        }
        failure => failure,
    }
}

fn entry(e: &mut Enc, path: &str, st: &Metadata) {
    let kind = if st.file_type().is_symlink() {
        2
    } else {
        u64::from(st.is_dir())
    };
    e.str(path)
        .leb(kind)
        .leb(if st.is_dir() { 0 } else { st.len() });
}

fn error(path: &str, variant: u64) -> Enc {
    let mut e = Enc::default();
    e.leb(1).leb(variant).str(path);
    e
}

fn failure(path: &str, method: &str, from: &str, failure: &Failure) -> Enc {
    let (err, syscall, error_path) = match failure {
        Failure::Deadline => unreachable!("deadline is a host stop, not an Fs error"),
        Failure::Message(message) => return error(message, 7),
        Failure::System {
            error,
            syscall,
            path,
        } => (error, *syscall, path),
    };
    let translated = node_errno(err, syscall);
    let variant = match translated.map(|(code, _)| code) {
        Some("ENOENT") => 0,
        Some("EACCES" | "EPERM") => 1,
        Some("EEXIST") => 3,
        Some("ENOTDIR") => 4,
        Some("EISDIR") => 5,
        Some(_) => 7,
        None => match err.kind() {
            io::ErrorKind::NotFound => 0,
            io::ErrorKind::PermissionDenied => 1,
            io::ErrorKind::AlreadyExists => 3,
            io::ErrorKind::NotADirectory => 4,
            io::ErrorKind::IsADirectory => 5,
            _ => 7,
        },
    };
    if variant != 7 {
        return error(path, variant);
    }
    // libuv's descriptions deliberately differ from strerror (for example
    // ELOOP and EXDEV). Raw errno retains distinctions Rust ErrorKind merges.
    let (code, message) = translated.unwrap_or(("UNKNOWN", "unknown error"));
    let path = error_path.as_deref().unwrap_or(path);
    let paths = if method == "rename" {
        format!("'{from}' -> '{path}'")
    } else {
        format!("'{path}'")
    };
    // Node descriptor-operation errors do not include a path, unlike open.
    let message = if matches!(syscall, "read" | "write" | "fstat") {
        format!("{code}: {message}, {syscall}")
    } else {
        format!("{code}: {message}, {syscall} {paths}")
    };
    error(&message, 7)
}

fn node_errno(err: &io::Error, syscall: &str) -> Option<(&'static str, &'static str)> {
    #[cfg(windows)]
    if let Some(code) = err.raw_os_error().and_then(|code| u32::try_from(code).ok()) {
        return Some(node_windows_errno(code, syscall));
    }
    #[cfg(not(windows))]
    let _ = syscall;
    #[cfg(unix)]
    if let Some(code) = err.raw_os_error() {
        // POSIX errno numbers vary between macOS and Linux. libc supplies
        // the target constants; this table uses libuv's stable spellings.
        let errors = [
            (libc::E2BIG, "E2BIG", "argument list too long"),
            (libc::EADDRINUSE, "EADDRINUSE", "address already in use"),
            (
                libc::EADDRNOTAVAIL,
                "EADDRNOTAVAIL",
                "address not available",
            ),
            (
                libc::EAFNOSUPPORT,
                "EAFNOSUPPORT",
                "address family not supported",
            ),
            (libc::EAGAIN, "EAGAIN", "resource temporarily unavailable"),
            (libc::EALREADY, "EALREADY", "connection already in progress"),
            (libc::EBADF, "EBADF", "bad file descriptor"),
            (libc::EBUSY, "EBUSY", "resource busy or locked"),
            (libc::ECANCELED, "ECANCELED", "operation canceled"),
            (
                libc::ECONNABORTED,
                "ECONNABORTED",
                "software caused connection abort",
            ),
            (libc::ECONNREFUSED, "ECONNREFUSED", "connection refused"),
            (libc::ECONNRESET, "ECONNRESET", "connection reset by peer"),
            (
                libc::EDESTADDRREQ,
                "EDESTADDRREQ",
                "destination address required",
            ),
            (
                libc::EFAULT,
                "EFAULT",
                "bad address in system call argument",
            ),
            (libc::EFBIG, "EFBIG", "file too large"),
            (libc::EHOSTDOWN, "EHOSTDOWN", "host is down"),
            (libc::EHOSTUNREACH, "EHOSTUNREACH", "host is unreachable"),
            (libc::EINTR, "EINTR", "interrupted system call"),
            (libc::EINVAL, "EINVAL", "invalid argument"),
            (libc::EIO, "EIO", "i/o error"),
            (libc::EISCONN, "EISCONN", "socket is already connected"),
            (libc::ELOOP, "ELOOP", "too many symbolic links encountered"),
            (libc::EMFILE, "EMFILE", "too many open files"),
            (libc::EMSGSIZE, "EMSGSIZE", "message too long"),
            (libc::ENAMETOOLONG, "ENAMETOOLONG", "name too long"),
            (libc::ENFILE, "ENFILE", "file table overflow"),
            (libc::ENETDOWN, "ENETDOWN", "network is down"),
            (libc::ENETUNREACH, "ENETUNREACH", "network is unreachable"),
            (libc::ENOBUFS, "ENOBUFS", "no buffer space available"),
            (libc::ENODATA, "ENODATA", "no data available"),
            (libc::ENODEV, "ENODEV", "no such device"),
            (libc::ENOMEM, "ENOMEM", "not enough memory"),
            (libc::ENOPROTOOPT, "ENOPROTOOPT", "protocol not available"),
            (libc::ENOSPC, "ENOSPC", "no space left on device"),
            (libc::ENOSYS, "ENOSYS", "function not implemented"),
            (libc::ENOTCONN, "ENOTCONN", "socket is not connected"),
            (libc::ENOTEMPTY, "ENOTEMPTY", "directory not empty"),
            (libc::ENOTSOCK, "ENOTSOCK", "socket operation on non-socket"),
            (
                libc::ENOTSUP,
                "ENOTSUP",
                "operation not supported on socket",
            ),
            (
                libc::EOVERFLOW,
                "EOVERFLOW",
                "value too large for defined data type",
            ),
            (libc::EPIPE, "EPIPE", "broken pipe"),
            (libc::EPROTO, "EPROTO", "protocol error"),
            (
                libc::EPROTONOSUPPORT,
                "EPROTONOSUPPORT",
                "protocol not supported",
            ),
            (
                libc::EPROTOTYPE,
                "EPROTOTYPE",
                "protocol wrong type for socket",
            ),
            (libc::ERANGE, "ERANGE", "result too large"),
            (libc::EROFS, "EROFS", "read-only file system"),
            (
                libc::ESHUTDOWN,
                "ESHUTDOWN",
                "cannot send after transport endpoint shutdown",
            ),
            (
                libc::ESOCKTNOSUPPORT,
                "ESOCKTNOSUPPORT",
                "socket type not supported",
            ),
            (libc::ESPIPE, "ESPIPE", "invalid seek"),
            (libc::ESRCH, "ESRCH", "no such process"),
            (libc::ETIMEDOUT, "ETIMEDOUT", "connection timed out"),
            (libc::ETXTBSY, "ETXTBSY", "text file is busy"),
            (libc::EXDEV, "EXDEV", "cross-device link not permitted"),
            (libc::ENXIO, "ENXIO", "no such device or address"),
            (libc::EMLINK, "EMLINK", "too many links"),
            (libc::ENOTTY, "ENOTTY", "inappropriate ioctl for device"),
            (libc::EILSEQ, "EILSEQ", "illegal byte sequence"),
            (libc::ENOEXEC, "ENOEXEC", "exec format error"),
        ];
        if let Some((_, name, message)) = errors.iter().find(|(errno, _, _)| *errno == code) {
            return Some((name, message));
        }
        #[cfg(any(
            target_os = "macos",
            target_os = "freebsd",
            target_os = "openbsd",
            target_os = "netbsd",
            target_os = "dragonfly"
        ))]
        if code == libc::EFTYPE {
            return Some(("EFTYPE", "inappropriate file type or format"));
        }
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            if code == libc::EREMOTEIO {
                return Some(("EREMOTEIO", "remote I/O error"));
            }
            if code == libc::ENONET {
                return Some(("ENONET", "machine is not on the network"));
            }
            if code == libc::EUNATCH {
                return Some(("EUNATCH", "protocol driver not attached"));
            }
        }
    }
    // Also cover errors produced by std itself, without raw errno.
    match err.kind() {
        io::ErrorKind::DirectoryNotEmpty => Some(("ENOTEMPTY", "directory not empty")),
        io::ErrorKind::InvalidInput => Some(("EINVAL", "invalid argument")),
        io::ErrorKind::StorageFull => Some(("ENOSPC", "no space left on device")),
        io::ErrorKind::ReadOnlyFilesystem => Some(("EROFS", "read-only file system")),
        _ => None,
    }
}

#[cfg(windows)]
fn node_windows_errno(code: u32, syscall: &str) -> (&'static str, &'static str) {
    use windows_sys::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_BAD_EXE_FORMAT, ERROR_BAD_PATHNAME,
        ERROR_BAD_PIPE, ERROR_BEGINNING_OF_MEDIA, ERROR_BROKEN_PIPE, ERROR_BUFFER_OVERFLOW,
        ERROR_BUS_RESET, ERROR_CANNOT_MAKE, ERROR_CANT_ACCESS_FILE, ERROR_CANT_RESOLVE_FILENAME,
        ERROR_CONNECTION_ABORTED, ERROR_CONNECTION_REFUSED, ERROR_CRC, ERROR_DEVICE_DOOR_OPEN,
        ERROR_DEVICE_REQUIRES_CLEANING, ERROR_DIR_NOT_EMPTY, ERROR_DIRECTORY, ERROR_DISK_CORRUPT,
        ERROR_DISK_FULL, ERROR_EA_TABLE_FULL, ERROR_ELEVATION_REQUIRED, ERROR_END_OF_MEDIA,
        ERROR_ENVVAR_NOT_FOUND, ERROR_EOM_OVERFLOW, ERROR_FILE_EXISTS, ERROR_FILE_NOT_FOUND,
        ERROR_FILEMARK_DETECTED, ERROR_FILENAME_EXCED_RANGE, ERROR_GEN_FAILURE,
        ERROR_HANDLE_DISK_FULL, ERROR_HOST_UNREACHABLE, ERROR_INSUFFICIENT_BUFFER,
        ERROR_INVALID_BLOCK_LENGTH, ERROR_INVALID_DATA, ERROR_INVALID_DRIVE, ERROR_INVALID_FLAGS,
        ERROR_INVALID_FUNCTION, ERROR_INVALID_HANDLE, ERROR_INVALID_NAME, ERROR_INVALID_PARAMETER,
        ERROR_INVALID_REPARSE_DATA, ERROR_IO_DEVICE, ERROR_LOCK_VIOLATION,
        ERROR_META_EXPANSION_TOO_LONG, ERROR_MOD_NOT_FOUND, ERROR_NETNAME_DELETED,
        ERROR_NETWORK_UNREACHABLE, ERROR_NO_DATA, ERROR_NO_DATA_DETECTED, ERROR_NO_SIGNAL_SENT,
        ERROR_NO_UNICODE_TRANSLATION, ERROR_NOACCESS, ERROR_NOT_CONNECTED, ERROR_NOT_ENOUGH_MEMORY,
        ERROR_NOT_SAME_DEVICE, ERROR_NOT_SUPPORTED, ERROR_OPEN_FAILED, ERROR_OPERATION_ABORTED,
        ERROR_OUTOFMEMORY, ERROR_PATH_NOT_FOUND, ERROR_PIPE_BUSY, ERROR_PIPE_NOT_CONNECTED,
        ERROR_PRIVILEGE_NOT_HELD, ERROR_SEM_TIMEOUT, ERROR_SETMARK_DETECTED,
        ERROR_SHARING_VIOLATION, ERROR_SIGNAL_REFUSED, ERROR_SYMLINK_NOT_SUPPORTED,
        ERROR_TOO_MANY_OPEN_FILES, ERROR_WRITE_PROTECT,
    };
    // libuv's filesystem-specific overrides in src/win/fs.c take precedence
    // over uv_translate_sys_error in src/win/error.c.
    if syscall == "mkdir" && matches!(code, ERROR_INVALID_NAME | ERROR_DIRECTORY) {
        return ("EINVAL", "invalid argument");
    }
    if syscall == "open" && code == ERROR_FILE_EXISTS {
        return ("EISDIR", "illegal operation on a directory");
    }
    if syscall == "write" && matches!(code, ERROR_BROKEN_PIPE | ERROR_NO_DATA) {
        return ("EPIPE", "broken pipe");
    }
    match code {
        ERROR_ELEVATION_REQUIRED | ERROR_CANT_ACCESS_FILE => ("EACCES", "permission denied"),
        ERROR_ACCESS_DENIED | ERROR_PRIVILEGE_NOT_HELD => ("EPERM", "operation not permitted"),
        ERROR_ALREADY_EXISTS | ERROR_FILE_EXISTS => ("EEXIST", "file already exists"),
        ERROR_BAD_PATHNAME
        | ERROR_DIRECTORY
        | ERROR_ENVVAR_NOT_FOUND
        | ERROR_FILE_NOT_FOUND
        | ERROR_INVALID_NAME
        | ERROR_INVALID_DRIVE
        | ERROR_INVALID_REPARSE_DATA
        | ERROR_MOD_NOT_FOUND
        | ERROR_PATH_NOT_FOUND => ("ENOENT", "no such file or directory"),
        ERROR_NO_DATA => ("EAGAIN", "resource temporarily unavailable"),
        ERROR_INVALID_FLAGS | ERROR_INVALID_HANDLE => ("EBADF", "bad file descriptor"),
        ERROR_LOCK_VIOLATION | ERROR_PIPE_BUSY | ERROR_SHARING_VIOLATION => {
            ("EBUSY", "resource busy or locked")
        }
        ERROR_OPERATION_ABORTED => ("ECANCELED", "operation canceled"),
        ERROR_NO_UNICODE_TRANSLATION => ("ECHARSET", "invalid Unicode character"),
        ERROR_CONNECTION_ABORTED => ("ECONNABORTED", "software caused connection abort"),
        ERROR_CONNECTION_REFUSED => ("ECONNREFUSED", "connection refused"),
        ERROR_NETNAME_DELETED => ("ECONNRESET", "connection reset by peer"),
        ERROR_HOST_UNREACHABLE => ("EHOSTUNREACH", "host is unreachable"),
        ERROR_NETWORK_UNREACHABLE => ("ENETUNREACH", "network is unreachable"),
        ERROR_NOT_CONNECTED => ("ENOTCONN", "socket is not connected"),
        ERROR_NOACCESS => ("EFAULT", "bad address in system call argument"),
        ERROR_INSUFFICIENT_BUFFER
        | ERROR_INVALID_DATA
        | ERROR_INVALID_PARAMETER
        | ERROR_SYMLINK_NOT_SUPPORTED => ("EINVAL", "invalid argument"),
        ERROR_BEGINNING_OF_MEDIA
        | ERROR_BUS_RESET
        | ERROR_CRC
        | ERROR_DEVICE_DOOR_OPEN
        | ERROR_DEVICE_REQUIRES_CLEANING
        | ERROR_DISK_CORRUPT
        | ERROR_EOM_OVERFLOW
        | ERROR_FILEMARK_DETECTED
        | ERROR_GEN_FAILURE
        | ERROR_INVALID_BLOCK_LENGTH
        | ERROR_IO_DEVICE
        | ERROR_NO_DATA_DETECTED
        | ERROR_NO_SIGNAL_SENT
        | ERROR_OPEN_FAILED
        | ERROR_SETMARK_DETECTED
        | ERROR_SIGNAL_REFUSED => ("EIO", "i/o error"),
        ERROR_CANT_RESOLVE_FILENAME => ("ELOOP", "too many symbolic links encountered"),
        ERROR_TOO_MANY_OPEN_FILES => ("EMFILE", "too many open files"),
        ERROR_BUFFER_OVERFLOW | ERROR_FILENAME_EXCED_RANGE => ("ENAMETOOLONG", "name too long"),
        ERROR_NOT_ENOUGH_MEMORY | ERROR_OUTOFMEMORY => ("ENOMEM", "not enough memory"),
        ERROR_CANNOT_MAKE
        | ERROR_DISK_FULL
        | ERROR_EA_TABLE_FULL
        | ERROR_END_OF_MEDIA
        | ERROR_HANDLE_DISK_FULL => ("ENOSPC", "no space left on device"),
        ERROR_DIR_NOT_EMPTY => ("ENOTEMPTY", "directory not empty"),
        ERROR_NOT_SUPPORTED => ("ENOTSUP", "operation not supported on socket"),
        ERROR_BROKEN_PIPE => ("EOF", "end of file"),
        ERROR_BAD_PIPE | ERROR_PIPE_NOT_CONNECTED => ("EPIPE", "broken pipe"),
        ERROR_WRITE_PROTECT => ("EROFS", "read-only file system"),
        ERROR_SEM_TIMEOUT => ("ETIMEDOUT", "connection timed out"),
        ERROR_NOT_SAME_DEVICE => ("EXDEV", "cross-device link not permitted"),
        ERROR_INVALID_FUNCTION => ("EISDIR", "illegal operation on a directory"),
        ERROR_META_EXPANSION_TOO_LONG => ("E2BIG", "argument list too long"),
        ERROR_BAD_EXE_FORMAT => ("EFTYPE", "inappropriate file type or format"),
        _ => ("UNKNOWN", "unknown error"),
    }
}

// The string inspection used by Node's ERR_INVALID_ARG_VALUE: choose an
// unescaped quote when possible, escape controls, then limit to 128 UTF-16
// code units. This is only used for invalid paths containing NUL.
pub(crate) fn inspect_string(path: &str) -> String {
    let quote = if !path.contains('\'') {
        '\''
    } else if !path.contains('"') {
        '"'
    } else if !path.contains('`') && !path.contains("${") {
        '`'
    } else {
        '\''
    };
    let mut out = String::new();
    out.push(quote);
    for c in path.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if c <= '\u{1f}' || ('\u{7f}'..='\u{9f}').contains(&c) => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\x{:02X}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    if out.encode_utf16().count() > 128 {
        let units = out.encode_utf16().take(128).collect::<Vec<_>>();
        out = String::from_utf16_lossy(&units);
        out.push_str("...");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Failure, call, call_until, call_until_at, error, inspect_string};
    use crate::codec::Dec;
    use crate::codec::Enc;
    use hd_run::{Grants, Limit};
    use std::fs;
    #[cfg(unix)]
    use std::io::{Read as _, Write as _};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "hd-fs-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("create test directory");
            Self(path)
        }

        fn path(&self, name: &str) -> String {
            self.0.join(name).to_string_lossy().into_owned()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn concurrent_case_directories_do_not_change_process_cwd() {
        let before = std::env::current_dir().unwrap();
        let left = TempDir::new();
        let right = TempDir::new();
        std::thread::scope(|scope| {
            for (dir, payload) in [(&left, b"left".as_slice()), (&right, b"right".as_slice())] {
                scope.spawn(move || {
                    let mut args = Enc::default();
                    args.str("value").bytes(payload);
                    let result = call_until_at(
                        "FsWrite",
                        "write_bytes",
                        args.done(),
                        &Grants::default(),
                        None,
                        &dir.0,
                    )
                    .unwrap();
                    assert_eq!(result.done(), &[0]);
                    let mut args = Enc::default();
                    args.str("value");
                    let result = call_until_at(
                        "FsRead",
                        "read_bytes",
                        args.done(),
                        &Grants::default(),
                        None,
                        &dir.0,
                    )
                    .unwrap();
                    let mut dec = Dec::new(result.done());
                    assert_eq!(dec.leb().unwrap(), 0);
                    assert_eq!(dec.bytes().unwrap(), payload);
                });
            }
        });
        assert_eq!(std::env::current_dir().unwrap(), before);
        assert_eq!(fs::read(left.0.join("value")).unwrap(), b"left");
        assert_eq!(fs::read(right.0.join("value")).unwrap(), b"right");
    }

    #[test]
    fn case_directory_results_keep_guest_paths_and_grant_resolution() {
        let dir = TempDir::new();
        fs::write(dir.0.join("item"), b"x").unwrap();
        let invoke = |key: &str, method: &str, path: &str, grants: &Grants| {
            let mut args = Enc::default();
            args.str(path);
            call_until_at(key, method, args.done(), grants, None, &dir.0).unwrap()
        };
        let result = invoke("FsRead", "list_dir", ".", &Grants::default());
        let mut dec = Dec::new(result.done());
        assert_eq!(dec.leb().unwrap(), 0);
        assert_eq!(dec.leb().unwrap(), 1);
        assert_eq!(dec.str().unwrap(), "./item");
        assert_eq!(dec.leb().unwrap(), 0);
        assert_eq!(dec.leb().unwrap(), 1);
        assert_eq!(
            invoke("FsRead", "read_text", "absent", &Grants::default()).done(),
            error("absent", 0).done()
        );
        let grants = Grants {
            limits: vec![(
                "FsRead".into(),
                Limit::Entries(vec![
                    hd_run::resolve_path(&dir.0.join("item"))
                        .to_string_lossy()
                        .into_owned(),
                ]),
            )],
        };
        assert_eq!(
            invoke("FsRead", "read_text", "item", &grants).done(),
            Enc::default().leb(0).str("x").done()
        );
        assert_eq!(
            invoke("FsRead", "read_text", "../outside", &grants).done(),
            error("../outside", 2).done()
        );
        assert_eq!(
            invoke("FsWrite", "create_dir_all", "", &Grants::default()).done(),
            error("", 0).done()
        );
    }

    fn invoke(method: &str, path: &str, payload: Option<&[u8]>) -> Enc {
        let mut args = Enc::default();
        args.str(path);
        if let Some(bytes) = payload {
            args.bytes(bytes);
        }
        call(
            if super::READ.contains(&method) {
                "FsRead"
            } else {
                "FsWrite"
            },
            method,
            args.done(),
            &Grants::default(),
        )
        .expect("host call")
    }

    fn limited(
        method: &str,
        path: &str,
        payload: Option<&[u8]>,
        deadline: Instant,
    ) -> wasmtime::Result<Enc> {
        let mut args = Enc::default();
        args.str(path);
        if let Some(bytes) = payload {
            args.bytes(bytes);
        }
        call_until(
            if super::READ.contains(&method) {
                "FsRead"
            } else {
                "FsWrite"
            },
            method,
            args.done(),
            &Grants::default(),
            Some(deadline),
        )
    }

    fn assert_time_limit(result: wasmtime::Result<Enc>) {
        let Err(err) = result else {
            panic!("deadline should stop the host call");
        };
        assert!(
            matches!(
                err.downcast_ref::<crate::host::Stop>(),
                Some(crate::host::Stop::TimeLimit)
            ),
            "{err}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn named_pipe_namespace_detection_preserves_ordinary_files() {
        for path in [
            r"\\.\pipe\hd-test",
            r"\\?\PIPE\hd-test",
            "//./pipe/hd-test",
            r"\\localhost\pipe\hd-test",
            r"\\?\UNC\host\pipe\hd-test",
            r"\\?\GLOBALROOT\Device\NamedPipe\hd-test",
        ] {
            assert!(super::named_pipe_path(path), "{path}");
        }
        for path in [
            r"C:\pipe\hd-test",
            r"\\host\share\pipe\hd-test",
            r"\\host\pipeline\hd-test",
            r"\\?\C:\pipe\hd-test",
            "pipe/hd-test",
        ] {
            assert!(!super::named_pipe_path(path), "{path}");
        }
    }

    #[cfg(windows)]
    fn pipe_name(dir: &TempDir) -> String {
        format!(r"\\.\pipe\{}", dir.0.file_name().unwrap().to_string_lossy())
    }

    #[cfg(windows)]
    #[test]
    fn windows_pipe_idle_read_and_backpressure_stop_at_deadline() {
        use interprocess::os::windows::named_pipe::{PipeListenerOptions, pipe_mode};

        for (method, payload) in [
            ("read_bytes", None),
            ("write_bytes", Some(vec![b'x'; 4 * 1024 * 1024])),
        ] {
            let dir = TempDir::new();
            let path = pipe_name(&dir);
            let _listener = PipeListenerOptions::new()
                .path(path.as_str())
                .nonblocking(true)
                .input_buffer_size_hint(64 * 1024)
                .output_buffer_size_hint(64 * 1024)
                .create_duplex::<pipe_mode::Bytes>()
                .unwrap();
            let start = Instant::now();
            assert_time_limit(limited(
                method,
                &path,
                payload.as_deref(),
                start + Duration::from_millis(60),
            ));
            assert!(start.elapsed() < Duration::from_secs(2));
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_pipe_busy_open_is_deadline_bounded() {
        use interprocess::os::windows::named_pipe::{PipeListenerOptions, pipe_mode};

        let dir = TempDir::new();
        let path = pipe_name(&dir);
        let _listener = PipeListenerOptions::new()
            .path(path.as_str())
            .nonblocking(true)
            .instance_limit(std::num::NonZeroU8::new(1))
            .create_duplex::<pipe_mode::Bytes>()
            .unwrap();
        let _peer = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let start = Instant::now();
        assert_time_limit(limited(
            "read_text",
            &path,
            None,
            start + Duration::from_millis(60),
        ));
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn expired_deadline_prevents_filesystem_mutation_and_is_a_host_stop() {
        let dir = TempDir::new();
        let path = dir.path("keep");
        fs::write(&path, b"original").unwrap();
        let expired = Instant::now();
        assert_time_limit(limited("write_bytes", &path, Some(b"replacement"), expired));
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert_time_limit(limited(
            "create_dir_all",
            &dir.path("new/a/b"),
            None,
            expired,
        ));
        assert!(!std::path::Path::new(&dir.path("new")).exists());
        assert_time_limit(limited("remove", &path, None, expired));
        assert_eq!(fs::read(&path).unwrap(), b"original");
    }

    #[cfg(unix)]
    fn fifo(dir: &TempDir) -> String {
        let path = dir.path("fifo");
        nix::unistd::mkfifo(
            std::path::Path::new(&path),
            nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
        )
        .unwrap();
        path
    }

    #[cfg(unix)]
    fn fifo_reader(path: &str) -> fs::File {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
            .unwrap()
    }

    #[cfg(unix)]
    fn fifo_peer_writer(path: &str, deadline: Instant) -> std::io::Result<fs::File> {
        use std::os::unix::fs::OpenOptionsExt;
        loop {
            if Instant::now() >= deadline {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            match fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(path)
            {
                Err(error)
                    if error.raw_os_error() == Some(libc::ENXIO)
                        || error.kind() == std::io::ErrorKind::Interrupted =>
                {
                    std::thread::sleep(Duration::from_millis(5));
                }
                result => return result,
            }
        }
    }

    #[cfg(unix)]
    fn fifo_peer_write(path: &str, mut bytes: &[u8], deadline: Instant) -> std::io::Result<()> {
        let mut writer = fifo_peer_writer(path, deadline)?;
        while !bytes.is_empty() {
            if Instant::now() >= deadline {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            match writer.write(bytes) {
                Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
                Ok(n) => bytes = &bytes[n..],
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    #[cfg(unix)]
    fn fifo_peer_read(
        path: &str,
        reader_ready: &std::sync::mpsc::Sender<()>,
        writer_done: &std::sync::mpsc::Receiver<()>,
        deadline: Instant,
    ) -> std::io::Result<Vec<u8>> {
        let mut reader = fifo_reader(path);
        reader_ready
            .send(())
            .map_err(|_| std::io::ErrorKind::BrokenPipe)?;
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 8 * 1024];
        let mut finished = false;
        loop {
            if Instant::now() >= deadline {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            finished |= writer_done.try_recv().is_ok();
            match reader.read(&mut buffer) {
                Ok(0) if finished => return Ok(bytes),
                // No writer yet is not EOF: the main thread signals only
                // after its write call has returned and closed the writer.
                Ok(0) => std::thread::sleep(Duration::from_millis(5)),
                Ok(n) => bytes.extend_from_slice(&buffer[..n]),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error),
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn fifo_without_peer_times_out_in_read_and_write_open() {
        let dir = TempDir::new();
        let path = fifo(&dir);
        for (method, payload) in [("read_bytes", None), ("append_text", Some(&b"hello"[..]))] {
            let start = Instant::now();
            assert_time_limit(limited(
                method,
                &path,
                payload,
                start + Duration::from_millis(60),
            ));
            assert!(start.elapsed() < Duration::from_secs(2));
        }
    }

    #[cfg(unix)]
    #[test]
    fn fifo_connected_but_idle_writer_does_not_block_past_deadline() {
        use std::os::unix::fs::OpenOptionsExt;
        let dir = TempDir::new();
        let path = fifo(&dir);
        let _reader = fifo_reader(&path);
        let _writer = fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&path)
            .unwrap();
        let start = Instant::now();
        assert_time_limit(limited(
            "read_text",
            &path,
            None,
            start + Duration::from_millis(60),
        ));
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn fifo_backpressure_times_out_and_closes_writer_without_background_mutation() {
        let dir = TempDir::new();
        let path = fifo(&dir);
        let mut reader = fifo_reader(&path);
        let payload = vec![b'x'; 4 * 1024 * 1024];
        let start = Instant::now();
        assert_time_limit(limited(
            "write_bytes",
            &path,
            Some(&payload),
            start + Duration::from_millis(60),
        ));
        assert!(start.elapsed() < Duration::from_secs(2));
        let mut buffer = [0u8; 8 * 1024];
        let mut written = 0;
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break, // The operation closed its only writer on timeout.
                Ok(n) => written += n,
                Err(err) => panic!("reader should reach EOF, not retain a writer: {err}"),
            }
        }
        assert!(written > 0 && written < payload.len());
    }

    #[cfg(unix)]
    #[test]
    fn fifo_delayed_writer_and_ready_reader_preserve_successful_transfers() {
        let dir = TempDir::new();
        let path = fifo(&dir);
        let writer_path = path.clone();
        let (sent, written) = std::sync::mpsc::channel();
        let _writer = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            std::thread::sleep(Duration::from_millis(20));
            let result = fifo_peer_write(&writer_path, b"hello", deadline);
            let _ = sent.send(result);
        });
        let mut expected = Enc::default();
        expected.leb(0).bytes(b"hello");
        let actual = limited(
            "read_bytes",
            &path,
            None,
            Instant::now() + Duration::from_secs(2),
        );
        written
            .recv_timeout(Duration::from_secs(3))
            .expect("delayed writer did not finish")
            .expect("delayed writer failed");
        assert_eq!(actual.unwrap().done(), expected.done());

        let reader_path = path.clone();
        let (reader_ready, ready) = std::sync::mpsc::channel();
        let (writer_done, finished) = std::sync::mpsc::channel();
        let (received, read) = std::sync::mpsc::channel();
        let _reader = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            std::thread::sleep(Duration::from_millis(20));
            let result = fifo_peer_read(&reader_path, &reader_ready, &finished, deadline);
            let _ = received.send(result);
        });
        // This direction tests a connected append, not an overlapping peer
        // open. Keep that lifecycle explicit before the short-lived writer.
        ready
            .recv_timeout(Duration::from_secs(3))
            .expect("reader did not become ready");
        let actual = limited(
            "append_text",
            &path,
            Some(b"world"),
            Instant::now() + Duration::from_secs(2),
        );
        writer_done.send(()).expect("delayed reader stopped early");
        let bytes = read
            .recv_timeout(Duration::from_secs(3))
            .expect("delayed reader did not finish")
            .expect("delayed reader failed");
        assert_eq!(actual.unwrap().done(), &[0]);
        assert_eq!(bytes, b"world");
    }

    #[cfg(unix)]
    #[test]
    fn fifo_empty_writer_is_eof_not_no_peer_timeout() {
        let dir = TempDir::new();
        let path = fifo(&dir);
        let writer_path = path.clone();
        let (sent, closed) = std::sync::mpsc::channel();
        let _writer = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            std::thread::sleep(Duration::from_millis(20));
            let result = fifo_peer_writer(&writer_path, deadline).map(drop);
            let _ = sent.send(result);
        });
        let actual = limited(
            "read_bytes",
            &path,
            None,
            Instant::now() + Duration::from_secs(2),
        );
        closed
            .recv_timeout(Duration::from_secs(3))
            .expect("empty writer did not finish")
            .expect("empty writer failed");
        assert_eq!(actual.unwrap().done(), &[0, 0]);
    }

    #[test]
    fn reads_text_fatally_and_removes_only_the_first_bom() {
        let dir = TempDir::new();
        let path = dir.path("text");
        fs::write(&path, "\u{feff}\u{feff}hé").unwrap();
        let actual = invoke("read_text", &path, None);
        let mut expected = Enc::default();
        expected.leb(0).str("\u{feff}hé");
        assert_eq!(actual.done(), expected.done());
        for bytes in [&[0xc0, 0xaf][..], &[0xed, 0xa0, 0x80], &[0xe2, 0x82]] {
            fs::write(&path, bytes).unwrap();
            assert_eq!(
                invoke("read_text", &path, None).done(),
                error(&path, 6).done()
            );
            let mut expected = Enc::default();
            expected.leb(0).bytes(bytes);
            assert_eq!(invoke("read_bytes", &path, None).done(), expected.done());
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_oversized_regular_files_before_reading_them() {
        let dir = TempDir::new();
        let path = dir.path("sparse");
        fs::File::create(&path)
            .unwrap()
            .set_len(0x8000_0000)
            .unwrap();
        let expected = error("File size (2147483648) is greater than 2 GiB", 7);
        for method in ["read_text", "read_bytes"] {
            assert_eq!(invoke(method, &path, None).done(), expected.done());
        }
    }

    #[test]
    fn recursive_mkdir_and_stat_have_node_edge_behavior() {
        let dir = TempDir::new();
        let path = dir.path("a/b/c");
        assert_eq!(invoke("create_dir_all", &path, None).done(), &[0]);
        assert_eq!(invoke("create_dir_all", &path, None).done(), &[0]);
        assert_eq!(
            invoke("create_dir_all", "", None).done(),
            error("", 0).done()
        );
        let file = dir.path("file");
        fs::write(&file, "hello").unwrap();
        assert_eq!(
            invoke("create_dir_all", &file, None).done(),
            error(&file, 4).done()
        );
        assert_eq!(
            invoke("stat", &format!("{file}/child"), None).done(),
            &[0, 0]
        );
        assert_eq!(invoke("stat", &dir.path("absent"), None).done(), &[0, 0]);
    }

    #[test]
    fn grants_precede_validation_and_payload_and_cover_both_rename_paths() {
        let denied = Grants {
            limits: vec![("FsWrite".into(), Limit::Deny)],
        };
        let mut args = Enc::default();
        args.str("bad\0path"); // No payload: the refusal must come first.
        assert_eq!(
            call("FsWrite", "write_text", args.done(), &denied)
                .unwrap()
                .done(),
            error("bad\0path", 2).done()
        );
        let dir = TempDir::new();
        let allowed = dir.path("from");
        let refused = format!("{allowed}-outside");
        fs::write(&allowed, "keep").unwrap();
        let grants = Grants {
            limits: vec![(
                "FsWrite".into(),
                Limit::Entries(vec![
                    fs::canonicalize(&allowed)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                ]),
            )],
        };
        for (from, to, rejected) in [
            (&allowed, &refused, &refused),
            (&refused, &allowed, &refused),
        ] {
            let mut args = Enc::default();
            args.str(from).str(to);
            assert_eq!(
                call("FsWrite", "rename", args.done(), &grants)
                    .unwrap()
                    .done(),
                error(rejected, 2).done()
            );
        }
        assert_eq!(fs::read(&allowed).unwrap(), b"keep");
    }

    #[test]
    fn nul_path_errors_keep_nodes_argument_names_and_inspection() {
        assert_eq!(inspect_string("a\0\n\u{1b}"), "'a\\x00\\n\\x1B'");
        assert_eq!(inspect_string("a\0'"), "\"a\\x00'\"");
        assert_eq!(inspect_string("a\0'\""), "`a\\x00'\"`");
        assert_eq!(inspect_string("a\0'\"`"), "'a\\x00\\'\"`'");
        assert_eq!(inspect_string(&format!("\0{}", "a".repeat(200))).len(), 131);
        for (from, to, argument) in [("a\0b", "to", "oldPath"), ("from", "a\0b", "newPath")] {
            let mut args = Enc::default();
            args.str(from).str(to);
            let expected = error(
                &format!(
                    "The argument '{argument}' must be a string, Uint8Array, or URL without null bytes. Received 'a\\x00b'"
                ),
                7,
            );
            assert_eq!(
                call("FsWrite", "rename", args.done(), &Grants::default())
                    .unwrap()
                    .done(),
                expected.done()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn lists_byte_order_and_reports_symlink_and_directory_sizes() {
        use std::os::unix::fs::symlink;
        let dir = TempDir::new();
        fs::write(dir.path("é"), "hello").unwrap();
        fs::write(dir.path("z"), "z").unwrap();
        fs::create_dir(dir.path("a")).unwrap();
        symlink("é", dir.path("link")).unwrap();
        let path = dir.0.to_string_lossy();
        let actual = invoke("list_dir", &path, None);
        let mut d = Dec::new(actual.done());
        assert_eq!(d.leb().unwrap(), 0);
        assert_eq!(d.leb().unwrap(), 4);
        for (name, kind, size) in [("a", 1, 0), ("link", 2, 2), ("z", 0, 1), ("é", 0, 5)] {
            assert_eq!(d.str().unwrap(), format!("{path}/{name}"));
            assert_eq!(d.leb().unwrap(), kind);
            assert_eq!(d.leb().unwrap(), size);
        }
        let link = dir.path("link");
        let mut expected = Enc::default();
        expected.leb(0).leb(1).str(&link).leb(2).leb(2);
        assert_eq!(invoke("stat", &link, None).done(), expected.done());
        assert_eq!(invoke("remove", &link, None).done(), &[0]);
        assert!(fs::metadata(dir.path("é")).is_ok());
        let dangling = dir.path("dangling");
        symlink("missing", &dangling).unwrap();
        assert_eq!(
            invoke("create_dir_all", &dangling, None).done(),
            error(&dangling, 0).done()
        );
    }

    #[cfg(unix)]
    #[test]
    fn other_errors_preserve_libuv_descriptions_and_syscalls() {
        let dir = TempDir::new();
        fs::write(dir.path("child"), "x").unwrap();
        let path = dir.0.to_string_lossy();
        assert_eq!(
            invoke("remove", &path, None).done(),
            error(
                &format!("ENOTEMPTY: directory not empty, rmdir '{path}'"),
                7
            )
            .done()
        );
        for (code, description, syscall) in [
            (
                libc::ELOOP,
                "ELOOP: too many symbolic links encountered",
                "open",
            ),
            (libc::ENAMETOOLONG, "ENAMETOOLONG: name too long", "lstat"),
            (
                libc::EXDEV,
                "EXDEV: cross-device link not permitted",
                "rename",
            ),
        ] {
            let failure = Failure::new(syscall, std::io::Error::from_raw_os_error(code));
            let paths = if syscall == "rename" {
                "'from' -> 'to'"
            } else {
                "'to'"
            };
            assert_eq!(
                super::failure("to", syscall, "from", &failure).done(),
                error(&format!("{description}, {syscall} {paths}"), 7).done()
            );
        }
        let failure = Failure::new("write", std::io::Error::from_raw_os_error(libc::ENOSPC));
        assert_eq!(
            super::failure("to", "write_text", "to", &failure).done(),
            error("ENOSPC: no space left on device, write", 7).done()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn write_errors_report_the_write_syscall_without_a_path() {
        assert_eq!(
            invoke("write_text", "/dev/full", Some(b"x")).done(),
            error("ENOSPC: no space left on device, write", 7).done()
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_errors_use_libuv_translation_before_rust_error_kinds() {
        use windows_sys::Win32::Foundation::{
            ERROR_ACCESS_DENIED, ERROR_BROKEN_PIPE, ERROR_DIRECTORY, ERROR_DISK_FULL,
            ERROR_FILENAME_EXCED_RANGE, ERROR_INVALID_NAME, ERROR_NO_DATA, ERROR_NOT_SAME_DEVICE,
            ERROR_SHARING_VIOLATION, ERROR_WRITE_PROTECT,
        };
        for (native, syscall, description) in [
            (
                ERROR_SHARING_VIOLATION,
                "open",
                "EBUSY: resource busy or locked",
            ),
            (
                ERROR_FILENAME_EXCED_RANGE,
                "lstat",
                "ENAMETOOLONG: name too long",
            ),
            (
                ERROR_NOT_SAME_DEVICE,
                "rename",
                "EXDEV: cross-device link not permitted",
            ),
            (ERROR_WRITE_PROTECT, "open", "EROFS: read-only file system"),
            (ERROR_INVALID_NAME, "mkdir", "EINVAL: invalid argument"),
            (ERROR_DIRECTORY, "mkdir", "EINVAL: invalid argument"),
        ] {
            let failure = Failure::new(
                syscall,
                std::io::Error::from_raw_os_error(i32::try_from(native).unwrap()),
            );
            let paths = if syscall == "rename" {
                "'from' -> 'to'"
            } else {
                "'to'"
            };
            assert_eq!(
                super::failure("to", syscall, "from", &failure).done(),
                error(&format!("{description}, {syscall} {paths}"), 7).done()
            );
        }
        for native in [ERROR_BROKEN_PIPE, ERROR_NO_DATA] {
            let failure = Failure::new(
                "write",
                std::io::Error::from_raw_os_error(i32::try_from(native).unwrap()),
            );
            assert_eq!(
                super::failure("to", "write_text", "to", &failure).done(),
                error("EPIPE: broken pipe, write", 7).done()
            );
        }
        let failure = Failure::new(
            "write",
            std::io::Error::from_raw_os_error(i32::try_from(ERROR_DISK_FULL).unwrap()),
        );
        assert_eq!(
            super::failure("to", "write_text", "to", &failure).done(),
            error("ENOSPC: no space left on device, write", 7).done()
        );
        let failure = Failure::new(
            "open",
            std::io::Error::from_raw_os_error(i32::try_from(ERROR_ACCESS_DENIED).unwrap()),
        );
        assert_eq!(
            super::failure("to", "read_text", "to", &failure).done(),
            error("to", 1).done()
        );
    }
}
