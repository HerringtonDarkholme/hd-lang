use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use flate2::read::{MultiGzDecoder, ZlibDecoder};
use hd_run::Grants;
use ureq::http::{HeaderName, HeaderValue};
use ureq::unversioned::transport::{Connector, RustlsConnector};

pub use crate::abi::WireHttpError as HttpError;
use crate::abi::{self, Method};
use crate::codec::{self, Dec, Enc, Wire};

use super::client::granted_url;
use super::connector::{Cancel, HttpResolver, HttpTcpConnector, Phase, earlier};
use super::target::{Target, resolve};
use super::tls::{HopTls, client_config};
use super::{Headers, is_token};

pub const MAX_DECODED_BYTES: usize = 64 * 1024 * 1024;

pub struct Request {
    pub method: String,
    pub url: String,
    pub headers: Headers,
    pub body: Vec<u8>,
    pub redirects: usize,
    pub settings: Settings,
}

/// A derived provider's transport settings (`std-http.settings.*`); each
/// `None` or empty value is the host default.
#[derive(Clone, Debug, Default)]
pub struct Settings {
    pub dns_servers: Option<Vec<String>>,
    pub tls_roots: Vec<String>,
    pub tls_identity: Option<abi::WireIdentity>,
    pub connect_timeout: Option<std::time::Duration>,
    pub tls_handshake_timeout: Option<std::time::Duration>,
    pub response_header_timeout: Option<std::time::Duration>,
    pub body_read_timeout: Option<std::time::Duration>,
}

impl Settings {
    /// The stable name of each setting changed from the host default, in
    /// the order every host reports them (`std-http.settings.unsupported-name`).
    pub(crate) fn changed(&self) -> Vec<&'static str> {
        [
            ("resolver", self.dns_servers.is_some()),
            ("tls.roots", !self.tls_roots.is_empty()),
            ("tls.identity", self.tls_identity.is_some()),
            ("connect_timeout", self.connect_timeout.is_some()),
            (
                "tls_handshake_timeout",
                self.tls_handshake_timeout.is_some(),
            ),
            (
                "response_header_timeout",
                self.response_header_timeout.is_some(),
            ),
            ("body_read_timeout", self.body_read_timeout.is_some()),
        ]
        .into_iter()
        .filter_map(|(name, changed)| changed.then_some(name))
        .collect()
    }
}

/// A wire duration as a std one; std never builds a negative one.
fn duration(value: Option<abi::Duration>) -> Option<std::time::Duration> {
    value.map(|d| std::time::Duration::from_millis(u64::try_from(d.millis).unwrap_or(0)))
}

impl Request {
    pub fn decode(bytes: &[u8]) -> wasmtime::Result<Self> {
        let abi::WireRequest {
            method,
            url,
            headers,
            body,
            redirects,
            dns_servers,
            tls_roots,
            tls_identity,
            connect_timeout,
            tls_handshake_timeout,
            response_header_timeout,
            body_read_timeout,
        } = Wire::dec(&mut Dec::new(bytes))?;
        let settings = Settings {
            dns_servers,
            tls_roots,
            tls_identity,
            connect_timeout: duration(connect_timeout),
            tls_handshake_timeout: duration(tls_handshake_timeout),
            response_header_timeout: duration(response_header_timeout),
            body_read_timeout: duration(body_read_timeout),
        };
        let method = match method {
            Method::Get => "GET".into(),
            Method::Head => "HEAD".into(),
            Method::Post => "POST".into(),
            Method::Put => "PUT".into(),
            Method::Patch => "PATCH".into(),
            Method::Delete => "DELETE".into(),
            Method::Options => "OPTIONS".into(),
            Method::Other { name } => name,
        };
        Ok(Self {
            method,
            url,
            headers,
            body,
            redirects,
            settings,
        })
    }
}

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    headers: Headers,
    pub body: Vec<u8>,
    url: String,
}

/// A send's encoded result.
#[must_use]
pub fn encode(response: Result<Response, HttpError>) -> Enc {
    codec::encode(&response.map(|r| abi::WireResponse {
        status: r.status,
        headers: r.headers,
        body: r.body,
        url: r.url,
    }))
}

/// Sends `request`, following its redirects, each rechecked against
/// `grants` before any DNS or I/O, until `deadline`. It blocks, so it runs
/// on the blocking pool; `cancel` ends it at once.
pub fn send(
    mut request: Request,
    grants: &Grants,
    deadline: Option<Instant>,
    cancel: &Cancel,
) -> Result<Response, HttpError> {
    for redirects in 0..=request.redirects {
        let url = granted_url(&request.url, grants)?;
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(HttpError::Timeout);
        }
        let got = once(&url, &request, deadline, cancel)?;
        let location = got
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("location"))
            .map(|(_, value)| value.as_str());
        if request.redirects == 0
            || !matches!(got.status, 301 | 302 | 303 | 307 | 308)
            || location.is_none()
        {
            return Ok(Response {
                url: request.url,
                ..got
            });
        }
        let location = location.unwrap_or_default();
        // A character outside printable ASCII is no RFC 3986 URL
        // (`std-http.url.valid`). Node catches a malformed Location as
        // Other, not InvalidUrl.
        if !location.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(HttpError::InvalidUrl {
                url: location.into(),
            });
        }
        let next = resolve(&request.url, location);
        if next.parse::<ureq::http::Uri>().is_err() {
            return Err(HttpError::Other {
                message: "Invalid URL".into(),
            });
        }
        request.url = next;
        if redirects == request.redirects {
            return Err(HttpError::TooManyRedirects { url: request.url });
        }
        if got.status == 303 || (matches!(got.status, 301 | 302) && request.method == "POST") {
            request.method = "GET".into();
            request.body.clear();
        }
    }
    unreachable!("exceeding the redirect limit returns above")
}

fn once(
    url: &Target,
    request: &Request,
    deadline: Option<Instant>,
    cancel: &Cancel,
) -> Result<Response, HttpError> {
    let method = if request.method.is_empty() {
        "GET".into()
    } else if is_token(&request.method) {
        request.method.to_ascii_uppercase()
    } else {
        return Err(HttpError::Other {
            message: format!("Method must be a valid HTTP token [\"{}\"]", request.method),
        });
    };
    let internal = outgoing(url, &method, &request.headers)?;
    let candidates = Arc::new(Mutex::new(None));
    // The connect phase begins before DNS and is reset for each redirect
    // hop (`std-http.settings.phase-timeouts`).
    let connect_deadline = request
        .settings
        .connect_timeout
        .and_then(|limit| Instant::now().checked_add(limit));
    let phase = Phase::default();
    let connector = HttpTcpConnector {
        deadline,
        connect_deadline,
        candidates: candidates.clone(),
        cancel: cancel.clone(),
        phase: phase.clone(),
    }
    .chain(HopTls {
        config: client_config(&request.settings)?,
        handshake_timeout: request.settings.tls_handshake_timeout,
        phase,
        default: RustlsConnector::default(),
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
        .timeout_recv_response(request.settings.response_header_timeout)
        .timeout_recv_body(request.settings.body_read_timeout)
        .build();
    let agent = ureq::Agent::with_parts(
        config,
        connector,
        HttpResolver {
            candidates,
            deadline: earlier(deadline, connect_deadline),
            cancel: cancel.clone(),
            #[cfg(feature = "testing")]
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
        agent.run(internal.body(()).map_err(|e| HttpError::Other {
            message: e.to_string(),
        })?)
    } else {
        agent.run(
            internal
                .body(request.body.as_slice())
                .map_err(|e| HttpError::Other {
                    message: e.to_string(),
                })?,
        )
    };
    let mut response = sent.map_err(|e| failure(&e, url))?;
    let status = response.status().as_u16();
    // `std-http.headers.case` and `.order`: ureq's header map lowercases
    // each name and keeps the values of one name in order.
    let mut headers: Headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            let text = value.as_bytes().iter().copied().map(char::from).collect();
            (name.as_str().to_owned(), text)
        })
        .collect();
    let body = if method == "HEAD" || (100..200).contains(&status) || matches!(status, 204 | 304) {
        Vec::new()
    } else {
        let codings = content_codings(&headers)?;
        let body = read_body(response.body_mut(), url, &codings)?;
        if !codings.is_empty() {
            headers.retain(|(name, _)| {
                !name.eq_ignore_ascii_case("content-encoding")
                    && !name.eq_ignore_ascii_case("content-length")
            });
        }
        body
    };
    Ok(Response {
        status,
        headers,
        body,
        url: String::new(),
    })
}

fn content_codings(headers: &Headers) -> Result<Vec<String>, HttpError> {
    let mut codings = Vec::new();
    for (_, value) in headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("content-encoding"))
    {
        for part in value.split(',') {
            let coding = part.trim().to_ascii_lowercase();
            if coding.is_empty() || !matches!(coding.as_str(), "gzip" | "deflate" | "identity") {
                return Err(HttpError::InvalidContentEncoding {
                    encoding: if coding.is_empty() {
                        value.clone()
                    } else {
                        coding
                    },
                });
            }
            codings.push(coding);
        }
    }
    Ok(codings)
}

/// Reads and decodes the whole body, stopping one byte past the public cap.
fn read_body(
    body: &mut ureq::Body,
    url: &Target,
    codings: &[String],
) -> Result<Vec<u8>, HttpError> {
    let mut bytes = Vec::new();
    let mut reader = body.with_config().reader();
    let transforms = codings.iter().any(|coding| coding != "identity");
    if transforms {
        reader
            .read_to_end(&mut bytes)
            .map_err(|error| failure(&ureq::Error::from(error), url))?;
    } else {
        reader
            .take((MAX_DECODED_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| failure(&ureq::Error::from(error), url))?;
    }
    decode_bytes(bytes, codings)
}

pub fn decode_bytes(mut bytes: Vec<u8>, codings: &[String]) -> Result<Vec<u8>, HttpError> {
    if bytes.len() > MAX_DECODED_BYTES && codings.iter().all(|coding| coding == "identity") {
        return Err(HttpError::OutputTooLarge);
    }
    for coding in codings.iter().rev() {
        if coding == "identity" {
            continue;
        }
        let input = std::io::Cursor::new(bytes);
        let reader: Box<dyn Read> = match coding.as_str() {
            "gzip" => Box::new(MultiGzDecoder::new(input)),
            "deflate" => Box::new(ZlibDecoder::new(input)),
            _ => unreachable!("content codings are validated above"),
        };
        let mut decoded = Vec::new();
        reader
            .take((MAX_DECODED_BYTES + 1) as u64)
            .read_to_end(&mut decoded)
            .map_err(|_| HttpError::InvalidContentEncoding {
                encoding: coding.clone(),
            })?;
        if decoded.len() > MAX_DECODED_BYTES {
            return Err(HttpError::OutputTooLarge);
        }
        bytes = decoded;
    }
    if bytes.len() > MAX_DECODED_BYTES {
        return Err(HttpError::OutputTooLarge);
    }
    Ok(bytes)
}

fn failure(error: &ureq::Error, url: &Target) -> HttpError {
    let host = url.host.as_str();
    match error {
        ureq::Error::Timeout(_) => HttpError::Timeout,
        ureq::Error::HostNotFound => HttpError::Dns { host: host.into() },
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => HttpError::Tls {
            message: error.to_string(),
        },
        ureq::Error::Io(e)
            if e.get_ref()
                .is_some_and(<dyn std::error::Error + Send + Sync>::is::<rustls::Error>) =>
        {
            HttpError::Tls {
                message: e.to_string(),
            }
        }
        ureq::Error::Io(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
            HttpError::Connect {
                message: format!("connect ECONNREFUSED {host}:{}", url.port),
            }
        }
        ureq::Error::Io(e) => HttpError::Connect {
            message: e.to_string(),
        },
        _ => HttpError::Connect {
            message: error.to_string(),
        },
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
) -> Result<ureq::http::request::Builder, HttpError> {
    let mut builder = ureq::http::Request::builder().method(method).uri(url.uri());
    for (name, value) in headers {
        let invalid = |what: &str| HttpError::Other {
            message: format!("{what} [\"{name}\"]"),
        };
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
        .any(|(name, _)| name.eq_ignore_ascii_case("accept-encoding"))
    {
        let encoding = if headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("range"))
        {
            "identity"
        } else {
            "gzip, deflate"
        };
        builder = builder.header("accept-encoding", encoding);
    }
    if !headers
        .iter()
        .any(|(n, _)| n.eq_ignore_ascii_case("connection"))
    {
        builder = builder.header("connection", "close");
    }
    Ok(builder)
}

fn check_credentials(value: &str) -> Result<(), HttpError> {
    let mut decoded = Vec::new();
    let mut bytes = value.bytes();
    while let Some(b) = bytes.next() {
        decoded.push(if b == b'%' {
            let high = bytes.next().and_then(|b| char::from(b).to_digit(16));
            let low = bytes.next().and_then(|b| char::from(b).to_digit(16));
            match (high, low) {
                (Some(high), Some(low)) => u8::try_from(high * 16 + low).unwrap_or(0),
                _ => {
                    return Err(HttpError::Other {
                        message: "URI malformed".into(),
                    });
                }
            }
        } else {
            b
        });
    }
    String::from_utf8(decoded)
        .map(|_| ())
        .map_err(|_| HttpError::Other {
            message: "URI malformed".into(),
        })
}
