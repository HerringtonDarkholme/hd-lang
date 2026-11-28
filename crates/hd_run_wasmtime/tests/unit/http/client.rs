mod tests {
    use crate::http::test_support::{
        bounded_accept, finished, offload, read_request, request, response, send, server,
        write_response,
    };
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use hd_run::Grants;
    use hd_run_wasmtime::internals::abi::{Duration as WireDuration, Method, WireRequest};
    use hd_run_wasmtime::internals::codec::Dec;
    use hd_run_wasmtime::internals::http::client::{Job, Start, start};
    use hd_run_wasmtime::internals::http::connector::Cancel;
    use hd_run_wasmtime::internals::http::request::{Request, send as send_blocking};
    use hd_run_wasmtime::internals::runtime::reactor::Reactor;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// A started request that will be sent.
    fn job(bytes: &[u8]) -> Job {
        match start(bytes, &Grants::default()).unwrap() {
            Start::Run(job) => job,
            Start::Ready(e) => panic!("the request is not sent: {:?}", e.done()),
        }
    }

    // Exercise the internal provider deadline without reintroducing a Request field.
    fn send_in(bytes: &[u8], budget: Duration) -> hd_run_wasmtime::internals::codec::Enc {
        let mut job = job(bytes);
        job.deadline = Instant::now().checked_add(budget);
        let mut reactor = Reactor::new(None);
        let h = offload(&mut reactor, job);
        hd_run_wasmtime::internals::http::client::finish(finished(&mut reactor, h)).unwrap()
    }

    #[test]
    fn resolver_is_refused_first_and_other_transport_settings_reach_url_checks() {
        let wire = |dns_servers, tls_roots, connect_timeout| {
            hd_run_wasmtime::internals::codec::encode(&WireRequest {
                method: Method::Get,
                url: "http://127.0.0.1:1/".into(),
                headers: vec![],
                body: vec![],
                redirects: 0,
                dns_servers,
                tls_roots,
                tls_identity: None,
                connect_timeout,
                tls_handshake_timeout: None,
                response_header_timeout: None,
                body_read_timeout: None,
            })
        };
        let Start::Ready(refused) = start(
            wire(
                Some(vec![]),
                vec!["not inspected before refusal".into()],
                Some(WireDuration { millis: 0 }),
            )
            .done(),
            &Grants::default(),
        )
        .unwrap() else {
            panic!("the resolver setting must be refused")
        };
        let mut decoded = Dec::new(refused.done());
        assert_eq!(decoded.leb().unwrap(), 1);
        assert_eq!(decoded.leb().unwrap(), 13);
        assert_eq!(decoded.str().unwrap(), "resolver");

        let deny = Grants::with_limits(vec![("Http".into(), hd_run::Limit::Deny)]);
        let Start::Ready(denied) = start(
            wire(None, vec![], Some(WireDuration { millis: 0 })).done(),
            &deny,
        )
        .unwrap() else {
            panic!("the URL denial must return before transport")
        };
        assert_eq!(&denied.done()[..2], &[1, 0]);
    }

    #[test]
    fn the_provider_deadline_covers_waiting_for_the_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (release, receive) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let mut stream = bounded_accept(&listener);
            read_request(&mut stream);
            let _ = receive.recv_timeout(Duration::from_secs(2));
        });
        let r = request("GET", &format!("http://{address}/"), &vec![], &[], 10);
        let started = Instant::now();
        let encoded = send_in(r.done(), Duration::from_millis(200));
        let elapsed = started.elapsed();
        let _ = release.send(());
        server.join().unwrap();
        assert_eq!(encoded.done(), [1, 5]);
        assert!(elapsed < Duration::from_secs(1));
    }

    #[test]
    fn the_provider_deadline_covers_the_tls_handshake() {
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
        let r = request("GET", &format!("https://{address}/"), &vec![], &[], 10);
        let started = Instant::now();
        let encoded = send_in(r.done(), Duration::from_millis(300));
        let elapsed = started.elapsed();
        let _ = release.send(());
        server.join().unwrap();
        assert_eq!(encoded.done(), [1, 5]);
        assert!(elapsed < Duration::from_secs(1));
    }

    /// `std-http.send.cancel.close`: an abort while the request waits for
    /// the response headers, in the middle of the body, or in the TLS
    /// handshake returns at once, and the server reads the end of the
    /// connection: its end of input, or a reset, which Windows sends for a
    /// connection closed with input still unread. The request's own
    /// timeout is 30 s, so only the abort can end it. A failure names the
    /// phase and what the server read.
    #[test]
    fn an_abort_closes_the_connection_in_every_phase() {
        for (phase, tls, partial_body) in [
            ("waiting for the headers", false, false),
            ("in the middle of the body", false, true),
            ("in the TLS handshake", true, false),
        ] {
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
                let ended = match stream.read_to_end(&mut Vec::new()) {
                    Ok(_) => Ok("its end of input"),
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::ConnectionReset
                                | std::io::ErrorKind::ConnectionAborted
                        ) =>
                    {
                        Ok("a reset")
                    }
                    Err(error) => Err(format!("{:?} ({error})", error.kind())),
                };
                (ended, started.elapsed())
            });
            let scheme = if tls { "https" } else { "http" };
            let r = request("GET", &format!("{scheme}://{address}/"), &vec![], &[], 10);
            let mut reactor = Reactor::new(None);
            let h = offload(&mut reactor, job(r.done()));
            arrived.recv_timeout(Duration::from_secs(5)).unwrap();
            let started = Instant::now();
            assert!(reactor.remove(h).is_some());
            let aborted = started.elapsed();
            let (ended, waited) = server.join().unwrap();
            let read = ended.unwrap_or_else(|error| {
                panic!(
                    "{phase}: the server read no end of the connection but {error} after {waited:?}; the abort took {aborted:?}"
                )
            });
            assert!(
                waited < Duration::from_secs(2),
                "{phase}: the server read {read} late, after {waited:?}"
            );
            assert!(
                aborted < Duration::from_secs(1),
                "{phase}: the abort took {aborted:?}"
            );
        }
    }

    /// A request cancelled before it connects makes no connection.
    #[test]
    fn a_cancelled_request_makes_no_connection() {
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let url = format!("http://{}/", target.local_addr().unwrap());
        let r = request("GET", &url, &vec![], &[], 10);
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
        let r = request("GET", &format!("http://{address}/"), &vec![], &[], 10);
        let mut reactor = Reactor::new(None);
        let first = offload(&mut reactor, job(r.done()));
        let second = offload(&mut reactor, job(r.done()));
        for h in [first, second] {
            let sent = finished(&mut reactor, h).unwrap().unwrap();
            assert_eq!(
                response(hd_run_wasmtime::internals::http::client::encode(Ok(sent)).done()).0,
                200
            );
        }
        server.join().unwrap();
    }

    /// `std-http.headers.case`, `.order` and `.pairs`: names arrive in
    /// lowercase, and the values of one name keep their order.
    #[test]
    fn client_decodes_gzip_and_removes_encoded_representation_headers() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(b"decoded").unwrap();
        let compressed = encoder.finish().unwrap();
        let mut wire = b"HTTP/1.1 500 Error\r\nX-A: first\r\nx-B: ".to_vec();
        wire.push(0xe9);
        wire.extend(
            format!(
                "\r\nX-A: second\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n",
                compressed.len()
            )
            .bytes(),
        );
        wire.extend(compressed);
        let (url, server) = server(wire);
        let r = request("GET", &url, &vec![], &[], 10);
        let reply = send(r.done(), &Grants::default());
        let (status, headers, body) = response(reply.done());
        assert_eq!(status, 500);
        assert_eq!(body, b"decoded");
        let values = |name: &str| {
            headers
                .iter()
                .filter(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(values("x-a"), ["first", "second"]);
        assert_eq!(values("x-b"), ["é"]);
        assert!(values("content-encoding").is_empty());
        assert!(values("content-length").is_empty());
        assert_eq!(headers.len(), 3);
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
            let r = request(method, &url, &headers, b"request-body", 10);
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
        let r = request("GET", &url, &vec![], &[], 10);
        let encoded = send(r.done(), &Grants::default());
        let mut d = hd_run_wasmtime::internals::codec::Dec::new(encoded.done());
        assert_eq!(d.leb().unwrap(), 1);
        assert_eq!(d.leb().unwrap(), 3);
        server.join().unwrap();
    }
}
