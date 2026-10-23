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
pub(crate) enum Job {
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
    pub(crate) result: Result<Got, NetError>,
}

pub(crate) enum Got {
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

pub(crate) enum Opened {
    Stream(Stream),
    Listener(tokio::net::TcpListener),
    Udp(Udp),
}

/// The owner name of a socket's pending operations in the reactor.
const OWNER: &str = "Net";

/// `close` of socket `handle`: every operation still pending on it ends
/// with `Disposed`, and its task drops the socket it held
/// (`std-net.close.pending-closed`).
pub(crate) fn dispose(reactor: &mut Reactor, handle: i64) {
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
    pub(crate) fn offload(self, reactor: &mut Reactor) -> Result<i32, String> {
        let owner = self.handle().map(|handle| (OWNER, handle));
        reactor.offload_for(owner, async move {
            crate::runtime::reactor::Done::Net(self.run().await)
        })
    }

    /// Runs the operation.
    pub(crate) async fn run(self) -> Done {
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

pub(crate) async fn connect_candidates(
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
async fn read_now(stream: &Stream, max: usize) -> Result<Vec<u8>, NetError> {
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
async fn accept_now(listener: &tokio::net::TcpListener) -> Result<Got, NetError> {
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
async fn receive(socket: &Udp) -> Result<(Vec<u8>, SocketAddr), NetError> {
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
async fn receive_now(socket: &Udp) -> Result<(Vec<u8>, SocketAddr), NetError> {
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

#[cfg(test)]
mod tests {
    use super::{NetError, connect_candidates};
    use crate::net::test_support::{reserved_refused_port, runtime};
    use std::net::TcpListener;

    fn queued_udp_after_timeout() -> (
        tokio::runtime::Runtime,
        std::sync::Arc<super::Udp>,
        std::net::UdpSocket,
    ) {
        use std::sync::Arc;
        use std::time::{Duration, Instant};

        let runtime = runtime();
        let socket = runtime.block_on(async {
            let socket = super::Udp {
                socket: tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap(),
                ipv6: false,
                unread: std::sync::Mutex::default(),
            };
            let socket = Arc::new(socket);
            let expired = super::Job::Receive {
                socket: Arc::clone(&socket),
                handle: 1,
                max: 8,
                bound: Some(Duration::from_millis(50)),
            }
            .run()
            .await;
            assert!(matches!(expired.result, Err(NetError::Timeout)));
            socket
        });
        let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = socket.socket.local_addr().unwrap();
        peer.send_to(b"queued", address).unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        let mut probe = [std::mem::MaybeUninit::<u8>::uninit(); 6];
        loop {
            match socket2::SockRef::from(&socket.socket).peek_from(&mut probe) {
                Ok((n, from)) => {
                    assert_eq!(n, 6);
                    assert_eq!(from.as_socket(), Some(peer.local_addr().unwrap()));
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < until, "the datagram reaches the kernel");
                    std::thread::yield_now();
                }
                Err(error) => panic!("peeking at the queued datagram: {error}"),
            }
        }
        (runtime, socket, peer)
    }

    #[test]
    fn expired_udp_receive_probes_kernel_queue_without_cached_readiness() {
        let (runtime, socket, peer) = queued_udp_after_timeout();
        // Handle::block_on does not drive this current-thread runtime's
        // I/O. The non-waiting deadline probe must inspect the kernel even
        // when Tokio has not observed the newly queued datagram yet.
        let received = runtime.handle().block_on(super::receive_now(&socket));
        let (bytes, from) = received.expect("a queued datagram is not a timeout");
        assert_eq!(bytes, b"queued");
        assert_eq!(from, peer.local_addr().unwrap());
    }

    #[test]
    fn udp_receive_entry_reads_kernel_queue_without_cached_readiness() {
        use std::task::{Context, Poll, Waker};

        let (runtime, socket, peer) = queued_udp_after_timeout();
        let _entered = runtime.enter();
        let mut receive = std::pin::pin!(super::receive(&socket));
        let mut context = Context::from_waker(Waker::noop());
        let Poll::Ready(received) = receive.as_mut().poll(&mut context) else {
            panic!("queued input must be probed before waiting for readiness");
        };
        let (bytes, from) = received.expect("the queued receive completes");
        assert_eq!(bytes, b"queued");
        assert_eq!(from, peer.local_addr().unwrap());
    }

    #[test]
    fn connection_candidates_advance_on_local_refusal_and_keep_the_first_error() {
        // Keep both established pairs, so their source ports stay reserved
        // with no listener through every attempt.
        let (refused_address, _refused) = reserved_refused_port();
        let (also_refused_address, _also_refused) = reserved_refused_port();
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = server.local_addr().unwrap();
        let runtime = runtime();
        let socket = runtime
            .block_on(connect_candidates(
                vec![refused_address, address],
                "localhost",
                address.port(),
            ))
            .unwrap_or_else(|e| panic!("connection failed: {e:?}"));
        assert_eq!(socket.peer_addr().unwrap(), address);
        let (_, _) = server.accept().unwrap();
        drop(socket);
        let error = runtime
            .block_on(connect_candidates(
                vec![refused_address, also_refused_address],
                "localhost",
                address.port(),
            ))
            .unwrap_err();
        // The error kept is the first candidate's. Where a closed port
        // refuses at once, that is `Refused`. Windows refuses a closed
        // loopback port only after about two seconds, past the 250 ms
        // attempt timer, so there the first candidate's error is its
        // timeout: an `Other` that names the first address.
        match error {
            NetError::Refused { address: shown } => {
                assert_eq!(shown, format!("localhost:{}", address.port()));
            }
            NetError::Other { message } => assert!(
                message.starts_with(&format!("connect {refused_address}: ")),
                "{message}"
            ),
            error => panic!("unexpected error {error:?}"),
        }
    }
}

#[cfg(test)]
mod queued_tests {
    use super::{Got, accept_now, read_now, receive_now};
    use crate::net::socket::{Bound, Stream, bind};
    use crate::net::test_support::runtime;
    use std::io::Write;
    use std::net::{TcpListener, TcpStream, UdpSocket};
    use std::time::Duration;

    #[test]
    fn queued_tcp_bytes_are_visible_before_the_driver_observes_readiness() {
        let runtime = runtime();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (socket, _) = listener.accept().unwrap();
        socket.set_nonblocking(true).unwrap();
        let stream = {
            let _enter = runtime.enter();
            Stream::new(tokio::net::TcpStream::from_std(socket).unwrap())
        };
        peer.write_all(b"queued").unwrap();
        std::thread::sleep(Duration::from_millis(50));
        // The single-thread runtime has not run since registration.
        let got = runtime.block_on(read_now(&stream, 8)).unwrap();
        assert_eq!(got, b"queued");
    }

    #[test]
    fn queued_udp_datagram_is_visible_before_the_driver_observes_readiness() {
        let runtime = runtime();
        let socket = {
            let _enter = runtime.enter();
            let Bound::Udp(socket) = bind(true, "127.0.0.1:0".parse().unwrap()).unwrap() else {
                panic!("UDP socket")
            };
            socket
        };
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        peer.send_to(b"queued", socket.socket.local_addr().unwrap())
            .unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let (got, address) = runtime.block_on(receive_now(&socket)).unwrap();
        assert_eq!(got, b"queued");
        assert_eq!(address, peer.local_addr().unwrap());
    }

    #[test]
    fn queued_connection_is_visible_before_the_driver_observes_readiness() {
        let runtime = runtime();
        let listener = {
            let _enter = runtime.enter();
            let Bound::Listener(socket) = bind(false, "127.0.0.1:0".parse().unwrap()).unwrap()
            else {
                panic!("listener")
            };
            socket
        };
        let _peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let got = runtime.block_on(accept_now(&listener)).unwrap();
        assert!(matches!(got, Got::Opened(_)));
    }
}
