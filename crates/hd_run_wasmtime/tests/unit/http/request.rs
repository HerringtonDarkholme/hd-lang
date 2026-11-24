mod tests {
    use crate::http::test_support::{bounded_accept, read_request, request, send, write_response};
    use hd_run::{Grants, Limit};
    use hd_run_wasmtime::internals::abi::WireIdentity;
    use hd_run_wasmtime::internals::codec::{Dec, Enc};
    use hd_run_wasmtime::internals::http::request::{
        HttpError, MAX_DECODED_BYTES, Request, Settings, decode_bytes, encode, send as send_request,
    };
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::time::Duration;

    const TEST_CA: &str = include_str!("../../../../../host/fixtures/tls/ca.pem");
    const TEST_SERVER_CERT: &str = include_str!("../../../../../host/fixtures/tls/server.pem");
    const TEST_SERVER_KEY: &str = include_str!("../../../../../host/fixtures/tls/server-key.pem");
    const TEST_CLIENT_CERT: &str = include_str!("../../../../../host/fixtures/tls/client.pem");
    const TEST_CLIENT_KEY: &str = include_str!("../../../../../host/fixtures/tls/client-key.pem");

    fn configured(url: String, settings: Settings) -> Request {
        Request {
            method: "GET".into(),
            url,
            headers: vec![],
            body: vec![],
            redirects: 0,
            settings,
        }
    }

    #[test]
    fn request_codec_and_errors_use_the_node_variant_order() {
        let wire = request("HEAD", "http://127.0.0.1/", &vec![], b"body", 7);
        let r = Request::decode(wire.done()).unwrap();
        assert_eq!(r.method, "HEAD");
        assert_eq!(r.redirects, 7);
        assert_eq!(r.body, b"body");
        assert!(Request::decode(&[128]).is_err());
        assert_eq!(encode(Err(HttpError::Timeout)).done(), [1, 5]);
        for (failure, variant) in [
            (
                HttpError::NotGranted {
                    host: "host".into(),
                },
                0,
            ),
            (HttpError::InvalidUrl { url: "url".into() }, 1),
            (
                HttpError::Dns {
                    host: "host".into(),
                },
                2,
            ),
            (
                HttpError::Connect {
                    message: "message".into(),
                },
                3,
            ),
            (
                HttpError::Tls {
                    message: "message".into(),
                },
                4,
            ),
            (HttpError::TooManyRedirects { url: "url".into() }, 6),
            (
                HttpError::Other {
                    message: "message".into(),
                },
                7,
            ),
            (
                HttpError::InvalidContentEncoding {
                    encoding: "gzip".into(),
                },
                11,
            ),
        ] {
            let encoded = encode(Err(failure));
            let mut d = Dec::new(encoded.done());
            assert_eq!(d.leb().unwrap(), 1);
            assert_eq!(d.leb().unwrap(), variant);
            assert!(!d.str().unwrap().is_empty());
        }
        assert_eq!(encode(Err(HttpError::OutputTooLarge)).done(), [1, 12]);
    }

    #[test]
    fn decoded_output_accepts_the_64_mib_boundary_and_rejects_the_next_byte() {
        let identity = vec!["identity".to_owned()];
        assert_eq!(
            decode_bytes(vec![0; MAX_DECODED_BYTES], &identity)
                .unwrap()
                .len(),
            MAX_DECODED_BYTES
        );
        assert_eq!(
            decode_bytes(vec![0; MAX_DECODED_BYTES + 1], &identity),
            Err(HttpError::OutputTooLarge)
        );
    }

    #[test]
    fn zero_phase_timeouts_expire_connect_headers_body_and_tls_handshake() {
        let timed_out = |request| {
            assert!(matches!(
                send_request(
                    request,
                    &Grants::default(),
                    None,
                    &hd_run_wasmtime::internals::http::connector::Cancel::default(),
                ),
                Err(HttpError::Timeout)
            ));
        };

        timed_out(configured(
            "http://127.0.0.1:1/".into(),
            Settings {
                connect_timeout: Some(Duration::ZERO),
                ..Settings::default()
            },
        ));

        for phase in ["headers", "body", "tls"] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let mut peer = bounded_accept(&listener);
                if phase == "body" {
                    read_request(&mut peer);
                    write_response(&mut peer, b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\n")
                        .unwrap();
                }
                std::thread::sleep(Duration::from_millis(100));
            });
            let settings = match phase {
                "headers" => Settings {
                    response_header_timeout: Some(Duration::ZERO),
                    ..Settings::default()
                },
                "body" => Settings {
                    body_read_timeout: Some(Duration::ZERO),
                    ..Settings::default()
                },
                _ => Settings {
                    tls_handshake_timeout: Some(Duration::ZERO),
                    ..Settings::default()
                },
            };
            let scheme = if phase == "tls" { "https" } else { "http" };
            timed_out(configured(format!("{scheme}://{address}/"), settings));
            server.join().unwrap();
        }
    }

    #[test]
    fn extra_root_and_client_identity_complete_a_mutual_tls_request() {
        use rustls::pki_types::pem::PemObject;
        use rustls::pki_types::{CertificateDer, PrivateKeyDer};
        use rustls::server::WebPkiClientVerifier;
        use rustls::{RootCertStore, ServerConfig, ServerConnection, StreamOwned};

        let mut client_roots = RootCertStore::empty();
        client_roots
            .add(CertificateDer::from_pem_slice(TEST_CA.as_bytes()).unwrap())
            .unwrap();
        let verifier = WebPkiClientVerifier::builder(Arc::new(client_roots))
            .build()
            .unwrap();
        let server_chain = CertificateDer::pem_slice_iter(TEST_SERVER_CERT.as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let server_key = PrivateKeyDer::from_pem_slice(TEST_SERVER_KEY.as_bytes()).unwrap();
        let config = ServerConfig::builder()
            .with_client_cert_verifier(verifier)
            .with_single_cert(server_chain, server_key)
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let peer = bounded_accept(&listener);
            peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            peer.set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let connection = ServerConnection::new(Arc::new(config)).unwrap();
            let mut tls = StreamOwned::new(connection, peer);
            let mut request = [0; 4096];
            assert!(tls.read(&mut request).unwrap() > 0);
            tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .unwrap();
        });
        let request = configured(
            format!("https://{address}/"),
            Settings {
                tls_roots: vec![TEST_CA.into()],
                tls_identity: Some(WireIdentity {
                    certificate_chain: TEST_CLIENT_CERT.into(),
                    private_key: TEST_CLIENT_KEY.into(),
                }),
                ..Settings::default()
            },
        );
        let response = send_request(
            request,
            &Grants::default(),
            None,
            &hd_run_wasmtime::internals::http::connector::Cancel::default(),
        )
        .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"ok");
        server.join().unwrap();
    }

    #[test]
    fn invalid_urls_and_denials_precede_header_validation() {
        let deny = Grants::with_limits(vec![("Http".into(), Limit::Deny)]);
        let r = request(
            "bad method",
            "http://127.0.0.1:1/",
            &vec![("bad name".into(), "\0".into())],
            &[],
            10,
        );
        let result = send(r.done(), &deny);
        let mut expected = Enc::default();
        expected.leb(1).leb(0).str("127.0.0.1");
        assert_eq!(result.done(), expected.done());
        let r = request("GET", "relative", &vec![], &[], 10);
        let result = send(r.done(), &deny);
        let mut expected = Enc::default();
        expected.leb(1).leb(1).str("relative");
        assert_eq!(result.done(), expected.done());
    }

    /// `std-http.url.valid`, `.non-ascii`, `.grant`, `.no-normalize`.
    #[test]
    fn a_non_ascii_host_is_invalid_and_a_grant_matches_the_lowercase_host() {
        let deny = Grants::with_limits(vec![("Http".into(), Limit::Deny)]);
        let denied = |url: &str| {
            let r = request("GET", url, &vec![], &[], 10);
            send(r.done(), &deny).done().to_vec()
        };
        let encoded = |variant: u64, text: &str| {
            let mut expected = Enc::default();
            expected.leb(1).leb(variant).str(text);
            expected.done().to_vec()
        };
        let url = "http://b\u{fc}cher.example/";
        assert_eq!(denied(url), encoded(1, url));
        let url = "http://b%C3%BCcher.example/";
        assert_eq!(denied(url), encoded(1, url));
        let url = "http://xn--bcher-kva.example/";
        assert_eq!(denied(url), encoded(0, "xn--bcher-kva.example"));
        assert_eq!(denied("HTTP://LocalHost:1/"), encoded(0, "localhost"));
        let only = Grants::with_limits(vec![(
            "Http".into(),
            Limit::Entries(vec!["Example.COM".into()]),
        )]);
        let r = request("GET", "http://EXAMPLE.com:1/", &vec![], &[], 10);
        assert!(matches!(
            hd_run_wasmtime::internals::http::client::start(r.done(), &only).unwrap(),
            hd_run_wasmtime::internals::http::client::Start::Run(_)
        ));
        let r = request("GET", "http://example.org:1/", &vec![], &[], 10);
        assert_eq!(send(r.done(), &only).done(), encoded(0, "example.org"));
    }

    fn redirecting(location: &'static [u8]) -> (String, std::thread::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/a/b/c?x", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut stream = bounded_accept(&listener);
            let first = read_request(&mut stream);
            let mut head = b"HTTP/1.1 302 Found\r\nLocation: ".to_vec();
            head.extend_from_slice(location);
            head.extend_from_slice(b"\r\nContent-Length: 0\r\n\r\n");
            write_response(&mut stream, &head).unwrap();
            if location.starts_with(b"..") {
                let mut stream = bounded_accept(&listener);
                let second = read_request(&mut stream);
                write_response(&mut stream, b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                    .unwrap();
                return [first, second].concat();
            }
            first
        });
        (url, server)
    }

    #[test]
    fn a_relative_location_joins_by_rfc_3986_and_drops_dot_segments() {
        let (url, server) = redirecting(b"../d/./e?y#frag");
        let r = request("GET", &url, &vec![], &[], 10);
        assert_eq!(send(r.done(), &Grants::default()).done()[0], 0);
        let wire = String::from_utf8(server.join().unwrap()).unwrap();
        assert!(wire.starts_with("GET /a/b/c?x HTTP/1.1\r\n"), "{wire}");
        assert!(wire.contains("GET /a/d/e?y HTTP/1.1\r\n"), "{wire}");
    }

    #[test]
    fn a_redirect_to_a_non_ascii_host_is_invalid_and_a_malformed_one_is_other() {
        for (location, variant) in [
            (&b"http://b\xc3\xbccher.example/"[..], 1),
            (&b"/a b"[..], 1),
            (&b"http://[bad/"[..], 7),
        ] {
            let (url, server) = redirecting(location);
            let r = request("GET", &url, &vec![], &[], 10);
            let encoded = send(r.done(), &Grants::default());
            let mut d = Dec::new(encoded.done());
            assert_eq!(d.leb().unwrap(), 1);
            assert_eq!(d.leb().unwrap(), variant, "{location:?}");
            server.join().unwrap();
        }
    }

    #[test]
    fn requests_keep_header_values_in_order_and_ureq_frames_the_body() {
        let wire = |method: &str, headers: Vec<(&str, &str)>, body: &[u8]| {
            let (url, server) = crate::http::test_support::server(
                b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec(),
            );
            let headers = headers
                .into_iter()
                .map(|(n, v)| (n.into(), v.into()))
                .collect();
            let r = request(method, &url, &headers, body, 10);
            assert_eq!(send(r.done(), &Grants::default()).done()[0], 0);
            let authority = url["http://".len()..]
                .split('/')
                .next()
                .unwrap_or_default()
                .to_owned();
            (
                String::from_utf8_lossy(&server.join().unwrap()).into_owned(),
                authority,
            )
        };
        let (text, authority) = wire(
            "GET",
            vec![
                ("X-First", "one"),
                ("x-Second", "\u{e9}"),
                ("X-First", "two"),
            ],
            b"",
        );
        assert!(text.contains(&format!("host: {authority}\r\n")), "{text}");
        assert!(
            text.contains("accept-encoding: gzip, deflate\r\n"),
            "{text}"
        );
        let one = text.find("x-first: one").unwrap();
        assert!(one < text.find("x-first: two").unwrap(), "{text}");
        assert!(
            text.contains("x-second: \u{e9}\r\n") || text.contains("x-second: \u{fffd}"),
            "{text}"
        );
        let (text, _) = wire("POST", vec![("hOsT", "custom.example:88")], b"body");
        assert!(text.contains("host: custom.example:88\r\n"), "{text}");
        assert!(text.contains("content-length: 4\r\n"), "{text}");
        assert!(text.ends_with("\r\n\r\nbody"), "{text}");
        let (text, _) = wire("PUT", vec![("Transfer-Encoding", "chunked")], b"body");
        assert!(text.ends_with("4\r\nbody\r\n0\r\n\r\n"), "{text}");
        assert!(!text.contains("content-length"), "{text}");
        let (text, _) = wire("GET", vec![("Range", "bytes=0-3")], b"");
        assert!(text.contains("accept-encoding: identity\r\n"), "{text}");
        assert!(!text.contains("accept-encoding: gzip, deflate"), "{text}");
        let (text, _) = wire("GET", vec![("Accept-Encoding", "br")], b"");
        assert!(text.contains("accept-encoding: br\r\n"), "{text}");
        assert!(!text.contains("accept-encoding: gzip, deflate"), "{text}");
    }

    #[test]
    fn redirect_rechecks_the_target_port_before_a_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let target_url = format!("http://{}/not-granted", target.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut stream = bounded_accept(&listener);
            read_request(&mut stream);
            write_response(
                &mut stream,
                format!(
                    "HTTP/1.1 302 Found\r\nLocation: {target_url}\r\nContent-Length: 0\r\n\r\n"
                )
                .as_bytes(),
            )
            .unwrap();
        });
        let grant = Grants::with_limits(vec![(
            "Http".into(),
            Limit::Entries(vec![address.to_string()]),
        )]);
        let r = request("GET", &format!("http://{address}/"), &vec![], &[], 10);
        let encoded = send(r.done(), &grant);
        let mut expected = Enc::default();
        expected.leb(1).leb(0).str("127.0.0.1");
        assert_eq!(encoded.done(), expected.done());
        assert_eq!(
            target.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        server.join().unwrap();
    }
}
