//! `Http.send!` (wasmtime-host-audit.md §4.3): `.start` decodes the
//! request and checks its URL and grant on the program thread, and a
//! refusal, or a timeout of zero or less, returns at once. Otherwise the
//! request runs as one task on the `hd-io` runtime, which sends it on the
//! runtime's blocking pool (ureq blocks) and delivers the response or the
//! failure for `.finish` to encode.
//!
//! Cancelling the operation drops the task, which shuts down the socket
//! of the request's connection through its cancel slot
//! (`connector::Cancel`): the blocked request ends at once, and the server
//! reads the end of the connection (`std-http.send.cancel.close`).

use std::time::Instant;

use hd_run::Grants;
use url::Url;

use crate::codec::Enc;
use crate::host::Stop;

use super::connector::Cancel;
use super::request::{Failure, Request, Response, encode, send};

/// What `.start` decides.
pub(crate) enum Start {
    /// The encoded result of a request that is not sent.
    Ready(Enc),
    /// A request to send.
    Run(Job),
}

/// A request to send, with the grants each redirect is checked against
/// and the end of its timeout, which covers every redirect
/// (`std-http.send.timeout.whole`); `None` when that end is too far away
/// to represent.
pub(crate) struct Job {
    request: Request,
    grants: Grants,
    deadline: Option<Instant>,
}

/// What the task delivers: the response or the failure, or why the
/// request stopped with neither (its blocking thread panicked).
pub(crate) type Sent = Result<Result<Response, Failure>, String>;

/// `send.start`'s checks, in the Node host's order: the URL and its
/// grant, then the timeout.
pub(crate) fn start(bytes: &[u8], grants: &Grants) -> wasmtime::Result<Start> {
    let started = Instant::now();
    let request = Request::decode(bytes)?;
    if let Err(failure) = granted_url(&request.url, grants) {
        return Ok(Start::Ready(encode(Err(failure))));
    }
    let timeout = request.timeout();
    if timeout.is_zero() {
        return Ok(Start::Ready(encode(Err(Failure::Timeout))));
    }
    Ok(Start::Run(Job {
        request,
        grants: grants.clone(),
        deadline: started.checked_add(timeout),
    }))
}

/// The encoded result of a start that cannot run its task.
pub(crate) fn not_run(error: &str) -> Enc {
    encode(Err(Failure::Other(error.into())))
}

impl Job {
    /// The task: sends the request on the blocking pool. Dropping it before
    /// the request ends cancels the request.
    pub(crate) async fn run(self) -> Sent {
        let cancel = Cancel::default();
        let _abort = CancelOnDrop(cancel.clone());
        let Self {
            request,
            grants,
            deadline,
        } = self;
        tokio::task::spawn_blocking(move || send(request, &grants, deadline, &cancel))
            .await
            .map_err(|error| format!("the Http request stopped: {error}"))
    }
}

/// Cancels the request when the task's work is dropped: at its abort, or
/// harmlessly once the request has ended.
struct CancelOnDrop(Cancel);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// `send.finish`'s result.
pub(crate) fn finish(sent: Sent) -> wasmtime::Result<Enc> {
    sent.map(encode)
        .map_err(|error| Stop::Internal(error).into())
}

pub(crate) fn granted_url(value: &str, grants: &Grants) -> Result<Url, Failure> {
    let url = Url::parse(value).map_err(|_| Failure::InvalidUrl(value.into()))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(Failure::InvalidUrl(value.into()));
    }
    let host = url.host_str().unwrap_or_default().trim_matches(['[', ']']);
    if !grants.covers_host("Http", host, url.port_or_known_default()) {
        return Err(Failure::NotGranted(host.into()));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::{Job, Start, start};
    use crate::http::connector::Cancel;
    use crate::http::request::{Request, outgoing, send as send_blocking};
    use crate::http::test_support::{
        bounded_accept, finished, offload, read_request, request, response, send, server,
        write_response,
    };
    use crate::reactor::Reactor;
    use hd_run::Grants;
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use url::Url;

    /// A started request that will be sent.
    fn job(bytes: &[u8]) -> Job {
        match start(bytes, &Grants::default()).unwrap() {
            Start::Run(job) => job,
            Start::Ready(e) => panic!("the request is not sent: {:?}", e.done()),
        }
    }

    #[test]
    fn the_request_timeout_covers_waiting_for_the_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (release, receive) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let mut stream = bounded_accept(&listener);
            read_request(&mut stream);
            let _ = receive.recv_timeout(Duration::from_secs(2));
        });
        let r = request("GET", &format!("http://{address}/"), &vec![], &[], 200);
        let started = Instant::now();
        let encoded = send(r.done(), &Grants::default());
        let elapsed = started.elapsed();
        let _ = release.send(());
        server.join().unwrap();
        assert_eq!(encoded.done(), [1, 5]);
        assert!(elapsed < Duration::from_secs(1));
    }

    #[test]
    fn the_request_timeout_covers_the_tls_handshake() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (release, released) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let mut stream = bounded_accept(&listener);
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut first = [0; 1];
            assert_eq!(stream.read(&mut first).unwrap(), 1);
            let _ = released.recv_timeout(Duration::from_secs(2));
        });
        let r = request("GET", &format!("https://{address}/"), &vec![], &[], 300);
        let started = Instant::now();
        let encoded = send(r.done(), &Grants::default());
        let elapsed = started.elapsed();
        let _ = release.send(());
        server.join().unwrap();
        assert_eq!(encoded.done(), [1, 5]);
        assert!(elapsed < Duration::from_secs(1));
    }

    /// `std-http.send.cancel.close`: an abort while the request waits for
    /// the response headers, in the middle of the body, or in the TLS
    /// handshake returns at once, and the server reads the end of the
    /// connection. The request's own timeout is 30 s, so only the abort can
    /// end it.
    #[test]
    fn an_abort_closes_the_connection_in_every_phase() {
        for (tls, partial_body) in [(false, false), (false, true), (true, false)] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let (ready, arrived) = mpsc::channel();
            let server = std::thread::spawn(move || {
                let mut stream = bounded_accept(&listener);
                if tls {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    assert_eq!(stream.read(&mut [0; 1]).unwrap(), 1);
                } else {
                    read_request(&mut stream);
                    if partial_body {
                        write_response(
                            &mut stream,
                            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nx",
                        )
                        .unwrap();
                    }
                }
                ready.send(()).unwrap();
                // Read to the end of the connection: the rest of the TLS
                // hello, then the end the abort sends.
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let started = Instant::now();
                let mut rest = Vec::new();
                let ended = stream.read_to_end(&mut rest).is_ok();
                (ended, started.elapsed())
            });
            let scheme = if tls { "https" } else { "http" };
            let r = request(
                "GET",
                &format!("{scheme}://{address}/"),
                &vec![],
                &[],
                30_000,
            );
            let mut reactor = Reactor::new(None);
            let h = offload(&mut reactor, job(r.done()));
            arrived.recv_timeout(Duration::from_secs(5)).unwrap();
            let started = Instant::now();
            assert!(reactor.remove(h).is_some());
            let aborted = started.elapsed();
            let (ended, waited) = server.join().unwrap();
            assert!(ended, "the server read the end of the connection");
            assert!(
                waited < Duration::from_secs(2),
                "the end came late: {waited:?}"
            );
            assert!(
                aborted < Duration::from_secs(1),
                "the abort waited: {aborted:?}"
            );
        }
    }

    /// A request cancelled before it connects makes no connection.
    #[test]
    fn a_cancelled_request_makes_no_connection() {
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let url = format!("http://{}/", target.local_addr().unwrap());
        let r = request("GET", &url, &vec![], &[], 1000);
        let cancel = Cancel::default();
        cancel.cancel();
        let result = send_blocking(
            Request::decode(r.done()).unwrap(),
            &Grants::default(),
            Some(Instant::now() + Duration::from_secs(1)),
            &cancel,
        );
        assert!(result.is_err());
        assert_eq!(
            target.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    /// `std-host.op.wait`: two requests to a server that answers only once
    /// both have arrived complete; one after the other, the first would
    /// never be answered.
    #[test]
    fn two_requests_wait_at_the_same_time() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut streams = Vec::new();
            for _ in 0..2 {
                let mut stream = bounded_accept(&listener);
                read_request(&mut stream);
                streams.push(stream);
            }
            for mut stream in streams {
                write_response(&mut stream, b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                    .unwrap();
            }
        });
        let r = request("GET", &format!("http://{address}/"), &vec![], &[], 10_000);
        let mut reactor = Reactor::new(None);
        let first = offload(&mut reactor, job(r.done()));
        let second = offload(&mut reactor, job(r.done()));
        for h in [first, second] {
            let sent = finished(&mut reactor, h).unwrap().unwrap();
            assert_eq!(response(super::encode(Ok(sent)).done()).0, 200);
        }
        server.join().unwrap();
    }

    #[test]
    fn client_keeps_status_raw_headers_bytes_and_does_not_decode_gzip() {
        let (url, server) = server(b"HTTP/1.1 103 Early Hints\r\nLink: ignored\r\n\r\nHTTP/1.1 500 Error\r\nX-A: first\r\nx-B: \xe9\r\nX-A: second\r\nContent-Encoding: gzip\r\nContent-Length: 3\r\n\r\n\xff\0z".to_vec());
        let r = request("GET", &url, &vec![], &[], 1000);
        let encoded = send(r.done(), &Grants::default());
        let (status, headers, body) = response(encoded.done());
        assert_eq!(status, 500);
        assert_eq!(body, b"\xff\0z");
        assert_eq!(
            headers,
            [
                ("X-A".into(), "first".into()),
                ("x-B".into(), "é".into()),
                ("X-A".into(), "second".into()),
                ("Content-Encoding".into(), "gzip".into()),
                ("Content-Length".into(), "3".into())
            ]
        );
        let wire = server.join().unwrap();
        assert_eq!(
            wire,
            outgoing(&Url::parse(&url).unwrap(), "GET", &vec![], &[]).unwrap()
        );
    }

    #[test]
    fn get_head_and_custom_methods_send_the_original_body() {
        for method in ["GET", "HEAD", "PROPFIND"] {
            let (url, server) = server(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec());
            let headers = vec![
                ("X-One".into(), "one".into()),
                ("X-Two".into(), "two".into()),
                ("X-One".into(), "three".into()),
            ];
            let r = request(method, &url, &headers, b"request-body", 1000);
            let encoded = send(r.done(), &Grants::default());
            assert_eq!(response(encoded.done()).0, 200);
            assert_eq!(
                server.join().unwrap(),
                outgoing(
                    &Url::parse(&url).unwrap(),
                    method,
                    &headers,
                    b"request-body"
                )
                .unwrap()
            );
        }
    }
}
