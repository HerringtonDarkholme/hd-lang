//! Exact exchange-buffer HTTP parity against deterministic loopback servers.

use crate::support::*;
use flate2::Compression;
use flate2::write::{DeflateEncoder, GzEncoder, ZlibEncoder};
use hd_run::Limit;
use std::fmt::Write;
use std::io::Write as IoWrite;
use std::net::ToSocketAddrs;
use std::time::Duration;

fn request(
    method: u64,
    url: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    redirects: Option<u64>,
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
    e = e.leb(redirects.unwrap_or(10));
    e.0
}

/// The expected response. Both hosts deliver a header name in lowercase
/// (`std-http.headers.case`), so the expectation lowercases the names the
/// fixture server writes; the values of one name keep their order. `url`
/// is the last request's URL (`std-http.response.optional-url`).
fn response(status: u16, headers: &[(&str, &str)], body: &[u8], url: &str) -> Vec<u8> {
    let mut e = Enc::default()
        .leb(0)
        .leb(u64::from(status))
        .leb(headers.len() as u64);
    for (name, value) in headers {
        e = e.str(&name.to_ascii_lowercase()).str(value);
    }
    e.bytes(body).str(url).0
}

fn error(variant: u64, text: &str) -> Vec<u8> {
    let e = Enc::default().leb(1).leb(variant);
    if matches!(variant, 5 | 12) {
        e.0
    } else {
        e.str(text).0
    }
}

fn gzip(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for part in parts {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(part).unwrap();
        out.extend(encoder.finish().unwrap());
    }
    out
}

fn zlib(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

fn raw_deflate(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
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

fn encoded_raw(status: u16, codings: &[&str], body: &[u8], extra: &[(&str, &str)]) -> Vec<u8> {
    let mut text = format!("HTTP/1.1 {status} Test\r\n");
    for coding in codings {
        write!(text, "Content-Encoding: {coding}\r\n").unwrap();
    }
    for (name, value) in extra {
        write!(text, "{name}: {value}\r\n").unwrap();
    }
    write!(text, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
    let mut bytes = text.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

/// `std-http.decode.portable-codings`, `.all-content-encoding-fields`,
/// `.stacked-reverse`, `.gzip-members`, and `.normalize-headers`.
#[test]
fn http_decodes_gzip_zlib_stacks_identity_and_concatenated_members() {
    let scratch = Scratch::new();
    let server = HttpFixture::new(|request| {
        let (codings, body) = match request.target.as_str() {
            "/gzip" => (vec!["gzip"], gzip(&[b"gzip body"])),
            "/stack" => (vec!["gzip", "deflate"], zlib(&gzip(&[b"stacked body"]))),
            "/members" => (vec!["gzip"], gzip(&[b"first ", b"second"])),
            "/identity" => (vec!["identity"], b"identity body".to_vec()),
            target => panic!("unexpected target {target}"),
        };
        (
            encoded_raw(200, &codings, &body, &[("X-Decoded", "yes")]),
            Duration::ZERO,
        )
    });
    let mut program = Program::new();
    for (target, decoded) in [
        ("/gzip", b"gzip body".as_slice()),
        ("/stack", b"stacked body".as_slice()),
        ("/members", b"first second".as_slice()),
        ("/identity", b"identity body".as_slice()),
    ] {
        let url = server.url(target);
        program.call_waiting(
            "hd:Http",
            "send",
            request(0, &url, &[("Accept-Encoding", "gzip, deflate")], b"", None),
            response(200, &[("x-decoded", "yes")], decoded, &url),
        );
    }
    assert_program(&scratch, program, vec![], || {});
    for request in server.seen.lock().unwrap().iter() {
        assert_eq!(request.values("Accept-Encoding"), ["gzip, deflate"]);
    }
}

/// `std-http.decode.no-partial-stack`, `.corrupt`, and `.cap`.
#[test]
fn http_rejects_unsupported_corrupt_raw_deflate_and_oversized_output() {
    let scratch = Scratch::new();
    let oversized_plain = vec![0; 64 * 1024 * 1024 + 1];
    let oversized = gzip(&[oversized_plain.as_slice()]);
    let server = HttpFixture::new(move |request| {
        let (codings, body) = match request.target.as_str() {
            "/unsupported" => (vec!["br, gzip"], gzip(&[b"body"])),
            "/raw-deflate" => (vec!["deflate"], raw_deflate(b"body")),
            "/corrupt" => {
                let mut body = gzip(&[b"body"]);
                body.truncate(body.len() - 3);
                (vec!["gzip"], body)
            }
            "/oversized" => (vec!["gzip"], oversized.clone()),
            target => panic!("unexpected target {target}"),
        };
        (encoded_raw(200, &codings, &body, &[]), Duration::ZERO)
    });
    let mut program = Program::new();
    for (target, expected) in [
        ("/unsupported", error(11, "br")),
        ("/raw-deflate", error(11, "deflate")),
        ("/corrupt", error(11, "gzip")),
        ("/oversized", error(12, "")),
    ] {
        program.call_waiting(
            "hd:Http",
            "send",
            request(0, &server.url(target), &[], b"", None),
            expected,
        );
    }
    assert_program(&scratch, program, vec![], || {});
}

/// Every redirect hop has the same decoded-response contract.
#[test]
fn http_decodes_and_rejects_a_corrupt_intermediate_redirect_body() {
    let scratch = Scratch::new();
    let server = HttpFixture::new(|request| {
        if request.target == "/start" {
            (
                encoded_raw(
                    302,
                    &["gzip"],
                    b"not a gzip stream",
                    &[("Location", "/final")],
                ),
                Duration::ZERO,
            )
        } else {
            panic!("a corrupt redirect body stops before the next request")
        }
    });
    let mut program = Program::new();
    let start = server.url("/start");
    program.call_waiting(
        "hd:Http",
        "send",
        request(0, &start, &[], b"", Some(1)),
        error(11, "gzip"),
    );
    assert_program(&scratch, program, vec![], || {});
}

/// `std-http.decode.bodyless`: an invalid coding body attached to HEAD is
/// neither exposed nor decoded.
#[test]
fn http_head_response_has_no_body_to_decode() {
    let scratch = Scratch::new();
    let server = HttpFixture::new(|_| (encoded_raw(200, &["gzip"], b"", &[]), Duration::ZERO));
    let mut program = Program::new();
    let url = server.url("/");
    program.call_waiting(
        "hd:Http",
        "send",
        request(1, &url, &[], b"", None),
        response(
            200,
            &[("content-encoding", "gzip"), ("content-length", "0")],
            b"",
            &url,
        ),
    );
    assert_program(&scratch, program, vec![], || {});
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
            response(
                status,
                &headers,
                &[0, 255, 195, 169],
                &server.url(&format!("/{status}")),
            ),
        );
    }
    assert_program(&scratch, p, vec![], || {});
    let seen = server.seen.lock().unwrap();
    assert_eq!(seen.len(), 6);
    for r in &*seen {
        assert_eq!(r.values("Host"), [format!("127.0.0.1:{}", server.port())]);
    }
}

/// This Node transport preserves raw pairs (`std-http.headers.typed-fields`), however
/// many. Node's default limit is 1000 pairs. The wasmtime host's HTTP
/// library stops at 128 (ureq-proto `MAX_RESPONSE_HEADERS`), so this runs
/// on the Node host alone.
#[test]
fn http_node_response_keeps_more_than_a_thousand_header_pairs() {
    let scratch = Scratch::new();
    let mut headers = vec![("X", "a"); 1001];
    headers.push(("Content-Length", "0"));
    let server = HttpFixture::new({
        let headers = headers.clone();
        move |_| (raw(200, &headers, b""), Duration::ZERO)
    });
    let mut p = Program::new();
    p.call_waiting(
        "hd:Http",
        "send",
        request(0, &server.url("/"), &[], b"", None),
        response(200, &headers, b"", &server.url("/")),
    );
    assert_node_program(&scratch, p, vec![]);
}

/// The default providers retain raw fields (`std-http.headers.combined` also
/// allows combining them). The values of one name keep their order, so a
/// name that repeats with another name between its values is delivered
/// with its values together. Names compare ignoring case, so `X-A` and
/// `x-a` are one name.
#[test]
fn http_values_of_one_name_keep_their_order_across_other_names() {
    let scratch = Scratch::new();
    let server = HttpFixture::new(|_| {
        let wire = raw(
            200,
            &[
                ("X-A", "1"),
                ("Y", "mid"),
                ("x-a", "2"),
                ("Z", "end"),
                ("X-a", "3"),
                ("Content-Length", "0"),
            ],
            b"",
        );
        (wire, Duration::ZERO)
    });
    let mut p = Program::new();
    p.call_waiting(
        "hd:Http",
        "send",
        request(0, &server.url("/order"), &[], b"", None),
        response(
            200,
            &[
                ("x-a", "1"),
                ("x-a", "2"),
                ("x-a", "3"),
                ("y", "mid"),
                ("z", "end"),
                ("content-length", "0"),
            ],
            b"",
            &server.url("/order"),
        ),
    );
    assert_program(&scratch, p, vec![], || {});
}

/// A provider may expose a combined non-cookie field; cookies always
/// remain separate pairs (`std-http.headers.cookies-separate`).
#[test]
fn http_combined_fields_keep_cookie_values_separate() {
    let scratch = Scratch::new();
    let headers = [
        ("X-Repeat", "one, two"),
        ("Set-Cookie", "a=1; Expires=Wed, 21 Oct 2030 07:28:00 GMT"),
        ("Set-Cookie", "b=2"),
        ("Content-Length", "0"),
    ];
    let server = HttpFixture::new(move |_| (raw(200, &headers, b""), Duration::ZERO));
    let mut program = Program::new();
    program.call_waiting(
        "hd:Http",
        "send",
        request(0, &server.url("/"), &[], b"", None),
        response(200, &headers, b"", &server.url("/")),
    );
    assert_program(&scratch, program, vec![], || {});
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
            response(
                200,
                &[("Content-Length", "2")],
                expected_body,
                &server.url("/echo?query=yes#ignored"),
            ),
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
        response(
            200,
            &[("Content-Length", "2")],
            b"ok",
            &server.url("/framed"),
        ),
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
            response(200, &[("Content-Length", "0")], b"", &server.url("/final")),
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

/// `std-http.url.grant`, `.no-normalize`: a host is matched and connected
/// as written, in lowercase. `127.1` is a name for the resolver, not the
/// address `127.0.0.1`, so only a `127.1` entry grants it.
#[test]
fn http_host_is_taken_as_written_in_lowercase() {
    let scratch = Scratch::new();
    let server = HttpFixture::new(|_| (raw(200, &[("Content-Length", "0")], b""), Duration::ZERO));
    let port = server.port();
    let mut p = Program::new();
    // The IPv4 shorthand is no `127.0.0.1`, so that entry does not cover it.
    for written in ["127.1", "0x7f.1", "2130706433", "127.0.0.1."] {
        p.call_waiting(
            "hd:Http",
            "send",
            request(0, &format!("http://{written}:{port}/"), &[], b"", None),
            error(0, written),
        );
    }
    // A host is compared in lowercase, and reported in lowercase.
    p.call_waiting(
        "hd:Http",
        "send",
        request(0, &format!("http://EXAMPLE.test:{port}/"), &[], b"", None),
        error(0, "example.test"),
    );
    assert_program(
        &scratch,
        p,
        vec![("Http", Limit::Entries(vec![format!("127.0.0.1:{port}")]))],
        || {},
    );
    assert!(server.seen.lock().unwrap().is_empty());
}

/// A `127.1` entry grants `127.1` (`std-http.url.grant`), and the host
/// goes to the OS resolver as written (`.no-normalize`). By RFC 3986 7.4
/// that name is the resolver's to read: glibc and Darwin turn it into the
/// loopback address, Windows finds no such host. So the test asks the
/// same resolver and expects a response or `Dns("127.1")` to match. The
/// `Host` header is the host as written, in lowercase.
#[test]
fn http_short_ipv4_host_is_granted_as_written_and_left_to_the_resolver() {
    let scratch = Scratch::new();
    let server = HttpFixture::new(|_| (raw(200, &[("Content-Length", "0")], b""), Duration::ZERO));
    let port = server.port();
    let resolves = match ("127.1", port).to_socket_addrs() {
        Ok(mut addresses) => {
            assert!(
                addresses.any(|a| a.ip() == std::net::Ipv4Addr::LOCALHOST),
                "the resolver turns 127.1 into a loopback address"
            );
            true
        }
        Err(_) => false,
    };
    let ok = |host: &str| {
        response(
            200,
            &[("Content-Length", "0")],
            b"",
            &format!("http://{host}:{port}/"),
        )
    };
    let mut p = Program::new();
    p.call_waiting(
        "hd:Http",
        "send",
        request(0, &format!("http://127.1:{port}/"), &[], b"", None),
        if resolves {
            ok("127.1")
        } else {
            error(2, "127.1")
        },
    );
    p.call_waiting(
        "hd:Http",
        "send",
        request(0, &format!("http://LocalHost:{port}/"), &[], b"", None),
        ok("LocalHost"),
    );
    assert_program(
        &scratch,
        p,
        vec![(
            "Http",
            Limit::Entries(vec![format!("127.1:{port}"), format!("localhost:{port}")]),
        )],
        || {},
    );
    let seen = server.seen.lock().unwrap();
    let hosts: Vec<_> = seen.iter().map(|r| r.values("Host")).collect();
    let mut expected = Vec::new();
    // Each engine sends both requests, one after the other.
    for _ in 0..2 {
        if resolves {
            expected.push(vec![format!("127.1:{port}")]);
        }
        expected.push(vec![format!("localhost:{port}")]);
    }
    assert_eq!(hosts, expected);
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
        response(200, &[("Content-Length", "0")], b"", &server.url("/10")),
    );
    assert_program(&scratch, p, vec![], || {});
    assert_eq!(server.seen.lock().unwrap().len(), 22);
}

#[test]
fn http_redirect_limits_control_following() {
    let scratch = Scratch::new();
    let server = HttpFixture::new(|_| {
        (
            raw(302, &[("Location", "/never"), ("Content-Length", "0")], b""),
            Duration::ZERO,
        )
    });
    let url = server.url("/first");
    let mut p = Program::new();
    p.call_waiting(
        "hd:Http",
        "send",
        request(0, &url, &[], b"", Some(0)),
        response(
            302,
            &[("Location", "/never"), ("Content-Length", "0")],
            b"",
            &url,
        ),
    );
    p.call_waiting(
        "hd:Http",
        "send",
        request(0, &url, &[], b"", Some(1)),
        error(6, &server.url("/never")),
    );
    assert_program(&scratch, p, vec![], || {});
    assert_eq!(server.seen.lock().unwrap().len(), 6);
}

#[test]
fn http_invalid_urls() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    for url in [
        "",
        "/relative",
        "ftp://127.0.0.1/x",
        "http://[bad/",
        "http://127.0.0.1:99999/",
        "http://b\u{fc}cher.example/",
        "http://b%C3%BCcher.example/",
    ] {
        p.call_waiting(
            "hd:Http",
            "send",
            request(0, url, &[], b"", None),
            error(1, url),
        );
    }
    assert_program(&scratch, p, vec![], || {});
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
        .leb(0)
        .bytes(b"")
        .leb(10)
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
fn http_loopback_tls_alert_is_tls_error_on_both_providers() {
    use std::io::{Read, Write};
    let scratch = Scratch::new();
    let server = TcpFixture::new(|mut peer| {
        let mut header = [0; 5];
        peer.read_exact(&mut header).unwrap();
        assert_eq!(header[0], 22);
        let mut hello = vec![0; usize::from(u16::from_be_bytes([header[3], header[4]]))];
        peer.read_exact(&mut hello).unwrap();
        // A fatal `unknown_ca` alert. Node reports it as `EPROTO`.
        peer.write_all(&[21, 3, 3, 0, 2, 2, 48]).unwrap();
        let _ = peer.read_to_end(&mut Vec::new());
    });
    let p = tls_program(&format!("https://127.0.0.1:{}/", server.address.port()));
    assert_program(&scratch, p, vec![], || {});
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
        (b"HTTP/1.1 103 Early Hints\r\nLink: discarded\r\n\r\nHTTP/1.1 200 Test\r\nTransfer-Encoding: chunked\r\nX-Final: retained\r\n\r\n2\r\n\0\xff\r\n2\r\nab\r\n0\r\n\r\n".to_vec(), Duration::ZERO)
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
            &server.url("/chunked"),
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
            response(
                status,
                &[("Content-Length", "0")],
                b"",
                &server.url(&format!("/{status}")),
            ),
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
fn http_malformed_redirect_location_preserves_error_text() {
    let scratch = Scratch::new();
    let server = HttpFixture::new(|_| {
        let head = raw(
            302,
            &[("Location", "http://[bad/"), ("Content-Length", "0")],
            b"",
        );
        (head, Duration::ZERO)
    });
    let mut p = Program::new();
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
        request(0, &url, &[], b"", None),
        response(200, &[("Content-Length", "0")], b"", &url),
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
