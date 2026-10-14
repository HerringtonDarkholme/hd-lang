//! The `Net` operations that wait, each run as one task on the `hd-io`
//! runtime (wasmtime-host-audit.md §4.3). Every wait is a tokio readiness
//! wait, so nothing polls. `read`, `accept` and `receive` take nothing
//! from the socket until they complete, so aborting one leaves the bytes,
//! the connection or the datagram in the kernel for the next call
//! (`std-net.cancel.read`).
//!
//! What a task holds counts against the program's heap budget
//! (`std-host.op.heap`): a write's or a send's payload from `.start`, and a
//! read or receive buffer, reserved once the socket is readable. A buffer
//! past the budget ends the operation, and `.finish` panics
//! `heap-exhausted`.

use std::io;
use std::net::{Ipv6Addr, Shutdown, SocketAddr};
use std::sync::Arc;

use super::CHUNK;
use super::client::NetError;
use super::socket::{Bound, End, Stream, Udp, bind, connect_attempts, resolve, socket_error};
use super::validate::ip_literal;
use crate::limits::{HeapExhausted, Ledger, Reservation};
use crate::reactor::Reactor;

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
        ledger: Arc<Ledger>,
    },
    Write {
        stream: Arc<Stream>,
        handle: i64,
        bytes: Vec<u8>,
        held: Reservation,
    },
    Accept {
        listener: Arc<tokio::net::TcpListener>,
        handle: i64,
    },
    Send {
        socket: Arc<Udp>,
        handle: i64,
        host: String,
        port: u16,
        bytes: Vec<u8>,
        held: Reservation,
    },
    Receive {
        socket: Arc<Udp>,
        handle: i64,
        max: usize,
        ledger: Arc<Ledger>,
    },
}

/// A completed operation, for its `.finish` to encode.
pub struct Done {
    /// Whether the result is a handle's, `Result[T, ResourceError[NetError]]`.
    pub(crate) resource: bool,
    pub(crate) result: Result<Got, NetError>,
}

pub(crate) enum Got {
    None,
    Addresses(Vec<String>),
    /// A new socket, which `.finish` puts in the handle table.
    Opened(Opened),
    /// The operation ended because its handle closed
    /// (`std-net.close.pending`).
    Disposed,
    /// A read's bytes from the stream with handle `handle`, and the heap
    /// they hold.
    Bytes {
        handle: i64,
        bytes: Vec<u8>,
        held: Reservation,
    },
    /// A whole datagram from the socket with handle `handle`, which
    /// `.finish` cuts to `max` bytes, and the heap it holds.
    Datagram {
        handle: i64,
        bytes: Vec<u8>,
        max: usize,
        peer: SocketAddr,
        held: Reservation,
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
/// (`std-net.close.pending`).
pub(crate) fn dispose(reactor: &mut Reactor, handle: i64) {
    reactor.dispose_owned((OWNER, handle), || {
        crate::reactor::Done::Net(Done {
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
            self.run()
                .await
                .map_or(crate::reactor::Done::Exhausted, crate::reactor::Done::Net)
        })
    }

    /// Runs the operation: its result, or `HeapExhausted` when its buffer
    /// would pass the heap budget.
    pub(crate) async fn run(self) -> Result<Done, HeapExhausted> {
        let resource = self.resource();
        let result = match self {
            Job::Lookup(host) => lookup(&host).await,
            Job::Connect { host, port } => connect(&host, port).await,
            Job::Bind { udp, host, port } => open(udp, &host, port).await,
            Job::Read {
                stream,
                handle,
                max,
                ledger,
            } => read(&stream, max, &ledger)
                .await?
                .map(|(bytes, held)| Got::Bytes {
                    handle,
                    bytes,
                    held,
                }),
            Job::Write {
                stream,
                bytes,
                held,
                ..
            } => {
                let written = write(&stream, &bytes).await;
                drop(held);
                written.map(|()| Got::None)
            }
            Job::Accept { listener, .. } => match listener.accept().await {
                Ok((socket, _)) => Ok(Got::Opened(Opened::Stream(Stream::new(socket)))),
                Err(error) => Err(socket_error(&error, "accept", None, "", 0)),
            },
            Job::Send {
                socket,
                host,
                port,
                bytes,
                held,
                ..
            } => {
                let sent = send_to(&socket, &host, port, &bytes).await;
                drop(held);
                sent
            }
            Job::Receive {
                socket,
                handle,
                max,
                ledger,
            } => receive(&socket, &ledger)
                .await?
                .map(|(bytes, peer, held)| Got::Datagram {
                    handle,
                    bytes,
                    max,
                    peer,
                    held,
                }),
        };
        Ok(Done { resource, result })
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
    Err(first_error.unwrap_or_else(|| NetError::new(2, host)))
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
        return Err(NetError::new(2, host));
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

/// At least one byte and at most `max`, or an empty list at the end, with
/// the heap the bytes hold. The buffer is reserved and made only once the
/// socket is readable, so a waiting read holds none.
pub(crate) async fn read(
    stream: &Stream,
    max: usize,
    ledger: &Arc<Ledger>,
) -> Result<Result<(Vec<u8>, Reservation), NetError>, HeapExhausted> {
    let mut bytes = Vec::new();
    let mut held = Reservation::empty(ledger);
    let at_end = |end| Ok(ended(end).map(|bytes| (bytes, Reservation::empty(ledger))));
    loop {
        if let Some(end) = stream.state().end.clone() {
            return at_end(end);
        }
        if let Err(error) = stream.socket.readable().await {
            return Ok(Err(stream_failed(stream, &error, "read")));
        }
        let size = max.min(CHUNK);
        held.grow(size.saturating_sub(bytes.len()))?;
        bytes.resize(size, 0);
        match stream.socket.try_read(&mut bytes) {
            Ok(0) => {
                let end = stream.state().end.get_or_insert(End::Eof).clone();
                return at_end(end);
            }
            Ok(n) => {
                bytes.truncate(n);
                held.shrink(size - n);
                return Ok(Ok((bytes, held)));
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Ok(Err(stream_failed(stream, &error, "read"))),
        }
    }
}

pub(crate) fn ended(end: End) -> Result<Vec<u8>, NetError> {
    match end {
        End::Eof => Ok(Vec::new()),
        End::Error(error) => Err(error),
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
        return Err(NetError::new(2, host));
    };
    match socket.socket.send_to(bytes, address).await {
        Ok(_) => Ok(Got::None),
        Err(error) => Err(socket_error(&error, "send", Some(address), host, port)),
    }
}

/// A received datagram: its bytes, its sender, and the heap it holds.
type Datagram = (Vec<u8>, SocketAddr, Reservation);

/// The next whole datagram and its sender. As `read`, the buffer is
/// reserved and made only once a datagram is there.
async fn receive(
    socket: &Udp,
    ledger: &Arc<Ledger>,
) -> Result<Result<Datagram, NetError>, HeapExhausted> {
    let mut bytes = Vec::new();
    let mut held = Reservation::empty(ledger);
    loop {
        if let Err(error) = socket.socket.readable().await {
            return Ok(Err(socket_error(&error, "recvmsg", None, "", 0)));
        }
        held.grow(CHUNK.saturating_sub(bytes.len()))?;
        bytes.resize(CHUNK, 0);
        match socket.socket.try_recv_from(&mut bytes) {
            Ok((n, peer)) => {
                bytes.truncate(n);
                held.shrink(CHUNK - n);
                return Ok(Ok((bytes, peer, held)));
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Ok(Err(socket_error(&error, "recvmsg", None, "", 0))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::connect_candidates;
    use crate::net::test_support::{reserved_refused_port, runtime};
    use std::net::TcpListener;

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
            .unwrap_or_else(|e| panic!("connection failed: {}", e.text));
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
        assert_eq!(error.variant, 3);
        assert_eq!(error.text, format!("localhost:{}", address.port()));
    }
}
