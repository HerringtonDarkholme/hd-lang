//! Synchronous filesystem providers, matching `core.mjs`'s `fsOp`.
//! Grants are checked against resolved paths before any filesystem access.

use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;

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
    let mut d = Dec::new(bytes);
    let path = d.str()?;
    let to = if method == "rename" {
        Some(d.str()?)
    } else {
        None
    };
    let cwd = std::env::current_dir().map_err(|e| crate::host::Stop::Internal(e.to_string()))?;
    for p in std::iter::once(&path).chain(to.iter()) {
        if !grants.covers_path(key, Path::new(p), &cwd) {
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
    let result = run(method, &path, to.as_deref(), payload, &mut e);
    Ok(match result {
        Ok(()) => e,
        Err(err) => failure(to.as_deref().unwrap_or(&path), method, &path, &err),
    })
}

fn run(
    method: &str,
    path: &str,
    to: Option<&str>,
    payload: Option<&[u8]>,
    e: &mut Enc,
) -> Result<(), Failure> {
    match method {
        "read_bytes" => {
            e.leb(0).bytes(&read(path)?);
        }
        "read_text" => {
            let bytes = read(path)?;
            if let Ok(text) = std::str::from_utf8(&bytes) {
                // TextDecoder strips the initial UTF-8 BOM.
                e.leb(0).str(text.strip_prefix('\u{feff}').unwrap_or(text));
            } else {
                *e = error(path, 6);
            }
        }
        "stat" => match fs::symlink_metadata(path) {
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
            if fs::metadata(path).is_ok_and(|st| !st.is_dir()) {
                return Err(Failure::new("scandir", io::ErrorKind::NotADirectory.into()));
            }
            let mut names = fs::read_dir(path)
                .map_err(|err| Failure::new("scandir", err))?
                .map(|item| item.map(|i| i.file_name().to_string_lossy().into_owned()))
                .collect::<io::Result<Vec<_>>>()
                .map_err(|err| Failure::new("scandir", err))?;
            names.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
            let under = if path.ends_with('/') {
                path.to_owned()
            } else {
                format!("{path}/")
            };
            let found: Vec<_> = names
                .into_iter()
                .filter_map(|n| {
                    let p = format!("{under}{n}");
                    fs::symlink_metadata(&p).ok().map(|st| (p, st))
                })
                .collect();
            e.leb(0).list(&found, |e, (p, st)| {
                entry(e, p, st);
            });
        }
        "write_bytes" | "write_text" => {
            let mut file = open(path, true, false)?;
            file.write_all(payload.unwrap_or_default())
                .map_err(|err| Failure::new("write", err))?;
            e.leb(0);
        }
        "append_text" => {
            open(path, true, true)?
                .write_all(payload.unwrap_or_default())
                .map_err(|err| Failure::new("write", err))?;
            e.leb(0);
        }
        "create_dir_all" => {
            if fs::metadata(path).is_ok_and(|st| !st.is_dir()) {
                *e = error(path, 4);
            } else {
                if path.is_empty() {
                    // std accepts the empty path; Node mkdir does not.
                    *e = error(path, 0);
                    return Ok(());
                }
                mkdir(Path::new(path))?;
                e.leb(0);
            }
        }
        "remove" => {
            if fs::symlink_metadata(path)
                .map_err(|err| Failure::new("lstat", err))?
                .is_dir()
            {
                fs::remove_dir(path).map_err(|err| Failure::new("rmdir", err))?;
            } else {
                fs::remove_file(path).map_err(|err| Failure::new("unlink", err))?;
            }
            e.leb(0);
        }
        "rename" => {
            fs::rename(path, to.unwrap_or_default()).map_err(|err| Failure::new("rename", err))?;
            e.leb(0);
        }
        _ => unreachable!("registered filesystem method"),
    }
    Ok(())
}

enum Failure {
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

fn read(path: &str) -> Result<Vec<u8>, Failure> {
    let mut file = open(path, false, false)?;
    let st = file.metadata().map_err(|err| Failure::new("fstat", err))?;
    if st.is_file() && st.len() > 0x7fff_ffff {
        return Err(Failure::Message(format!(
            "File size ({}) is greater than 2 GiB",
            st.len()
        )));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|err| Failure::new("read", err))?;
    Ok(bytes)
}

fn open(path: &str, write: bool, append: bool) -> Result<File, Failure> {
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
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
        // libuv opens directories too; a read then returns EISDIR, rather
        // than failing the open with Windows' ERROR_ACCESS_DENIED.
        options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
    }
    options.open(path).map_err(|err| Failure::new("open", err))
}

fn mkdir(path: &Path) -> Result<(), Failure> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                mkdir(parent)?;
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
    use super::{Failure, call, error, inspect_string};
    use crate::codec::{Dec, Enc};
    use hd_run::{Grants, Limit};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

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
