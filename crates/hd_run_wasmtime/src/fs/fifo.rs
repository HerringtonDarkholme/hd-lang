use std::fs;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

use super::test_support::TempDir;

#[cfg(unix)]
fn fifo(dir: &TempDir) -> String {
    let path = dir.path("fifo");
    nix::unistd::mkfifo(
        std::path::Path::new(&path),
        nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
    )
    .unwrap();
    path
}

#[cfg(unix)]
fn fifo_reader(path: &str) -> fs::File {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .unwrap()
}

#[cfg(unix)]
fn fifo_peer_writer(path: &str, deadline: Instant) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    loop {
        if Instant::now() >= deadline {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        match fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
        {
            Err(error)
                if error.raw_os_error() == Some(libc::ENXIO)
                    || error.kind() == std::io::ErrorKind::Interrupted =>
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            result => return result,
        }
    }
}

#[cfg(unix)]
fn fifo_peer_write(path: &str, mut bytes: &[u8], deadline: Instant) -> std::io::Result<()> {
    let mut writer = fifo_peer_writer(path, deadline)?;
    while !bytes.is_empty() {
        if Instant::now() >= deadline {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        match writer.write(bytes) {
            Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
            Ok(n) => bytes = &bytes[n..],
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn fifo_peer_read(
    path: &str,
    reader_ready: &std::sync::mpsc::Sender<()>,
    writer_done: &std::sync::mpsc::Receiver<()>,
    deadline: Instant,
) -> std::io::Result<Vec<u8>> {
    let mut reader = fifo_reader(path);
    reader_ready
        .send(())
        .map_err(|_| std::io::ErrorKind::BrokenPipe)?;
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8 * 1024];
    let mut finished = false;
    loop {
        if Instant::now() >= deadline {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        finished |= writer_done.try_recv().is_ok();
        match reader.read(&mut buffer) {
            Ok(0) if finished => return Ok(bytes),
            // No writer yet is not EOF: the main thread signals only
            // after its write call has returned and closed the writer.
            Ok(0) => std::thread::sleep(Duration::from_millis(5)),
            Ok(n) => bytes.extend_from_slice(&buffer[..n]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fifo, fifo_peer_read, fifo_peer_write, fifo_peer_writer, fifo_reader};
    use crate::codec::Enc;
    use crate::fs::test_support::{TempDir, assert_time_limit, limited};
    use std::fs;
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};

    #[cfg(unix)]
    #[test]
    fn fifo_without_peer_times_out_in_read_and_write_open() {
        let dir = TempDir::new();
        let path = fifo(&dir);
        for (method, payload) in [("read_bytes", None), ("append_text", Some(&b"hello"[..]))] {
            let start = Instant::now();
            assert_time_limit(limited(
                method,
                &path,
                payload,
                start + Duration::from_millis(60),
            ));
            assert!(start.elapsed() < Duration::from_secs(2));
        }
    }
    #[cfg(unix)]
    #[test]
    fn fifo_connected_but_idle_writer_does_not_block_past_deadline() {
        use std::os::unix::fs::OpenOptionsExt;
        let dir = TempDir::new();
        let path = fifo(&dir);
        let _reader = fifo_reader(&path);
        let _writer = fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
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
    #[cfg(unix)]
    #[test]
    fn fifo_backpressure_times_out_and_closes_writer_without_background_mutation() {
        let dir = TempDir::new();
        let path = fifo(&dir);
        let mut reader = fifo_reader(&path);
        // Fill before arming the deadline. Backpressure must not depend on
        // the tested writer making progress during its short wall budget.
        let mut setup_writer =
            fifo_peer_writer(&path, Instant::now() + Duration::from_secs(2)).unwrap();
        let fill_until = Instant::now() + Duration::from_secs(2);
        let marker = [b'p'; 8 * 1024];
        let mut filled = 0;
        loop {
            assert!(
                Instant::now() < fill_until,
                "fixture pipe reaches backpressure"
            );
            match setup_writer.write(&marker) {
                Ok(0) => panic!("fixture pipe stopped accepting bytes"),
                Ok(n) => filled += n,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("prefilling fixture pipe: {error}"),
            }
        }
        drop(setup_writer);
        assert!(filled > 0);
        let payload = vec![b'x'; 4 * 1024 * 1024];
        let start = Instant::now();
        assert_time_limit(limited(
            "write_bytes",
            &path,
            Some(&payload),
            start + Duration::from_millis(60),
        ));
        assert!(start.elapsed() < Duration::from_secs(2));
        let mut buffer = [0u8; 8 * 1024];
        let mut written = 0;
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break, // The operation closed its only writer on timeout.
                Ok(n) => {
                    assert!(buffer[..n].iter().all(|byte| *byte == b'p'));
                    written += n;
                }
                Err(err) => panic!("reader should reach EOF, not retain a writer: {err}"),
            }
        }
        assert_eq!(written, filled, "no tested or background writer adds bytes");
    }
    #[cfg(unix)]
    #[test]
    fn fifo_delayed_writer_and_ready_reader_preserve_successful_transfers() {
        let dir = TempDir::new();
        let path = fifo(&dir);
        let writer_path = path.clone();
        let (sent, written) = std::sync::mpsc::channel();
        let _writer = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            std::thread::sleep(Duration::from_millis(20));
            let result = fifo_peer_write(&writer_path, b"hello", deadline);
            let _ = sent.send(result);
        });
        let mut expected = Enc::default();
        expected.leb(0).bytes(b"hello");
        let actual = limited(
            "read_bytes",
            &path,
            None,
            Instant::now() + Duration::from_secs(2),
        );
        written
            .recv_timeout(Duration::from_secs(3))
            .expect("delayed writer did not finish")
            .expect("delayed writer failed");
        assert_eq!(actual.unwrap().done(), expected.done());

        let reader_path = path.clone();
        let (reader_ready, ready) = std::sync::mpsc::channel();
        let (writer_done, finished) = std::sync::mpsc::channel();
        let (received, read) = std::sync::mpsc::channel();
        let _reader = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            std::thread::sleep(Duration::from_millis(20));
            let result = fifo_peer_read(&reader_path, &reader_ready, &finished, deadline);
            let _ = received.send(result);
        });
        // This direction tests a connected append, not an overlapping peer
        // open. Keep that lifecycle explicit before the short-lived writer.
        ready
            .recv_timeout(Duration::from_secs(3))
            .expect("reader did not become ready");
        let actual = limited(
            "append_text",
            &path,
            Some(b"world"),
            Instant::now() + Duration::from_secs(2),
        );
        writer_done.send(()).expect("delayed reader stopped early");
        let bytes = read
            .recv_timeout(Duration::from_secs(3))
            .expect("delayed reader did not finish")
            .expect("delayed reader failed");
        assert_eq!(actual.unwrap().done(), &[0]);
        assert_eq!(bytes, b"world");
    }
    #[cfg(unix)]
    #[test]
    fn fifo_empty_writer_is_eof_not_no_peer_timeout() {
        let dir = TempDir::new();
        let path = fifo(&dir);
        let writer_path = path.clone();
        let (sent, closed) = std::sync::mpsc::channel();
        let _writer = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            std::thread::sleep(Duration::from_millis(20));
            let result = fifo_peer_writer(&writer_path, deadline).map(drop);
            let _ = sent.send(result);
        });
        let actual = limited(
            "read_bytes",
            &path,
            None,
            Instant::now() + Duration::from_secs(2),
        );
        closed
            .recv_timeout(Duration::from_secs(3))
            .expect("empty writer did not finish")
            .expect("empty writer failed");
        assert_eq!(actual.unwrap().done(), &[0, 0]);
    }
}
