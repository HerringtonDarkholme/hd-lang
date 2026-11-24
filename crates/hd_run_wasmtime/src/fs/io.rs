//! Plain blocking file I/O at a `Place`. A regular file or a directory runs
//! on the program thread (`api::start`); a special file runs on the
//! blocking pool, so nothing here polls.

use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::time::Instant;

use super::api::{Failure, check_deadline};
use super::sandbox::{Place, below_a_file};

/// The whole file.
pub(crate) fn read(place: &Place, deadline: Option<Instant>) -> Result<Vec<u8>, Failure> {
    let mut file = open(place, false, false, deadline)?;
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
    place: &Place,
    write: bool,
    append: bool,
    deadline: Option<Instant>,
) -> Result<File, Failure> {
    check_deadline(deadline)?;
    let file = place.open(write, append).map_err(|err| {
        // Windows refuses a write-open of a directory with "The parameter
        // is incorrect"; a path that is a directory is `IsADirectory`
        // whatever the OS code is, as the Unix `EISDIR` already is.
        let err = if write && place.metadata().is_ok_and(|st| st.dir) {
            ErrorKind::IsADirectory.into()
        } else {
            err
        };
        Failure::new("open", below_a_file(place, err))
    })?;
    // An append-open of a directory succeeds on Windows (the backup flag
    // of `Place::open`) and fails only at the write, as "Incorrect function". The
    // handle itself says what was opened, so no path check can race.
    if write
        && file
            .metadata()
            .map_err(|err| Failure::new("fstat", err))?
            .is_dir()
    {
        return Err(Failure::new("open", ErrorKind::IsADirectory.into()));
    }
    Ok(file)
}

/// A path in the Windows device namespace, where named pipes and other
/// devices live: `\\.\pipe\name` and its spellings, and a UNC pipe share.
#[cfg(windows)]
#[must_use]
pub fn device_path(path: &str) -> bool {
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
