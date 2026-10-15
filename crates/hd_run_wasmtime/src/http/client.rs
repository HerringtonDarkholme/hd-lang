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
//!
//! The request body and the response body count against the program's
//! heap budget (`std-host.op.heap`): a body past it ends the request, and
//! `.finish` panics `heap-exhausted`.

use std::sync::Arc;
use std::time::Instant;

use hd_run::Grants;

use crate::codec::Enc;
use crate::host::Stop;
use crate::limits::{Ledger, Reservation};
use crate::reactor::Done;

use super::connector::Cancel;
use super::request::{Ended, Failure, Request, Response, encode, send};
use super::target::Target;

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
    /// The heap budget the response body is charged to.
    ledger: Arc<Ledger>,
    /// The heap the request body holds.
    held: Reservation,
}

/// What the task delivers: the response or the failure, or why the
/// request stopped with neither (its blocking thread panicked).
pub(crate) type Sent = Result<Result<Response, Failure>, String>;

/// `send.start`'s checks, in the Node host's order: the URL and its
/// grant, then the timeout. A request body past the heap budget panics.
pub(crate) fn start(
    bytes: &[u8],
    grants: &Grants,
    ledger: &Arc<Ledger>,
) -> wasmtime::Result<Start> {
    let started = Instant::now();
    let request = Request::decode(bytes)?;
    if let Err(failure) = granted_url(&request.url, grants) {
        return Ok(Start::Ready(encode(Err(failure))));
    }
    let timeout = request.timeout();
    if timeout.is_zero() {
        return Ok(Start::Ready(encode(Err(Failure::Timeout))));
    }
    let held = ledger.reserve(request.body_len())?;
    Ok(Start::Run(Job {
        request,
        grants: grants.clone(),
        deadline: started.checked_add(timeout),
        ledger: Arc::clone(ledger),
        held,
    }))
}

/// The encoded result of a start that cannot run its task.
pub(crate) fn not_run(error: &str) -> Enc {
    encode(Err(Failure::Other(error.into())))
}

impl Job {
    /// The task: sends the request on the blocking pool. Dropping it before
    /// the request ends cancels the request. `Done::Exhausted` when the
    /// response body would pass the heap budget.
    pub(crate) async fn run(self) -> Done {
        let cancel = Cancel::default();
        let _abort = CancelOnDrop(cancel.clone());
        let Self {
            request,
            grants,
            deadline,
            ledger,
            held,
        } = self;
        let sent = tokio::task::spawn_blocking(move || {
            let sent = send(request, &grants, deadline, &cancel, &ledger);
            drop(held);
            sent
        })
        .await;
        match sent {
            Ok(Err(Ended::HeapExhausted)) => Done::Exhausted,
            Ok(Ok(response)) => Done::Http(Ok(Ok(response))),
            Ok(Err(Ended::Failed(failure))) => Done::Http(Ok(Err(failure))),
            Err(error) => Done::Http(Err(format!("the Http request stopped: {error}"))),
        }
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

/// `send.finish`'s result. The body's heap is released first.
pub(crate) fn finish(sent: Sent) -> wasmtime::Result<Enc> {
    let response = sent.map_err(Stop::Internal)?;
    Ok(encode(response.map(Response::released)))
}

pub(crate) fn granted_url(value: &str, grants: &Grants) -> Result<Target, Failure> {
    let url = Target::parse(value).ok_or_else(|| Failure::InvalidUrl(value.into()))?;
    if !grants.covers_host("Http", &url.host, Some(url.port)) {
        return Err(Failure::NotGranted(url.host));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::{Job, Start, finish, start};
    use crate::http::connector::Cancel;
    use crate::http::request::{Request, send as send_blocking};
    use crate::http::test_support::{
        bounded_accept, delivered, finished, offload, read_request, request, response, send,
        server, unlimited, write_response,
    };
    use crate::limits::Ledger;
    use crate::reactor::Done;
    use crate::reactor::Reactor;
    use hd_run::Grants;
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// `std-host.op.heap`: the request body and the response body count
    /// against the heap budget. A response body past it ends the request
    /// with `heap-exhausted`, and one within it is released at `.finish`.
    #[test]
    fn bodies_count_against_the_heap() {
        let ledger = Ledger::new(1000);
        let r = request("POST", "http://127.0.0.1:1/", &vec![], &[0; 1001], 1000);
        let error = start(r.done(), &Grants::default(), &ledger)
            .err()
            .expect("the request body passes the budget");
        assert!(crate::limits::is_heap_exhausted(&error));
        for (length, exhausted) in [(1001, true), (1000, false)] {
            let mut raw =
                format!("HTTP/1.1 200 OK\r\nContent-Length: {length}\r\n\r\n").into_bytes();
            raw.resize(raw.len() + length, b'x');
            let (url, server) = server(raw);
            let r = request("GET", &url, &vec![], &[], 5000);
            let Start::Run(job) = start(r.done(), &Grants::default(), &ledger).unwrap() else {
                panic!("the request is sent");
            };
            let mut reactor = Reactor::new(None);
            let h = offload(&mut reactor, job);
            let done = delivered(&mut reactor, h);
            let _ = server.join();
            if exhausted {
                assert!(matches!(done, Done::Exhausted));
            } else {
                assert_eq!(ledger.charged(), 1000);
                let Done::Http(sent) = done else {
                    panic!("the request has a response");
                };
                assert_eq!(response(finish(sent).unwrap().done()).2.len(), 1000);
            }
            assert_eq!(ledger.charged(), 0);
        }
    }

    /// A started request that will be sent.
    fn job(bytes: &[u8]) -> Job {
        match start(bytes, &Grants::default(), &unlimited()).unwrap() {
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
                // Windows resets a connection whose receiving side shuts
                // down with the response still unread, as it may be in the
                // middle of the body. A reset closes it too.
                let started = Instant::now();
                let mut rest = Vec::new();
                let ended = match stream.read_to_end(&mut rest) {
                    Ok(_) => true,
                    Err(error) => matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                    ),
                };
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
            &unlimited(),
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

    /// `std-http.headers.case`, `.order` and `.pairs`: names arrive in
    /// lowercase, and the values of one name keep their order.
    #[test]
    fn client_keeps_status_headers_bytes_and_does_not_decode_gzip() {
        let (url, server) = server(b"HTTP/1.1 500 Error\r\nX-A: first\r\nx-B: \xe9\r\nX-A: second\r\nContent-Encoding: gzip\r\nContent-Length: 3\r\n\r\n\xff\0z".to_vec());
        let r = request("GET", &url, &vec![], &[], 1000);
        let encoded = send(r.done(), &Grants::default());
        let (status, headers, body) = response(encoded.done());
        assert_eq!(status, 500);
        assert_eq!(body, b"\xff\0z");
        let values = |name: &str| {
            headers
                .iter()
                .filter(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(values("x-a"), ["first", "second"]);
        assert_eq!(values("x-b"), ["é"]);
        assert_eq!(values("content-encoding"), ["gzip"]);
        assert_eq!(values("content-length"), ["3"]);
        assert_eq!(headers.len(), 5);
        assert!(headers.iter().all(|(n, _)| *n == n.to_ascii_lowercase()));
        let wire = String::from_utf8(server.join().unwrap()).unwrap();
        assert!(wire.starts_with("GET /path?q=1 HTTP/1.1\r\n"), "{wire}");
        assert!(wire.contains("connection: close\r\n"), "{wire}");
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
            let wire = String::from_utf8(server.join().unwrap()).unwrap();
            assert!(
                wire.starts_with(&format!("{method} /path?q=1 HTTP/1.1\r\n")),
                "{wire}"
            );
            assert!(wire.contains("x-one: one\r\n"), "{wire}");
            assert!(wire.contains("x-one: three\r\n"), "{wire}");
            assert!(
                wire.find("x-one: one").unwrap() < wire.find("x-one: three").unwrap(),
                "{wire}"
            );
            assert!(wire.ends_with("\r\n\r\nrequest-body"), "{wire}");
        }
    }

    /// A response with a malformed header is a connect failure, whose
    /// message is the provider's own (`std-http.error.connect`).
    #[test]
    fn a_malformed_response_header_is_a_connect_error() {
        let (url, server) =
            server(b"HTTP/1.1 200 OK\r\nBad Header: value\r\nContent-Length: 0\r\n\r\n".to_vec());
        let r = request("GET", &url, &vec![], &[], 1000);
        let encoded = send(r.done(), &Grants::default());
        let mut d = crate::codec::Dec::new(encoded.done());
        assert_eq!(d.leb().unwrap(), 1);
        assert_eq!(d.leb().unwrap(), 3);
        server.join().unwrap();
    }
}
