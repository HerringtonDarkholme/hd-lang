use std::mem::MaybeUninit;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex, mpsc};
use std::task::Poll;
use std::time::{Duration, Instant};

use socket2::SockRef;
use tokio::runtime::Handle;
use tokio::sync::watch;

use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, LazyBuffers, NextTimeout, Transport,
};

type Candidates = Arc<Mutex<Option<Vec<SocketAddr>>>>;

#[cfg(test)]
type Lookup = Arc<dyn Fn(String) -> std::io::Result<Vec<SocketAddr>> + Send + Sync>;

pub(crate) struct HttpResolver {
    pub(crate) candidates: Candidates,
    pub(crate) deadline: Option<Instant>,
    #[cfg(test)]
    pub(crate) lookup: Option<Lookup>,
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
pub(crate) struct HttpTcpConnector {
    pub(crate) deadline: Option<Instant>,
    pub(crate) candidates: Candidates,
    pub(crate) cancel: Cancel,
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
                self.cancel.check()?;
                let stream = match budget {
                    Some(budget) => TcpStream::connect_timeout(&address, budget),
                    None => TcpStream::connect(address),
                }
                .map_err(|error| match error.kind() {
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => {
                        ureq::Error::Timeout(ureq::Timeout::Connect)
                    }
                    _ => ureq::Error::Io(error),
                })?;
                if details.config.no_delay() {
                    stream.set_nodelay(true)?;
                }
                let transport = SocketTransport::new(
                    stream,
                    LazyBuffers::new(
                        details.config.input_buffer_size(),
                        details.config.output_buffer_size(),
                    ),
                    self.deadline,
                    self.cancel.clone(),
                )?;
                Ok(Some(Box::new(transport) as Box<dyn Transport>))
            },
        )
    }
}

/// A request's cancel signal (`std-http.send.cancel.close`). The request
/// runs on the `hd-io` runtime's blocking pool, so a cancel cannot drop
/// it. Its socket never blocks a thread instead: each read or write waits
/// for the socket on the runtime's I/O driver and for this signal at once.
/// A cancel ends the wait, and the request's own thread then drops, and so
/// closes, the socket. Shutting the socket down from the cancelling thread
/// is not enough on Windows: a shutdown does not end a `recv` blocked on
/// another thread, and the end it sends can stay unsent until that `recv`
/// returns (#160). A connection made after the cancel is closed as soon
/// as it is made.
#[derive(Clone, Debug)]
pub(crate) struct Cancel(Arc<watch::Sender<bool>>);

impl Default for Cancel {
    fn default() -> Self {
        Self(Arc::new(watch::Sender::new(false)))
    }
}

/// How a socket wait ended.
enum Waited<T> {
    Done(std::io::Result<T>),
    Cancelled,
    TimedOut,
}

impl Cancel {
    pub(crate) fn cancel(&self) {
        self.0.send_replace(true);
    }

    fn check(&self) -> Result<(), ureq::Error> {
        if *self.0.borrow() {
            return Err(cancelled());
        }
        Ok(())
    }

    /// Runs `work` on `runtime` until it ends, `timeout` passes or the
    /// request is cancelled. A cancel made before the wait ends it at once.
    fn wait<T>(
        &self,
        runtime: &Handle,
        timeout: NextTimeout,
        work: impl Future<Output = std::io::Result<T>>,
    ) -> Result<T, ureq::Error> {
        let mut signal = self.0.subscribe();
        let waited = runtime.block_on(async move {
            let raced = async {
                let mut work = std::pin::pin!(work);
                let mut cancelled = std::pin::pin!(signal.wait_for(|cancelled| *cancelled));
                std::future::poll_fn(|cx| {
                    if cancelled.as_mut().poll(cx).is_ready() {
                        return Poll::Ready(Waited::Cancelled);
                    }
                    work.as_mut().poll(cx).map(Waited::Done)
                })
                .await
            };
            match timeout.not_zero() {
                Some(after) => tokio::time::timeout(*after, raced)
                    .await
                    .unwrap_or(Waited::TimedOut),
                None => raced.await,
            }
        });
        match waited {
            Waited::Done(result) => result.map_err(|error| socket_error(error, timeout)),
            Waited::Cancelled => Err(cancelled()),
            Waited::TimedOut => Err(ureq::Error::Timeout(timeout.reason)),
        }
    }
}

fn cancelled() -> ureq::Error {
    ureq::Error::Io(std::io::Error::new(
        std::io::ErrorKind::ConnectionAborted,
        "the request was cancelled",
    ))
}

/// The connection's socket, used as ureq's own TCP transport uses it
/// (ureq does not export that one), except that each wait is on the
/// `hd-io` runtime and ends at a cancel (`Cancel`), and every wait is
/// capped by the whole-request deadline (`std-http.send.timeout.whole`).
/// Dropping it closes the connection.
struct SocketTransport {
    stream: tokio::net::TcpStream,
    runtime: &'static Handle,
    buffers: LazyBuffers,
    deadline: Option<Instant>,
    cancel: Cancel,
}

impl std::fmt::Debug for SocketTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SocketTransport").finish_non_exhaustive()
    }
}

impl SocketTransport {
    /// The transport over the connected `stream`, or the request's cancel
    /// error when the request was cancelled while it connected, which
    /// closes the connection.
    fn new(
        stream: TcpStream,
        buffers: LazyBuffers,
        deadline: Option<Instant>,
        cancel: Cancel,
    ) -> Result<Self, ureq::Error> {
        cancel.check()?;
        let runtime = crate::io_runtime::handle()
            .map_err(|error| ureq::Error::Io(std::io::Error::other(error)))?;
        stream.set_nonblocking(true)?;
        let stream = {
            let _entered = runtime.enter();
            tokio::net::TcpStream::from_std(stream)?
        };
        Ok(Self {
            stream,
            runtime,
            buffers,
            deadline,
            cancel,
        })
    }

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

/// A socket error as ureq's transport reports it: a read or write that
/// passed its timeout is a timeout of the wait's kind.
fn socket_error(error: std::io::Error, timeout: NextTimeout) -> ureq::Error {
    match error.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => {
            ureq::Error::Timeout(timeout.reason)
        }
        _ => ureq::Error::Io(error),
    }
}

impl Transport for SocketTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        let timeout = self.cap(timeout)?;
        let stream = &self.stream;
        let mut output = &self.buffers.output()[..amount];
        self.cancel.wait(self.runtime, timeout, async move {
            while !output.is_empty() {
                stream.writable().await?;
                match stream.try_write(output) {
                    Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
                    Ok(written) => output = &output[written..],
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(error),
                }
            }
            Ok(())
        })
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        let timeout = self.cap(timeout)?;
        let stream = &self.stream;
        let input = self.buffers.input_append_buf();
        let amount = self.cancel.wait(self.runtime, timeout, async move {
            loop {
                stream.readable().await?;
                match stream.try_read(input) {
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    read => return read,
                }
            }
        })?;
        self.buffers.input_appended(amount);
        Ok(amount > 0)
    }

    /// Whether the connection can take another request: the server sent
    /// nothing unasked and has not closed it.
    fn is_open(&mut self) -> bool {
        let mut byte = [MaybeUninit::uninit()];
        matches!(
            SockRef::from(&self.stream).recv(&mut byte),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
        )
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

#[cfg(test)]
mod tests {
    use super::{HttpResolver, HttpTcpConnector, connect_candidates, resolve_lookup};
    use crate::http::test_support::{
        bounded_accept, non_listening_socket, read_request, write_response_with_budget,
    };
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use ureq::unversioned::resolver::Resolver;
    use ureq::unversioned::transport::{Connector, NextTimeout, RustlsConnector};

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
        let accepted = bounded_accept(&working);
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
            let request = read_request(&mut stream); // Also bounds reads to two seconds.
            write_response_with_budget(
                &mut stream,
                b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc",
                Duration::from_secs(1),
            )
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
            cancel: super::Cancel::default(),
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
}
