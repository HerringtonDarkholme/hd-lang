use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, LazyBuffers, NextTimeout, Transport,
};

use super::Headers;
use super::chunk::ChunkScan;
use super::headers::{HeaderScan, framing, header_end, response_pairs};

pub(crate) struct RawConnector {
    pub(crate) raw: Vec<u8>,
    pub(crate) captured: Arc<Mutex<Option<Headers>>>,
    pub(crate) head: bool,
}

impl std::fmt::Debug for RawConnector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawConnector").finish_non_exhaustive()
    }
}

impl<In: Transport> Connector<In> for RawConnector {
    type Out = RawTransport;

    fn connect(
        &self,
        details: &ConnectionDetails<'_>,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        Ok(chained.map(|inner| RawTransport {
            inner: Box::new(inner),
            buffers: LazyBuffers::new(
                details.config.input_buffer_size(),
                details.config.output_buffer_size(),
            ),
            raw: self.raw.clone(),
            sent_headers: Vec::new(),
            sent: false,
            response_headers: Vec::new(),
            scan: HeaderScan::default(),
            received_final_headers: false,
            head: self.head,
            chunks: None,
            pending: VecDeque::new(),
            captured: self.captured.clone(),
        }))
    }
}

pub(crate) struct RawTransport {
    inner: Box<dyn Transport>,
    buffers: LazyBuffers,
    raw: Vec<u8>,
    sent_headers: Vec<u8>,
    sent: bool,
    response_headers: Vec<u8>,
    scan: HeaderScan,
    received_final_headers: bool,
    head: bool,
    chunks: Option<ChunkScan>,
    pending: VecDeque<u8>,
    captured: Arc<Mutex<Option<Headers>>>,
}

impl std::fmt::Debug for RawTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Do not render URL credentials, headers, or body bytes in logs.
        f.debug_struct("RawTransport").finish_non_exhaustive()
    }
}

impl RawTransport {
    fn transmit(&mut self, bytes: &[u8], timeout: NextTimeout) -> Result<(), ureq::Error> {
        let started = Instant::now();
        let mut remaining = bytes;
        while !remaining.is_empty() {
            let mut timeout = timeout;
            if let ureq::unversioned::transport::time::Duration::Exact(duration) = timeout.after {
                let after = duration.saturating_sub(started.elapsed());
                if after.is_zero() {
                    return Err(ureq::Error::Timeout(timeout.reason));
                }
                timeout.after = after.into();
            }
            let output = self.inner.buffers().output();
            let amount = remaining.len().min(output.len());
            output[..amount].copy_from_slice(&remaining[..amount]);
            self.inner.transmit_output(amount, timeout)?;
            remaining = &remaining[amount..];
        }
        Ok(())
    }

    fn receive(&mut self, bytes: &[u8]) -> Result<(), ureq::Error> {
        if self.received_final_headers {
            return self.receive_body(bytes);
        }
        for (at, byte) in bytes.iter().copied().enumerate() {
            if self.scan.byte(byte)? {
                self.response_headers.push(byte);
            }
            if !self.response_headers.ends_with(b"\r\n\r\n")
                && !self.response_headers.ends_with(b"\n\n")
            {
                continue;
            }
            let (status, pairs) = response_pairs(&self.response_headers)?;
            let (length, chunked) = framing(&pairs)?;
            // Give ureq only framing metadata. Keeping every original pair
            // separately avoids HeaderMap's normalization and ureq-proto's
            // fixed 128-header parsing limit. Validate original framing
            // first, since ureq accepts CL+TE and repeated identical CL,
            // which Node refuses. The last TE coding decides chunking.
            let first_line = self
                .response_headers
                .split(|b| *b == b'\n')
                .next()
                .unwrap_or_default();
            // Node decodes HTTP/1.0 chunking, whereas ureq treats every
            // HTTP/1.0 body as close-delimited. The original line was
            // validated below; only the private framing shim changes.
            if chunked && first_line.starts_with(b"HTTP/1.0 ") {
                self.pending.extend(b"HTTP/1.1 ");
                self.pending.extend(first_line[9..].iter().copied());
            } else {
                self.pending.extend(first_line.iter().copied());
            }
            self.pending.push_back(b'\n');
            if status != 100 {
                if let Some(length) = length {
                    self.pending
                        .extend(format!("Content-Length: {length}\r\n").bytes());
                }
                if chunked {
                    self.pending.extend(b"Transfer-Encoding: chunked\r\n");
                }
            }
            self.pending.extend(b"\r\n");
            self.response_headers.clear();
            self.scan = HeaderScan::default();
            if !(100..200).contains(&status) || status == 101 {
                *self.captured.lock().map_err(|_| {
                    ureq::Error::Io(std::io::Error::other("the Http header lock is poisoned"))
                })? = Some(pairs);
                self.received_final_headers = true;
                if chunked && !self.head && !matches!(status, 101 | 204 | 304) {
                    self.chunks = Some(ChunkScan::default());
                }
                self.receive_body(&bytes[at + 1..])?;
                break;
            }
        }
        Ok(())
    }

    fn receive_body(&mut self, bytes: &[u8]) -> Result<(), ureq::Error> {
        if let Some(chunks) = &mut self.chunks {
            chunks.receive(bytes, &mut self.pending)
        } else {
            self.pending.extend(bytes);
            Ok(())
        }
    }
}

impl Transport for RawTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        if self.sent {
            return Err(ureq::Error::Io(std::io::Error::other(
                "the Http wrapper received an unexpected request body",
            )));
        }
        self.sent_headers
            .extend_from_slice(&self.buffers.output()[..amount]);
        if header_end(&self.sent_headers).is_some() {
            let raw = std::mem::take(&mut self.raw);
            self.transmit(&raw, timeout)?;
            self.sent_headers.clear();
            self.sent = true;
        }
        Ok(())
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        let started = Instant::now();
        while self.pending.is_empty() {
            let mut timeout = timeout;
            if let ureq::unversioned::transport::time::Duration::Exact(duration) = timeout.after {
                let after = duration.saturating_sub(started.elapsed());
                if after.is_zero() {
                    return Err(ureq::Error::Timeout(timeout.reason));
                }
                timeout.after = after.into();
            }
            if !self.inner.maybe_await_input(timeout)? {
                return Ok(false);
            }
            let bytes = self.inner.buffers().input().to_vec();
            self.inner.buffers().input_consume(bytes.len());
            self.receive(&bytes)?;
        }
        let output = self.buffers.input_append_buf();
        let amount = output.len().min(self.pending.len());
        for (slot, byte) in output.iter_mut().zip(self.pending.drain(..amount)) {
            *slot = byte;
        }
        self.buffers.input_appended(amount);
        Ok(amount > 0)
    }

    fn is_open(&mut self) -> bool {
        self.inner.is_open()
    }

    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}

#[cfg(test)]
mod tests {
    use crate::codec::Dec;
    use crate::http::Client;
    use crate::http::test_support::{request, server};
    use hd_run::Grants;

    #[test]
    fn incomplete_response_body_is_a_connect_error() {
        let (url, server) = server(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nabc".to_vec());
        let r = request("GET", &url, &vec![], &[], 1000);
        let encoded = Client::default()
            .send(r.done(), &Grants::default())
            .unwrap();
        let mut d = Dec::new(encoded.done());
        assert_eq!(d.leb().unwrap(), 1);
        assert_eq!(d.leb().unwrap(), 3);
        server.join().unwrap();
    }
}
