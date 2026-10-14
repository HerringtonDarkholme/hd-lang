use std::fs::{File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use super::api::{Failure, check_deadline};

pub(crate) enum OpenedFile {
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

pub(crate) fn read(path: &Path, deadline: Option<Instant>) -> Result<Vec<u8>, Failure> {
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

pub(crate) fn write(
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

pub(crate) fn open(
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
                if !std::fs::metadata(path).is_ok_and(|st| st.file_type().is_fifo()) {
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

#[cfg(all(test, windows))]
mod tests {
    use crate::fs::api::{Failure, error, failure};
    use crate::fs::test_support::{TempDir, assert_time_limit, limited};
    use std::fs;
    use std::time::{Duration, Instant};

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
    fn pipe_name(dir: &TempDir) -> String {
        format!(r"\\.\pipe\{}", dir.0.file_name().unwrap().to_string_lossy())
    }
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
