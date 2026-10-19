//! The sockets the `Net` handle table holds, and what the operations on
//! them share: name resolution, the connect candidate order, binding, and
//! the `NetError` of a failed socket call.

use std::collections::VecDeque;
use std::io;
use std::net::{IpAddr, Shutdown, SocketAddr, ToSocketAddrs};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use socket2::{Domain, Protocol, SockRef, Socket, TcpKeepalive, Type};

use super::CONNECT_ATTEMPT_TIMEOUT;
use super::client::NetError;

#[derive(Clone, Copy)]
pub(crate) struct ConnectAttempt {
    pub(crate) address: SocketAddr,
    /// How long this candidate may take before the next one is tried; the
    /// last candidate has no limit of its own.
    pub(crate) timeout: Option<Duration>,
}

/// Node's automatic-family order: the first address's family, then the
/// other, alternating, each address once. Every candidate but the last
/// gets `CONNECT_ATTEMPT_TIMEOUT`.
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

/// The addresses of `host` with `port`: an IP address as it is, a name
/// through the OS resolver on the runtime's blocking pool. `Dns(host)` when
/// the name does not resolve.
pub(crate) async fn resolve(host: &str, port: u16) -> Result<Vec<SocketAddr>, NetError> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    let owned = host.to_owned();
    resolve_with(host, move || lookup_addresses((owned.as_str(), port))).await
}

/// `resolve` with the blocking lookup `lookup`. The OS lookup cannot be
/// stopped: an aborted task leaves it to finish on its pool thread, and
/// its result is dropped there.
pub(crate) async fn resolve_with(
    host: &str,
    lookup: impl FnOnce() -> io::Result<Vec<SocketAddr>> + Send + 'static,
) -> Result<Vec<SocketAddr>, NetError> {
    match tokio::task::spawn_blocking(lookup).await {
        Ok(Ok(addresses)) => Ok(addresses),
        Ok(Err(_)) | Err(_) => Err(NetError::Dns { host: host.into() }),
    }
}

/// Resolves names with the configured-family policy shared by Net and Http.
pub(crate) fn lookup_addresses(address: impl ToSocketAddrs) -> io::Result<Vec<SocketAddr>> {
    configured_lookup(
        || address.to_socket_addrs().map(Iterator::collect),
        configured_families,
    )
}

/// Injectable lookup and configured-family probe; the pair is (IPv4, IPv6).
pub(crate) fn configured_lookup(
    lookup: impl FnOnce() -> io::Result<Vec<SocketAddr>>,
    configured: impl FnOnce() -> (bool, bool),
) -> io::Result<Vec<SocketAddr>> {
    let addresses = lookup()?;
    #[cfg(windows)]
    {
        let _ = configured;
        Ok(addresses)
    }
    #[cfg(not(windows))]
    {
        let (ipv4, ipv6) = configured();
        let keep = |address: &SocketAddr| if address.is_ipv4() { ipv4 } else { ipv6 };
        // A route probe may miss an otherwise usable family. Preserve the
        // original candidates when filtering would leave no way to connect.
        if addresses.iter().any(keep) {
            Ok(addresses.into_iter().filter(keep).collect())
        } else {
            Ok(addresses)
        }
    }
}

/// Approximate `AI_ADDRCONFIG` using safe route selection, once per lookup.
/// Connecting a UDP socket chooses a local address without sending a packet.
#[cfg(not(windows))]
fn configured_families() -> (bool, bool) {
    fn configured(bind: &str, destination: &str) -> bool {
        std::net::UdpSocket::bind(bind)
            .and_then(|socket| {
                socket.connect(destination)?;
                socket.local_addr()
            })
            .is_ok_and(|address| !address.ip().is_unspecified() && !address.ip().is_loopback())
    }
    (
        configured("0.0.0.0:0", "192.0.2.1:9"),
        configured("[::]:0", "[2001:db8::1]:9"),
    )
}

#[cfg(windows)]
fn configured_families() -> (bool, bool) {
    (true, true)
}

/// A TCP stream and what its operations share.
pub(crate) struct Stream {
    pub(crate) socket: tokio::net::TcpStream,
    pub(crate) state: Mutex<StreamState>,
}

#[derive(Default)]
pub(crate) struct StreamState {
    /// Bytes a read took from the socket but never returned, because its
    /// suspension was cancelled after it completed: the next read returns
    /// them first (`std-net.cancel.read`).
    pub(crate) unread: VecDeque<u8>,
    /// How reading ended, which every later read reports again; a failed
    /// write ends the stream too.
    pub(crate) end: Option<End>,
}

#[derive(Clone)]
pub(crate) enum End {
    Eof,
    Error(NetError),
}

impl Stream {
    pub(crate) fn new(socket: tokio::net::TcpStream) -> Self {
        Self {
            socket,
            state: Mutex::default(),
        }
    }

    pub(crate) fn state(&self) -> std::sync::MutexGuard<'_, StreamState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn shutdown(&self, how: Shutdown) {
        let _ = SockRef::from(&self.socket).shutdown(how);
    }
}

/// A UDP socket, and the datagrams cancelled receives took but never
/// returned (`std-net.cancel.read`), for the next receive.
pub(crate) struct Udp {
    pub(crate) socket: tokio::net::UdpSocket,
    pub(crate) ipv6: bool,
    pub(crate) unread: Mutex<VecDeque<Unread>>,
}

/// A datagram a cancelled receive took: its bytes and its sender.
pub(crate) type Unread = (Vec<u8>, SocketAddr);

impl Udp {
    pub(crate) fn unread(&self) -> std::sync::MutexGuard<'_, VecDeque<Unread>> {
        self.unread.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A listening TCP socket, or a UDP socket, bound to `address`. It runs in
/// a task: the tokio socket registers with the runtime that runs it.
pub(crate) fn bind(udp: bool, address: SocketAddr) -> io::Result<Bound> {
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
    // tokio's sockets set neither the dual-stack option nor the backlog.
    if address.is_ipv6() {
        socket.set_only_v6(false)?;
    }
    socket.bind(&address.into())?;
    if udp {
        Ok(Bound::Udp(Udp {
            socket: tokio::net::UdpSocket::from_std(socket.into())?,
            ipv6: address.is_ipv6(),
            unread: Mutex::default(),
        }))
    } else {
        socket.listen(511)?;
        Ok(Bound::Listener(tokio::net::TcpListener::from_std(
            socket.into(),
        )?))
    }
}

pub(crate) enum Bound {
    Listener(tokio::net::TcpListener),
    Udp(Udp),
}

pub(crate) fn wrong_kind() -> NetError {
    NetError::Other {
        message: "an operation on a handle of another kind".into(),
    }
}

/// `std-net.option.nodelay`: `TCP_NODELAY` on or off.
pub(crate) fn set_nodelay(stream: &Stream, enabled: bool) -> Result<(), NetError> {
    stream
        .socket
        .set_nodelay(enabled)
        .map_err(|error| option_error(&error, "set_nodelay"))
}

/// `std-net.option.keepalive`: keep-alive off, or on with `idle` before the
/// first probe. The system counts whole seconds, so a shorter idle rounds up
/// to one second, as on Node.
pub(crate) fn set_keepalive(stream: &Stream, idle: Option<Duration>) -> Result<(), NetError> {
    let socket = SockRef::from(&stream.socket);
    let set = match idle {
        None => socket.set_keepalive(false),
        Some(idle) => {
            let secs = idle.as_secs() + u64::from(idle.subsec_nanos() > 0);
            socket
                .set_tcp_keepalive(&TcpKeepalive::new().with_time(Duration::from_secs(secs.max(1))))
        }
    };
    set.map_err(|error| option_error(&error, "set_keepalive"))
}

/// `std-net.local-address`: the system could not report the bound address.
pub(crate) fn local_address_error(error: &io::Error) -> NetError {
    NetError::Other {
        message: format!("local_address: {error}"),
    }
}

/// An option the system refused, with its message (`std-net.option.failure`).
fn option_error(error: &io::Error, option: &str) -> NetError {
    NetError::Other {
        message: format!("{option}: {error}"),
    }
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
        return NetError::Refused { address };
    }
    let text = actual.map_or_else(
        || format!("{syscall}: {error}"),
        |address| format!("{syscall} {address}: {error}"),
    );
    NetError::Other { message: text }
}

#[cfg(test)]
mod tests {
    use super::{CONNECT_ATTEMPT_TIMEOUT, NetError, connect_attempts, resolve_with};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    #[cfg(not(windows))]
    #[test]
    fn ipv4_only_configuration_tries_ipv4_before_a_resolved_ipv6_candidate() {
        let v6 = "[2001:db8::8]:80".parse().unwrap();
        let v4 = "192.0.2.8:80".parse().unwrap();
        let probed = std::cell::Cell::new(0);
        let addresses = super::configured_lookup(
            || Ok(vec![v6, v4]),
            || {
                probed.set(probed.get() + 1);
                (true, false)
            },
        )
        .unwrap();
        let attempts = connect_attempts(addresses);
        assert_eq!(attempts[0].address, v4);
        assert_eq!(attempts.len(), 1);
        assert_eq!(probed.get(), 1);
    }

    #[test]
    fn configured_lookup_preserves_order_and_falls_back_when_filtering_is_empty() {
        let v6a = "[2001:db8::1]:80".parse().unwrap();
        let v4a = "192.0.2.1:80".parse().unwrap();
        let v6b = "[2001:db8::2]:80".parse().unwrap();
        let v4b = "192.0.2.2:80".parse().unwrap();
        let original = vec![v6a, v4a, v6b, v4b];
        for (families, expected) in [
            ((true, true), original.clone()),
            ((true, false), vec![v4a, v4b]),
            ((false, true), vec![v6a, v6b]),
            ((false, false), original.clone()),
        ] {
            let found = super::configured_lookup(|| Ok(original.clone()), || families).unwrap();
            assert_eq!(
                found,
                if cfg!(windows) {
                    original.clone()
                } else {
                    expected
                }
            );
        }
        assert_eq!(
            super::configured_lookup(|| Ok(vec![v6a, v6b]), || (true, false)).unwrap(),
            vec![v6a, v6b],
        );
        let empty = super::configured_lookup(|| Ok(Vec::new()), || (true, false)).unwrap();
        assert!(empty.is_empty());
        let failed = super::configured_lookup(
            || Err(std::io::ErrorKind::NotFound.into()),
            || panic!("a failed lookup needs no family probe"),
        );
        assert_eq!(failed.unwrap_err().kind(), std::io::ErrorKind::NotFound);
    }

    #[cfg(windows)]
    #[test]
    fn windows_lookup_keeps_every_family_without_probing() {
        let addresses = vec!["[::1]:80".parse().unwrap(), "127.0.0.1:80".parse().unwrap()];
        let found = super::configured_lookup(
            || Ok(addresses.clone()),
            || panic!("Windows does not apply AI_ADDRCONFIG"),
        )
        .unwrap();
        assert_eq!(found, addresses);
    }

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
            assert_eq!(
                error,
                NetError::Refused {
                    address: "example.test:9".into()
                }
            );
        }
        let native = Error::from(ErrorKind::ConnectionReset);
        let error = super::socket_error(&native, "read", None, "", 0);
        assert_eq!(
            error,
            NetError::Other {
                message: format!("read: {native}")
            }
        );
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
        // Every candidate but the last waits at most 250 ms; the last one
        // stays until it connects, fails, or its operation is aborted.
        assert_eq!(
            attempts.iter().map(|a| a.timeout).collect::<Vec<_>>(),
            [
                Some(CONNECT_ATTEMPT_TIMEOUT),
                Some(CONNECT_ATTEMPT_TIMEOUT),
                Some(CONNECT_ATTEMPT_TIMEOUT),
                None
            ]
        );
        assert_eq!(CONNECT_ATTEMPT_TIMEOUT, Duration::from_millis(250));
        let single = connect_attempts(vec![addresses[0], addresses[0]]);
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].timeout, None);
        let v4first = connect_attempts(vec![addresses[2], addresses[0], addresses[4]]);
        assert_eq!(v4first[0].address, addresses[2]);
        assert_eq!(v4first[1].address, addresses[0]);
        assert_eq!(v4first[2].address, addresses[4]);
    }

    /// An OS lookup that does not return holds no task: aborting the
    /// lookup's task ends it at once, and the lookup finishes later on its
    /// own pool thread.
    #[test]
    fn an_aborted_lookup_does_not_wait_for_the_os_resolver() {
        // The `hd-io` runtime, which its own thread drives.
        let runtime = crate::runtime::io::handle().unwrap();
        let (ready, arrived) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        let (finished, completion) = mpsc::channel();
        let task = runtime.spawn(async move {
            resolve_with("local.test", move || {
                ready.send(()).unwrap();
                let _ = released.recv_timeout(Duration::from_secs(10));
                let _ = finished.send(());
                Ok(vec!["127.0.0.1:9".parse().unwrap()])
            })
            .await
        });
        arrived.recv_timeout(Duration::from_secs(10)).unwrap();
        let start = Instant::now();
        task.abort();
        assert!(runtime.block_on(task).unwrap_err().is_cancelled());
        assert!(start.elapsed() < Duration::from_secs(1));
        assert_eq!(completion.try_recv(), Err(mpsc::TryRecvError::Empty));
        release.send(()).unwrap();
        completion.recv_timeout(Duration::from_secs(10)).unwrap();
    }
}
