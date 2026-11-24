mod tests {
    use hd_run_wasmtime::internals::host::output::*;
    use std::io::Write;
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
