//! Polling Windows pipes without pending I/O or background flush workers.
//!
//! `PIPE_NOWAIT` is deliberately used as synchronous polling, not as an
//! asynchronous-I/O facility. Windows documents immediate reads/writes in
//! this mode; callers retry `WouldBlock` after checking their deadline or
//! cancellation flag. No operation or buffer survives a completed call.
//!
//! See <https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-type-read-and-wait-modes>.
//! Named-pipe clients need extra attribute access to inspect/set their wait
//! mode. An ACL refusal is returned unchanged, never replaced by blocking I/O.
//! Opening a remote SMB pipe remains an OS call: its kernel/network delay
//! cannot be interrupted by this polling backend.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::windows::fs::{FileExt, OpenOptionsExt};
use std::os::windows::io::OwnedHandle;

use interprocess::os::windows::named_pipe::{DuplexPipeStream, pipe_mode};
#[cfg(test)]
use interprocess::os::windows::named_pipe::{PipeListener, PipeListenerOptions};
use windows_sys::Win32::Foundation::{
    ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_NOT_CONNECTED, GENERIC_READ, GENERIC_WRITE,
};
use windows_sys::Win32::Storage::FileSystem::{FILE_READ_ATTRIBUTES, FILE_WRITE_ATTRIBUTES};

/// A single-owner, byte-read-mode pipe, configured for immediate polling.
/// There is no userspace buffer and dropping it closes its handle directly.
pub struct Pipe(File);

/// Classifies an already-open handle without requesting additional access.
/// This catches reparse-point aliases which do not spell a pipe namespace.
///
/// # Errors
/// Returns the OS error if querying the handle's file type fails.
pub fn is_pipe(file: &File) -> io::Result<bool> {
    winapi_util::file::typ(file).map(|kind| kind.is_pipe())
}

impl Pipe {
    /// Opens an existing named pipe. A busy server is an ordinary OS error;
    /// the caller may retry it while its deadline has not expired.
    ///
    /// Read clients additionally request `FILE_WRITE_ATTRIBUTES` to set the
    /// wait mode. Write clients request `FILE_READ_ATTRIBUTES` to inspect the
    /// handle during safe conversion. Ordinary files must not use this API.
    ///
    /// # Errors
    /// Returns the OS error if opening, inspection, or mode setting fails.
    pub fn open(path: &str, write: bool) -> io::Result<Self> {
        let access = if write {
            GENERIC_WRITE | FILE_READ_ATTRIBUTES
        } else {
            GENERIC_READ | FILE_WRITE_ATTRIBUTES
        };
        let file = OpenOptions::new().access_mode(access).open(path)?;
        Self::from_file(file)
    }

    /// Upgrades an already-open pipe, including one reached through an alias.
    /// Mode-setting access refusals are errors, never a blocking fallback.
    /// Ordinary files must not use this API; classify them with [`is_pipe`].
    ///
    /// # Errors
    /// Returns the OS error if pipe inspection or mode setting fails.
    pub fn from_file(file: File) -> io::Result<Self> {
        let pipe = DuplexPipeStream::<pipe_mode::Bytes>::try_from(OwnedHandle::from(file))
            .map_err(|error| {
                error
                    .cause
                    .unwrap_or_else(|| io::Error::other("cannot inspect named pipe"))
            })?;
        let result = pipe.set_nonblocking(true);
        // Consuming conversion bypasses interprocess's potentially indefinite
        // background flush-on-Drop. Consume even when configuration failed.
        let file = into_file(pipe)?;
        result?;
        Ok(Self(file))
    }
}

#[cfg(test)]
fn private_name() -> io::Result<String> {
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).map_err(io::Error::other)?;
    Ok(format!(
        r"\\.\pipe\hd-{}-{:032x}",
        std::process::id(),
        u128::from_ne_bytes(nonce)
    ))
}

#[cfg(test)]
fn pair(path: &str, parent_reads: bool) -> io::Result<(Pipe, File)> {
    if !parent_reads {
        return stdin_pair();
    }
    // A duplex parent has both metadata-access rights. Existing std child
    // pipes can lack these rights, especially a send-only server handle.
    // interprocess uses FILE_FLAG_FIRST_PIPE_INSTANCE and rejects remote
    // clients by default, so a pre-existing pipe fails rather than being used.
    let listener = PipeListenerOptions::new()
        .path(path)
        .nonblocking(true)
        .input_buffer_size_hint(64 * 1024)
        .output_buffer_size_hint(64 * 1024)
        .create_duplex::<pipe_mode::Bytes>()?;
    // FILE_FLAG_OVERLAPPED is intentionally absent: the child's standard I/O
    // must be synchronous. File -> Stdio is direct handle inheritance, unlike
    // converting a std ChildStdout/ChildStdin, which creates a pipe relay.
    let child = OpenOptions::new()
        .read(!parent_reads)
        .write(parent_reads)
        .open(path)?;
    let parent = into_file(listener.accept()?)?;
    Ok((Pipe(parent), child))
}

#[cfg(test)]
fn stdin_pair() -> io::Result<(Pipe, File)> {
    // std's Windows implementation calls synchronous CreatePipe. Its writer
    // may close directly after writing: Microsoft's redirected-stdio example
    // relies on the remaining read handle to drain the buffered input to EOF.
    // A named server writer must not be substituted: closing it can discard
    // queued input, while flushing it can wait indefinitely for the child.
    // https://learn.microsoft.com/en-us/windows/win32/procthread/creating-a-child-process-with-redirected-input-and-output
    let (reader, writer) = io::pipe()?;

    // Narrow configure-only use of the pinned interprocess 2.4.4 listener API:
    // from_handle_and_options only stores the owned handle; set_nonblocking
    // calls SetNamedPipeHandleState; consuming conversion returns that handle
    // without stream conversion, flushing, or a background Drop worker.
    // The creation options are inert: never call accept/incoming or create an
    // instance through this adapter. None/Bytes matches this send-only byte
    // handle. Microsoft expressly permits anonymous handles in the mode call,
    // and GENERIC_WRITE already supplies its required rights.
    // https://docs.rs/crate/interprocess/2.4.4/source/src/os/windows/named_pipe/listener.rs
    // https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-setnamedpipehandlestate
    let adapter = PipeListener::<pipe_mode::None, pipe_mode::Bytes>::from_handle_and_options(
        OwnedHandle::from(writer),
        PipeListenerOptions::new(),
    );
    let result = adapter.set_nonblocking(true);
    // Consume before propagating either result, so both paths close directly.
    let writer = File::from(OwnedHandle::from(adapter));
    result?;
    Ok((Pipe(writer), File::from(OwnedHandle::from(reader))))
}

fn into_file(pipe: DuplexPipeStream<pipe_mode::Bytes>) -> io::Result<File> {
    OwnedHandle::try_from(pipe).map(File::from).map_err(|pipe| {
        // Not expected: this module never splits or clones a stream.
        // Even the failure path must not invoke a background flush.
        pipe.evade_limbo();
        io::Error::other("unexpected shared named-pipe handle")
    })
}

fn error_code(error: &io::Error) -> Option<u32> {
    error
        .raw_os_error()
        .and_then(|code| u32::try_from(code).ok())
}

impl Read for Pipe {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        // File::read and interprocess's Read both turn ERROR_NO_DATA into EOF.
        // FileExt preserves that error. Pipe offsets are ignored, and this
        // single-owner wrapper never performs concurrent I/O on its handle.
        // Rust uses NtReadFile and waits for completion before returning:
        // https://github.com/rust-lang/rust/blob/1.97.0/library/std/src/sys/pal/windows/handle.rs
        match self.0.seek_read(buffer, 0) {
            Err(error) if error_code(&error) == Some(ERROR_NO_DATA) => {
                Err(io::ErrorKind::WouldBlock.into())
            }
            Err(error)
                if matches!(
                    error_code(&error),
                    Some(ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED)
                ) =>
            {
                Ok(0)
            }
            result => result,
        }
    }
}

impl Write for Pipe {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        // PIPE_NOWAIT byte pipes may write only the available capacity,
        // including zero. A closed peer's ERROR_NO_DATA remains BrokenPipe.
        match self.0.seek_write(buffer, 0) {
            Ok(0) => Err(io::ErrorKind::WouldBlock.into()),
            result => result,
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        // No userspace buffer. FlushFileBuffers waits for the peer to consume
        // all bytes, even on a PIPE_NOWAIT handle, and must not be called here.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;

    fn bounded(test: impl FnOnce() + Send + 'static) {
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(test));
            let _ = tx.send(result);
        });
        let result = rx
            .recv_timeout(Duration::from_secs(3))
            .expect("pipe I/O blocked");
        worker.join().unwrap();
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }

    #[test]
    fn idle_pipe_is_retryable_binary_data_is_preserved_and_close_is_eof() {
        bounded(|| {
            let (mut parent, mut peer) = pair(&private_name().unwrap(), true).unwrap();
            let mut buffer = [0; 8];
            assert_eq!(parent.read(&mut []).unwrap(), 0);
            assert_eq!(
                parent.read(&mut buffer).unwrap_err().kind(),
                io::ErrorKind::WouldBlock
            );
            peer.write_all(b"\0\xffbytes").unwrap();
            assert_eq!(parent.read(&mut buffer).unwrap(), 7);
            assert_eq!(&buffer[..7], b"\0\xffbytes");
            drop(peer);
            assert_eq!(parent.read(&mut buffer).unwrap(), 0);
        });
    }

    #[test]
    fn backpressure_is_retryable_and_dropping_a_full_pipe_does_not_flush() {
        bounded(|| {
            let (mut parent, _peer) = pair(&private_name().unwrap(), false).unwrap();
            assert_eq!(parent.write(&[]).unwrap(), 0);
            let buffer = [0u8; 8 * 1024];
            let mut full = false;
            for _ in 0..1024 {
                match parent.write(&buffer) {
                    Ok(n) => assert!(n > 0 && n <= buffer.len()),
                    Err(error) => {
                        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
                        full = true;
                        break;
                    }
                }
            }
            assert!(full, "pipe never reached backpressure within 8 MiB");
            let start = Instant::now();
            parent.flush().unwrap();
            drop(parent);
            assert!(start.elapsed() < Duration::from_secs(1));
        });
    }

    #[test]
    fn closed_writer_error_is_not_retryable() {
        bounded(|| {
            let (mut parent, peer) = pair(&private_name().unwrap(), false).unwrap();
            drop(peer);
            assert_eq!(
                parent.write(b"x").unwrap_err().kind(),
                io::ErrorKind::BrokenPipe
            );
        });
    }

    #[test]
    fn direct_close_preserves_already_written_bytes_for_a_delayed_reader() {
        bounded(|| {
            for _ in 0..32 {
                let (mut parent, mut peer) = stdin_pair().unwrap();
                parent.write_all(b"\0\xffdelayed").unwrap();
                // No reader has run yet. A successful stdin writer must give
                // the child EOF without losing the queued input.
                drop(parent);
                let mut bytes = Vec::new();
                peer.read_to_end(&mut bytes).unwrap();
                assert_eq!(bytes, b"\0\xffdelayed");
            }
        });
    }

    #[test]
    fn named_output_client_close_preserves_queued_binary_bytes() {
        bounded(|| {
            for _ in 0..32 {
                let (mut parent, mut peer) = pair(&private_name().unwrap(), true).unwrap();
                peer.write_all(b"\0\xffoutput").unwrap();
                drop(peer);
                let mut bytes = Vec::new();
                parent.read_to_end(&mut bytes).unwrap();
                assert_eq!(bytes, b"\0\xffoutput");
            }
        });
    }

    #[test]
    fn occupied_single_instance_open_returns_busy_without_waiting() {
        bounded(|| {
            let path = private_name().unwrap();
            let _listener = PipeListenerOptions::new()
                .path(path.as_str())
                .instance_limit(std::num::NonZeroU8::new(1))
                .create_duplex::<pipe_mode::Bytes>()
                .unwrap();
            let _peer = OpenOptions::new().read(true).open(&path).unwrap();
            match Pipe::open(&path, false) {
                Ok(_) => panic!("opened an occupied pipe"),
                Err(error) => assert_eq!(error_code(&error), Some(ERROR_PIPE_BUSY)),
            }
        });
    }

    #[test]
    fn classification_is_handle_based_and_upgrade_never_falls_back() {
        bounded(|| {
            let path = private_name().unwrap();
            let listener = PipeListenerOptions::new()
                .path(path.as_str())
                .create_duplex::<pipe_mode::Bytes>()
                .unwrap();
            let reader = OpenOptions::new()
                .access_mode(GENERIC_READ | FILE_WRITE_ATTRIBUTES)
                .open(&path)
                .unwrap();
            assert!(is_pipe(&reader).unwrap());
            let mut reader = Pipe::from_file(reader).unwrap();
            assert_eq!(
                reader.read(&mut [0]).unwrap_err().kind(),
                io::ErrorKind::WouldBlock
            );
            drop(reader);
            drop(listener);

            let path = private_name().unwrap();
            let _listener = PipeListenerOptions::new()
                .path(path.as_str())
                .create_duplex::<pipe_mode::Bytes>()
                .unwrap();
            let reader = File::open(&path).unwrap();
            assert!(is_pipe(&reader).unwrap());
            match Pipe::from_file(reader) {
                // Safe conversion may reopen a handle with sufficient rights.
                // Either it polls or it returns the ACL error; it never blocks.
                Ok(mut reader) => assert_eq!(
                    reader.read(&mut [0]).unwrap_err().kind(),
                    io::ErrorKind::WouldBlock
                ),
                Err(error) => assert_eq!(error.kind(), io::ErrorKind::PermissionDenied),
            }

            let ordinary = File::open(std::env::current_exe().unwrap()).unwrap();
            assert!(!is_pipe(&ordinary).unwrap());
        });
    }
}
