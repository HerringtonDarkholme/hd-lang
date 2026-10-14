//! Exact exchange-buffer HTTP parity against deterministic loopback servers.

use crate::support::*;
use hd_run::Limit;
use std::fmt::Write;
use std::time::Duration;

fn request(
    method: u64,
    url: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    timeout: Option<i64>,
) -> Vec<u8> {
    let mut e = Enc::default().leb(method);
    if method == 7 {
        e = e.str("CUSTOM");
    }
    e = e.str(url).leb(headers.len() as u64);
    for (name, value) in headers {
        e = e.str(name).str(value);
    }
    e = e.bytes(body);
    e = match timeout {
        Some(ms) => e.leb(1).leb(if ms < 0 {
            ms.unsigned_abs() * 2 - 1
        } else {
            ms.unsigned_abs() * 2
        }),
        None => e.leb(0),
    };
    e.0
}

fn response(status: u16, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut e = Enc::default()
        .leb(0)
        .leb(u64::from(status))
        .leb(headers.len() as u64);
    for (name, value) in headers {
        e = e.str(name).str(value);
    }
    e.bytes(body).0
}

fn error(variant: u64, text: &str) -> Vec<u8> {
    let e = Enc::default().leb(1).leb(variant);
    if variant == 5 { e.0 } else { e.str(text).0 }
}

fn raw(status: u16, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut text = format!("HTTP/1.1 {status} Test\r\n");
    for (name, value) in headers {
        write!(text, "{name}: {value}\r\n").unwrap();
    }
    text.push_str("\r\n");
    let mut bytes = text.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

#[test]
fn http_statuses_binary_bodies_header_case_and_repeats() {
    let scratch = Scratch::new();
    let headers = [
        ("x-Mixed", "first"),
        ("Set-Cookie", "a=1"),
        ("Set-Cookie", "b=2"),
        ("Content-Length", "4"),
    ];
    let server = HttpFixture::new(move |r| {
        let status = r.target[1..].parse().unwrap();
        (raw(status, &headers, &[0, 255, 195, 169]), Duration::ZERO)
    });
    let mut p = Program::new();
    for status in [200, 404, 500] {
        p.call_waiting(
            "hd:Http",
            "send",
            request(0, &server.url(&format!("/{status}")), &[], b"", None),
            response(status, &headers, &[0, 255, 195, 169]),
        );
    }
    assert_program(&scratch, p, vec![], || {});
    let seen = server.seen.lock().unwrap();
    assert_eq!(seen.len(), 6);
    for r in &*seen {
        assert_eq!(r.values("Host"), [format!("127.0.0.1:{}", server.port())]);
    }
}

#[test]
fn http_all_methods_custom_host_request_pairs_and_framing() {
    let scratch = Scratch::new();
    let server = HttpFixture::new(|r| {
        let body = if r.method == "HEAD" {
            b"".as_slice()
        } else {
            b"ok"
        };
        (raw(200, &[("Content-Length", "2")], body), Duration::ZERO)
    });
    let mut p = Program::new();
    for method in 0..8 {
        let body = b"\0\xffab";
        let headers = [
            ("hOsT", "custom.example:88"),
            ("X-Tag", "one"),
            ("X-Tag", "two"),
        ];
        let expected_body = if method == 1 {
            b"".as_slice()
        } else {
            b"ok".as_slice()
        };
        p.call_waiting(
            "hd:Http",
            "send",
            request(
                method,
                &server.url("/echo?query=yes#ignored"),
                &headers,
                body,
                None,
            ),
            response(200, &[("Content-Length", "2")], expected_body),
        );
    }
    // Explicit body framing must not receive a second Content-Length.
    p.call_waiting(
        "hd:Http",
        "send",
        request(
            2,
            &server.url("/framed"),
            &[("Content-Length", "4")],
            b"body",
            None,
        ),
        response(200, &[("Content-Length", "2")], b"ok"),
    );
    assert_program(&scratch, p, vec![], || {});
    let seen = server.seen.lock().unwrap();
    assert_eq!(seen.len(), 18);
    let methods = [
        "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "CUSTOM",
    ];
    for batch in seen.chunks(9) {
        for (r, method) in batch.iter().take(8).zip(methods) {
            assert_eq!(r.method, method);
            assert_eq!(r.target, "/echo?query=yes");
            assert_eq!(r.values("Host"), ["custom.example:88"]);
            assert_eq!(r.values("X-Tag"), ["one", "two"]);
            assert_eq!(r.body, [0, 255, b'a', b'b']);
            assert_eq!(r.values("Content-Length"), ["4"]);
        }
        assert_eq!(batch[8].values("Content-Length"), ["4"]);
        assert_eq!(batch[8].body, b"body");
    }
}

#[test]
fn http_redirect_method_rewrites_and_body_preservation() {
    let scratch = Scratch::new();
    let server = HttpFixture::new(|r| {
        if r.target == "/final" {
            (raw(200, &[("Content-Length", "0")], b""), Duration::ZERO)
        } else {
            (
                raw(
                    r.target[1..].parse().unwrap(),
                    &[("Location", "/final"), ("Content-Length", "0")],
                    b"",
                ),
                Duration::ZERO,
            )
        }
    });
    let mut p = Program::new();
    for (status, method) in [(301, 2), (302, 2), (303, 3), (307, 2), (308, 2), (301, 3)] {
        p.call_waiting(
            "hd:Http",
            "send",
            request(
                method,
                &server.url(&format!("/{status}")),
                &[],
                b"payload",
                None,
            ),
            response(200, &[("Content-Length", "0")], b""),
        );
    }
    assert_program(&scratch, p, vec![], || {});
    let seen = server.seen.lock().unwrap();
    assert_eq!(seen.len(), 24);
    for batch in seen.chunks(12) {
        for (pair, (method, body)) in batch.chunks(2).zip([
            ("GET", b"".as_slice()),
            ("GET", b""),
            ("GET", b""),
            ("POST", b"payload"),
            ("POST", b"payload"),
            ("PUT", b"payload"),
        ]) {
            assert_eq!(pair[1].method, method);
            assert_eq!(pair[1].body, body);
        }
    }
}

#[test]
fn http_initial_and_redirect_grants_cover_host_and_port() {
    let scratch = Scratch::new();
    let target = HttpFixture::new(|_| (raw(200, &[("Content-Length", "0")], b""), Duration::ZERO));
    let destination = target.url("/secret");
    let from = HttpFixture::new(move |_| {
        (
            raw(
                307,
                &[("Location", &destination), ("Content-Length", "0")],
                b"",
            ),
            Duration::ZERO,
        )
    });
    let mut p = Program::new();
    p.call_waiting(
        "hd:Http",
        "send",
        request(0, &target.url("/secret"), &[], b"", None),
        error(0, "127.0.0.1"),
    );
    p.call_waiting(
        "hd:Http",
        "send",
        request(0, &from.url("/redirect"), &[], b"", None),
        error(0, "127.0.0.1"),
    );
    assert_program(
        &scratch,
        p,
        vec![(
            "Http",
            Limit::Entries(vec![format!("127.0.0.1:{}", from.port())]),
        )],
        || {},
    );
    assert_eq!(from.seen.lock().unwrap().len(), 2);
    assert!(target.seen.lock().unwrap().is_empty());
}

#[test]
fn http_redirect_limit_precedes_next_hop_grant() {
    let scratch = Scratch::new();
    let denied = HttpFixture::new(|_| (raw(200, &[("Content-Length", "0")], b""), Duration::ZERO));
    let last = denied.url("/final");
    let destination = last.clone();
    let server = HttpFixture::new(move |r| {
        let hop: u32 = r.target[1..].parse().unwrap();
        let to = if hop == 10 {
            destination.clone()
        } else {
            format!("/{}", hop + 1)
        };
        (
            raw(302, &[("Location", &to), ("Content-Length", "0")], b""),
            Duration::ZERO,
        )
    });
    let mut p = Program::new();
    p.call_waiting(
        "hd:Http",
        "send",
        request(0, &server.url("/0"), &[], b"", None),
        error(6, &last),
    );
    assert_program(
        &scratch,
        p,
        vec![(
            "Http",
            Limit::Entries(vec![format!("127.0.0.1:{}", server.port())]),
        )],
        || {},
    );
    assert_eq!(server.seen.lock().unwrap().len(), 22);
    assert!(denied.seen.lock().unwrap().is_empty());
}

#[test]
fn http_exactly_ten_redirects_reach_the_successful_response() {
    let scratch = Scratch::new();
    let server = HttpFixture::new(|r| {
        let hop: u32 = r.target[1..].parse().unwrap();
        let bytes = if hop == 10 {
            raw(200, &[("Content-Length", "0")], b"")
        } else {
            raw(
                302,
                &[
                    ("Location", &format!("/{}", hop + 1)),
                    ("Content-Length", "0"),
                ],
                b"",
            )
        };
        (bytes, Duration::ZERO)
    });
    let mut p = Program::new();
    p.call_waiting(
        "hd:Http",
        "send",
        request(0, &server.url("/0"), &[], b"", None),
        response(200, &[("Content-Length", "0")], b""),
    );
    assert_program(&scratch, p, vec![], || {});
    assert_eq!(server.seen.lock().unwrap().len(), 22);
}

#[test]
fn http_invalid_urls_and_zero_or_negative_timeout() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    for url in [
        "",
        "/relative",
        "ftp://127.0.0.1/x",
        "http://[bad/",
        "http://127.0.0.1:99999/",
    ] {
        p.call_waiting(
            "hd:Http",
            "send",
            request(0, url, &[], b"", None),
            error(1, url),
        );
    }
    for timeout in [0, -1] {
        p.call_waiting(
            "hd:Http",
            "send",
            request(0, "http://127.0.0.1:9/", &[], b"", Some(timeout)),
            error(5, ""),
        );
    }
    assert_program(&scratch, p, vec![], || {});
}

#[test]
fn http_delayed_response_times_out() {
    let scratch = Scratch::new();
    let server = HttpFixture::new(|_| {
        (
            raw(200, &[("Content-Length", "0")], b""),
            Duration::from_millis(150),
        )
    });
    let mut p = Program::new();
    p.call_waiting(
        "hd:Http",
        "send",
        request(0, &server.url("/slow"), &[], b"", Some(30)),
        error(5, ""),
    );
    assert_program(&scratch, p, vec![], || {});
}

#[test]
fn http_timeout_budget_is_shared_across_redirect_hops() {
    let scratch = Scratch::new();
    // Generous margins under load: each hop (2 s) fits well within the 3 s
    // whole-request budget (1 s margin), while their sum (4 s) exceeds it by
    // 1 s. Previously 1.2 s delays with a 2 s budget left only a 400 ms
    // margin for the second hop, so a late timeout under load could return
    // Ok instead of Timeout. The test still proves the budget is shared:
    // resetting per hop would succeed.
    let target = HttpFixture::new(|_| {
        (
            raw(200, &[("Content-Length", "0")], b""),
            Duration::from_secs(2),
        )
    });
    let destination = target.url("/final");
    let from = HttpFixture::new(move |_| {
        (
            raw(
                302,
                &[("Location", &destination), ("Content-Length", "0")],
                b"",
            ),
            Duration::from_secs(2),
        )
    });
    let mut p = Program::new();
    // Each individual response fits within 3 s, but their cumulative delay
    // exceeds it. Resetting the timeout at the redirect would return Ok.
    p.call_waiting(
        "hd:Http",
        "send",
        request(0, &from.url("/start"), &[], b"", Some(3000)),
        error(5, ""),
    );
    assert_program(
        &scratch,
        p,
        vec![(
            "Http",
            Limit::Entries(vec![
                format!("127.0.0.1:{}", from.port()),
                format!("127.0.0.1:{}", target.port()),
            ]),
        )],
        || {},
    );
    // Prove both engines reached the delayed second hop rather than timing
    // out before the redirect was followed.
    assert_eq!(
        from.seen
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.target.as_str())
            .collect::<Vec<_>>(),
        ["/start", "/start"]
    );
    assert_eq!(
        target
            .seen
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.target.as_str())
            .collect::<Vec<_>>(),
        ["/final", "/final"]
    );
}

#[test]
fn http_timeout_still_applies_after_response_headers_arrive() {
    use std::io::{Read, Write};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let scratch = Scratch::new();
    let headers_sent = Arc::new(AtomicUsize::new(0));
    let recorded = headers_sent.clone();
    let server = TcpFixture::new(move |mut peer| {
        let mut request = Vec::new();
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let mut bytes = [0; 1024];
            let count = peer.read(&mut bytes).unwrap();
            assert_ne!(count, 0, "request headers arrive before the peer closes");
            request.extend_from_slice(&bytes[..count]);
            assert!(request.len() <= 65_536, "bounded fixture request headers");
        }
        peer.write_all(b"HTTP/1.1 200 Test\r\nContent-Length: 4\r\n\r\n")
            .unwrap();
        recorded.fetch_add(1, Ordering::SeqCst);
        // Generous margin under load: headers arrive immediately, the body
        // follows 3 s later, and the 2 s request timeout fires 1 s before the
        // body. Previously 2.2 s vs 1.5 s left only 700 ms, so a late timeout
        // under load could observe the body and return Ok. The serial fixture
        // still satisfies 3 s < 2*2 s, so the second engine's headers (delayed
        // by the first handler's sleep) arrive ~1 s after its start, well
        // before its 2 s timeout.
        std::thread::sleep(Duration::from_secs(3));
        // Both clients must close before the body completes. BrokenPipe is
        // expected; the fixture always returns and drops its peer.
        let _ = peer.write_all(b"late");
    });
    let mut p = Program::new();
    p.call_waiting(
        "hd:Http",
        "send",
        request(
            0,
            &format!("http://{}/body", server.address),
            &[],
            b"",
            Some(2000),
        ),
        error(5, ""),
    );
    assert_program(&scratch, p, vec![], || {});
    assert_eq!(
        headers_sent.load(Ordering::SeqCst),
        2,
        "both engines received a complete response header before the delayed body"
    );
}

#[test]
fn http_refused_connection_keeps_node_message() {
    let scratch = Scratch::new();
    let reservation = RefusedPort::new();
    let port = reservation.address.port();
    let url = format!("http://127.0.0.1:{port}/");
    let mut p = Program::new();
    p.call_waiting(
        "hd:Http",
        "send",
        request(0, &url, &[], b"", None),
        error(3, &format!("connect ECONNREFUSED 127.0.0.1:{port}")),
    );
    assert_program(&scratch, p, vec![], || {});
}

#[test]
fn http_malformed_codec_and_stale_finish_are_contract_panics() {
    use wasm_encoder::{Instruction as I, ValType};
    let scratch = Scratch::new();
    for finish in [false, true] {
        let mut p = Program::new();
        let name = if finish { "send.finish" } else { "send.start" };
        let results = if finish {
            vec![ValType::I32]
        } else {
            vec![ValType::I32, ValType::I32]
        };
        let call = p.import("hd:Http", name, &[ValType::I32], &results);
        p.instructions(&[I::I32Const(0), I::Call(call), I::Drop]);
        if !finish {
            p.instructions(&[I::Drop]);
        }
        let (wasm, _) = p.finish(7);
        let result = parity(&scratch, &wasm, vec![], || {});
        assert_eq!(result.status, 3);
        let message = if finish {
            "a finish of an operation that is not pending"
        } else {
            "an argument does not decode"
        };
        assert_eq!(
            result.stderr,
            format!("panic: host-contract: {message}\n").as_bytes()
        );
    }
}

#[test]
fn http_invalid_method_and_headers_keep_exact_validation_errors() {
    let scratch = Scratch::new();
    let url = "http://127.0.0.1:9/";
    let invalid_method = Enc::default()
        .leb(7)
        .str("bad method")
        .str(url)
        .leb(0)
        .bytes(b"")
        .leb(0)
        .0;
    let mut p = Program::new();
    p.call_waiting(
        "hd:Http",
        "send",
        invalid_method,
        error(7, "Method must be a valid HTTP token [\"bad method\"]"),
    );
    for (headers, message) in [
        (
            [("bad name", "value")],
            "Header name must be a valid HTTP token [\"bad name\"]",
        ),
        (
            [("X-Test", "bad\nvalue")],
            "Invalid character in header content [\"X-Test\"]",
        ),
    ] {
        p.call_waiting(
            "hd:Http",
            "send",
            request(0, url, &headers, b"", None),
            error(7, message),
        );
    }
    assert_program(&scratch, p, vec![], || {});
}

fn tls_program(url: &str) -> Program {
    use wasm_encoder::{BlockType, Instruction as I, ValType};
    let mut p = Program::new();
    let bytes = request(0, url, &[], b"", None);
    let call = p.import(
        "hd:Http",
        "send.start",
        &[ValType::I32],
        &[ValType::I32, ValType::I32],
    );
    let block = p.import("hd:rt", "block", &[], &[ValType::I32]);
    let finish = p.import("hd:Http", "send.finish", &[ValType::I32], &[ValType::I32]);
    p.load_args(&bytes);
    p.instructions(&[
        I::I32Const(i32::try_from(bytes.len()).unwrap()),
        I::Call(call),
        I::LocalSet(0),
        I::LocalSet(1),
        I::LocalGet(1),
        I::If(BlockType::Empty),
        // The only pending operation, so the first batch holds it.
        I::Call(block),
        I::Drop,
        I::LocalGet(0),
        I::Call(finish),
        I::LocalSet(0),
        I::End,
        I::LocalGet(0),
        I::I32Const(3),
        I::I32LtU,
        I::If(BlockType::Empty),
        I::Unreachable,
        I::End,
        I::I32Const(2),
        I::LocalSet(0),
    ]);
    // TLS library diagnostics are explicitly provider-specific. Compare the
    // exact Result::Err / HttpError::Tls tags, not OpenSSL versus rustls text.
    p.dump();
    p.expected.push(vec![1, 4]);
    p
}

#[test]
fn http_loopback_tls_alert_is_tls_error_on_wasmtime() {
    use std::io::{Read, Write};
    let server = TcpFixture::new(|mut peer| {
        let mut header = [0; 5];
        peer.read_exact(&mut header).unwrap();
        assert_eq!(header[0], 22);
        let mut hello = vec![0; usize::from(u16::from_be_bytes([header[3], header[4]]))];
        peer.read_exact(&mut hello).unwrap();
        peer.write_all(&[21, 3, 3, 0, 2, 2, 48]).unwrap();
        let _ = peer.read_to_end(&mut Vec::new());
    });
    let p = tls_program(&format!("https://127.0.0.1:{}/", server.address.port()));
    let (wasm, expected) = p.finish(7);
    // Node emits EPROTO for this actual TLS alert and wrongly classifies it
    // as Connect. Preserve the specified Tls category on the Rust provider.
    let setup = hd_run::HostSetup {
        providers: vec![],
        grants: hd_run::Grants::default(),
        limits: hd_run::Limits::default(),
        args: vec![],
        program: String::new(),
    };
    let result =
        hd_run_wasmtime::run_captured(hd_run_wasmtime::EngineConfig::default(), &wasm, &setup)
            .unwrap();
    assert_eq!(result.status, 7);
    assert_eq!(result.stderr, expected.as_bytes());
}

fn unhex(text: &str) -> Vec<u8> {
    text.split_whitespace()
        .flat_map(|part| {
            part.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        })
        .collect()
}

#[test]
fn http_self_signed_loopback_certificate_is_tls_error_on_both_providers() {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use std::io::Read;
    use std::sync::Arc;
    let scratch = Scratch::new();
    // Public, deliberately untrusted fixture material, generated for this
    // test alone. EC P-256, SAN 127.0.0.1; never a real server credential.
    let cert = unhex(
        "3082019030820136a00302010202143199ef33e647256cf55e945ed69ba6
        debf83b6f9300a06082a8648ce3d04030230143112301006035504030c09
        6c6f63616c686f73743020170d3236313031303233353835395a180f3231
        3236303931363233353835395a30143112301006035504030c096c6f6361
        6c686f73743059301306072a8648ce3d020106082a8648ce3d0301070342
        0004dfb3a55f6601d2592c1ec29b99f718d956ed44512a10d736869478d8
        4d7856534552254b53db89755b164c06b5005926853395ff77004cc99b4a
        59de98de6fa6a3643062301d0603551d0e04160414ddb38d980596765aff
        f0d3ac51f5f31dfc2050ea301f0603551d23041830168014ddb38d980596
        765afff0d3ac51f5f31dfc2050ea300f0603551d130101ff040530030101
        ff300f0603551d110408300687047f000001300a06082a8648ce3d040302
        0348003045022100be019ae4807e7e28a6e3d228219b2f675b47ce5ef3c8
        0eca59fc018e1236967d022048cb9f8f17f58e9c9e7d17b956d35a181ede
        e20b2138e697d9ebcf1c82766a05",
    );
    let key = unhex(
        "308187020100301306072a8648ce3d020106082a8648ce3d030107046d30
        6b0201010420dc5e5834838d5ab1047d232b4ef73a480e7833c16997bda3
        ddcb269f8c975220a14403420004dfb3a55f6601d2592c1ec29b99f718d9
        56ed44512a10d736869478d84d7856534552254b53db89755b164c06b500
        5926853395ff77004cc99b4a59de98de6fa6",
    );
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(cert)],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)),
        )
        .unwrap();
    let config = Arc::new(config);
    let server = TcpFixture::new(move |peer| {
        let connection = rustls::ServerConnection::new(config.clone()).unwrap();
        let mut tls = rustls::StreamOwned::new(connection, peer);
        // Both clients must reject the fixture certificate. The handshake's
        // alert is expected, not a fixture worker failure.
        let _ = tls.read(&mut [0; 1]);
    });
    let p = tls_program(&format!("https://127.0.0.1:{}/", server.address.port()));
    assert_program(&scratch, p, vec![], || {});
}

#[test]
fn http_chunked_request_response_and_informational_response_are_exact() {
    let scratch = Scratch::new();
    let server = HttpFixture::new(|_| {
        (b"HTTP/1.1 100 Continue\r\nX-Interim: discarded\r\n\r\nHTTP/1.1 200 Test\r\nTransfer-Encoding: chunked\r\nX-Final: retained\r\n\r\n2\r\n\0\xff\r\n2\r\nab\r\n0\r\n\r\n".to_vec(), Duration::ZERO)
    });
    let mut p = Program::new();
    p.call_waiting(
        "hd:Http",
        "send",
        request(
            2,
            &server.url("/chunked"),
            &[("Transfer-Encoding", "chunked")],
            &[0, 255, b'a', b'b'],
            None,
        ),
        response(
            200,
            &[("Transfer-Encoding", "chunked"), ("X-Final", "retained")],
            &[0, 255, b'a', b'b'],
        ),
    );
    assert_program(&scratch, p, vec![], || {});
    let seen = server.seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    for r in &*seen {
        assert_eq!(r.body, [0, 255, b'a', b'b']);
        assert_eq!(r.values("Transfer-Encoding"), ["chunked"]);
        assert!(r.values("Content-Length").is_empty());
    }
}

#[test]
fn http_bodyless_statuses_and_redirect_without_location_are_responses() {
    let scratch = Scratch::new();
    let server = HttpFixture::new(|r| {
        (
            raw(
                r.target[1..].parse().unwrap(),
                &[("Content-Length", "0")],
                b"",
            ),
            Duration::ZERO,
        )
    });
    let mut p = Program::new();
    for status in [204, 302, 304] {
        p.call_waiting(
            "hd:Http",
            "send",
            request(0, &server.url(&format!("/{status}")), &[], b"", None),
            response(status, &[("Content-Length", "0")], b""),
        );
    }
    assert_program(
        &scratch,
        p,
        vec![("Http", Limit::Entries(vec!["127.0.0.1".into()]))],
        || {},
    );
    assert_eq!(server.seen.lock().unwrap().len(), 6);
}

#[test]
fn http_malformed_response_and_redirect_location_preserve_error_text() {
    let scratch = Scratch::new();
    let server = HttpFixture::new(|r| {
        let bytes = if r.target == "/redirect" {
            raw(
                302,
                &[("Location", "http://[bad/"), ("Content-Length", "0")],
                b"",
            )
        } else {
            b"HTTP/1.1 200 Test\r\nBad Header: value\r\nContent-Length: 0\r\n\r\n".to_vec()
        };
        (bytes, Duration::ZERO)
    });
    let mut p = Program::new();
    p.call_waiting(
        "hd:Http",
        "send",
        request(0, &server.url("/response"), &[], b"", None),
        error(3, "Parse Error: Invalid header token"),
    );
    p.call_waiting(
        "hd:Http",
        "send",
        request(0, &server.url("/redirect"), &[], b"", None),
        error(7, "Invalid URL"),
    );
    assert_program(&scratch, p, vec![], || {});
}

/// `std-host.op.wait` across instances (`hd test --jobs 2`): two instances
/// each send a request to a server that answers only once both requests
/// have arrived. One queued behind the other would reach its 10 s timeout
/// instead.
#[test]
fn http_two_instances_send_at_the_same_time() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let mut peers = Vec::new();
        for _ in 0..2 {
            let (mut peer, _) = listener.accept().unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut request = Vec::new();
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let mut bytes = [0; 1024];
                let count = peer.read(&mut bytes).unwrap();
                assert_ne!(count, 0, "the request arrives whole");
                request.extend_from_slice(&bytes[..count]);
            }
            peers.push(peer);
        }
        for mut peer in peers {
            peer.write_all(&raw(200, &[("Content-Length", "0")], b""))
                .unwrap();
        }
    });
    let mut p = Program::new();
    p.call_waiting(
        "hd:Http",
        "send",
        request(0, &url, &[], b"", Some(10_000)),
        response(200, &[("Content-Length", "0")], b""),
    );
    let (wasm, expected) = p.finish(7);
    let instances: Vec<_> = (0..2)
        .map(|_| {
            let wasm = wasm.clone();
            std::thread::spawn(move || {
                let setup = hd_run::HostSetup {
                    providers: vec![],
                    grants: hd_run::Grants::default(),
                    limits: hd_run::Limits::default(),
                    args: vec![],
                    program: String::new(),
                };
                hd_run_wasmtime::run_captured(
                    hd_run_wasmtime::EngineConfig::default(),
                    &wasm,
                    &setup,
                )
                .unwrap()
            })
        })
        .collect();
    for instance in instances {
        let result = instance.join().unwrap();
        assert_eq!(result.status, 7);
        assert_eq!(String::from_utf8_lossy(&result.stderr), expected);
    }
    server.join().unwrap();
}
