//! One ordered writer owns both output streams. A full pipe cannot stop a
//! timer or another host operation. Barriers at run boundaries drain
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

use crate::runtime::reactor::{OutputWait, Stream};

type Completion = (OutputWait, Box<dyn FnOnce() + Send>);

type Streams = (Box<dyn Write + Send>, Box<dyn Write + Send>);
enum Message {
    Write(Stream, Vec<u8>, Option<Completion>),
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

    /// Queues `bytes` for `to` after everything already accepted. A write
    /// with no completion goes straight to the stream when it can
    /// ([`Output::write_now`]); one with a completion waits for its start.
    pub(super) fn write(&mut self, to: Stream, bytes: Vec<u8>, done: Option<Completion>) {
        let bytes = match done {
            None => match self.write_now(to, bytes) {
                None => return,
                Some(rest) => rest,
            },
            Some(_) => bytes,
        };
        if self.sender.is_none() {
            let streams = Arc::clone(&self.streams);
            let queued = Arc::clone(&self.queued);
            let (send, recv) = channel();
            self.thread = Some(std::thread::spawn(move || {
                while let Ok(message) = recv.recv() {
                    match message {
                        Message::Write(to, bytes, done) => {
                            if let Some((wait, _)) = &done {
                                wait.wait();
                            }
                            let mut streams =
                                streams.lock().unwrap_or_else(PoisonError::into_inner);
                            let stream = match to {
                                Stream::Out => &mut streams.0,
                                Stream::Err => &mut streams.1,
                            };
                            let cancelled =
                                || done.as_ref().is_some_and(|(wait, _)| wait.cancelled());
                            let mut rest = bytes.as_slice();
                            while !rest.is_empty() && !cancelled() {
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
                            if let Some((_, done)) = done {
                                done();
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
        self.queued.fetch_add(1, Ordering::SeqCst);
        if let Err(error) = self
            .sender
            .as_ref()
            .unwrap()
            .send(Message::Write(to, bytes, done))
        {
            self.queued.fetch_sub(1, Ordering::SeqCst);
            if let Message::Write(_, _, Some((_, done))) = error.0 {
                done();
            }
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
    use std::time::Duration;

    struct Writer {
        bytes: Arc<Mutex<Vec<u8>>>,
        held: Option<std::sync::mpsc::Receiver<()>>,
        partial: Option<Sender<()>>,
    }
    impl Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if let Some(signal) = self.partial.take() {
                self.bytes.lock().unwrap().extend_from_slice(&bytes[..3]);
                signal.send(()).unwrap();
                return Ok(3);
            }
            if let Some(held) = &self.held {
                if held.try_recv().is_err() {
                    return Err(std::io::ErrorKind::WouldBlock.into());
                }
                self.held = None;
            }
            self.bytes.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn blocked_writer_keeps_timer_ready_and_drops_a_cancelled_queued_chunk() {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let (release, held) = channel();
        let mut output = Output::new(
            Box::new(Writer {
                bytes: bytes.clone(),
                held: Some(held),
                partial: None,
            }),
            Box::new(Writer {
                bytes: bytes.clone(),
                held: None,
                partial: None,
            }),
        );
        let mut reactor = crate::runtime::reactor::Reactor::new(None);
        output.write(Stream::Out, b"before".to_vec(), None);
        let (handle, wait, done) = reactor.output();
        output.write(
            Stream::Out,
            b"must-not-be-written".to_vec(),
            Some((wait, Box::new(done))),
        );
        let timer = reactor.sleep(0);
        assert_eq!(reactor.due(), [timer]);
        reactor.remove(handle);
        output.write(Stream::Err, b"panic".to_vec(), None);
        assert!(bytes.lock().unwrap().is_empty());
        release.send(()).unwrap();
        output.drain();
        assert_eq!(*bytes.lock().unwrap(), b"beforepanic");
        assert!(
            reactor.due().is_empty(),
            "an aborted write cannot wake a new operation"
        );
    }

    #[test]
    fn immediate_abort_writes_nothing_and_partial_abort_drops_the_suffix() {
        for polled in [false, true] {
            let bytes = Arc::new(Mutex::new(Vec::new()));
            let (first, entered) = channel();
            let (_release, held) = channel();
            let mut output = Output::new(
                Box::new(Writer {
                    bytes: bytes.clone(),
                    held: Some(held),
                    partial: Some(first),
                }),
                Box::new(Writer {
                    bytes: bytes.clone(),
                    held: None,
                    partial: None,
                }),
            );
            let mut reactor = crate::runtime::reactor::Reactor::new(None);
            let (handle, wait, done) = reactor.output();
            output.write(
                Stream::Out,
                b"abc-must-not-be-written".to_vec(),
                Some((wait, Box::new(done))),
            );
            if polled {
                reactor.due();
                entered.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            reactor.remove(handle);
            output.write(Stream::Err, b"after".to_vec(), None);
            output.drain();
            assert_eq!(
                *bytes.lock().unwrap(),
                if polled {
                    b"abcafter".as_slice()
                } else {
                    b"after"
                }
            );
            assert!(reactor.due().is_empty());
        }
    }
}
