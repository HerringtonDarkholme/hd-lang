use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use hd_run::Grants;

use crate::abi::{self, Entry, EntryKind, FsError};
use crate::codec::{Dec, Enc, Wire, encode};
use crate::runtime::reactor::Done;

use super::io::{open, open_read, read, write};
use super::ops::{Env, at, run};
use super::sandbox::{Place, Sandbox, Stat, is_escape};

/// The methods that open the path, so that a special file can make them
/// wait. The others never open it, or fail at once on it.
const OPENING: &[&str] = &[
    "open",
    "read_bytes",
    "read_text",
    "write_bytes",
    "write_text",
    "append_text",
];

/// What `.start` of an `Fs` method does.
pub enum Start {
    /// A fresh directory is registered with the host before its path enters hd.
    Temporary(super::temp::Directory),
    /// A successful recursive removal releases the host's fallback owner.
    Removed {
        path: PathBuf,
        succeeded: bool,
        encoded: Enc,
    },
    /// The result, at once: a refusal, an error, or a regular file or a
    /// directory, which run inline on the program thread.
    Ready(Enc),
    /// A special file (`std-fs.special-file`): the call waits on the
    /// blocking pool, and `.finish` delivers its result.
    Run(Box<Job>),
    /// `open` of a regular file or a directory, run inline: the file, or
    /// the encoded error.
    Opened(Result<std::fs::File, Enc>),
}

/// A call on a special file, ready to run off the program thread.
pub struct Job {
    pub method: String,
    pub path: String,
    pub to: Option<String>,
    /// Where the call acts, and where a `rename` moves to.
    pub place: Place,
    pub to_place: Option<Place>,
    pub payload: Option<Vec<u8>>,
    pub cwd: PathBuf,
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
    pub async fn run(self: Box<Self>) -> Done {
        if self.method == "open" {
            return match tokio::task::spawn_blocking(move || self.opened(None)).await {
                Ok(Some(opened)) => Done::Opened(opened),
                Ok(None) => Done::Opened(Err(error_text("the open ran past a deadline"))),
                Err(error) => Done::Opened(Err(error_text(&format!("the open failed: {error}")))),
            };
        }
        match tokio::task::spawn_blocking(move || self.run_blocking()).await {
            Ok(encoded) => Done::Fs(encoded),
            Err(error) => Done::Fs(error_text(&format!("the file operation failed: {error}"))),
        }
    }

    /// The call, run on this thread, which waits for the file.
    #[must_use]
    pub fn run_blocking(self: Box<Self>) -> Enc {
        self.execute(None, None)
            .unwrap_or_else(|| error_text("the file operation ran past a deadline"))
    }

    fn copy(&self, deadline: Option<Instant>) -> Option<Enc> {
        let to = self.to_place.as_ref()?;
        let path = self.to.as_deref()?;
        let failed = |shown: &str, err: Failure| match err {
            Failure::Deadline => None,
            Failure::NotGranted => Some(not_granted_at(shown, &self.cwd, true)),
            err => Some(failure(shown, "copy", &self.path, &err)),
        };
        let source = match open(&self.place, false, false, deadline) {
            Ok(file) => cap_std::fs::File::from_std(file),
            Err(err) => return failed(&self.path, err),
        };
        let source_meta = match source.metadata() {
            Ok(metadata) if !metadata.is_dir() => metadata,
            Ok(_) => {
                return failed(
                    &self.path,
                    Failure::new("read", io::ErrorKind::IsADirectory.into()),
                );
            }
            Err(err) => return failed(&self.path, Failure::new("fstat", err)),
        };
        check_deadline(deadline).ok()?;
        let target = match to.open_copy_target() {
            Ok(file) => cap_std::fs::File::from_std(file),
            Err(err) => {
                let err = if to.metadata().is_ok_and(|st| st.dir) {
                    io::ErrorKind::IsADirectory.into()
                } else {
                    super::sandbox::below_a_file(to, err)
                };
                return failed(path, Failure::new("open", err));
            }
        };
        let target_meta = match target.metadata() {
            Ok(metadata) if !metadata.is_dir() => metadata,
            Ok(_) => {
                return failed(
                    path,
                    Failure::new("write", io::ErrorKind::IsADirectory.into()),
                );
            }
            Err(err) => return failed(path, Failure::new("fstat", err)),
        };
        // Compare opened identities before truncation: aliases and hard links
        // must not destroy the file they are copying from.
        if !same_file(&source_meta, &target_meta) {
            let mut source = source.into_std();
            let mut target = target.into_std();
            if let Err(err) = target
                .set_len(0)
                .and_then(|()| io::copy(&mut source, &mut target).map(|_| ()))
            {
                return failed(path, Failure::new("copy", err));
            }
        }
        check_deadline(deadline).ok()?;
        Some(ok(()))
    }

    /// `open`: the file opened for reading, or the encoded error as
    /// `read_bytes` reports it; `None` once the deadline passed.
    fn opened(&self, deadline: Option<Instant>) -> Option<Result<std::fs::File, Enc>> {
        match open_read(&self.place, deadline) {
            Ok((file, _)) => Some(Ok(file)),
            Err(Failure::Deadline) => None,
            Err(Failure::NotGranted) => Some(Err(not_granted_at(&self.path, &self.cwd, true))),
            Err(err) => Some(Err(failure(&self.path, "open", &self.path, &err))),
        }
    }

    /// The encoded result, or `None` once the deadline passed.
    fn execute(&self, deadline: Option<Instant>, sandbox: Option<&Sandbox>) -> Option<Enc> {
        if self.method == "copy" {
            return self.copy(deadline);
        }
        let result = run(
            &self.method,
            &self.path,
            &self.place,
            self.to_place.as_ref(),
            self.payload.as_deref(),
            Env {
                cwd: &self.cwd,
                deadline,
                sandbox,
            },
        );
        if check_deadline(deadline).is_err() {
            return None;
        }
        match result {
            Ok(e) => Some(e),
            Err(Failure::Deadline) => None,
            Err(Failure::NotGranted) => Some(not_granted_at(
                self.to.as_deref().unwrap_or(&self.path),
                &self.cwd,
                !matches!(self.method.as_str(), "stat" | "remove" | "rename"),
            )),
            Err(err) => Some(failure(
                self.to.as_deref().unwrap_or(&self.path),
                &self.method,
                &self.path,
                &err,
            )),
        }
    }
}

/// Identity of opened files, including aliases across independently scoped grants.
fn same_file(left: &cap_std::fs::Metadata, right: &cap_std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        left.dev() == right.dev() && left.ino() == right.ino()
    }
    #[cfg(windows)]
    {
        // Both values come from `File::metadata`, so the volume serial
        // number and file index are present.
        use cap_fs_ext::MetadataExt;
        left.dev() == right.dev() && left.ino() == right.ino()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (left, right);
        false
    }
}

/// The error a `.start` that cannot offload returns.
pub(crate) fn not_run(error: &str) -> Enc {
    error_text(error)
}

fn error_text(text: &str) -> Enc {
    error(other(text))
}

/// Executes relative paths in the case's directory without changing the
/// process-wide working directory. Encoded paths and errors retain the
/// guest's original spelling, except a grant refusal names a changed
/// resolved target. Under a grant with scope entries, the call
/// acts under the handle of the granted directory that covers it
/// (`sandbox`). A regular file or a directory runs here;
/// anything else (a FIFO, a device, a named pipe) comes back as a `Job`.
pub fn start(
    key: &str,
    method: &str,
    bytes: &[u8],
    grants: &Grants,
    sandbox: &Sandbox,
    deadline: Option<Instant>,
    cwd: &Path,
) -> wasmtime::Result<Start> {
    check_deadline(deadline).map_err(|_| crate::host::Stop::TimeLimit)?;
    let mut d = Dec::new(bytes);
    if method == "create_temp_dir" {
        // A sole string argument uses the ABI's raw UTF-8 fast path.
        let prefix = std::str::from_utf8(bytes).map_err(|_| {
            crate::host::Stop::Contract("temporary-directory prefix is not UTF-8".into())
        })?;
        return super::temp::create(prefix, grants, sandbox, deadline, cwd);
    }
    let abi::Path(path) = Wire::dec(&mut d)?;
    let to = if matches!(method, "rename" | "copy") {
        let abi::Path(to) = Wire::dec(&mut d)?;
        Some(to)
    } else {
        None
    };
    // Every argument decodes before the grant is checked, as on Node: a
    // payload is a `List[u8]` for `write_bytes`, a `string`'s bytes for the
    // others.
    let payload = if matches!(method, "write_bytes" | "write_text" | "append_text") {
        Some(Vec::<u8>::dec(&mut d)?)
    } else {
        None
    };
    let source_key = if method == "copy" { "FsRead" } else { key };
    for (p, resource) in std::iter::once((&path, source_key)).chain(to.iter().map(|p| (p, key))) {
        if !grants.covers_path(resource, Path::new(p), cwd) {
            return Ok(Start::Ready(not_granted_at(p, cwd, true)));
        }
    }
    // Reject NUL before OS access, after checking the grant.
    for (p, name) in std::iter::once((
        &path,
        if matches!(method, "rename" | "copy") {
            "source path"
        } else {
            "path"
        },
    ))
    .chain(to.iter().map(|p| (p, "destination path")))
    {
        if p.contains('\0') {
            return Ok(Start::Ready(error_text(&format!(
                "{name} contains a NUL byte"
            ))));
        }
    }
    // `stat`, `remove` and `rename` act on a final symbolic link itself.
    let follow = !matches!(method, "stat" | "remove" | "remove_dir_all" | "rename");
    let create = method == "create_dir_all";
    let place_of =
        |p: &str, resource: &str| sandbox.place(grants, resource, &at(cwd, p), follow, create);
    let Some(place) = place_of(&path, source_key) else {
        return Ok(Start::Ready(not_granted_at(&path, cwd, follow)));
    };
    let to_place = match &to {
        Some(p) => match place_of(p, key) {
            Some(place) => Some(place),
            None => return Ok(Start::Ready(not_granted_at(p, cwd, follow))),
        },
        None => None,
    };
    if method == "remove_dir_all" {
        sandbox.forget(&place);
        let result = place.remove_dir_all();
        let succeeded = result.is_ok();
        let encoded = match result {
            Ok(()) => ok(()),
            Err(error) => failure(&path, method, &path, &Failure::new("rmdir", error)),
        };
        return Ok(Start::Removed {
            path: at(cwd, &path),
            succeeded,
            encoded,
        });
    }
    let special = (OPENING.contains(&method) || method == "copy") && place.is_special()
        || method == "copy" && to_place.as_ref().is_some_and(Place::is_special);
    let job = Job {
        method: method.to_owned(),
        path,
        to,
        place,
        to_place,
        payload,
        cwd: cwd.to_owned(),
    };
    if special {
        return Ok(Start::Run(Box::new(job)));
    }
    if method == "open" {
        return match job.opened(deadline) {
            Some(opened) => Ok(Start::Opened(opened)),
            None => Err(crate::host::Stop::TimeLimit.into()),
        };
    }
    match job.execute(deadline, Some(sandbox)) {
        Some(encoded) => Ok(Start::Ready(encoded)),
        None => Err(crate::host::Stop::TimeLimit.into()),
    }
}

/// `start` for a test that waits for a special file on its own thread.
#[cfg(feature = "testing")]
pub fn call_until_at(
    key: &str,
    method: &str,
    bytes: &[u8],
    grants: &Grants,
    deadline: Option<Instant>,
    cwd: &Path,
) -> wasmtime::Result<Enc> {
    let sandbox = Sandbox::default();
    Ok(
        match start(key, method, bytes, grants, &sandbox, deadline, cwd)? {
            Start::Ready(encoded)
            | Start::Opened(Err(encoded))
            | Start::Removed { encoded, .. } => encoded,
            // The direct API test wrapper retains no resource, as for File.
            Start::Temporary(directory) => ok(crate::abi::Path(
                directory.path.to_string_lossy().into_owned(),
            )),
            // A test's call registers no handle: the file closes here.
            Start::Opened(Ok(_)) => ok(crate::codec::Handle(0)),
            Start::Run(job) => job.run_blocking(),
        },
    )
}

#[cfg(feature = "testing")]
pub fn call_until(
    key: &str,
    method: &str,
    bytes: &[u8],
    grants: &Grants,
    deadline: Option<Instant>,
) -> wasmtime::Result<Enc> {
    let cwd = std::env::current_dir().map_err(|e| crate::host::Stop::Internal(e.to_string()))?;
    call_until_at(key, method, bytes, grants, deadline, &cwd)
}

#[cfg(feature = "testing")]
pub fn call(key: &str, method: &str, bytes: &[u8], grants: &Grants) -> wasmtime::Result<Enc> {
    call_until(key, method, bytes, grants, None)
}

pub enum Failure {
    Deadline,
    System {
        error: io::Error,
        syscall: &'static str,
        // Recursive mkdir can fail while creating an ancestor. Node's Other
        // message names that ancestor, while typed errors name the input path.
        path: Option<String>,
    },
    Message(String),
    /// The path would leave the granted directory that covers it
    /// (`sandbox`): `NotGranted`.
    NotGranted,
}

impl Failure {
    #[must_use]
    pub fn new(syscall: &'static str, error: io::Error) -> Self {
        if is_escape(&error) {
            return Self::NotGranted;
        }
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
    let place = Place::Ambient(path.to_owned());
    bounded(path, deadline, move || read(&place, None)).map_err(file_failure)
}

pub(crate) fn write_file_until(
    path: &Path,
    bytes: &[u8],
    deadline: Option<Instant>,
) -> io::Result<()> {
    let (place, bytes) = (Place::Ambient(path.to_owned()), bytes.to_vec());
    let result = bounded(path, deadline, move || {
        let mut file = open(&place, true, false, None)?;
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
    let Some(limit) = deadline.filter(|_| Place::Ambient(path.to_owned()).is_special()) else {
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
        Failure::NotGranted => io::ErrorKind::PermissionDenied.into(),
    }
}

/// The `Entry` of `path`, whose metadata is `st`.
pub(crate) fn entry(path: &str, st: &Stat) -> Entry {
    Entry {
        path: abi::Path(path.to_owned()),
        kind: if st.link {
            EntryKind::Symlink
        } else if st.dir {
            EntryKind::Directory
        } else {
            EntryKind::File
        },
        size: if st.dir { 0 } else { st.len },
        modified: st.modified.map(|millis| abi::Timestamp { millis }),
    }
}

/// A method's result: `.Ok(value)`.
pub(crate) fn ok<T: Wire>(value: T) -> Enc {
    encode(&Ok::<T, FsError>(value))
}

/// A method's result: `.Err(error)`.
pub(crate) fn error(error: FsError) -> Enc {
    encode(&Err::<(), _>(error))
}

fn not_granted(path: &str) -> Enc {
    error(FsError::NotGranted {
        path: abi::Path(path.to_owned()),
    })
}

fn not_granted_at(path: &str, cwd: &Path, follow: bool) -> Enc {
    not_granted(&hd_run::refused_path(Path::new(path), cwd, follow))
}

fn other(message: &str) -> FsError {
    FsError::Other {
        message: message.to_owned(),
    }
}

#[must_use]
pub fn failure(path: &str, method: &str, from: &str, failure: &Failure) -> Enc {
    let (err, syscall, error_path) = match failure {
        Failure::Deadline => unreachable!("a deadline is a host stop, not an Fs error"),
        Failure::Message(message) => return error(other(message)),
        Failure::NotGranted => return not_granted(path),
        Failure::System {
            error,
            syscall,
            path,
        } => (error, *syscall, path),
    };
    let typed = abi::Path(path.to_owned());
    match crate::runtime::native_io::error_kind(err) {
        io::ErrorKind::NotFound => return error(FsError::NotFound { path: typed }),
        io::ErrorKind::PermissionDenied => {
            return error(FsError::PermissionDenied { path: typed });
        }
        io::ErrorKind::AlreadyExists => return error(FsError::AlreadyExists { path: typed }),
        io::ErrorKind::NotADirectory => return error(FsError::NotADirectory { path: typed }),
        io::ErrorKind::IsADirectory => return error(FsError::IsADirectory { path: typed }),
        _ => {}
    }
    let path = error_path.as_deref().unwrap_or(path);
    let paths = if method == "rename" {
        format!("{from:?} -> {path:?}")
    } else {
        format!("{path:?}")
    };
    error(other(&format!("{syscall} {paths}: {err}")))
}
