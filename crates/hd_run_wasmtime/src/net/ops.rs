//! The `Net` operations that wait, each run as one task on the `hd-io`
//! runtime. Every wait is a tokio readiness
//! wait, so nothing polls. `read`, `accept` and `receive` take nothing
//! from the socket until they complete, so aborting one leaves the bytes,
//! the connection or the datagram in the kernel for the next call
//! (`std-net.cancel.read`).
//!
//! A handle's timeout bounds its `read`, `accept` and `receive` with a
//! tokio timer of host time (`std-net.timeout.host-time`). When the timer
//! ends first, the wait is dropped, so it took nothing, and a nonblocking
//! probe returns queued input or `Timeout` (`std-net.timeout.error`). UDP
//! probes the actual kernel queue before waiting and at the deadline:
//! cached readiness alone cannot prove that queue empty. A negative UDP
//! probe still follows the managed Tokio/Mio path, whose actual
//! `WouldBlock` results re-arm Windows readiness before further waiting.

use std::io;
use std::net::{Ipv6Addr, Shutdown, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use std::task::{Context, Poll, Waker};

use super::CHUNK;
use super::client::NetError;
use super::socket::{Bound, End, Stream, Udp, bind, connect_attempts, resolve, socket_error};
use super::validate::ip_literal;
use crate::runtime::reactor::Reactor;

/// An operation that `.start` checked and that now waits.
pub enum Job {
    Lookup(String),
    Connect {
        host: String,
        port: u16,
    },
    Bind {
        udp: bool,
        host: String,
        port: u16,
    },
    Read {
        stream: Arc<Stream>,
        handle: i64,
        max: usize,
        bound: Option<Duration>,
    },
    Write {
        stream: Arc<Stream>,
        handle: i64,
        bytes: Vec<u8>,
    },
    Accept {
        listener: Arc<tokio::net::TcpListener>,
        handle: i64,
        bound: Option<Duration>,
    },
    Send {
        socket: Arc<Udp>,
        handle: i64,
        host: String,
        port: u16,
        bytes: Vec<u8>,
    },
    Receive {
        socket: Arc<Udp>,
        handle: i64,
        max: usize,
        bound: Option<Duration>,
    },
}

/// A completed operation, for its `.finish` to encode.
pub struct Done {
    /// Whether the result is a handle's (`Result[T, NetError]` too).
    pub(crate) resource: bool,
    pub result: Result<Got, NetError>,
}

pub enum Got {
    None,
    Addresses(Vec<String>),
    /// A new socket, which `.finish` puts in the handle table.
    Opened(Opened),
    /// The operation ended because its handle closed
    /// (`std-net.close.pending-closed`).
    Disposed,
    /// A read's bytes from the stream with handle `handle`.
    Bytes {
        handle: i64,
        bytes: Vec<u8>,
    },
    /// A whole datagram from the socket with handle `handle`, which
    /// `.finish` cuts to `max` bytes.
    Datagram {
        handle: i64,
        bytes: Vec<u8>,
        max: usize,
        peer: SocketAddr,
    },
}

pub enum Opened {
    Stream(Stream),
    Listener(tokio::net::TcpListener),
    Udp(Udp),
}

/// The owner name of a socket's pending operations in the reactor.
const OWNER: &str = "Net";

/// `close` of socket `handle`: every operation still pending on it ends
/// with `Disposed`, and its task drops the socket it held
/// (`std-net.close.pending-closed`).
pub fn dispose(reactor: &mut Reactor, handle: i64) {
    reactor.dispose_owned((OWNER, handle), || {
        crate::runtime::reactor::Done::Net(Done {
            resource: true,
            result: Ok(Got::Disposed),
        })
    });
}

impl Job {
    /// Whether the operation is a handle's.
    pub(crate) fn resource(&self) -> bool {
        !matches!(
            self,
            Job::Lookup(_) | Job::Connect { .. } | Job::Bind { .. }
        )
    }

    /// The handle the operation is on, for `close` to find it.
    fn handle(&self) -> Option<i64> {
        match self {
            Job::Lookup(_) | Job::Connect { .. } | Job::Bind { .. } => None,
            Job::Read { handle, .. }
            | Job::Write { handle, .. }
            | Job::Accept { handle, .. }
            | Job::Send { handle, .. }
            | Job::Receive { handle, .. } => Some(*handle),
        }
    }

    /// Runs the job as a task of the reactor, owned by its handle.
    pub fn offload(self, reactor: &mut Reactor) -> Result<i32, String> {
        let owner = self.handle().map(|handle| (OWNER, handle));
        reactor.offload_for(owner, async move {
            crate::runtime::reactor::Done::Net(self.run().await)
        })
    }

    /// Runs the operation.
    pub async fn run(self) -> Done {
        let resource = self.resource();
        let result = match self {
            Job::Lookup(host) => lookup(&host).await,
            // Cancelling the suspension drops the connect and its socket.
            Job::Connect { host, port } => connect(&host, port).await,
            Job::Bind { udp, host, port } => open(udp, &host, port).await,
            Job::Read {
                stream,
                handle,
                max,
                bound,
            } => within(bound, read(&stream, max), || read_now(&stream, max))
                .await
                .map(|bytes| Got::Bytes { handle, bytes }),
            Job::Write { stream, bytes, .. } => write(&stream, &bytes).await.map(|()| Got::None),
            Job::Accept {
                listener, bound, ..
            } => within(bound, accept(&listener), || accept_now(&listener)).await,
            Job::Send {
                socket,
                host,
                port,
                bytes,
                ..
            } => send_to(&socket, &host, port, &bytes).await,
            Job::Receive {
                socket,
                handle,
                max,
                bound,
            } => within(bound, receive(&socket), || receive_now(&socket))
                .await
                .map(|(bytes, peer)| Got::Datagram {
                    handle,
                    bytes,
                    max,
                    peer,
                }),
        };
        Done { resource, result }
    }
}

async fn lookup(host: &str) -> Result<Got, NetError> {
    let addresses = resolve(host, 0).await?;
    Ok(Got::Addresses(
        addresses.iter().map(|a| a.ip().to_string()).collect(),
    ))
}

/// Tries each candidate in Node's family order, giving every one but the
/// last 250 ms; the first failure names the error.
pub(crate) async fn connect(host: &str, port: u16) -> Result<Got, NetError> {
    let addresses = resolve(host, port).await?;
    connect_candidates(addresses, host, port)
        .await
        .map(|socket| Got::Opened(Opened::Stream(Stream::new(socket))))
}

pub async fn connect_candidates(
    addresses: Vec<SocketAddr>,
    host: &str,
    port: u16,
) -> Result<tokio::net::TcpStream, NetError> {
    let mut first_error = None;
    for attempt in connect_attempts(addresses) {
        let connecting = tokio::net::TcpStream::connect(attempt.address);
        // A candidate past its time is dropped, which closes it, before the
        // next family is tried, as Node's automatic-family timer does.
        let result = match attempt.timeout {
            Some(timeout) => tokio::time::timeout(timeout, connecting)
                .await
                .unwrap_or_else(|_| Err(io::ErrorKind::TimedOut.into())),
            None => connecting.await,
        };
        match result {
            Ok(socket) => return Ok(socket),
            Err(error) => {
                first_error.get_or_insert_with(|| {
                    socket_error(&error, "connect", Some(attempt.address), host, port)
                });
            }
        }
    }
    Err(first_error.unwrap_or_else(|| NetError::Dns { host: host.into() }))
}

/// `listen!` (`udp` false) or `bind_udp!`, on the first address of `host`;
/// for UDP, the first of the family the host is written in.
async fn open(udp: bool, host: &str, port: u16) -> Result<Got, NetError> {
    let addresses = resolve(host, port).await?;
    let ipv6 = host
        .split('%')
        .next()
        .is_some_and(|ip| ip.parse::<Ipv6Addr>().is_ok());
    let Some(address) = addresses.into_iter().find(|a| !udp || a.is_ipv6() == ipv6) else {
        return Err(NetError::Dns { host: host.into() });
    };
    match bind(udp, address) {
        Ok(Bound::Listener(listener)) => Ok(Got::Opened(Opened::Listener(listener))),
        Ok(Bound::Udp(socket)) => Ok(Got::Opened(Opened::Udp(socket))),
        Err(error) => Err(socket_error(
            &error,
            if udp { "bind" } else { "listen" },
            Some(address),
            host,
            port,
        )),
    }
}

/// At least one byte and at most `max`, or an empty list at the end. The
/// buffer is made only once the socket is readable, so a waiting read
/// holds none.
pub(crate) async fn read(stream: &Stream, max: usize) -> Result<Vec<u8>, NetError> {
    let mut bytes = Vec::new();
    loop {
        if let Some(end) = stream.state().end.clone() {
            return ended(end);
        }
        if let Err(error) = stream.socket.readable().await {
            return Err(stream_failed(stream, &error, "read"));
        }
        bytes.resize(max.min(CHUNK), 0);
        match stream.socket.try_read(&mut bytes) {
            Ok(0) => {
                let end = stream.state().end.get_or_insert(End::Eof).clone();
                return ended(end);
            }
            Ok(n) => {
                bytes.truncate(n);
                return Ok(bytes);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(stream_failed(stream, &error, "read")),
        }
    }
}

pub(crate) fn ended(end: End) -> Result<Vec<u8>, NetError> {
    match end {
        End::Eof => Ok(Vec::new()),
        End::Error(error) => Err(error),
    }
}

/// `wait` bounded by `bound`: past it, `now` answers from what the socket
/// already holds.
async fn within<T, F: Future<Output = Result<T, NetError>>>(
    bound: Option<Duration>,
    wait: impl Future<Output = Result<T, NetError>>,
    now: impl FnOnce() -> F,
) -> Result<T, NetError> {
    match bound {
        None => wait.await,
        Some(bound) => match tokio::time::timeout(bound, wait).await {
            Ok(result) => result,
            Err(_) => now().await,
        },
    }
}

/// Whether a non-blocking socket call found nothing there.
fn would_block(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}

/// Give the driver a turn to observe input already queued in the kernel,
/// then poll readiness once. A zero bound must not wait for future input,
/// and the following tracked probe must still re-arm Mio on `WouldBlock`.
async fn observe_readiness<T>(readiness: impl Future<Output = io::Result<T>>) -> io::Result<()> {
    tokio::task::yield_now().await;
    let mut readiness = std::pin::pin!(readiness);
    std::future::poll_fn(|context| {
        Poll::Ready(match readiness.as_mut().poll(context) {
            Poll::Ready(Err(error)) => Err(error),
            Poll::Ready(Ok(_)) | Poll::Pending => Ok(()),
        })
    })
    .await
}

/// A bounded read whose bound ended: the bytes or the end the kernel
/// already holds, or `Timeout`.
pub async fn read_now(stream: &Stream, max: usize) -> Result<Vec<u8>, NetError> {
    if let Some(end) = stream.state().end.clone() {
        return ended(end);
    }
    observe_readiness(stream.socket.ready(tokio::io::Interest::READABLE))
        .await
        .map_err(|error| stream_failed(stream, &error, "read"))?;
    let mut bytes = vec![0; max.min(CHUNK)];
    match stream.socket.try_read(&mut bytes) {
        Ok(0) => {
            let end = stream.state().end.get_or_insert(End::Eof).clone();
            ended(end)
        }
        Ok(n) => {
            bytes.truncate(n);
            Ok(bytes)
        }
        Err(error) if would_block(&error) => Err(NetError::Timeout),
        Err(error) => Err(stream_failed(stream, &error, "read")),
    }
}

/// The next connection's stream.
async fn accept(listener: &tokio::net::TcpListener) -> Result<Got, NetError> {
    match listener.accept().await {
        Ok((socket, _)) => Ok(Got::Opened(Opened::Stream(Stream::new(socket)))),
        Err(error) => Err(socket_error(&error, "accept", None, "", 0)),
    }
}

/// A bounded accept whose bound ended: a connection already queued, or
/// `Timeout`.
pub async fn accept_now(listener: &tokio::net::TcpListener) -> Result<Got, NetError> {
    tokio::task::yield_now().await;
    let mut context = Context::from_waker(Waker::noop());
    let accepted = match listener.poll_accept(&mut context) {
        Poll::Ready(result) => result.map(|(socket, _)| socket),
        Poll::Pending => return Err(NetError::Timeout),
    };
    match accepted {
        Ok(socket) => Ok(Got::Opened(Opened::Stream(Stream::new(socket)))),
        Err(error) if would_block(&error) => Err(NetError::Timeout),
        Err(error) => Err(socket_error(&error, "accept", None, "", 0)),
    }
}

/// A read's failure, which stays: every later read reports it.
fn stream_failed(stream: &Stream, error: &io::Error, syscall: &str) -> NetError {
    let error = socket_error(error, syscall, None, "", 0);
    let mut state = stream.state();
    match state.end.get_or_insert_with(|| End::Error(error.clone())) {
        End::Error(first) => first.clone(),
        End::Eof => error,
    }
}

/// Completes once every byte is written. A failure ends the stream: both
/// directions shut, and later reads and writes report it.
async fn write(stream: &Stream, bytes: &[u8]) -> Result<(), NetError> {
    let mut written = 0;
    while written < bytes.len() {
        if let Err(error) = stream.socket.writable().await {
            return Err(write_failed(stream, &error));
        }
        match stream.socket.try_write(&bytes[written..]) {
            Ok(0) => return Err(write_failed(stream, &io::ErrorKind::WriteZero.into())),
            Ok(n) => written += n,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(write_failed(stream, &error)),
        }
    }
    Ok(())
}

fn write_failed(stream: &Stream, error: &io::Error) -> NetError {
    let error = socket_error(error, "write", None, "", 0);
    stream.state().end = Some(End::Error(error.clone()));
    stream.shutdown(Shutdown::Both);
    error
}

/// One datagram to the first address of `host`, of the socket's family
/// unless `host` is an IP address.
async fn send_to(socket: &Udp, host: &str, port: u16, bytes: &[u8]) -> Result<Got, NetError> {
    let addresses = resolve(host, port).await?;
    let address = if ip_literal(host) {
        addresses.into_iter().next()
    } else {
        addresses.into_iter().find(|a| a.is_ipv6() == socket.ipv6)
    };
    let Some(address) = address else {
        return Err(NetError::Dns { host: host.into() });
    };
    match socket.socket.send_to(bytes, address).await {
        Ok(_) => Ok(Got::None),
        Err(error) => Err(socket_error(&error, "send", Some(address), host, port)),
    }
}

/// Probe the nonblocking kernel socket, without treating Tokio's cached
/// readiness as evidence that its receive queue is empty. The temporary
/// duplicate shares the queue and nonblocking mode, but registers no new
/// reactor interest. It lets safe std receive bytes and their sender
/// together, without unsafe conversions of socket2's uninitialized buffer.
fn receive_queued(socket: &Udp) -> Result<Option<(Vec<u8>, SocketAddr)>, NetError> {
    let probe: std::net::UdpSocket = socket2::SockRef::from(&socket.socket)
        .try_clone()
        .map_err(|error| socket_error(&error, "recvmsg", None, "", 0))?
        .into();
    let mut bytes = vec![0; CHUNK];
    loop {
        match probe.recv_from(&mut bytes) {
            Ok((n, peer)) => {
                bytes.truncate(n);
                return Ok(Some((bytes, peer)));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
            Err(error) => return Err(socket_error(&error, "recvmsg", None, "", 0)),
        }
    }
}

/// The next whole datagram and its sender. Already queued input is read
/// before waiting for a readiness notification.
pub async fn receive(socket: &Udp) -> Result<(Vec<u8>, SocketAddr), NetError> {
    if let Some(queued) = receive_queued(socket)? {
        return Ok(queued);
    }
    let mut bytes = Vec::new();
    loop {
        if let Err(error) = socket.socket.readable().await {
            return Err(socket_error(&error, "recvmsg", None, "", 0));
        }
        bytes.resize(CHUNK, 0);
        match socket.socket.try_recv_from(&mut bytes) {
            Ok((n, peer)) => {
                bytes.truncate(n);
                return Ok((bytes, peer));
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(socket_error(&error, "recvmsg", None, "", 0)),
        }
    }
}

/// A bounded receive whose bound ended: inspect the actual queue before
/// deciding `Timeout`. A negative kernel probe still follows the tracked
/// path: Windows must re-arm Mio after an actual `WouldBlock` whenever
/// cached readiness indicated that the socket was readable.
pub async fn receive_now(socket: &Udp) -> Result<(Vec<u8>, SocketAddr), NetError> {
    if let Some(queued) = receive_queued(socket)? {
        return Ok(queued);
    }
    observe_readiness(socket.socket.ready(tokio::io::Interest::READABLE))
        .await
        .map_err(|error| socket_error(&error, "recvmsg", None, "", 0))?;
    let mut bytes = vec![0; CHUNK];
    match socket.socket.try_recv_from(&mut bytes) {
        Ok((n, peer)) => {
            bytes.truncate(n);
            Ok((bytes, peer))
        }
        Err(error) if would_block(&error) => Err(NetError::Timeout),
        Err(error) => Err(socket_error(&error, "recvmsg", None, "", 0)),
    }
}
