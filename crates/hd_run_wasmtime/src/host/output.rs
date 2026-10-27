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
    Write(Stream, Vec<u8>),
    Drain(Sender<()>),
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
    /// has nothing queued: `None` once every byte is written, or the stream
    /// failed (the thread's broken-stream policy drops the rest too).
    /// Otherwise the bytes not written, which the caller hands to
    /// [`Output::write`].
    pub(super) fn write_now(&mut self, to: Stream, bytes: Vec<u8>) -> Option<Vec<u8>> {
        if !cfg!(unix) || self.queued.load(Ordering::SeqCst) != 0 {
            return Some(bytes);
        }
        let mut streams = self.streams.lock().unwrap_or_else(PoisonError::into_inner);
        let stream = match to {
            Stream::Out => &mut streams.0,
            Stream::Err => &mut streams.1,
        };
        let mut at = 0;
        while at < bytes.len() {
            match stream.write(&bytes[at..bytes.len().min(at + CHUNK)]) {
                Ok(0) => break,
                Ok(n) => at += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    at = bytes.len();
                }
            }
        }
        let _ = stream.flush();
        if at == bytes.len() {
            return None;
        }
        Some(if at == 0 { bytes } else { bytes[at..].to_vec() })
    }

    /// Queues `bytes` for `to` after everything already accepted. It goes
    /// straight to the stream when it can ([`Output::write_now`]).
    pub(super) fn write(&mut self, to: Stream, bytes: Vec<u8>) {
        let Some(bytes) = self.write_now(to, bytes) else {
            return;
        };
        if self.sender.is_none() {
            let streams = Arc::clone(&self.streams);
            let queued = Arc::clone(&self.queued);
            let (send, recv) = channel();
            self.thread = Some(std::thread::spawn(move || {
                while let Ok(message) = recv.recv() {
                    match message {
                        Message::Write(to, bytes) => {
                            let mut streams =
                                streams.lock().unwrap_or_else(PoisonError::into_inner);
                            let stream = match to {
                                Stream::Out => &mut streams.0,
                                Stream::Err => &mut streams.1,
                            };
                            let mut rest = bytes.as_slice();
                            while !rest.is_empty() {
                                match stream.write(&rest[..rest.len().min(CHUNK)]) {
                                    Ok(0) => break,
                                    Ok(n) => rest = &rest[n..],
                                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                        std::thread::sleep(std::time::Duration::from_millis(1));
                                    }
                                    // Preserve the host's existing broken-stream policy.
                                    Err(_) => break,
                                }
                            }
                            let _ = stream.flush();
                            drop(streams);
                            queued.fetch_sub(1, Ordering::SeqCst);
                        }
                        Message::Drain(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            }));
            self.sender = Some(send);
        }
        self.queued.fetch_add(1, Ordering::SeqCst);
        if self
            .sender
            .as_ref()
            .unwrap()
            .send(Message::Write(to, bytes))
            .is_err()
        {
            self.queued.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// Writes `bytes` to `to` after everything already accepted, and returns
    /// once the stream has taken every byte: the caller's thread may block.
    pub(super) fn write_blocking(&mut self, to: Stream, bytes: Vec<u8>) {
        self.write(to, bytes);
        if self.queued.load(Ordering::SeqCst) != 0 {
            self.drain();
        }
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
        output.write_blocking(Stream::Out, b"first ".to_vec());
        assert_eq!(*bytes.lock().unwrap(), b"first ");
        output.write_blocking(Stream::Err, b"second".to_vec());
        assert_eq!(*bytes.lock().unwrap(), b"first second");
    }
}
