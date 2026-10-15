//! Plain blocking `std::fs` I/O. A regular file or a directory runs on the
//! program thread (`api::start`); a special file runs on the blocking pool,
//! so nothing here polls.

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::time::Instant;

use super::api::{Failure, check_deadline};

/// Whether opening `path` can wait on another process: a FIFO, a device,
/// a socket, or a Windows named pipe. A regular file, a directory, and a
/// path that does not exist or cannot be inspected are not (the open
/// reports the error at once).
pub(crate) fn is_special(path: &Path) -> bool {
    // std cannot tell a Windows pipe from a file, and inspecting a pipe
    // opens it, so this goes by the name.
    #[cfg(windows)]
    if device_path(&path.to_string_lossy()) {
        return true;
    }
    #[cfg(unix)]
    return std::fs::metadata(path).is_ok_and(|st| !st.is_file() && !st.is_dir());
    #[cfg(not(unix))]
    false
}

pub(crate) fn read(path: &Path, deadline: Option<Instant>) -> Result<Vec<u8>, Failure> {
    let mut file = open(path, false, false, deadline)?;
    check_deadline(deadline)?;
    let st = file.metadata().map_err(|err| Failure::new("fstat", err))?;
    if st.is_dir() {
        // Windows reads a directory handle with "Incorrect function"; no
        // OS is asked, so every OS reports the same `IsADirectory`.
        return Err(Failure::new("read", ErrorKind::IsADirectory.into()));
    }
    if st.is_file() && st.len() > 0x7fff_ffff {
        return Err(Failure::Message(format!(
            "File size ({}) is greater than 2 GiB",
            st.len()
        )));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|err| Failure::new("read", err))?;
    check_deadline(deadline)?;
    Ok(bytes)
}

pub(crate) fn write(
    file: &mut File,
    bytes: &[u8],
    deadline: Option<Instant>,
) -> Result<(), Failure> {
    file.write_all(bytes)
        .map_err(|err| Failure::new("write", err))?;
    check_deadline(deadline)
}

pub(crate) fn open(
    path: &Path,
    write: bool,
    append: bool,
    deadline: Option<Instant>,
) -> Result<File, Failure> {
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
    check_deadline(deadline)?;
    options.open(path).map_err(|err| {
        // Windows refuses a write-open of a directory with "The parameter
        // is incorrect"; a path that is a directory is `IsADirectory`
        // whatever the OS code is, as the Unix `EISDIR` already is.
        let err = if write && path.is_dir() {
            ErrorKind::IsADirectory.into()
        } else {
            err
        };
        Failure::new("open", err)
    })
}

/// A path in the Windows device namespace, where named pipes and other
/// devices live: `\\.\pipe\name` and its spellings, and a UNC pipe share.
#[cfg(windows)]
fn device_path(path: &str) -> bool {
    let normalized = path.replace('/', "\\");
    let path = normalized.to_ascii_lowercase();
    if path.starts_with(r"\\.\")
        || path.starts_with(r"\\?\pipe\")
        || path.starts_with(r"\\?\globalroot\device\")
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

#[cfg(all(test, windows))]
mod tests {
    use crate::fs::api::{Failure, error, failure};

    #[test]
    fn device_namespace_detection_preserves_ordinary_files() {
        for path in [
            r"\\.\pipe\hd-test",
            r"\\?\PIPE\hd-test",
            "//./pipe/hd-test",
            r"\\localhost\pipe\hd-test",
            r"\\?\UNC\host\pipe\hd-test",
            r"\\?\GLOBALROOT\Device\NamedPipe\hd-test",
            r"\\.\NUL",
        ] {
            assert!(super::device_path(path), "{path}");
        }
        for path in [
            r"C:\pipe\hd-test",
            r"\\host\share\pipe\hd-test",
            r"\\host\pipeline\hd-test",
            r"\\?\C:\pipe\hd-test",
            "pipe/hd-test",
        ] {
            assert!(!super::device_path(path), "{path}");
        }
    }
    #[test]
    fn windows_errors_use_rust_error_categories_and_native_messages() {
        use std::io::ErrorKind;
        use windows_sys::Win32::Foundation::{
            ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_DIRECTORY,
            ERROR_DIRECTORY_NOT_SUPPORTED, ERROR_FILE_NOT_FOUND, ERROR_INVALID_NAME, ERROR_NO_DATA,
            ERROR_SHARING_VIOLATION,
        };
        // Rust std's Windows decode_error_kind, not libuv's translation.
        for (code, kind, variant) in [
            (ERROR_ACCESS_DENIED, ErrorKind::PermissionDenied, 1),
            (ERROR_ALREADY_EXISTS, ErrorKind::AlreadyExists, 3),
            (ERROR_FILE_NOT_FOUND, ErrorKind::NotFound, 0),
            (ERROR_DIRECTORY, ErrorKind::NotADirectory, 4),
            (ERROR_DIRECTORY_NOT_SUPPORTED, ErrorKind::IsADirectory, 5),
            (ERROR_INVALID_NAME, ErrorKind::InvalidFilename, 7),
            (ERROR_NO_DATA, ErrorKind::BrokenPipe, 7),
        ] {
            let native = std::io::Error::from_raw_os_error(i32::try_from(code).unwrap());
            assert_eq!(native.kind(), kind);
            let message = format!("open \"to\": {native}");
            let refused = Failure::new("open", native);
            assert_eq!(
                failure("to", "read_text", "to", &refused).done(),
                error(if variant == 7 { &message } else { "to" }, variant).done()
            );
        }
        // Unclassified platform errors remain informative Other values.
        let native =
            std::io::Error::from_raw_os_error(i32::try_from(ERROR_SHARING_VIOLATION).unwrap());
        let message = format!("open \"to\": {native}");
        let refused = Failure::new("open", native);
        assert_eq!(
            failure("to", "read_text", "to", &refused).done(),
            error(&message, 7).done()
        );
    }
}
