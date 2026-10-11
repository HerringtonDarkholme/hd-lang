//! The default Http provider, on one lazy shared client thread (§20.4).
//! Requests finish inside send.start, as the Node worker's synchronous
//! reply does. Each redirect rechecks the URL's grant before DNS or I/O.
//!
//! ureq supplies TCP, TLS, deadlines and response-body framing. Its
//! `HeaderMap` cannot retain raw header spelling and order, so a plaintext
//! transport wrapper preserves response pairs and writes Node's original
//! request headers and byte body. Compression and environment proxies
//! are deliberately disabled. Timed DNS returns promptly, but std's
//! underlying getaddrinfo call in the DNS helper thread cannot be aborted.

use std::collections::VecDeque;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak, mpsc};
use std::time::{Duration, Instant};

use hd_run::Grants;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, LazyBuffers, NextTimeout, RustlsConnector, TcpConnector,
    Transport,
};
use url::Url;

use crate::codec::{Dec, Enc};
use crate::host::Stop;

type Headers = Vec<(String, String)>;
type Candidates = Arc<Mutex<Option<Vec<SocketAddr>>>>;

#[cfg(test)]
type Lookup = Arc<dyn Fn(String) -> std::io::Result<Vec<SocketAddr>> + Send + Sync>;

#[derive(Default)]
pub struct Client {
    worker: Option<Arc<Worker>>,
}

impl Client {
    #[cfg(test)]
    pub fn send(&mut self, bytes: &[u8], grants: &Grants) -> wasmtime::Result<Enc> {
        self.send_until(bytes, grants, None)
    }

    /// The earlier request/program deadline includes queueing and every hop.
    /// A program deadline traps, rather than becoming an Http error value.
    pub fn send_until(
        &mut self,
        bytes: &[u8],
        grants: &Grants,
        program_deadline: Option<Instant>,
    ) -> wasmtime::Result<Enc> {
        let started = Instant::now();
        if program_deadline.is_some_and(|deadline| started >= deadline) {
            return Err(Stop::TimeLimit.into());
        }
        let request = Request::decode(bytes)?;
        // Preserve invalid-URL/grant precedence even if the shared worker
        // is busy. Queue time belongs to the whole-request deadline.
        if let Err(failure) = granted_url(&request.url, grants) {
            return Ok(encode(Err(failure)));
        }
        let timeout = request.timeout();
        let request_deadline = started.checked_add(timeout);
        let deadline = match (request_deadline, program_deadline) {
            (Some(request), Some(program)) => Some(request.min(program)),
            (request, None) => request,
            (None, program) => program,
        };
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Ok(encode(Err(Failure::Timeout)));
        }
        if self.worker.is_none() {
            self.worker = Some(shared_worker()?);
        }
        let (reply, receive) = mpsc::channel();
        self.worker
            .as_ref()
            .and_then(|w| w.sender.as_ref())
            .ok_or_else(|| Stop::Internal("the Http client is closed".into()))?
            .send(Job {
                request,
                grants: grants.clone(),
                reply,
                deadline,
            })
            .map_err(|_| Stop::Internal("the Http client stopped".into()))?;
        let Some(deadline) = deadline else {
            return receive.recv().map_err(|_| {
                Stop::Internal("the Http client stopped without a reply".into()).into()
            });
        };
        let program_first = program_deadline
            .is_some_and(|program| request_deadline.is_none_or(|request| program <= request));
        match receive.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(reply) if !(program_first && Instant::now() >= deadline) => Ok(reply),
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) if program_first => {
                if let Some(worker) = &self.worker {
                    worker.detach_on_drop.store(true, Ordering::Relaxed);
                }
                Err(Stop::TimeLimit.into())
            }
            Ok(reply) => Ok(reply),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(encode(Err(Failure::Timeout))),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(Stop::Internal("the Http client stopped without a reply".into()).into())
            }
        }
    }
}

struct Worker {
    sender: Option<mpsc::Sender<Job>>,
    thread: Option<std::thread::JoinHandle<()>>,
    detach_on_drop: AtomicBool,
}

impl Drop for Worker {
    fn drop(&mut self) {
        // The worker owns no Arc<Worker>. Closing the final guard drops
        // the queue sender. Usually it is idle; a caller-side timeout may
        // race the active job's same network deadline. A program must not
        // exceed its budget waiting for worker teardown. Only that final
        // join is detached; each expired job still skips all future I/O.
        self.sender.take();
        if let Some(thread) = self.thread.take()
            && (!self.detach_on_drop.load(Ordering::Relaxed) || thread.is_finished())
        {
            let _ = thread.join();
        }
    }
}

struct Job {
    request: Request,
    grants: Grants,
    reply: mpsc::Sender<Enc>,
    deadline: Option<Instant>,
}

fn shared_worker() -> wasmtime::Result<Arc<Worker>> {
    static WORKER: OnceLock<Mutex<Weak<Worker>>> = OnceLock::new();
    let mut shared = WORKER
        .get_or_init(Mutex::default)
        .lock()
        .map_err(|_| Stop::Internal("the Http client lock is poisoned".into()))?;
    if let Some(worker) = shared.upgrade() {
        return Ok(worker);
    }
    let worker = spawn_worker()?;
    *shared = Arc::downgrade(&worker);
    Ok(worker)
}

fn spawn_worker() -> wasmtime::Result<Arc<Worker>> {
    let (sender, receive) = mpsc::channel::<Job>();
    let thread = std::thread::Builder::new()
        .name("hd-http".into())
        .spawn(move || {
            for job in receive {
                let response = if job
                    .deadline
                    .is_some_and(|deadline| Instant::now() >= deadline)
                {
                    Err(Failure::Timeout)
                } else {
                    send(job.request, &job.grants, job.deadline)
                };
                let _ = job.reply.send(encode(response));
            }
        })
        .map_err(|e| Stop::Internal(format!("the Http client cannot start: {e}")))?;
    let worker = Arc::new(Worker {
        sender: Some(sender),
        thread: Some(thread),
        detach_on_drop: AtomicBool::new(false),
    });
    Ok(worker)
}

struct Request {
    method: String,
    url: String,
    headers: Headers,
    body: Vec<u8>,
    timeout: i64,
}

impl Request {
    fn timeout(&self) -> Duration {
        Duration::from_millis(u64::try_from(self.timeout).unwrap_or(0))
    }

    fn decode(bytes: &[u8]) -> wasmtime::Result<Self> {
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
struct Response {
    status: u16,
    headers: Headers,
    body: Vec<u8>,
}

#[derive(Debug)]
enum Failure {
    NotGranted(String),
    InvalidUrl(String),
    Dns(String),
    Connect(String),
    Tls(String),
    Timeout,
    TooManyRedirects(String),
    Other(String),
}

fn encode(response: Result<Response, Failure>) -> Enc {
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

fn granted_url(value: &str, grants: &Grants) -> Result<Url, Failure> {
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

fn send(
    mut request: Request,
    grants: &Grants,
    deadline: Option<Instant>,
) -> Result<Response, Failure> {
    for redirects in 0..=10 {
        let url = granted_url(&request.url, grants)?;
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(Failure::Timeout);
        }
        let got = once(&url, &request, deadline)?;
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
            return Err(Failure::TooManyRedirects(request.url));
        }
        if got.status == 303 || (matches!(got.status, 301 | 302) && request.method == "POST") {
            request.method = "GET".into();
            request.body.clear();
        }
    }
    unreachable!("the eleventh redirect returns above")
}

fn once(url: &Url, request: &Request, deadline: Option<Instant>) -> Result<Response, Failure> {
    let method = if request.method.is_empty() {
        "GET".into()
    } else if is_token(&request.method) {
        request.method.to_ascii_uppercase()
    } else {
        return Err(Failure::Other(format!(
            "Method must be a valid HTTP token [\"{}\"]",
            request.method
        )));
    };
    let raw = outgoing(url, &method, &request.headers, &request.body)?;
    let captured = Arc::new(Mutex::new(None));
    let candidates = Arc::new(Mutex::new(None));
    let connector = HttpTcpConnector {
        deadline,
        candidates: candidates.clone(),
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
    let body = response
        .body_mut()
        .with_config()
        .read_to_vec()
        .map_err(|e| failure(&e, url))?;
    let headers = captured
        .lock()
        .map_err(|_| Failure::Other("the Http header lock is poisoned".into()))?
        .take()
        .ok_or_else(|| Failure::Connect("the response has no complete headers".into()))?;
    Ok(Response {
        status,
        headers,
        body,
    })
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

struct HttpResolver {
    candidates: Candidates,
    deadline: Option<Instant>,
    #[cfg(test)]
    lookup: Option<Lookup>,
}

impl std::fmt::Debug for HttpResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpResolver").finish_non_exhaustive()
    }
}

impl Resolver for HttpResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let address = uri
            .scheme()
            .zip(uri.authority())
            .and_then(|(scheme, authority)| DefaultResolver::host_and_port(scheme, authority))
            .ok_or(ureq::Error::HostNotFound)?;
        #[cfg(test)]
        let lookup = self.lookup.clone();
        let addresses = resolve_lookup(timeout, self.deadline, move || {
            #[cfg(test)]
            if let Some(lookup) = lookup {
                return lookup(address);
            }
            address.to_socket_addrs().map(Iterator::collect)
        })?;
        let addresses: Vec<_> = config
            .ip_family()
            .keep_wanted(addresses.into_iter())
            .collect();
        let first = addresses
            .first()
            .copied()
            .ok_or(ureq::Error::HostNotFound)?;
        *self.candidates.lock().map_err(|_| {
            ureq::Error::Io(std::io::Error::other("the Http candidate lock is poisoned"))
        })? = Some(addresses);
        // ureq's public resolver result has capacity 16. Its connector is
        // ours, so a hop-local side channel carries every std candidate;
        // the bounded result only needs a real, nonempty representative.
        let mut representative = self.empty();
        representative.push(first);
        Ok(representative)
    }
}

fn resolve_lookup(
    timeout: NextTimeout,
    deadline: Option<Instant>,
    lookup: impl FnOnce() -> std::io::Result<Vec<SocketAddr>> + Send + 'static,
) -> Result<Vec<SocketAddr>, ureq::Error> {
    let started = Instant::now();
    let budget = || {
        let relative = match timeout.after {
            ureq::unversioned::transport::time::Duration::Exact(duration) => {
                Some(duration.saturating_sub(started.elapsed()))
            }
            ureq::unversioned::transport::time::Duration::NotHappening => None,
        };
        let absolute = deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
        match (relative, absolute) {
            (Some(relative), Some(absolute)) => Some(relative.min(absolute)),
            (relative, None) => relative,
            (None, absolute) => absolute,
        }
    };
    let Some(initial) = budget() else {
        return lookup().map_err(|_| ureq::Error::HostNotFound);
    };
    if initial.is_zero() {
        return Err(ureq::Error::Timeout(timeout.reason));
    }
    let (reply, receive) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("hd-http-dns".into())
        .spawn(move || {
            let _ = reply.send(lookup());
        })
        .map_err(ureq::Error::Io)?;
    // Spawning the helper must not restart either the relative resolver
    // budget or the absolute whole-request deadline.
    match receive.recv_timeout(budget().unwrap_or(initial)) {
        Ok(result) => result.map_err(|_| ureq::Error::HostNotFound),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(ureq::Error::Timeout(timeout.reason)),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(ureq::Error::Io(std::io::Error::other(
            "the Http DNS helper stopped without a reply",
        ))),
    }
}

#[derive(Debug)]
struct HttpTcpConnector {
    deadline: Option<Instant>,
    candidates: Candidates,
}

impl Connector<()> for HttpTcpConnector {
    type Out = Box<dyn Transport>;

    fn connect(
        &self,
        details: &ConnectionDetails<'_>,
        _: Option<()>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        let started = Instant::now();
        let total = self
            .deadline
            .map(|deadline| deadline.saturating_duration_since(started));
        let addresses = self
            .candidates
            .lock()
            .map_err(|_| {
                ureq::Error::Io(std::io::Error::other("the Http candidate lock is poisoned"))
            })?
            .take()
            .ok_or_else(|| {
                ureq::Error::Io(std::io::Error::other(
                    "the Http resolver returned no candidates",
                ))
            })?;
        connect_candidates(
            addresses,
            total,
            || started.elapsed(),
            |address, budget| {
                let mut addrs = ResolvedSocketAddrs::from_fn(|_| address);
                addrs.push(address);
                let mut timeout = details.timeout;
                timeout.after = budget.map_or(
                    ureq::unversioned::transport::time::Duration::NotHappening,
                    Into::into,
                );
                let candidate = ConnectionDetails {
                    uri: details.uri,
                    addrs,
                    config: details.config,
                    request_level: details.request_level,
                    resolver: details.resolver,
                    now: (details.current_time)(),
                    timeout,
                    current_time: details.current_time.clone(),
                    run_connector: details.run_connector.clone(),
                };
                TcpConnector::default()
                    .connect(&candidate, None::<()>)
                    .map(|transport| {
                        transport.map(|t| {
                            Box::new(DeadlineTransport {
                                inner: Box::new(t),
                                deadline: self.deadline,
                            }) as Box<dyn Transport>
                        })
                    })
            },
        )
    }
}

#[derive(Debug)]
struct DeadlineTransport {
    inner: Box<dyn Transport>,
    deadline: Option<Instant>,
}

impl DeadlineTransport {
    fn cap(&self, mut timeout: NextTimeout) -> Result<NextTimeout, ureq::Error> {
        if let Some(deadline) = self.deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ureq::Error::Timeout(ureq::Timeout::Global));
            }
            if timeout
                .not_zero()
                .is_none_or(|duration| *duration > remaining)
            {
                timeout.after = remaining.into();
                timeout.reason = ureq::Timeout::Global;
            }
        }
        Ok(timeout)
    }
}

impl Transport for DeadlineTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        self.inner.buffers()
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        let timeout = self.cap(timeout)?;
        self.inner.transmit_output(amount, timeout)
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        let timeout = self.cap(timeout)?;
        self.inner.maybe_await_input(timeout)
    }

    fn is_open(&mut self) -> bool {
        self.inner.is_open()
    }

    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}

fn connect_candidates<T>(
    addresses: Vec<std::net::SocketAddr>,
    total: Option<Duration>,
    elapsed: impl Fn() -> Duration,
    mut connect: impl FnMut(std::net::SocketAddr, Option<Duration>) -> Result<T, ureq::Error>,
) -> Result<T, ureq::Error> {
    let attempts = crate::net::connect_attempts(addresses);
    let multiple = attempts.len() > 1;
    let mut first_error = None;
    for attempt in attempts {
        let remaining = total.map(|duration| duration.saturating_sub(elapsed()));
        if remaining.is_some_and(|duration| duration.is_zero()) {
            return Err(ureq::Error::Timeout(ureq::Timeout::Global));
        }
        let budget = match (remaining, attempt.timeout) {
            (Some(remaining), Some(candidate)) => Some(remaining.min(candidate)),
            (remaining, None) => remaining,
            (None, candidate) => candidate,
        };
        match connect(attempt.address, budget) {
            Ok(transport) => return Ok(transport),
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    if total.is_some_and(|duration| elapsed() >= duration) {
        return Err(ureq::Error::Timeout(ureq::Timeout::Global));
    }
    if multiple {
        // Node's automatic-family AggregateError has an empty message.
        return Err(ureq::Error::Io(std::io::Error::other(String::new())));
    }
    Err(first_error.unwrap_or(ureq::Error::HostNotFound))
}

fn is_token(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(is_token_byte)
}

fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

fn latin1(value: &str) -> Option<Vec<u8>> {
    value
        .chars()
        .map(|c| u8::try_from(u32::from(c)).ok())
        .collect()
}

fn outgoing(url: &Url, method: &str, headers: &Headers, body: &[u8]) -> Result<Vec<u8>, Failure> {
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

struct RawConnector {
    raw: Vec<u8>,
    captured: Arc<Mutex<Option<Headers>>>,
    head: bool,
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

struct RawTransport {
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

fn parse_error(reason: &str) -> ureq::Error {
    ureq::Error::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("Parse Error: {reason}"),
    ))
}

#[derive(Default)]
enum ChunkPart {
    #[default]
    Size,
    SizeLf,
    Data(u64),
    DataCr,
    DataLf,
    Trailers,
    End,
}

#[derive(Clone, Copy)]
enum ExtensionPart {
    Start,
    Name,
    ValueStart,
    Value,
    Quote,
    Escape,
    AfterQuote,
}

/// Validate Node's chunk framing and strip extensions before ureq sees
/// it: ureq-proto limits the entire size line to 20 bytes, even though a
/// valid extension may contain 16384 bytes. Body bytes stay untouched.
#[derive(Default)]
struct ChunkScan {
    part: ChunkPart,
    size: u64,
    digits: bool,
    extension: Option<ExtensionPart>,
    extension_size: usize,
    trailers: HeaderScan,
    trailer_line_size: usize,
}

impl ChunkScan {
    fn receive(&mut self, mut bytes: &[u8], output: &mut VecDeque<u8>) -> Result<(), ureq::Error> {
        while let Some((&byte, rest)) = bytes.split_first() {
            bytes = rest;
            match self.part {
                ChunkPart::Size => self.size_byte(byte)?,
                ChunkPart::SizeLf => {
                    if byte != b'\n' {
                        return Err(parse_error("Invalid character in chunk size"));
                    }
                    output.extend(format!("{:x}\r\n", self.size).bytes());
                    self.part = if self.size == 0 {
                        self.trailers.part = HeaderPart::Name;
                        self.trailers.name_empty = true;
                        ChunkPart::Trailers
                    } else {
                        ChunkPart::Data(self.size)
                    };
                }
                ChunkPart::Data(left) => {
                    output.push_back(byte);
                    let amount = usize::try_from(left - 1)
                        .unwrap_or(usize::MAX)
                        .min(bytes.len());
                    output.extend(&bytes[..amount]);
                    bytes = &bytes[amount..];
                    let left = left - 1 - u64::try_from(amount).unwrap_or(u64::MAX);
                    self.part = if left == 0 {
                        ChunkPart::DataCr
                    } else {
                        ChunkPart::Data(left)
                    };
                }
                ChunkPart::DataCr => {
                    if byte != b'\r' {
                        return Err(parse_error("Expected CR after chunk"));
                    }
                    output.push_back(byte);
                    self.part = ChunkPart::DataLf;
                }
                ChunkPart::DataLf => {
                    if byte != b'\n' {
                        return Err(parse_error("Expected LF after chunk"));
                    }
                    output.push_back(byte);
                    self.part = ChunkPart::Size;
                    self.size = 0;
                    self.digits = false;
                    self.extension = None;
                    self.extension_size = 0;
                }
                ChunkPart::Trailers => {
                    self.trailers.byte(byte)?;
                    if byte == b'\n' {
                        if self.trailer_line_size == 0 {
                            // The API exposes no trailers. Only release the
                            // framing terminator after validating all of the
                            // originals; ureq otherwise buffers a whole
                            // trailer line in its fixed-size input buffer.
                            output.extend(b"\r\n");
                            self.part = ChunkPart::End;
                        }
                        self.trailer_line_size = 0;
                    } else if byte != b'\r' {
                        self.trailer_line_size += 1;
                    }
                }
                ChunkPart::End => return Ok(()),
            }
        }
        Ok(())
    }

    fn size_byte(&mut self, byte: u8) -> Result<(), ureq::Error> {
        if byte == b'\r' {
            if !self.digits {
                return Err(parse_error("Invalid character in chunk size"));
            }
            if matches!(
                self.extension,
                Some(ExtensionPart::Start | ExtensionPart::Quote | ExtensionPart::Escape)
            ) {
                return Err(parse_error("Invalid character in chunk extensions"));
            }
            self.part = ChunkPart::SizeLf;
            return Ok(());
        }
        if let Some(part) = self.extension {
            self.extension_size += 1;
            if self.extension_size > 16_384 {
                return Err(parse_error("Chunk extensions overflow"));
            }
            self.extension = Some(match part {
                ExtensionPart::Start if is_token_byte(byte) => ExtensionPart::Name,
                ExtensionPart::Name if is_token_byte(byte) => ExtensionPart::Name,
                ExtensionPart::Name if byte == b'=' => ExtensionPart::ValueStart,
                ExtensionPart::Name
                | ExtensionPart::ValueStart
                | ExtensionPart::Value
                | ExtensionPart::AfterQuote
                    if byte == b';' =>
                {
                    ExtensionPart::Start
                }
                ExtensionPart::ValueStart if byte == b'"' => ExtensionPart::Quote,
                ExtensionPart::ValueStart | ExtensionPart::Value if is_token_byte(byte) => {
                    ExtensionPart::Value
                }
                ExtensionPart::Quote if byte == b'"' => ExtensionPart::AfterQuote,
                ExtensionPart::Quote if byte == b'\\' => ExtensionPart::Escape,
                ExtensionPart::Quote | ExtensionPart::Escape
                    if byte == b'\t' || byte >= 32 && byte != 127 =>
                {
                    ExtensionPart::Quote
                }
                _ => {
                    return Err(parse_error(match part {
                        ExtensionPart::Start => "Invalid character in chunk extensions",
                        ExtensionPart::Name => "Invalid character in chunk extensions name",
                        ExtensionPart::AfterQuote => {
                            "Invalid character in chunk extensions quote value"
                        }
                        _ => "Invalid character in chunk extensions value",
                    }));
                }
            });
        } else if byte == b';' && self.digits {
            self.extension = Some(ExtensionPart::Start);
        } else if let Some(digit) = char::from(byte)
            .to_digit(16)
            .filter(|_| byte.is_ascii_hexdigit())
        {
            self.size = self
                .size
                .checked_mul(16)
                .and_then(|n| n.checked_add(u64::from(digit)))
                .ok_or_else(|| parse_error("Chunk size overflow"))?;
            self.digits = true;
        } else {
            return Err(parse_error("Invalid character in chunk size"));
        }
        Ok(())
    }
}

#[derive(Default)]
enum HeaderPart {
    #[default]
    Status,
    Name,
    BeforeValue,
    Value,
}

/// Node's `TrackHeader` counts the reason phrase, field names and values,
/// rejecting at 16384, independently for each informational/final block.
/// Discard leading value whitespace as llhttp does; this also bounds the
/// retained header buffer without imposing an unrelated raw-byte cap.
#[derive(Default)]
struct HeaderScan {
    part: HeaderPart,
    size: usize,
    status_spaces: u8,
    status_prefix: usize,
    name_empty: bool,
    previous_cr: bool,
}

impl HeaderScan {
    fn byte(&mut self, byte: u8) -> Result<bool, ureq::Error> {
        if self.previous_cr && byte != b'\n' {
            return Err(parse_error(match self.part {
                HeaderPart::Status => "Expected LF after CR",
                _ => "Missing expected LF after header value",
            }));
        }
        if byte == b'\n' && !self.previous_cr {
            return Err(parse_error(match self.part {
                HeaderPart::Status => "Missing expected CR after response line",
                _ => "Missing expected CR after header value",
            }));
        }
        self.previous_cr = byte == b'\r';
        let counted = match self.part {
            HeaderPart::Status => {
                if byte == b'\n' {
                    self.part = HeaderPart::Name;
                    self.name_empty = true;
                    false
                } else if byte == b'\r' {
                    false
                } else if self.status_spaces >= 2 {
                    true
                } else {
                    self.status_prefix += 1;
                    if self.status_prefix > 20 {
                        return Err(parse_error("Invalid HTTP version"));
                    }
                    if byte == b' ' {
                        self.status_spaces += 1;
                    }
                    false
                }
            }
            HeaderPart::Name => {
                if byte == b'\r' || byte == b'\n' {
                    if !self.name_empty {
                        return Err(parse_error("Invalid header token"));
                    }
                    false
                } else if byte == b':' && !self.name_empty {
                    self.part = HeaderPart::BeforeValue;
                    false
                } else if matches!(byte, b' ' | b'\t') && self.name_empty {
                    return Err(parse_error("Unexpected whitespace after header value"));
                } else if !is_token_byte(byte) {
                    return Err(parse_error("Invalid header token"));
                } else {
                    self.name_empty = false;
                    true
                }
            }
            HeaderPart::BeforeValue if matches!(byte, b' ' | b'\t') => return Ok(false),
            HeaderPart::BeforeValue | HeaderPart::Value => {
                if byte == b'\n' {
                    self.part = HeaderPart::Name;
                    self.name_empty = true;
                    false
                } else if byte == b'\r' {
                    false
                } else {
                    if byte != 9 && (byte < 32 || byte == 127) {
                        return Err(parse_error("Invalid header value char"));
                    }
                    self.part = HeaderPart::Value;
                    true
                }
            }
        };
        self.size += usize::from(counted);
        if self.size >= 16_384 {
            return Err(parse_error("Header overflow"));
        }
        Ok(true)
    }
}

fn framing(headers: &Headers) -> Result<(Option<u64>, bool), ureq::Error> {
    let mut length = None;
    let mut transfer = false;
    let mut last_coding = "";
    for (name, value) in headers {
        if name.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                return Err(parse_error("Duplicate Content-Length"));
            }
            if transfer {
                return Err(parse_error(
                    "Content-Length can't be present with Transfer-Encoding",
                ));
            }
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(parse_error("Invalid character in Content-Length"));
            }
            length = Some(
                value
                    .parse()
                    .map_err(|_| parse_error("Content-Length overflow"))?,
            );
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            if value.is_empty() {
                return Err(parse_error("Empty Transfer-Encoding"));
            }
            if length.is_some() {
                return Err(parse_error(
                    "Transfer-Encoding can't be present with Content-Length",
                ));
            }
            transfer = true;
            last_coding = value.split(',').next_back().unwrap_or_default().trim();
        }
    }
    Ok((length, last_coding.eq_ignore_ascii_case("chunked")))
}

fn header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|at| at + 4)
        .or_else(|| bytes.windows(2).position(|w| w == b"\n\n").map(|at| at + 2))
}

fn response_pairs(bytes: &[u8]) -> Result<(u16, Headers), ureq::Error> {
    let invalid = || {
        ureq::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Parse Error: Invalid header token",
        ))
    };
    let mut lines = bytes.split(|b| *b == b'\n');
    let first = lines.next().ok_or_else(invalid)?;
    if !first.starts_with(b"HTTP/1.0 ") && !first.starts_with(b"HTTP/1.1 ") {
        return Err(parse_error("Invalid HTTP version"));
    }
    let status = first
        .get(9..12)
        .filter(|bytes| bytes.iter().all(u8::is_ascii_digit))
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .and_then(|s| s.parse().ok())
        .ok_or_else(invalid)?;
    let mut pairs: Headers = Vec::new();
    for line in lines {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            break;
        }
        let text = |bytes: &[u8]| bytes.iter().copied().map(char::from).collect::<String>();
        let trim = |bytes: &[u8]| {
            let start = bytes
                .iter()
                .position(|b| !matches!(b, b' ' | b'\t'))
                .unwrap_or(bytes.len());
            let end = bytes
                .iter()
                .rposition(|b| !matches!(b, b' ' | b'\t'))
                .map_or(start, |at| at + 1);
            text(&bytes[start..end])
        };
        if matches!(line.first(), Some(b' ' | b'\t')) {
            return Err(ureq::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Parse Error: Unexpected whitespace after header value",
            )));
        }
        let colon = line.iter().position(|b| *b == b':').ok_or_else(invalid)?;
        let name = std::str::from_utf8(&line[..colon]).map_err(|_| invalid())?;
        if !is_token(name)
            || line[colon + 1..]
                .iter()
                .any(|b| *b != 9 && (*b < 32 || *b == 127))
        {
            return Err(invalid());
        }
        pairs.push((name.into(), trim(&line[colon + 1..])));
    }
    Ok((status, pairs))
}

#[cfg(test)]
mod tests {
    use super::{
        ChunkScan, Client, Failure, HeaderScan, Headers, HttpResolver, HttpTcpConnector, Job,
        Request, Worker, connect_candidates, encode, framing, header_end, outgoing, resolve_lookup,
        response_pairs, spawn_worker,
    };
    use crate::codec::{Dec, Enc};
    use crate::host::Stop;
    use hd_run::{Grants, Limit};
    use socket2::{Domain, Protocol, Socket, Type};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use ureq::unversioned::resolver::Resolver;
    use ureq::unversioned::transport::{Connector, NextTimeout, RustlsConnector};
    use url::Url;

    fn request(method: &str, url: &str, headers: &Headers, body: &[u8], timeout: i64) -> Enc {
        let mut e = Enc::default();
        e.leb(7)
            .str(method)
            .str(url)
            .list(headers, |e, (name, value)| {
                e.str(name).str(value);
            })
            .bytes(body)
            .leb(1)
            .zz(timeout);
        e
    }

    fn read_request(stream: &mut TcpStream) -> Vec<u8> {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buf = [0; 8192];
        loop {
            let n = stream.read(&mut buf).unwrap();
            assert_ne!(n, 0);
            bytes.extend_from_slice(&buf[..n]);
            if let Some(end) = header_end(&bytes) {
                let header = String::from_utf8_lossy(&bytes[..end]);
                let length = header
                    .lines()
                    .filter_map(|s| s.split_once(':'))
                    .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
                    .map_or(0, |(_, v)| v.trim().parse::<usize>().unwrap());
                let chunked = header
                    .to_ascii_lowercase()
                    .contains("transfer-encoding: chunked");
                if bytes.len() >= end + length && (!chunked || bytes.ends_with(b"0\r\n\r\n")) {
                    return bytes;
                }
            }
        }
    }

    fn server(response: Vec<u8>) -> (String, std::thread::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/path?q=1", listener.local_addr().unwrap());
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            stream.write_all(&response).unwrap();
            request
        });
        (url, thread)
    }

    fn non_listening_socket() -> Socket {
        let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP)).unwrap();
        socket
            .bind(
                &"127.0.0.1:0"
                    .parse::<std::net::SocketAddr>()
                    .unwrap()
                    .into(),
            )
            .unwrap();
        // Keep the port reserved without accepting connections. Darwin
        // times these out; other platforms may refuse immediately.
        socket
    }

    fn response(encoded: &[u8]) -> (u64, Headers, Vec<u8>) {
        let mut d = Dec::new(encoded);
        assert_eq!(d.leb().unwrap(), 0);
        let status = d.leb().unwrap();
        let headers = (0..d.leb().unwrap())
            .map(|_| (d.str().unwrap(), d.str().unwrap()))
            .collect();
        let body = d.bytes().unwrap().to_vec();
        (status, headers, body)
    }

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
        let mut client = Client::default();
        let r = request(
            "bad method",
            "http://127.0.0.1:1/",
            &vec![("bad name".into(), "\0".into())],
            &[],
            0,
        );
        let result = client.send(r.done(), &deny).unwrap();
        let mut expected = Enc::default();
        expected.leb(1).leb(0).str("127.0.0.1");
        assert_eq!(result.done(), expected.done());
        let r = request("GET", "relative", &vec![], &[], 0);
        let result = client.send(r.done(), &deny).unwrap();
        let mut expected = Enc::default();
        expected.leb(1).leb(1).str("relative");
        assert_eq!(result.done(), expected.done());
        let r = request("GET", "http://127.0.0.1:1/", &vec![], &[], 0);
        assert_eq!(
            client.send(r.done(), &Grants::default()).unwrap().done(),
            [1, 5]
        );
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
    fn parser_preserves_raw_header_case_order_duplicates_and_latin1() {
        let (status, headers) = response_pairs(
            b"HTTP/1.1 500 Error\r\nX-A: first \t\r\nx-b: \xe9\r\nX-A: second\r\n\r\n",
        )
        .unwrap();
        assert_eq!(status, 500);
        assert_eq!(
            headers,
            [
                ("X-A".into(), "first".into()),
                ("x-b".into(), "é".into()),
                ("X-A".into(), "second".into())
            ]
        );
        assert!(response_pairs(b"HTTP/1.1 200 OK\r\nbad name: value\r\n\r\n").is_err());
    }

    #[test]
    fn client_keeps_status_raw_headers_bytes_and_does_not_decode_gzip() {
        let (url, server) = server(b"HTTP/1.1 103 Early Hints\r\nLink: ignored\r\n\r\nHTTP/1.1 500 Error\r\nX-A: first\r\nx-B: \xe9\r\nX-A: second\r\nContent-Encoding: gzip\r\nContent-Length: 3\r\n\r\n\xff\0z".to_vec());
        let r = request("GET", &url, &vec![], &[], 1000);
        let encoded = Client::default()
            .send(r.done(), &Grants::default())
            .unwrap();
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
    fn client_accepts_over_128_headers_and_over_ten_mib_body() {
        let body = vec![0xaa; 10 * 1024 * 1024 + 17];
        let mut wire = b"HTTP/1.1 200 OK\r\n".to_vec();
        for i in 0..150 {
            wire.extend_from_slice(format!("X-{i}: {i}\r\n").as_bytes());
        }
        wire.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
        wire.extend_from_slice(&body);
        let (url, server) = server(wire);
        let r = request("GET", &url, &vec![], &[], 5000);
        let encoded = Client::default()
            .send(r.done(), &Grants::default())
            .unwrap();
        let (status, headers, received) = response(encoded.done());
        assert_eq!(status, 200);
        assert_eq!(headers.len(), 151);
        assert_eq!(headers[0], ("X-0".into(), "0".into()));
        assert_eq!(headers[149], ("X-149".into(), "149".into()));
        assert_eq!(received, body);
        server.join().unwrap();
    }

    #[test]
    fn client_keeps_all_1001_header_pairs_as_required_by_the_spec() {
        // Node v24.19 returns only the first 1000 pairs for this response.
        // std-http.headers.pairs requires all pairs; 2017 accounted header
        // bytes are well below the independent 16384-byte parser budget.
        let mut wire = b"HTTP/1.1 200 OK\r\n".to_vec();
        for _ in 0..1000 {
            wire.extend_from_slice(b"X: a\r\n");
        }
        wire.extend_from_slice(b"Content-Length: 0\r\n\r\n");
        let (url, server) = server(wire);
        let r = request("GET", &url, &vec![], &[], 1000);
        let encoded = Client::default()
            .send(r.done(), &Grants::default())
            .unwrap();
        let (status, headers, body) = response(encoded.done());
        assert_eq!(status, 200);
        assert_eq!(headers.len(), 1001);
        assert!(
            headers[..1000]
                .iter()
                .all(|pair| pair == &("X".into(), "a".into()))
        );
        assert_eq!(headers[1000], ("Content-Length".into(), "0".into()));
        assert!(body.is_empty());
        server.join().unwrap();
    }

    #[test]
    fn header_scanner_bounds_incomplete_headers_and_rejects_node_invalid_syntax() {
        let mut scan = HeaderScan::default();
        for byte in b"HTTP/1.1 200 OK\r\nX:" {
            scan.byte(*byte).unwrap();
        }
        for _ in 0..16_380 {
            scan.byte(b'a').unwrap();
        }
        assert_eq!(
            scan.byte(b'a').unwrap_err().to_string(),
            "io: Parse Error: Header overflow"
        );
        for (wire, reason) in [
            (
                b"HTTP/1.1 200 OK\n".as_slice(),
                "Missing expected CR after response line",
            ),
            (
                b"HTTP/1.1 200 OK\r\nX: a\n".as_slice(),
                "Missing expected CR after header value",
            ),
            (
                b"HTTP/1.1 200 OK\r\nX: a\rb".as_slice(),
                "Missing expected LF after header value",
            ),
            (
                b"HTTP/1.1 200 OK\r\nX: a\x01".as_slice(),
                "Invalid header value char",
            ),
            (
                b"HTTP/1.1 200 OK\r\nX: a\r\n\tcontinued".as_slice(),
                "Unexpected whitespace after header value",
            ),
        ] {
            let mut scan = HeaderScan::default();
            let error = wire
                .iter()
                .try_for_each(|byte| scan.byte(*byte).map(|_| ()))
                .unwrap_err();
            assert_eq!(error.to_string(), format!("io: Parse Error: {reason}"));
        }
        let mut scan = HeaderScan::default();
        for byte in b"HTTP/1.1 200 OK\r\nX:" {
            scan.byte(*byte).unwrap();
        }
        for _ in 0..100_000 {
            assert!(!scan.byte(b' ').unwrap());
        }
        assert_eq!(scan.size, 3);
    }

    #[test]
    fn original_framing_is_validated_before_reducing_headers() {
        for (headers, reason) in [
            (
                vec![
                    ("Content-Length".into(), "0".into()),
                    ("Content-Length".into(), "0".into()),
                ],
                "Duplicate Content-Length",
            ),
            (
                vec![
                    ("Content-Length".into(), "0".into()),
                    ("Transfer-Encoding".into(), "chunked".into()),
                ],
                "Transfer-Encoding can't be present with Content-Length",
            ),
            (
                vec![
                    ("Transfer-Encoding".into(), "chunked".into()),
                    ("Content-Length".into(), "0".into()),
                ],
                "Content-Length can't be present with Transfer-Encoding",
            ),
            (
                vec![("Content-Length".into(), "0, 0".into())],
                "Invalid character in Content-Length",
            ),
            (
                vec![("Transfer-Encoding".into(), String::new())],
                "Empty Transfer-Encoding",
            ),
        ] {
            assert_eq!(
                framing(&headers).unwrap_err().to_string(),
                format!("io: Parse Error: {reason}")
            );
        }
        assert_eq!(
            framing(&vec![("Transfer-Encoding".into(), "chunked, gzip".into())]).unwrap(),
            (None, false)
        );
        assert_eq!(
            framing(&vec![("Transfer-Encoding".into(), "gzip, chunked".into())]).unwrap(),
            (None, true)
        );
    }

    #[test]
    fn client_decodes_http10_chunked_like_node_and_rejects_invalid_framing() {
        let (url, first_server) = server(
            b"HTTP/1.0 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\n\r\n".to_vec(),
        );
        let r = request("GET", &url, &vec![], &[], 1000);
        let encoded = Client::default()
            .send(r.done(), &Grants::default())
            .unwrap();
        assert_eq!(
            response(encoded.done()),
            (
                200,
                vec![("Transfer-Encoding".into(), "chunked".into())],
                b"abc".to_vec()
            )
        );
        first_server.join().unwrap();

        let (url, server) =
            server(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n".to_vec());
        let r = request("GET", &url, &vec![], &[], 1000);
        let encoded = Client::default()
            .send(r.done(), &Grants::default())
            .unwrap();
        let mut expected = Enc::default();
        expected
            .leb(1)
            .leb(3)
            .str("Parse Error: Duplicate Content-Length");
        assert_eq!(encoded.done(), expected.done());
        server.join().unwrap();
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
            let encoded = Client::default()
                .send(r.done(), &Grants::default())
                .unwrap();
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

    #[test]
    fn redirect_rechecks_the_target_port_before_a_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let target_url = format!("http://{}/not-granted", target.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: {target_url}\r\nContent-Length: 0\r\n\r\n"
            )
            .unwrap();
        });
        let grant = Grants {
            limits: vec![("Http".into(), Limit::Entries(vec![address.to_string()]))],
        };
        let r = request("GET", &format!("http://{address}/"), &vec![], &[], 1000);
        let encoded = Client::default().send(r.done(), &grant).unwrap();
        let mut expected = Enc::default();
        expected.leb(1).leb(0).str("127.0.0.1");
        assert_eq!(encoded.done(), expected.done());
        assert_eq!(
            target.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        server.join().unwrap();
    }

    #[test]
    fn deadline_covers_waiting_for_the_response_and_worker_closes_promptly() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (release, receive) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_request(&mut stream);
            let _ = receive.recv_timeout(Duration::from_secs(2));
        });
        let r = request("GET", &format!("http://{address}/"), &vec![], &[], 200);
        let mut client = Client {
            worker: Some(spawn_worker().unwrap()),
        };
        let started = Instant::now();
        let encoded = client.send(r.done(), &Grants::default()).unwrap();
        assert_eq!(encoded.done(), [1, 5]);
        drop(client);
        assert!(started.elapsed() < Duration::from_secs(1));
        let _ = release.send(());
        server.join().unwrap();
    }

    #[test]
    fn deadline_includes_shared_worker_queue_and_expired_job_never_connects() {
        // An isolated worker avoids interfering with concurrent loopback
        // tests while exercising the same two-client sharing mechanism.
        let worker = spawn_worker().unwrap();
        let mut second = Client {
            worker: Some(worker.clone()),
        };
        let mut first = Client {
            worker: Some(worker),
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (arrived, arrival) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_request(&mut stream);
            arrived.send(()).unwrap();
            released.recv_timeout(Duration::from_secs(2)).unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        });
        let first_request = request("GET", &format!("http://{address}/"), &vec![], &[], 2000);
        let first_reply = std::thread::spawn(move || {
            first
                .send(first_request.done(), &Grants::default())
                .unwrap()
        });
        arrival.recv_timeout(Duration::from_secs(1)).unwrap();
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let queued = request(
            "GET",
            &format!("http://{}/", target.local_addr().unwrap()),
            &vec![],
            &[],
            30,
        );
        let started = Instant::now();
        assert_eq!(
            second
                .send(queued.done(), &Grants::default())
                .unwrap()
                .done(),
            [1, 5]
        );
        assert!(started.elapsed() < Duration::from_millis(500));
        release.send(()).unwrap();
        assert_eq!(response(first_reply.join().unwrap().done()).0, 200);
        // A FIFO barrier proves that the timed-out job has been skipped,
        // not merely that the worker has not reached it yet.
        let (reply, barrier) = std::sync::mpsc::channel();
        second
            .worker
            .as_ref()
            .unwrap()
            .sender
            .as_ref()
            .unwrap()
            .send(Job {
                request: Request::decode(request("GET", "relative", &vec![], &[], 1000).done())
                    .unwrap(),
                grants: Grants::default(),
                reply,
                deadline: Some(Instant::now() + Duration::from_secs(1)),
            })
            .unwrap();
        let encoded = barrier.recv_timeout(Duration::from_secs(1)).unwrap();
        let mut expected = Enc::default();
        expected.leb(1).leb(1).str("relative");
        assert_eq!(encoded.done(), expected.done());
        assert_eq!(
            target.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        server.join().unwrap();
    }

    #[test]
    fn tls_handshake_timeout_and_final_worker_drop_use_the_same_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (release, released) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut first = [0; 1];
            assert_eq!(stream.read(&mut first).unwrap(), 1);
            let _ = released.recv_timeout(Duration::from_secs(2));
        });
        let r = request("GET", &format!("https://{address}/"), &vec![], &[], 300);
        let mut client = Client {
            worker: Some(spawn_worker().unwrap()),
        };
        let started = Instant::now();
        assert_eq!(
            client.send(r.done(), &Grants::default()).unwrap().done(),
            [1, 5]
        );
        drop(client);
        assert!(started.elapsed() < Duration::from_secs(1));
        let _ = release.send(());
        server.join().unwrap();
    }

    #[test]
    fn chunk_extensions_are_streamed_validated_and_normalized_without_changing_body() {
        let raw = b"3;foo=01234567890123456789;quoted=\"a b\"\r\nabc\r\n2;empty=;flag\r\nde\r\n0\r\nX-Trailer: yes\r\n\r\n";
        let mut chunks = ChunkScan::default();
        let mut output = std::collections::VecDeque::new();
        for byte in raw {
            chunks.receive(&[*byte], &mut output).unwrap();
        }
        assert_eq!(
            output.into_iter().collect::<Vec<_>>(),
            b"3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n"
        );
        for (raw, reason) in [
            (b"Z\r\n".as_slice(), "Invalid character in chunk size"),
            (
                b"3;\r\n".as_slice(),
                "Invalid character in chunk extensions",
            ),
            (
                b"3;f=\x01\r\n".as_slice(),
                "Invalid character in chunk extensions value",
            ),
        ] {
            let error = ChunkScan::default()
                .receive(raw, &mut std::collections::VecDeque::new())
                .unwrap_err();
            assert_eq!(error.to_string(), format!("io: Parse Error: {reason}"));
        }
        let mut wire = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        wire.extend_from_slice(raw);
        let (url, server) = server(wire);
        let r = request("GET", &url, &vec![], &[], 1000);
        let encoded = Client::default()
            .send(r.done(), &Grants::default())
            .unwrap();
        assert_eq!(response(encoded.done()).2, b"abcde");
        server.join().unwrap();
    }

    #[test]
    fn valid_long_leading_trailer_whitespace_does_not_fill_ureq_buffers() {
        let mut wire =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\nX-Trailer:"
                .to_vec();
        wire.extend(std::iter::repeat_n(b' ', 100_000));
        wire.extend_from_slice(b"yes\r\n\r\n");
        let (url, server) = server(wire);
        let r = request("GET", &url, &vec![], &[], 1000);
        let encoded = Client::default()
            .send(r.done(), &Grants::default())
            .unwrap();
        let (status, headers, body) = response(encoded.done());
        assert_eq!(status, 200);
        assert_eq!(headers, [("Transfer-Encoding".into(), "chunked".into())]);
        assert_eq!(body, b"abc");
        server.join().unwrap();
    }

    #[test]
    fn tcp_candidates_use_node_family_order_and_250ms_whole_deadline_budgets() {
        let v4a = "127.0.0.1:1".parse().unwrap();
        let v4b = "127.0.0.1:2".parse().unwrap();
        let v6 = "[::1]:1".parse().unwrap();
        let elapsed = std::cell::Cell::new(Duration::ZERO);
        let mut calls = Vec::new();
        let result = connect_candidates(
            vec![v4a, v4b, v4a, v6],
            Some(Duration::from_secs(30)),
            || elapsed.get(),
            |address, budget| {
                calls.push((address, budget));
                elapsed.set(elapsed.get() + budget.unwrap());
                if address == v4b {
                    Ok(())
                } else {
                    Err(ureq::Error::Io(std::io::ErrorKind::TimedOut.into()))
                }
            },
        );
        assert!(result.is_ok());
        assert_eq!(
            calls,
            [
                (v4a, Some(Duration::from_millis(250))),
                (v6, Some(Duration::from_millis(250))),
                (v4b, Some(Duration::from_millis(29_500)))
            ]
        );
        let elapsed = std::cell::Cell::new(Duration::ZERO);
        let mut calls = 0;
        let result: Result<(), _> = connect_candidates(
            vec![v4a, v6],
            Some(Duration::from_millis(5)),
            || elapsed.get(),
            |_, budget| {
                calls += 1;
                assert_eq!(budget, Some(Duration::from_millis(5)));
                elapsed.set(Duration::from_millis(5));
                Err(ureq::Error::Timeout(ureq::Timeout::Connect))
            },
        );
        assert!(matches!(
            result,
            Err(ureq::Error::Timeout(ureq::Timeout::Global))
        ));
        assert_eq!(calls, 1);
    }

    #[test]
    fn tcp_candidates_advance_from_a_reserved_non_listening_port_to_a_working_listener() {
        let refused = non_listening_socket();
        let bad = refused.local_addr().unwrap().as_socket().unwrap();
        let working = TcpListener::bind("127.0.0.1:0").unwrap();
        let good = working.local_addr().unwrap();
        let started = Instant::now();
        let stream = connect_candidates(
            vec![bad, good],
            Some(Duration::from_secs(1)),
            || started.elapsed(),
            |address, budget| {
                TcpStream::connect_timeout(&address, budget.unwrap()).map_err(ureq::Error::Io)
            },
        )
        .unwrap();
        assert_eq!(stream.peer_addr().unwrap(), good);
        let (accepted, _) = working.accept().unwrap();
        drop(accepted);
        drop(refused);
    }

    #[test]
    fn resolver_connector_handoff_reaches_the_working_seventeenth_candidate() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let good = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let started = Instant::now();
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(started.elapsed() < Duration::from_secs(8));
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("candidate17 accept failed: {error}"),
                }
            };
            // Accepted sockets may inherit O_NONBLOCK on Darwin; reads
            // below intentionally use a blocking socket with a timeout.
            stream.set_nonblocking(false).unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let request = read_request(&mut stream); // Also bounds reads to two seconds.
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc")
                .unwrap();
            request
        });
        // Keep all sixteen distinct ports bound but non-listening until
        // the request finishes, so parallel tests cannot steal any port.
        // Darwin blackholes these SYNs: sixteen nonfinal 250ms attempts
        // legitimately take four seconds. Budget eight seconds for this
        // path rather than retrying or weakening the 17-address assertion.
        // No external DNS/I/O occurs.
        let refused: Vec<_> = (0..16).map(|_| non_listening_socket()).collect();
        let mut addresses: Vec<_> = refused
            .iter()
            .map(|socket| socket.local_addr().unwrap().as_socket().unwrap())
            .collect();
        addresses.push(good);
        assert_eq!(addresses.len(), 17);
        assert!(addresses[..16].iter().all(|address| *address != good));
        let candidates = Arc::new(Mutex::new(None));
        let deadline = Some(Instant::now() + Duration::from_secs(8));
        let resolver = HttpResolver {
            candidates: candidates.clone(),
            deadline,
            lookup: Some(Arc::new(move |_| Ok(addresses.clone()))),
        };
        let connector = HttpTcpConnector {
            deadline,
            candidates: candidates.clone(),
        }
        .chain(RustlsConnector::default());
        let config = ureq::Agent::config_builder()
            .proxy(None)
            .max_idle_connections(0)
            .timeout_global(Some(Duration::from_secs(8)))
            .build();
        let agent = ureq::Agent::with_parts(config, connector, resolver);
        let mut response = agent.get("http://candidate.test/path").call().unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.body_mut().with_config().read_to_vec().unwrap(),
            b"abc"
        );
        assert!(candidates.lock().unwrap().is_none());
        server.join().unwrap();
        drop(refused);
    }

    #[test]
    fn full_candidate_resolver_preserves_numeric_literals() {
        for (uri, expected) in [
            ("http://127.0.0.1:8123/", "127.0.0.1:8123"),
            ("http://[::1]:8123/", "[::1]:8123"),
        ] {
            let candidates = Arc::new(Mutex::new(None));
            let resolver = HttpResolver {
                candidates: candidates.clone(),
                deadline: Some(Instant::now() + Duration::from_secs(1)),
                lookup: None,
            };
            let representative = resolver
                .resolve(
                    &uri.parse().unwrap(),
                    &ureq::config::Config::default(),
                    NextTimeout {
                        after: Duration::from_secs(1).into(),
                        reason: ureq::Timeout::Global,
                    },
                )
                .unwrap();
            let expected = expected.parse().unwrap();
            assert_eq!(&*representative, [expected]);
            assert_eq!(
                candidates.lock().unwrap().as_deref(),
                Some([expected].as_slice())
            );
        }
    }

    #[test]
    fn timed_dns_handoff_returns_promptly_without_waiting_for_the_blocked_lookup() {
        let (release, released) = std::sync::mpsc::channel();
        let (finished, completion) = std::sync::mpsc::channel();
        let started = Instant::now();
        let result = resolve_lookup(
            NextTimeout {
                after: Duration::from_secs(1).into(),
                reason: ureq::Timeout::Global,
            },
            Some(started + Duration::from_millis(20)),
            move || {
                released.recv_timeout(Duration::from_secs(2)).unwrap();
                finished.send(()).unwrap();
                Ok(vec!["127.0.0.1:8123".parse().unwrap()])
            },
        );
        assert!(matches!(
            result,
            Err(ureq::Error::Timeout(ureq::Timeout::Global))
        ));
        assert!(started.elapsed() < Duration::from_millis(500));
        release.send(()).unwrap();
        completion.recv_timeout(Duration::from_secs(1)).unwrap();
        let result = resolve_lookup(
            NextTimeout {
                after: Duration::from_secs(1).into(),
                reason: ureq::Timeout::Global,
            },
            None,
            || Err(std::io::ErrorKind::NotFound.into()),
        );
        assert!(matches!(result, Err(ureq::Error::HostNotFound)));
    }

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

    fn bounded_accept(listener: &TcpListener) -> TcpStream {
        listener.set_nonblocking(true).unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            match listener.accept() {
                Ok((stream, _)) => return stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < until, "request reaches loopback fixture");
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("accept: {error}"),
            }
        }
    }

    #[test]
    fn expired_program_deadline_is_lazy_and_traps_instead_of_encoding_http_timeout() {
        let mut client = Client::default();
        let error = client
            .send_until(&[], &Grants::default(), Some(Instant::now()))
            .err()
            .expect("program deadline traps");
        assert!(matches!(
            error.downcast_ref::<Stop>(),
            Some(Stop::TimeLimit)
        ));
        assert!(client.worker.is_none());
    }

    #[test]
    fn program_deadline_caps_headers_body_tls_and_final_worker_drop() {
        for (tls, partial_body) in [(false, false), (false, true), (true, false)] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = listener.local_addr().unwrap();
            let (release, released) = std::sync::mpsc::channel();
            let (ready, arrived) = std::sync::mpsc::channel();
            let server = std::thread::spawn(move || {
                let until = Instant::now() + Duration::from_secs(2);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < until, "request reaches loopback fixture");
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        Err(error) => panic!("accept: {error}"),
                    }
                };
                if tls {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    assert_eq!(stream.read(&mut [0; 1]).unwrap(), 1);
                } else {
                    read_request(&mut stream);
                    if partial_body {
                        stream
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nx")
                            .unwrap();
                    }
                }
                ready.send(()).unwrap();
                let _ = released.recv_timeout(Duration::from_secs(2));
            });
            let scheme = if tls { "https" } else { "http" };
            let request = request(
                "GET",
                &format!("{scheme}://{address}/"),
                &vec![],
                &[],
                30_000,
            );
            let mut client = Client {
                worker: Some(spawn_worker().unwrap()),
            };
            let start = Instant::now();
            let error = client
                .send_until(
                    request.done(),
                    &Grants::default(),
                    Some(start + Duration::from_millis(100)),
                )
                .err()
                .expect("program deadline traps");
            assert!(matches!(
                error.downcast_ref::<Stop>(),
                Some(Stop::TimeLimit)
            ));
            arrived.recv_timeout(Duration::from_secs(1)).unwrap();
            drop(client);
            assert!(
                start.elapsed() < Duration::from_millis(500),
                "caller and final drop use program budget"
            );
            release.send(()).unwrap();
            server.join().unwrap();
        }
    }

    #[test]
    fn earlier_request_deadline_remains_an_http_error() {
        let worker = spawn_worker().unwrap();
        // Keep this isolated worker occupied: the queued request times out
        // without any external DNS or loopback connection being attempted.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (release, released) = std::sync::mpsc::channel();
        let (ready, arrived) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let mut stream = bounded_accept(&listener);
            read_request(&mut stream);
            ready.send(()).unwrap();
            let _ = released.recv_timeout(Duration::from_secs(2));
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        });
        let first_request = request("GET", &format!("http://{address}/"), &vec![], &[], 1000);
        let mut first = Client {
            worker: Some(worker.clone()),
        };
        let first_reply = std::thread::spawn(move || {
            first
                .send(first_request.done(), &Grants::default())
                .unwrap()
        });
        arrived.recv_timeout(Duration::from_secs(1)).unwrap();
        let mut second = Client {
            worker: Some(worker),
        };
        let queued = request("GET", "http://127.0.0.1:9/", &vec![], &[], 20);
        assert_eq!(
            second
                .send_until(
                    queued.done(),
                    &Grants::default(),
                    Some(Instant::now() + Duration::from_secs(1))
                )
                .unwrap()
                .done(),
            [1, 5]
        );
        release.send(()).unwrap();
        assert_eq!(response(first_reply.join().unwrap().done()).0, 200);
        drop(second);
        server.join().unwrap();
    }

    #[test]
    fn program_expiration_skips_queued_io_without_stopping_another_guest() {
        let worker = spawn_worker().unwrap();
        let mut first = Client {
            worker: Some(worker.clone()),
        };
        let mut second = Client {
            worker: Some(worker),
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (ready, arrived) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let mut stream = bounded_accept(&listener);
            read_request(&mut stream);
            ready.send(()).unwrap();
            let _ = released.recv_timeout(Duration::from_secs(2));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        });
        let first_request = request("GET", &format!("http://{address}/"), &vec![], &[], 1000);
        let first_reply = std::thread::spawn(move || {
            first
                .send(first_request.done(), &Grants::default())
                .unwrap()
        });
        arrived.recv_timeout(Duration::from_secs(1)).unwrap();
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let queued = request(
            "GET",
            &format!("http://{}/", target.local_addr().unwrap()),
            &vec![],
            &[],
            30_000,
        );
        let error = second
            .send_until(
                queued.done(),
                &Grants::default(),
                Some(Instant::now() + Duration::from_millis(20)),
            )
            .err()
            .expect("program deadline traps");
        assert!(matches!(
            error.downcast_ref::<Stop>(),
            Some(Stop::TimeLimit)
        ));
        release.send(()).unwrap();
        assert_eq!(response(first_reply.join().unwrap().done()).0, 200);
        // The subsequent FIFO reply proves the expired job was processed
        // and skipped, rather than merely still waiting in the queue.
        let invalid = request("GET", "relative", &vec![], &[], 1000);
        let (reply, barrier) = std::sync::mpsc::channel();
        second
            .worker
            .as_ref()
            .unwrap()
            .sender
            .as_ref()
            .unwrap()
            .send(Job {
                request: Request::decode(invalid.done()).unwrap(),
                grants: Grants::default(),
                reply,
                deadline: Some(Instant::now() + Duration::from_secs(1)),
            })
            .unwrap();
        assert_eq!(
            barrier.recv_timeout(Duration::from_secs(1)).unwrap().done(),
            encode(Err(Failure::InvalidUrl("relative".into()))).done()
        );
        assert_eq!(
            target.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        drop(second);
        server.join().unwrap();
    }

    #[test]
    fn expired_caller_does_not_join_a_worker_that_has_not_finished() {
        let (sender, receive) = std::sync::mpsc::channel::<Job>();
        let (release, released) = std::sync::mpsc::channel();
        let (finished, completion) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _job = receive.recv_timeout(Duration::from_secs(1)).unwrap();
            let _ = released.recv_timeout(Duration::from_secs(2));
            let _ = finished.send(());
        });
        let worker = Arc::new(Worker {
            sender: Some(sender),
            thread: Some(thread),
            detach_on_drop: std::sync::atomic::AtomicBool::new(false),
        });
        let mut client = Client {
            worker: Some(worker),
        };
        let request = request("GET", "http://127.0.0.1:9/", &vec![], &[], 30_000);
        let start = Instant::now();
        let error = client
            .send_until(
                request.done(),
                &Grants::default(),
                Some(start + Duration::from_millis(20)),
            )
            .err()
            .expect("program deadline traps");
        assert!(matches!(
            error.downcast_ref::<Stop>(),
            Some(Stop::TimeLimit)
        ));
        drop(client);
        assert!(start.elapsed() < Duration::from_millis(500));
        assert_eq!(
            completion.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        );
        release.send(()).unwrap();
        completion.recv_timeout(Duration::from_secs(1)).unwrap();
    }
}
