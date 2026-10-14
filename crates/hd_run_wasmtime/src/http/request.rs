use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hd_run::Grants;
use ureq::unversioned::transport::{Connector, RustlsConnector};
use url::Url;

use crate::codec::{Dec, Enc};
use crate::limits::{HeapExhausted, Ledger, Reservation};

use super::client::granted_url;
use super::connector::{Cancel, HttpResolver, HttpTcpConnector};
use super::transport::RawConnector;
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
        // Node catches a malformed Location as Other, not InvalidUrl.
        request.url = url
            .join(location.unwrap_or_default())
            .map_err(|_| Failure::Other("Invalid URL".into()))?
            .into();
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
    url: &Url,
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
    let raw = outgoing(url, &method, &request.headers, &request.body)?;
    let captured = Arc::new(Mutex::new(None));
    let candidates = Arc::new(Mutex::new(None));
    let connector = HttpTcpConnector {
        deadline,
        candidates: candidates.clone(),
        cancel: cancel.clone(),
    }
    .chain(RustlsConnector::default())
    .chain(RawConnector {
        raw,
        captured: captured.clone(),
        head: method == "HEAD",
    });
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
    let mut target = url.clone();
    // Node's raw headers skip automatic Basic Authorization. URL-to-HTTP
    // options still validates percent-encoded credentials before sending.
    for credential in [target.username(), target.password().unwrap_or_default()] {
        check_credentials(credential)?;
    }
    let _ = target.set_username("");
    let _ = target.set_password(None);
    target.set_fragment(None);
    // The raw transport writes the actual method/body. HEAD is the one
    // method whose response-body framing differs; no ureq request body
    // means its stricter method/content-length checks cannot alter Node's
    // wire request (including explicit framing supplied by the caller).
    let internal = ureq::http::Request::builder()
        .method(if method == "HEAD" { "HEAD" } else { "GET" })
        .uri(target.as_str())
        .header("connection", "close")
        .body(())
        .map_err(|e| Failure::Other(e.to_string()))?;
    let mut response = agent.run(internal).map_err(|e| failure(&e, url))?;
    let status = response.status().as_u16();
    let (body, held) = read_body(response.body_mut(), url, ledger)?;
    let headers = captured
        .lock()
        .map_err(|_| Failure::Other("the Http header lock is poisoned".into()))?
        .take()
        .ok_or_else(|| Failure::Connect("the response has no complete headers".into()))?;
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
    url: &Url,
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

fn failure(error: &ureq::Error, url: &Url) -> Failure {
    let host = url.host_str().unwrap_or_default().trim_matches(['[', ']']);
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
            Failure::Connect(format!(
                "connect ECONNREFUSED {host}:{}",
                url.port_or_known_default().unwrap_or(80)
            ))
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

pub(crate) fn outgoing(
    url: &Url,
    method: &str,
    headers: &Headers,
    body: &[u8],
) -> Result<Vec<u8>, Failure> {
    let mut pairs = headers.clone();
    for (name, value) in &pairs {
        if !is_token(name) {
            return Err(Failure::Other(format!(
                "Header name must be a valid HTTP token [\"{name}\"]"
            )));
        }
        if value.chars().any(|c| {
            let code = u32::from(c);
            code != 9 && (code < 32 || code == 127 || code > 255)
        }) {
            return Err(Failure::Other(format!(
                "Invalid character in header content [\"{name}\"]"
            )));
        }
    }
    let has = |pairs: &Headers, name: &str| pairs.iter().any(|(n, _)| n.eq_ignore_ascii_case(name));
    if !body.is_empty() && !has(&pairs, "content-length") && !has(&pairs, "transfer-encoding") {
        pairs.push(("Content-Length".into(), body.len().to_string()));
    }
    if !has(&pairs, "host") {
        let authority = &url[url::Position::BeforeHost..url::Position::AfterPort];
        pairs.insert(0, ("Host".into(), authority.into()));
    }
    if !has(&pairs, "connection") {
        pairs.push(("Connection".into(), "close".into()));
    }
    if !has(&pairs, "content-length")
        && !has(&pairs, "transfer-encoding")
        && !matches!(
            method,
            "GET" | "HEAD" | "DELETE" | "OPTIONS" | "TRACE" | "CONNECT"
        )
    {
        // Node renders raw header arrays in the request constructor, before
        // end() supplies an empty body's length, so it uses chunked here.
        pairs.push(("Transfer-Encoding".into(), "chunked".into()));
    }
    let chunked = pairs.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("transfer-encoding")
            && value
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .any(|word| word.eq_ignore_ascii_case("chunked"))
    });
    let target = &url[url::Position::BeforePath..url::Position::AfterQuery];
    let mut raw = format!("{method} {target} HTTP/1.1\r\n").into_bytes();
    for (name, value) in pairs {
        raw.extend_from_slice(name.as_bytes());
        raw.extend_from_slice(b": ");
        raw.extend(latin1(&value).unwrap_or_default());
        raw.extend_from_slice(b"\r\n");
    }
    raw.extend_from_slice(b"\r\n");
    if chunked {
        if !body.is_empty() {
            raw.extend_from_slice(format!("{:x}\r\n", body.len()).as_bytes());
            raw.extend_from_slice(body);
            raw.extend_from_slice(b"\r\n");
        }
        raw.extend_from_slice(b"0\r\n\r\n");
    } else {
        raw.extend_from_slice(body);
    }
    Ok(raw)
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
    use super::{Failure, Request, encode, outgoing};
    use crate::codec::{Dec, Enc};
    use crate::http::test_support::{bounded_accept, read_request, request, send, write_response};
    use hd_run::{Grants, Limit};
    use std::net::TcpListener;
    use url::Url;

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

    #[test]
    fn outgoing_preserves_authority_header_pairs_latin1_and_framing() {
        let url = Url::parse("http://[::1]:8080/p?q#fragment").unwrap();
        let headers = vec![
            ("X-First".into(), "one".into()),
            ("x-Second".into(), "é".into()),
            ("X-First".into(), "two".into()),
        ];
        let raw = outgoing(&url, "GET", &headers, b"body").unwrap();
        assert_eq!(raw, b"GET /p?q HTTP/1.1\r\nHost: [::1]:8080\r\nX-First: one\r\nx-Second: \xe9\r\nX-First: two\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody");
        let raw = outgoing(&url, "POST", &vec![], &[]).unwrap();
        assert_eq!(raw, b"POST /p?q HTTP/1.1\r\nHost: [::1]:8080\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n");
        let raw = outgoing(
            &url,
            "HEAD",
            &vec![
                ("hOsT".into(), "custom".into()),
                ("Transfer-Encoding".into(), "chunked".into()),
            ],
            b"body",
        )
        .unwrap();
        assert_eq!(raw, b"HEAD /p?q HTTP/1.1\r\nhOsT: custom\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n4\r\nbody\r\n0\r\n\r\n");
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
