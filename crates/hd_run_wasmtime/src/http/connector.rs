use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, NextTimeout, TcpConnector, Transport,
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
