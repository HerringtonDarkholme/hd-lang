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
pub struct ConnectAttempt {
    pub address: SocketAddr,
    /// How long this candidate may take before the next one is tried; the
    /// last candidate has no limit of its own.
    pub timeout: Option<Duration>,
}

/// Node's automatic-family order: the first address's family, then the
/// other, alternating, each address once. Every candidate but the last
/// gets `CONNECT_ATTEMPT_TIMEOUT`.
pub fn connect_attempts(addresses: Vec<SocketAddr>) -> Vec<ConnectAttempt> {
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
pub async fn resolve_with(
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
pub fn configured_lookup(
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
pub struct Stream {
    pub socket: tokio::net::TcpStream,
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
    pub fn new(socket: tokio::net::TcpStream) -> Self {
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
pub struct Udp {
    pub socket: tokio::net::UdpSocket,
    pub ipv6: bool,
    pub unread: Mutex<VecDeque<Unread>>,
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
pub fn bind(udp: bool, address: SocketAddr) -> io::Result<Bound> {
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

pub enum Bound {
    Listener(tokio::net::TcpListener),
    Udp(Udp),
}

#[must_use]
pub fn wrong_kind() -> NetError {
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

#[must_use]
pub fn socket_error(
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
