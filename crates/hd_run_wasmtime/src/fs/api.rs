use std::fs::Metadata;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use hd_run::Grants;

use crate::codec::{Dec, Enc};

use super::io::{is_special, open, read, write};
use super::ops::{at, run};

pub(crate) const READ: &[&str] = &["read_bytes", "read_text", "list_dir", "stat"];
pub(crate) const WRITE: &[&str] = &[
    "write_bytes",
    "write_text",
    "append_text",
    "create_dir_all",
    "remove",
    "rename",
];

/// The methods that open the path, so that a special file can make them
/// wait. The others never open it, or fail at once on it.
const OPENING: &[&str] = &[
    "read_bytes",
    "read_text",
    "write_bytes",
    "write_text",
    "append_text",
];

/// What `.start` of an `Fs` method does.
pub(crate) enum Start {
    /// The result, at once: a refusal, an error, or a regular file or a
    /// directory, which run inline on the program thread.
    Ready(Enc),
    /// A special file (`std-fs.special-file`): the call waits on the
    /// blocking pool, and `.finish` delivers its result.
    Run(Job),
}

/// A call on a special file, ready to run off the program thread.
pub(crate) struct Job {
    method: String,
    path: String,
    to: Option<String>,
    payload: Option<Vec<u8>>,
    cwd: PathBuf,
}

impl Job {
    /// Runs the call on the blocking pool and gives its encoded result.
    ///
    /// Aborting drops only this future, which discards the result: the
    /// blocking thread cannot be cancelled, so it lingers until its I/O
    /// ends, such as a FIFO read until a writer arrives and closes. This
    /// is within the spec: a special file has no deadline guarantee
    /// beyond "other tasks run" (`std-fs.special-file.deadline`), and a
    /// cancelled write may have happened (`std-fs.cancel.write`).
    pub(crate) async fn run(self) -> Enc {
        match tokio::task::spawn_blocking(move || self.run_blocking()).await {
            Ok(encoded) => encoded,
            Err(error) => error_text(&format!("the file operation failed: {error}")),
        }
    }

    /// The call, run on this thread, which waits for the file.
    pub(crate) fn run_blocking(&self) -> Enc {
        self.execute(None)
            .unwrap_or_else(|| error_text("the file operation ran past a deadline"))
    }

    /// The encoded result, or `None` once the deadline passed.
    fn execute(&self, deadline: Option<Instant>) -> Option<Enc> {
        let mut e = Enc::default();
        let result = run(
            &self.method,
            &self.path,
            self.to.as_deref(),
            self.payload.as_deref(),
            &mut e,
            deadline,
            &self.cwd,
        );
        if check_deadline(deadline).is_err() {
            return None;
        }
        match result {
            Ok(()) => Some(e),
            Err(Failure::Deadline) => None,
            Err(err) => Some(failure(
                self.to.as_deref().unwrap_or(&self.path),
                &self.method,
                &self.path,
                &err,
            )),
        }
    }
}

/// The error a `.start` that cannot offload returns.
pub(crate) fn not_run(error: &str) -> Enc {
    error_text(error)
}

fn error_text(text: &str) -> Enc {
    error(text, 7)
}

/// Executes relative paths in the case's directory without changing the
/// process-wide working directory. Encoded paths and errors retain the
/// guest's original spelling. A regular file or a directory runs here;
/// anything else (a FIFO, a device, a named pipe) comes back as a `Job`.
pub(crate) fn start(
    key: &str,
    method: &str,
    bytes: &[u8],
    grants: &Grants,
    deadline: Option<Instant>,
    cwd: &Path,
) -> wasmtime::Result<Start> {
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
            return Ok(Start::Ready(error(p, 2)));
        }
    }
    // Refusal takes precedence over decoding write payloads.
    let payload = if matches!(method, "write_bytes" | "write_text" | "append_text") {
        Some(d.bytes()?.to_vec())
    } else {
        None
    };
    // Reject NUL before OS access, after checking the grant.
    for (p, name) in std::iter::once((
        &path,
        if method == "rename" {
            "source path"
        } else {
            "path"
        },
    ))
    .chain(to.iter().map(|p| (p, "destination path")))
    {
        if p.contains('\0') {
            return Ok(Start::Ready(error(
                &format!("{name} contains a NUL byte"),
                7,
            )));
        }
    }
    let special = OPENING.contains(&method) && is_special(&at(cwd, &path));
    let job = Job {
        method: method.to_owned(),
        path,
        to,
        payload,
        cwd: cwd.to_owned(),
    };
    if special {
        return Ok(Start::Run(job));
    }
    job.execute(deadline)
        .map(Start::Ready)
        .ok_or_else(|| crate::host::Stop::TimeLimit.into())
}

/// `start` for a test that waits for a special file on its own thread.
#[cfg(test)]
pub(crate) fn call_until_at(
    key: &str,
    method: &str,
    bytes: &[u8],
    grants: &Grants,
    deadline: Option<Instant>,
    cwd: &Path,
) -> wasmtime::Result<Enc> {
    Ok(match start(key, method, bytes, grants, deadline, cwd)? {
        Start::Ready(encoded) => encoded,
        Start::Run(job) => job.run_blocking(),
    })
}

#[cfg(test)]
pub(crate) fn call_until(
    key: &str,
    method: &str,
    bytes: &[u8],
    grants: &Grants,
    deadline: Option<Instant>,
) -> wasmtime::Result<Enc> {
    let cwd = std::env::current_dir().map_err(|e| crate::host::Stop::Internal(e.to_string()))?;
    call_until_at(key, method, bytes, grants, deadline, &cwd)
}

#[cfg(test)]
pub(crate) fn call(
    key: &str,
    method: &str,
    bytes: &[u8],
    grants: &Grants,
) -> wasmtime::Result<Enc> {
    call_until(key, method, bytes, grants, None)
}

pub(crate) enum Failure {
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
    pub(crate) fn new(syscall: &'static str, error: io::Error) -> Self {
        Self::System {
            error,
            syscall,
            path: None,
        }
    }
}

pub(crate) fn check_deadline(deadline: Option<Instant>) -> Result<(), Failure> {
    if deadline.is_some_and(|limit| Instant::now() >= limit) {
        Err(Failure::Deadline)
    } else {
        Ok(())
    }
}

// Runner-owned snapshots and regressions use the same file operations as
// guest filesystem calls, but retain ordinary Rust I/O errors. A special
// file (a FIFO, say) waits on a thread of its own, and the deadline cuts
// the wait: the thread lingers until its I/O ends
// (`std-fs.special-file.deadline`).
pub(crate) fn read_file_until(path: &Path, deadline: Option<Instant>) -> io::Result<Vec<u8>> {
    let owned = path.to_owned();
    bounded(path, deadline, move || read(&owned, None)).map_err(file_failure)
}

pub(crate) fn write_file_until(
    path: &Path,
    bytes: &[u8],
    deadline: Option<Instant>,
) -> io::Result<()> {
    let (owned, bytes) = (path.to_owned(), bytes.to_vec());
    let result = bounded(path, deadline, move || {
        let mut file = open(&owned, true, false, None)?;
        write(&mut file, &bytes, None)
    });
    result.map_err(file_failure)
}

fn bounded<T: Send + 'static>(
    path: &Path,
    deadline: Option<Instant>,
    work: impl FnOnce() -> Result<T, Failure> + Send + 'static,
) -> Result<T, Failure> {
    check_deadline(deadline)?;
    let Some(limit) = deadline.filter(|_| is_special(path)) else {
        let result = work()?;
        check_deadline(deadline)?;
        return Ok(result);
    };
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = send.send(work());
    });
    match receive.recv_timeout(limit.saturating_duration_since(Instant::now())) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(Failure::Deadline),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err(Failure::Message("the file operation failed".into()))
        }
    }
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

pub(crate) fn entry(e: &mut Enc, path: &str, st: &Metadata) {
    let kind = if st.file_type().is_symlink() {
        2
    } else {
        u64::from(st.is_dir())
    };
    e.str(path)
        .leb(kind)
        .leb(if st.is_dir() { 0 } else { st.len() });
}

pub(crate) fn error(path: &str, variant: u64) -> Enc {
    let mut e = Enc::default();
    e.leb(1).leb(variant).str(path);
    e
}

pub(crate) fn failure(path: &str, method: &str, from: &str, failure: &Failure) -> Enc {
    let (err, syscall, error_path) = match failure {
        Failure::Deadline => unreachable!("deadline is a host stop, not an Fs error"),
        Failure::Message(message) => return error(message, 7),
        Failure::System {
            error,
            syscall,
            path,
        } => (error, *syscall, path),
    };
    let variant = match crate::native_io::error_kind(err) {
        io::ErrorKind::NotFound => 0,
        io::ErrorKind::PermissionDenied => 1,
        io::ErrorKind::AlreadyExists => 3,
        io::ErrorKind::NotADirectory => 4,
        io::ErrorKind::IsADirectory => 5,
        _ => 7,
    };
    if variant != 7 {
        return error(path, variant);
    }
    let path = error_path.as_deref().unwrap_or(path);
    let paths = if method == "rename" {
        format!("{from:?} -> {path:?}")
    } else {
        format!("{path:?}")
    };
    error(&format!("{syscall} {paths}: {err}"), 7)
}

#[cfg(test)]
mod tests {
    use super::{Failure, call, call_until_at, error};
    use crate::codec::{Dec, Enc};
    use crate::fs::test_support::{TempDir, assert_time_limit, limited};
    use hd_run::{Grants, Limit};
    use std::fs;
    use std::time::Instant;

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
                    hd_run::canonicalize(&allowed)
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
    fn nul_paths_have_readable_validation_errors() {
        for (from, to, argument) in [
            ("a\0b", "to", "source path"),
            ("from", "a\0b", "destination path"),
        ] {
            let mut args = Enc::default();
            args.str(from).str(to);
            assert_eq!(
                call("FsWrite", "rename", args.done(), &Grants::default())
                    .unwrap()
                    .done(),
                error(&format!("{argument} contains a NUL byte"), 7).done()
            );
        }
    }
    #[test]
    fn other_errors_keep_native_messages_and_operation_paths() {
        for (method, syscall) in [
            ("read_bytes", "read"),
            ("write_text", "write"),
            ("rename", "rename"),
        ] {
            let native = std::io::Error::other("native failure");
            let failure = Failure::new(syscall, native);
            let paths = if method == "rename" {
                "\"from\" -> \"to\""
            } else {
                "\"to\""
            };
            assert_eq!(
                super::failure("to", method, "from", &failure).done(),
                error(&format!("{syscall} {paths}: native failure"), 7).done()
            );
        }
    }
}
