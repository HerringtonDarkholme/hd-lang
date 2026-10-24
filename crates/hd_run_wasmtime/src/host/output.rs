//! One ordered writer owns both output streams. Polling only queues work;
//! a full pipe cannot stop a timer or another host operation. Barriers at
//! run boundaries drain accepted bytes, including output preceding a panic.
use std::io::Write;
use std::sync::mpsc::{Sender, channel};

use crate::runtime::reactor::Stream;

type Streams = (Box<dyn Write + Send>, Box<dyn Write + Send>);
enum Message {
    Write(Stream, Vec<u8>, Option<Box<dyn FnOnce() + Send>>),
    Drain(Sender<()>),
}

pub(super) struct Output {
    streams: Option<Streams>,
    sender: Option<Sender<Message>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Output {
    pub(super) fn new(out: Box<dyn Write + Send>, err: Box<dyn Write + Send>) -> Self {
        Self {
            streams: Some((out, err)),
            sender: None,
            thread: None,
        }
    }

    pub(super) fn write(
        &mut self,
        to: Stream,
        bytes: Vec<u8>,
        done: Option<Box<dyn FnOnce() + Send>>,
    ) {
        if self.sender.is_none() {
            let (mut out, mut err) = self.streams.take().expect("one output writer");
            let (send, recv) = channel();
            self.thread = Some(std::thread::spawn(move || {
                while let Ok(message) = recv.recv() {
                    match message {
                        Message::Write(to, bytes, done) => {
                            let stream = match to {
                                Stream::Out => &mut out,
                                Stream::Err => &mut err,
                            };
                            // Preserve the host's existing broken-stream policy.
                            let _ = stream.write_all(&bytes);
                            let _ = stream.flush();
                            if let Some(done) = done {
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
        if let Err(error) = self
            .sender
            .as_ref()
            .unwrap()
            .send(Message::Write(to, bytes, done))
            && let Message::Write(_, _, Some(done)) = error.0
        {
            done();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    struct Writer {
        tag: u8,
        bytes: Arc<Mutex<Vec<u8>>>,
        release: Option<std::sync::mpsc::Receiver<()>>,
    }
    impl Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if let Some(release) = self.release.take() {
                release.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            let mut output = self.bytes.lock().unwrap();
            output.push(self.tag);
            output.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn blocked_writer_keeps_timer_ready_and_drains_before_error_output() {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let (release, held) = channel();
        let mut output = Output::new(
            Box::new(Writer {
                tag: 1,
                bytes: bytes.clone(),
                release: Some(held),
            }),
            Box::new(Writer {
                tag: 2,
                bytes: bytes.clone(),
                release: None,
            }),
        );
        let mut reactor = crate::runtime::reactor::Reactor::new(None);
        let (handle, done) = reactor.output();
        output.write(Stream::Out, b"accepted".to_vec(), Some(Box::new(done)));
        let timer = reactor.sleep(0);
        assert_eq!(reactor.due(), [timer]);
        // Cancel the wait, preserving already accepted output. A panic's
        // bytes queued later cannot overtake it, even on another stream.
        reactor.remove(handle);
        output.write(Stream::Err, b"panic".to_vec(), None);
        assert!(bytes.lock().unwrap().is_empty());
        release.send(()).unwrap();
        output.drain();
        assert_eq!(*bytes.lock().unwrap(), b"\x01accepted\x02panic");
        assert!(
            reactor.due().is_empty(),
            "an aborted write cannot wake a new operation"
        );
    }
}
