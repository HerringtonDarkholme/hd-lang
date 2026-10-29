//! One ordered writer owns both output streams. A console write blocks
//! until its stream has taken every byte. Barriers at run boundaries drain
//! accepted bytes, including output preceding a panic.
//!
//! While the writer thread has nothing queued, a write goes straight to
//! its stream on the program thread, which is nonblocking. What a short
//! write or `WouldBlock` leaves goes to the thread, and so does every
//! write while the thread has chunks queued, which keeps both streams in
//! order. Windows, where a process stream blocks, always uses the thread.
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use crate::runtime::reactor::Stream;

type Streams = (Box<dyn Write + Send>, Box<dyn Write + Send>);
enum Message {
    Write(Stream, Vec<u8>, Option<Sender<Result<(), OutputError>>>),
    Drain(Sender<()>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputError {
    Closed,
    Other(i32),
}

impl OutputError {
    fn from(error: &std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::NotConnected => Self::Closed,
            _ => Self::Other(error.raw_os_error().unwrap_or(0)),
        }
    }
}

/// The largest slice passed to one stream write.
const CHUNK: usize = 65536;

pub(super) struct Output {
    streams: Arc<Mutex<Streams>>,
    /// Chunks sent to the writer thread that it has not finished.
    queued: Arc<AtomicUsize>,
    sender: Option<Sender<Message>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Output {
    pub(super) fn new(out: Box<dyn Write + Send>, err: Box<dyn Write + Send>) -> Self {
        Self {
            streams: Arc::new(Mutex::new((out, err))),
            queued: Arc::new(AtomicUsize::new(0)),
            sender: None,
            thread: None,
        }
    }

    /// Writes `bytes` to `to` on the program thread when the writer thread
    /// has nothing queued: `Ok(None)` once every byte is written, an error
    /// when the stream fails, or `Ok(Some(rest))` for bytes the caller hands
    /// to [`Output::write`].
    pub(super) fn write_now(
        &mut self,
        to: Stream,
        bytes: Vec<u8>,
    ) -> Result<Option<Vec<u8>>, OutputError> {
        if !cfg!(unix) || self.queued.load(Ordering::SeqCst) != 0 {
            return Ok(Some(bytes));
        }
        let mut streams = self.streams.lock().unwrap_or_else(PoisonError::into_inner);
        let stream = match to {
            Stream::Out => &mut streams.0,
            Stream::Err => &mut streams.1,
        };
        let mut at = 0;
        while at < bytes.len() {
            match stream.write(&bytes[at..bytes.len().min(at + CHUNK)]) {
                Ok(0) => return Err(OutputError::Other(0)),
                Ok(n) => at += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(OutputError::from(&error)),
            }
        }
        stream.flush().map_err(|error| OutputError::from(&error))?;
        if at == bytes.len() {
            return Ok(None);
        }
        Ok(Some(if at == 0 { bytes } else { bytes[at..].to_vec() }))
    }

    fn ensure_thread(&mut self) {
        if self.sender.is_none() {
            let streams = Arc::clone(&self.streams);
            let queued = Arc::clone(&self.queued);
            let (send, recv) = channel();
            self.thread = Some(std::thread::spawn(move || {
                while let Ok(message) = recv.recv() {
                    match message {
                        Message::Write(to, bytes, done) => {
                            let mut streams =
                                streams.lock().unwrap_or_else(PoisonError::into_inner);
                            let stream = match to {
                                Stream::Out => &mut streams.0,
                                Stream::Err => &mut streams.1,
                            };
                            let mut rest = bytes.as_slice();
                            let mut result = Ok(());
                            while !rest.is_empty() {
                                match stream.write(&rest[..rest.len().min(CHUNK)]) {
                                    Ok(0) => {
                                        result = Err(OutputError::Other(0));
                                        break;
                                    }
                                    Ok(n) => rest = &rest[n..],
                                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                        std::thread::sleep(std::time::Duration::from_millis(1));
                                    }
                                    Err(error) => {
                                        result = Err(OutputError::from(&error));
                                        break;
                                    }
                                }
                            }
                            if result.is_ok()
                                && let Err(error) = stream.flush()
                            {
                                result = Err(OutputError::from(&error));
                            }
                            drop(streams);
                            queued.fetch_sub(1, Ordering::SeqCst);
                            if let Some(done) = done {
                                let _ = done.send(result);
                            }
                        }
                        Message::Drain(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            }));
            self.sender = Some(send);
        }
    }

    fn queue(&mut self, to: Stream, bytes: Vec<u8>, done: Option<Sender<Result<(), OutputError>>>) {
        self.ensure_thread();
        self.queued.fetch_add(1, Ordering::SeqCst);
        if self
            .sender
            .as_ref()
            .unwrap()
            .send(Message::Write(to, bytes, done))
            .is_err()
        {
            self.queued.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// Queues `bytes` for `to` after everything already accepted. It goes
    /// straight to the stream when it can ([`Output::write_now`]). A host
    /// diagnostic write keeps the prior policy of dropping a failed stream.
    pub(super) fn write(&mut self, to: Stream, bytes: Vec<u8>) {
        let Ok(Some(bytes)) = self.write_now(to, bytes) else {
            return;
        };
        self.queue(to, bytes, None);
    }

    /// Writes `bytes` to `to` after everything already accepted, and returns
    /// once the stream has taken every byte: the caller's thread may block.
    pub(super) fn write_blocking(&mut self, to: Stream, bytes: Vec<u8>) -> Result<(), OutputError> {
        let Some(bytes) = self.write_now(to, bytes)? else {
            return Ok(());
        };
        let (done, result) = channel();
        self.queue(to, bytes, Some(done));
        result.recv().unwrap_or(Err(OutputError::Closed))
    }

    pub(super) fn drain(&self) {
        if let Some(sender) = &self.sender {
            let (done, drained) = channel();
            if sender.send(Message::Drain(done)).is_ok() {
                let _ = drained.recv();
            }
        }
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        // Closing the queue drains accepted writes before joining.
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

// A process pipe must not hold a blocking syscall after cancellation. Use
// nonblocking writes even when stdout was inherited in blocking mode. Restore
// its flags before returning, so the embedding keeps its descriptor settings.
#[cfg(unix)]
struct ProcessStream<T>(T);

#[cfg(unix)]
impl<T: std::os::fd::AsFd + Send> Write for ProcessStream<T> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
        use std::sync::{Mutex, PoisonError};
        static FLAGS: Mutex<()> = Mutex::new(());
        let _guard = FLAGS.lock().unwrap_or_else(PoisonError::into_inner);
        let flags = fcntl_getfl(&self.0)?;
        let changed = !flags.contains(OFlags::NONBLOCK);
        if changed {
            fcntl_setfl(&self.0, flags | OFlags::NONBLOCK)?;
        }
        let result = rustix::io::write(&self.0, bytes).map_err(Into::into);
        if changed {
            // Do not replace a successful short write with an error: retrying
            // its bytes would duplicate them. The original descriptor is live.
            let _ = fcntl_setfl(&self.0, flags);
        }
        result
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(()) // Writes go directly to the descriptor, without a stdio buffer.
    }
}

pub(super) fn process_stdout() -> Box<dyn Write + Send> {
    #[cfg(unix)]
    return Box::new(ProcessStream(std::io::stdout()));
    #[cfg(not(unix))]
    Box::new(std::io::stdout())
}

pub(super) fn process_stderr() -> Box<dyn Write + Send> {
    #[cfg(unix)]
    return Box::new(ProcessStream(std::io::stderr()));
    #[cfg(not(unix))]
    Box::new(std::io::stderr())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A stream that is full for its first `full` writes.
    struct Writer {
        bytes: Arc<Mutex<Vec<u8>>>,
        full: usize,
    }

    struct Failing(std::io::ErrorKind, i32);
    impl Write for Failing {
        fn write(&mut self, _bytes: &[u8]) -> std::io::Result<usize> {
            if self.1 == 0 {
                Err(self.0.into())
            } else {
                Err(std::io::Error::from_raw_os_error(self.1))
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.full > 0 {
                self.full -= 1;
                return Err(std::io::ErrorKind::WouldBlock.into());
            }
            self.bytes.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_blocking_write_to_a_full_stream_returns_once_every_byte_is_written() {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let writer = |full| {
            Box::new(Writer {
                bytes: bytes.clone(),
                full,
            })
        };
        let mut output = Output::new(writer(3), writer(0));
        output
            .write_blocking(Stream::Out, b"first ".to_vec())
            .expect("writes");
        assert_eq!(*bytes.lock().unwrap(), b"first ");
        output
            .write_blocking(Stream::Err, b"second".to_vec())
            .expect("writes");
        assert_eq!(*bytes.lock().unwrap(), b"first second");
    }

    #[test]
    fn a_blocking_write_returns_heap_free_stream_errors() {
        let good = || Box::new(std::io::sink()) as Box<dyn Write + Send>;
        let mut closed = Output::new(Box::new(Failing(std::io::ErrorKind::BrokenPipe, 0)), good());
        assert_eq!(
            closed.write_blocking(Stream::Out, b"x".to_vec()),
            Err(OutputError::Closed)
        );
        let mut other = Output::new(Box::new(Failing(std::io::ErrorKind::Other, 5)), good());
        assert_eq!(
            other.write_blocking(Stream::Out, b"x".to_vec()),
            Err(OutputError::Other(5))
        );
    }
}
