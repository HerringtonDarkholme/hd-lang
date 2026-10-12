use std::collections::VecDeque;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use socket2::{Domain, Protocol, Socket, Type};

use super::client::{NetError, Outcome};
use super::state::SocketKind;
use super::{CONNECT_ATTEMPT_TIMEOUT, TICK};

#[derive(Clone, Copy)]
pub(crate) struct ConnectAttempt {
    pub(crate) address: SocketAddr,
    pub(crate) timeout: Option<Duration>,
}

impl ConnectAttempt {
    pub(crate) fn expired(self, elapsed: Duration) -> bool {
        self.timeout.is_some_and(|timeout| elapsed >= timeout)
    }
}

pub(crate) fn connect_attempts(addresses: Vec<SocketAddr>) -> Vec<ConnectAttempt> {
    let preferred_v6 = addresses.first().is_some_and(SocketAddr::is_ipv6);
    let mut preferred = Vec::new();
    let mut other = Vec::new();
    for address in addresses {
        let family = if address.is_ipv6() == preferred_v6 {
            &mut preferred
        } else {
            &mut other
        };
        if !family.contains(&address) {
            family.push(address);
        }
    }
    let count = preferred.len() + other.len();
    let mut ordered = Vec::with_capacity(count);
    for i in 0..preferred.len().max(other.len()) {
        for address in [preferred.get(i), other.get(i)].into_iter().flatten() {
            ordered.push(ConnectAttempt {
                address: *address,
                timeout: (ordered.len() + 1 < count).then_some(CONNECT_ATTEMPT_TIMEOUT),
            });
        }
    }
    ordered
}

pub(crate) fn cancelled(stop: &AtomicBool, deadline: Option<Instant>) -> bool {
    stop.load(Ordering::Relaxed) || deadline.is_some_and(|deadline| Instant::now() >= deadline)
}

pub(crate) fn resolve(
    host: &str,
    port: u16,
    stop: &AtomicBool,
    deadline: Option<Instant>,
) -> Result<Vec<SocketAddr>, NetError> {
    let owned = host.to_owned();
    resolve_lookup(host, stop, deadline, move || {
        (owned.as_str(), port)
            .to_socket_addrs()
            .map(Iterator::collect)
    })
}

fn resolve_lookup(
    host: &str,
    stop: &AtomicBool,
    deadline: Option<Instant>,
    lookup: impl FnOnce() -> io::Result<Vec<SocketAddr>> + Send + 'static,
) -> Result<Vec<SocketAddr>, NetError> {
    if cancelled(stop, deadline) {
        return Err(NetError::new(4, "the socket stopped"));
    }
    let (reply, result) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("hd-net-dns".into())
        .spawn(move || {
            // std's OS lookup cannot be aborted. The helper owns no socket or
            // worker state, so its late result cannot cause network side effects.
            let _ = reply.send(lookup());
        })
        .map_err(|_| NetError::new(2, host))?;
    loop {
        if cancelled(stop, deadline) {
            return Err(NetError::new(4, "the socket stopped"));
        }
        let wait = deadline.map_or(TICK, |deadline| {
            TICK.min(deadline.saturating_duration_since(Instant::now()))
        });
        match result.recv_timeout(wait) {
            Ok(_) if cancelled(stop, deadline) => {
                return Err(NetError::new(4, "the socket stopped"));
            }
            Ok(result) => return result.map_err(|_| NetError::new(2, host)),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(NetError::new(2, host)),
        }
    }
}

pub(crate) fn bind(method: &str, address: SocketAddr) -> io::Result<SocketKind> {
    let udp = method == "bind_udp";
    let socket = Socket::new(
        Domain::for_address(address),
        if udp { Type::DGRAM } else { Type::STREAM },
        Some(if udp { Protocol::UDP } else { Protocol::TCP }),
    )?;
    socket.set_nonblocking(true)?;
    #[cfg(unix)]
    if !udp {
        socket.set_reuse_address(true)?;
    }
    if address.is_ipv6() {
        socket.set_only_v6(false)?;
    }
    socket.bind(&address.into())?;
    if udp {
        Ok(SocketKind::Udp {
            socket: Some(socket.into()),
            inbox: VecDeque::new(),
            end: None,
            ipv6: address.is_ipv6(),
        })
    } else {
        socket.listen(511)?;
        Ok(SocketKind::Listener {
            socket: Some(socket.into()),
            inbox: VecDeque::new(),
            end: None,
        })
    }
}

pub(crate) fn connecting(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::WouldBlock {
        return true;
    }
    #[cfg(unix)]
    if matches!(
        error.raw_os_error(),
        Some(libc::EINPROGRESS | libc::EALREADY)
    ) {
        return true;
    }
    false
}

pub(crate) fn wrong_kind() -> Outcome {
    Outcome::Error(NetError::new(4, "an operation on a handle of another kind"))
}

pub(crate) fn socket_error(
    error: &io::Error,
    syscall: &str,
    actual: Option<SocketAddr>,
    host: &str,
    port: u16,
) -> NetError {
    if matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused | io::ErrorKind::AddrInUse
    ) {
        let address = if actual.is_some() {
            format!("{host}:{port}")
        } else {
            String::new()
        };
        return NetError::new(3, address);
    }
    let text = actual.map_or_else(
        || format!("{syscall}: {error}"),
        |address| format!("{syscall} {address}: {error}"),
    );
    NetError::new(4, text)
}

#[cfg(test)]
mod tests {
    use super::{connect_attempts, resolve_lookup};
    use crate::net::state::State;
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn socket_errors_keep_spec_categories_and_native_context() {
        use std::io::{Error, ErrorKind};
        for kind in [ErrorKind::ConnectionRefused, ErrorKind::AddrInUse] {
            let error = super::socket_error(
                &Error::from(kind),
                "connect",
                Some("127.0.0.1:9".parse().unwrap()),
                "example.test",
                9,
            );
            assert_eq!(error.variant, 3);
            assert_eq!(error.text, "example.test:9");
        }
        let native = Error::from(ErrorKind::ConnectionReset);
        let error = super::socket_error(&native, "read", None, "", 0);
        assert_eq!(error.variant, 4);
        assert_eq!(error.text, format!("read: {native}"));
    }
    #[test]
    fn family_selection_alternates_deduplicates_and_times_only_nonfinal_attempts() {
        let addresses = [
            "[::1]:80",
            "[::2]:80",
            "127.0.0.1:80",
            "[::1]:80",
            "127.0.0.2:80",
        ]
        .map(|s| s.parse().unwrap());
        let attempts = connect_attempts(addresses.to_vec());
        assert_eq!(
            attempts.iter().map(|a| a.address).collect::<Vec<_>>(),
            [addresses[0], addresses[2], addresses[1], addresses[4]]
        );
        // Fake elapsed time and outcomes: pending candidates expire at 250ms,
        // immediate failure advances without waiting, and the final pending
        // candidate stays live until success, OS failure, or instance stop.
        assert!(!attempts[0].expired(Duration::from_millis(249)));
        assert!(attempts[0].expired(Duration::from_millis(250)));
        assert!(attempts[1].expired(Duration::from_millis(250)));
        assert!(attempts[2].expired(Duration::from_millis(250)));
        assert!(!attempts[3].expired(Duration::from_hours(1)));
        let single = connect_attempts(vec![addresses[0], addresses[0]]);
        assert_eq!(single.len(), 1);
        assert!(!single[0].expired(Duration::from_hours(1)));
        let v4first = connect_attempts(vec![addresses[2], addresses[0], addresses[4]]);
        assert_eq!(v4first[0].address, addresses[2]);
        assert_eq!(v4first[1].address, addresses[0]);
        assert_eq!(v4first[2].address, addresses[4]);
    }
    #[test]
    fn blocked_os_lookup_does_not_hold_deadline_or_stop_cleanup() {
        for stop_lookup in [false, true] {
            let stop = Arc::new(AtomicBool::new(false));
            let cancel = Arc::clone(&stop);
            let (ready, arrived) = mpsc::channel();
            let (release, released) = mpsc::channel();
            let (finished, completion) = mpsc::channel();
            let stopper = thread::spawn(move || {
                arrived.recv_timeout(Duration::from_secs(1)).unwrap();
                if stop_lookup {
                    cancel.store(true, Ordering::Relaxed);
                }
            });
            let start = Instant::now();
            let deadline = (!stop_lookup).then_some(start + Duration::from_millis(20));
            let error = resolve_lookup("local.test", &stop, deadline, move || {
                ready.send(()).unwrap();
                let _ = released.recv_timeout(Duration::from_secs(2));
                let _ = finished.send(());
                Ok(vec!["127.0.0.1:9".parse().unwrap()])
            })
            .expect_err("blocked lookup is cancelled");
            assert_eq!(error.variant, 4);
            assert_eq!(error.text, "the socket stopped");
            assert!(start.elapsed() < Duration::from_millis(500));
            assert_eq!(completion.try_recv(), Err(mpsc::TryRecvError::Empty));
            release.send(()).unwrap();
            completion.recv_timeout(Duration::from_secs(1)).unwrap();
            stopper.join().unwrap();
        }
    }
    #[test]
    fn expired_resolution_and_connection_candidates_do_not_open_sockets() {
        let stop = AtomicBool::new(false);
        let deadline = Some(Instant::now());
        let error = resolve_lookup("local.test", &stop, deadline, || {
            panic!("expired request must not invoke OS lookup")
        })
        .expect_err("expired lookup");
        assert_eq!(error.variant, 4);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let mut state = State {
            deadline,
            ..State::default()
        };
        assert!(
            state
                .connect_candidates(vec![address], "127.0.0.1", address.port(), &stop)
                .is_err()
        );
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}
