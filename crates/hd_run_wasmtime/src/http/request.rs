use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hd_run::Grants;
use ureq::http::{HeaderName, HeaderValue};
use ureq::unversioned::transport::{Connector, RustlsConnector};

use crate::codec::{Dec, Enc};
use crate::limits::{HeapExhausted, Ledger, Reservation};

use super::client::granted_url;
use super::connector::{Cancel, HttpResolver, HttpTcpConnector};
use super::target::{Target, resolve};
use super::{Headers, is_token};

pub(crate) struct Request {
    method: String,
    pub(crate) url: String,
    headers: Headers,
    body: Vec<u8>,
    timeout: i64,
}

/// The most bytes of a response body one read takes.
const BODY_CHUNK: usize = 16 * 1024;

impl Request {
    /// The bytes of the body, which the request holds until it ends.
    pub(crate) fn body_len(&self) -> usize {
        self.body.len()
    }

    pub(crate) fn timeout(&self) -> Duration {
        Duration::from_millis(u64::try_from(self.timeout).unwrap_or(0))
    }

    pub(crate) fn decode(bytes: &[u8]) -> wasmtime::Result<Self> {
        let mut d = Dec::new(bytes);
        let method = match d.leb()? {
            0 => "GET".into(),
            1 => "HEAD".into(),
            2 => "POST".into(),
            3 => "PUT".into(),
            4 => "PATCH".into(),
            5 => "DELETE".into(),
            6 => "OPTIONS".into(),
            _ => d.str()?,
        };
        let url = d.str()?;
        let headers = (0..d.leb()?)
            .map(|_| Ok((d.str()?, d.str()?)))
            .collect::<wasmtime::Result<_>>()?;
        let body = d.bytes()?.to_vec();
        let timeout = if d.leb()? == 1 { d.zz()? } else { 30_000 };
        Ok(Self {
            method,
            url,
            headers,
            body,
            timeout,
        })
    }
}

#[derive(Debug)]
pub(crate) struct Response {
    status: u16,
    headers: Headers,
    body: Vec<u8>,
    /// The heap the body holds.
    held: Reservation,
}

impl Response {
    /// The response with its body's heap released, for `.finish` to copy.
    pub(crate) fn released(mut self) -> Response {
        drop(self.held.take());
        self
    }
}

/// Why a request ended with no response.
#[derive(Debug)]
pub(crate) enum Ended {
    Failed(Failure),
    /// The body would pass the heap budget (`std-host.op.heap.exhausted`).
    HeapExhausted,
}

impl From<Failure> for Ended {
    fn from(failure: Failure) -> Ended {
        Ended::Failed(failure)
    }
}

impl From<HeapExhausted> for Ended {
    fn from(_: HeapExhausted) -> Ended {
        Ended::HeapExhausted
    }
}

#[derive(Debug)]
pub(crate) enum Failure {
    NotGranted(String),
    InvalidUrl(String),
    Dns(String),
    Connect(String),
    Tls(String),
    Timeout,
    TooManyRedirects(String),
    Other(String),
}

pub(crate) fn encode(response: Result<Response, Failure>) -> Enc {
    let mut e = Enc::default();
    match response {
        Ok(r) => {
            e.leb(0)
                .leb(u64::from(r.status))
                .list(&r.headers, |e, (name, value)| {
                    e.str(name).str(value);
                })
                .bytes(&r.body);
        }
        Err(failure) => {
            let (variant, text) = match failure {
                Failure::NotGranted(s) => (0, Some(s)),
                Failure::InvalidUrl(s) => (1, Some(s)),
                Failure::Dns(s) => (2, Some(s)),
                Failure::Connect(s) => (3, Some(s)),
                Failure::Tls(s) => (4, Some(s)),
                Failure::Timeout => (5, None),
                Failure::TooManyRedirects(s) => (6, Some(s)),
                Failure::Other(s) => (7, Some(s)),
            };
            e.leb(1).leb(variant);
            if let Some(text) = text {
                e.str(&text);
            }
        }
    }
    e
}

/// Sends `request`, following its redirects, each rechecked against
/// `grants` before any DNS or I/O, until `deadline`. It blocks, so it runs
/// on the blocking pool; `cancel` ends it at once. Each response body is
/// charged to `ledger` as it is read.
pub(crate) fn send(
    mut request: Request,
    grants: &Grants,
    deadline: Option<Instant>,
    cancel: &Cancel,
    ledger: &Arc<Ledger>,
) -> Result<Response, Ended> {
    for redirects in 0..=10 {
        let url = granted_url(&request.url, grants)?;
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(Failure::Timeout.into());
        }
        let got = once(&url, &request, deadline, cancel, ledger)?;
        let location = got
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("location"))
            .map(|(_, value)| value.as_str());
        if !matches!(got.status, 301 | 302 | 303 | 307 | 308) || location.is_none() {
            return Ok(got);
        }
        let location = location.unwrap_or_default();
        // A character outside printable ASCII is no RFC 3986 URL
        // (`std-http.url.valid`). Node catches a malformed Location as
        // Other, not InvalidUrl.
        if !location.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(Failure::InvalidUrl(location.into()).into());
        }
        let next = resolve(&request.url, location);
        if next.parse::<ureq::http::Uri>().is_err() {
            return Err(Failure::Other("Invalid URL".into()).into());
        }
        request.url = next;
        if redirects == 10 {
            return Err(Failure::TooManyRedirects(request.url).into());
        }
        if got.status == 303 || (matches!(got.status, 301 | 302) && request.method == "POST") {
            request.method = "GET".into();
            request.body.clear();
        }
    }
    unreachable!("the eleventh redirect returns above")
}

fn once(
    url: &Target,
    request: &Request,
    deadline: Option<Instant>,
    cancel: &Cancel,
    ledger: &Arc<Ledger>,
) -> Result<Response, Ended> {
    let method = if request.method.is_empty() {
        "GET".into()
    } else if is_token(&request.method) {
        request.method.to_ascii_uppercase()
    } else {
        return Err(Failure::Other(format!(
            "Method must be a valid HTTP token [\"{}\"]",
            request.method
        ))
        .into());
    };
    let internal = outgoing(url, &method, &request.headers)?;
    let candidates = Arc::new(Mutex::new(None));
    let connector = HttpTcpConnector {
        deadline,
        candidates: candidates.clone(),
        cancel: cancel.clone(),
    }
    .chain(RustlsConnector::default());
    let config = ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .proxy(None)
        .user_agent("")
        .accept("")
        .accept_encoding("")
        .max_idle_connections(0)
        .allow_non_standard_methods(true)
        // An unrepresentably distant deadline is effectively infinite;
        // avoid overflowing ureq's Instant arithmetic for i64 durations.
        .timeout_global(deadline.map(|deadline| deadline.saturating_duration_since(Instant::now())))
        .build();
    let agent = ureq::Agent::with_parts(
        config,
        connector,
        HttpResolver {
            candidates,
            deadline,
            #[cfg(test)]
            lookup: None,
        },
    );
    // The URL's credentials never become an Authorization header, but a
    // malformed percent-encoding is still an error.
    for credential in &url.credentials {
        check_credentials(credential)?;
    }
    // An empty body sends no body, so a GET carries no `content-length`.
    let sent = if request.body.is_empty() {
        agent.run(
            internal
                .body(())
                .map_err(|e| Failure::Other(e.to_string()))?,
        )
    } else {
        agent.run(
            internal
                .body(request.body.as_slice())
                .map_err(|e| Failure::Other(e.to_string()))?,
        )
    };
    let mut response = sent.map_err(|e| failure(&e, url))?;
    let status = response.status().as_u16();
    // `std-http.headers.case` and `.order`: ureq's header map lowercases
    // each name and keeps the values of one name in order.
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            let text = value.as_bytes().iter().copied().map(char::from).collect();
            (name.as_str().to_owned(), text)
        })
        .collect();
    let (body, held) = read_body(response.body_mut(), url, ledger)?;
    Ok(Response {
        status,
        headers,
        body,
        held,
    })
}

/// Reads the whole body, reserving each chunk before keeping it, so a body
/// past the heap budget stops the read. No other limit applies
/// (`std-host.op.heap.no-other-cap`).
fn read_body(
    body: &mut ureq::Body,
    url: &Target,
    ledger: &Arc<Ledger>,
) -> Result<(Vec<u8>, Reservation), Ended> {
    let mut reader = body.with_config().reader();
    let mut bytes = Vec::new();
    let mut held = Reservation::empty(ledger);
    let mut chunk = vec![0; BODY_CHUNK];
    loop {
        let n = match reader.read(&mut chunk) {
            Ok(0) => return Ok((bytes, held)),
            Ok(n) => n,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(failure(&ureq::Error::from(error), url).into()),
        };
        held.grow(n)?;
        bytes.extend_from_slice(&chunk[..n]);
    }
}

fn failure(error: &ureq::Error, url: &Target) -> Failure {
    let host = url.host.as_str();
    match error {
        ureq::Error::Timeout(_) => Failure::Timeout,
        ureq::Error::HostNotFound => Failure::Dns(host.into()),
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => Failure::Tls(error.to_string()),
        ureq::Error::Io(e)
            if e.get_ref()
                .is_some_and(<dyn std::error::Error + Send + Sync>::is::<rustls::Error>) =>
        {
            Failure::Tls(e.to_string())
        }
        ureq::Error::Io(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
            Failure::Connect(format!("connect ECONNREFUSED {host}:{}", url.port))
        }
        ureq::Error::Io(e) => Failure::Connect(e.to_string()),
        _ => Failure::Connect(error.to_string()),
    }
}

fn latin1(value: &str) -> Option<Vec<u8>> {
    value
        .chars()
        .map(|c| u8::try_from(u32::from(c)).ok())
        .collect()
}

/// The request ureq sends, without its body: the caller's headers in
/// order, with ureq supplying `Host` and the body's framing, and
/// `Connection: close` unless the caller names its own.
pub(crate) fn outgoing(
    url: &Target,
    method: &str,
    headers: &Headers,
) -> Result<ureq::http::request::Builder, Failure> {
    let mut builder = ureq::http::Request::builder().method(method).uri(url.uri());
    for (name, value) in headers {
        let invalid = |what: &str| Failure::Other(format!("{what} [\"{name}\"]"));
        if !is_token(name) {
            return Err(invalid("Header name must be a valid HTTP token"));
        }
        let bytes = latin1(value)
            .filter(|bytes| bytes.iter().all(|b| *b == 9 || (*b >= 32 && *b != 127)))
            .ok_or_else(|| invalid("Invalid character in header content"))?;
        let value = HeaderValue::from_bytes(&bytes)
            .map_err(|_| invalid("Invalid character in header content"))?;
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| invalid("Header name must be a valid HTTP token"))?;
        builder = builder.header(name, value);
    }
    if !headers
        .iter()
        .any(|(n, _)| n.eq_ignore_ascii_case("connection"))
    {
        builder = builder.header("connection", "close");
    }
    Ok(builder)
}

fn check_credentials(value: &str) -> Result<(), Failure> {
    let mut decoded = Vec::new();
    let mut bytes = value.bytes();
    while let Some(b) = bytes.next() {
        decoded.push(if b == b'%' {
            let high = bytes.next().and_then(|b| char::from(b).to_digit(16));
            let low = bytes.next().and_then(|b| char::from(b).to_digit(16));
            match (high, low) {
                (Some(high), Some(low)) => u8::try_from(high * 16 + low).unwrap_or(0),
                _ => return Err(Failure::Other("URI malformed".into())),
            }
        } else {
            b
        });
    }
    String::from_utf8(decoded)
        .map(|_| ())
        .map_err(|_| Failure::Other("URI malformed".into()))
}

#[cfg(test)]
mod tests {
    use super::{Failure, Request, encode};
    use crate::codec::{Dec, Enc};
    use crate::http::test_support::{bounded_accept, read_request, request, send, write_response};
    use hd_run::{Grants, Limit};
    use std::net::TcpListener;

    #[test]
    fn request_codec_and_errors_use_the_node_variant_order() {
        let wire = request("HEAD", "http://127.0.0.1/", &vec![], b"body", -1);
        let r = Request::decode(wire.done()).unwrap();
        assert_eq!(r.method, "HEAD");
        assert_eq!(r.timeout, -1);
        assert_eq!(r.body, b"body");
        assert!(Request::decode(&[128]).is_err());
        assert_eq!(encode(Err(Failure::Timeout)).done(), [1, 5]);
        for (failure, variant) in [
            (Failure::NotGranted("host".into()), 0),
            (Failure::InvalidUrl("url".into()), 1),
            (Failure::Dns("host".into()), 2),
            (Failure::Connect("message".into()), 3),
            (Failure::Tls("message".into()), 4),
            (Failure::TooManyRedirects("url".into()), 6),
            (Failure::Other("message".into()), 7),
        ] {
            let encoded = encode(Err(failure));
            let mut d = Dec::new(encoded.done());
            assert_eq!(d.leb().unwrap(), 1);
            assert_eq!(d.leb().unwrap(), variant);
            assert!(!d.str().unwrap().is_empty());
        }
    }

    #[test]
    fn invalid_urls_and_denials_precede_timeout_and_header_validation() {
        let deny = Grants {
            limits: vec![("Http".into(), Limit::Deny)],
        };
        let r = request(
            "bad method",
            "http://127.0.0.1:1/",
            &vec![("bad name".into(), "\0".into())],
            &[],
            0,
        );
        let result = send(r.done(), &deny);
        let mut expected = Enc::default();
        expected.leb(1).leb(0).str("127.0.0.1");
        assert_eq!(result.done(), expected.done());
        let r = request("GET", "relative", &vec![], &[], 0);
        let result = send(r.done(), &deny);
        let mut expected = Enc::default();
        expected.leb(1).leb(1).str("relative");
        assert_eq!(result.done(), expected.done());
        let r = request("GET", "http://127.0.0.1:1/", &vec![], &[], 0);
        assert_eq!(send(r.done(), &Grants::default()).done(), [1, 5]);
    }

    /// `std-http.url.valid`, `.non-ascii`, `.grant`, `.no-normalize`.
    #[test]
    fn a_non_ascii_host_is_invalid_and_a_grant_matches_the_lowercase_host() {
        let deny = Grants {
            limits: vec![("Http".into(), Limit::Deny)],
        };
        let denied = |url: &str| {
            let r = request("GET", url, &vec![], &[], 1000);
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
        let only = Grants {
            limits: vec![("Http".into(), Limit::Entries(vec!["Example.COM".into()]))],
        };
        let r = request("GET", "http://EXAMPLE.com:1/", &vec![], &[], 0);
        assert_eq!(send(r.done(), &only).done(), [1, 5]);
        let r = request("GET", "http://example.org:1/", &vec![], &[], 0);
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
        let r = request("GET", &url, &vec![], &[], 2000);
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
            let r = request("GET", &url, &vec![], &[], 2000);
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
            let r = request(method, &url, &headers, body, 1000);
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
        let grant = Grants {
            limits: vec![("Http".into(), Limit::Entries(vec![address.to_string()]))],
        };
        let r = request("GET", &format!("http://{address}/"), &vec![], &[], 1000);
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
