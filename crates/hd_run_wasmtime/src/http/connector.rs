use std::mem::MaybeUninit;
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::{Duration, Instant};

use socket2::SockRef;
use tokio::runtime::Handle;
use tokio::sync::{oneshot, watch};

use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, LazyBuffers, NextTimeout, Transport,
};

type Candidates = Arc<Mutex<Option<Vec<SocketAddr>>>>;

#[cfg(feature = "testing")]
type Lookup = Arc<dyn Fn(String) -> std::io::Result<Vec<SocketAddr>> + Send + Sync>;

pub struct HttpResolver {
    pub candidates: Candidates,
    pub deadline: Option<Instant>,
    pub cancel: Cancel,
    #[cfg(feature = "testing")]
    pub lookup: Option<Lookup>,
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
        #[cfg(feature = "testing")]
        let lookup = self.lookup.clone();
        let addresses = resolve_lookup(timeout, self.deadline, &self.cancel, move || {
            #[cfg(feature = "testing")]
            if let Some(lookup) = lookup {
                return lookup(address);
            }
            if let Ok(numeric) = address.parse::<SocketAddr>() {
                return Ok(vec![numeric]);
            }
            crate::net::socket::lookup_addresses(address.as_str())
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

pub fn resolve_lookup(
    timeout: NextTimeout,
    deadline: Option<Instant>,
    cancel: &Cancel,
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
    cancel.check()?;
    if budget().is_some_and(|remaining| remaining.is_zero()) {
        return Err(ureq::Error::Timeout(timeout.reason));
    }
    // The OS lookup cannot be cancelled. Its detached helper owns only
    // the lookup and reply; cancellation drops the receiver and ends the
    // request without waiting for the helper to finish.
    let (reply, receive) = oneshot::channel();
    std::thread::Builder::new()
        .name("hd-http-dns".into())
        .spawn(move || {
            let _ = reply.send(lookup());
        })
        .map_err(ureq::Error::Io)?;
    let runtime = crate::runtime::io::handle()
        .map_err(|error| ureq::Error::Io(std::io::Error::other(error)))?;
    // Neither spawning the helper nor starting the runtime restarts the
    // relative resolver budget or the absolute whole-request deadline.
    let remaining = budget();
    if remaining.is_some_and(|remaining| remaining.is_zero()) {
        return Err(ureq::Error::Timeout(timeout.reason));
    }
    let waiting = NextTimeout {
        after: remaining.map_or(
            ureq::unversioned::transport::time::Duration::NotHappening,
            Into::into,
        ),
        reason: timeout.reason,
    };
    cancel
        .wait(runtime, waiting, async move {
            receive
                .await
                .map_err(|_| std::io::Error::other("the Http DNS helper stopped without a reply"))
        })?
        .map_err(|_| ureq::Error::HostNotFound)
}

#[derive(Debug)]
pub struct HttpTcpConnector {
    pub deadline: Option<Instant>,
    /// The end of the hop's connect phase (`std-http.settings.phase-timeouts`),
    /// which bounds only the connection, never the later I/O.
    pub connect_deadline: Option<Instant>,
    pub candidates: Candidates,
    pub cancel: Cancel,
    /// The TLS handshake phase the TLS connector arms, which the socket
    /// enforces on its waits.
    pub phase: Phase,
}

/// The end of a hop's current transport phase, shared by the TLS connector
/// that arms it and the socket whose waits it caps (`std-http.settings.phase-timeouts`).
#[derive(Clone, Debug, Default)]
pub struct Phase(Arc<Mutex<Option<Instant>>>);

impl Phase {
    /// Starts a phase that ends `limit` from now, or ends the current one.
    pub(crate) fn set(&self, limit: Option<Duration>) {
        let end = limit.map(|limit| Instant::now().checked_add(limit).unwrap_or_else(far_future));
        if let Ok(mut phase) = self.0.lock() {
            *phase = end;
        }
    }

    fn end(&self) -> Option<Instant> {
        self.0.lock().ok().and_then(|phase| *phase)
    }
}

/// An instant beyond any deadline a request can reach.
fn far_future() -> Instant {
    Instant::now() + Duration::from_hours(876_000)
}

/// The earlier of two optional deadlines.
pub(crate) fn earlier(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, None) => a,
        (None, b) => b,
    }
}

impl Connector<()> for HttpTcpConnector {
    type Out = Box<dyn Transport>;

    fn connect(
        &self,
        details: &ConnectionDetails<'_>,
        _: Option<()>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        let started = Instant::now();
        let total = earlier(self.deadline, self.connect_deadline)
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
                    self.phase.clone(),
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
pub struct Cancel(Arc<watch::Sender<bool>>);

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
    pub fn cancel(&self) {
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
    phase: Phase,
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
        phase: Phase,
    ) -> Result<Self, ureq::Error> {
        cancel.check()?;
        let runtime = crate::runtime::io::handle()
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
            phase,
        })
    }

    /// Caps a wait at the whole-request deadline and at the end of the
    /// current phase; a phase that has already ended times out at once
    /// (`std-http.settings.zero-timeout`).
    fn cap(&self, mut timeout: NextTimeout) -> Result<NextTimeout, ureq::Error> {
        for (deadline, reason) in [
            (self.deadline, ureq::Timeout::Global),
            (self.phase.end(), ureq::Timeout::Connect),
        ] {
            let Some(deadline) = deadline else { continue };
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ureq::Error::Timeout(reason));
            }
            if timeout
                .not_zero()
                .is_none_or(|duration| *duration > remaining)
            {
                timeout.after = remaining.into();
                timeout.reason = reason;
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

pub fn connect_candidates<T>(
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
