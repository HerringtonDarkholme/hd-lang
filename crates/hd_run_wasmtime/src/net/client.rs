//! The instance's side of `Net`: the socket handle table, and each
//! operation's `.start` and `.finish` on the program thread.

use hd_base::HashMap;
use std::net::{Shutdown, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use hd_run::Grants;

pub(crate) use crate::abi::NetError;
use crate::abi::{self, Datagram, ResourceError};
use crate::codec::{Dec, Enc, Handle, Wire, encode};
use crate::host::Stop;

use super::ops::{Done, Got, Job, Opened, ended};
use super::socket::{End, Stream, Udp, wrong_kind};
use super::validate::{at, covers_lookup, ip_literal, valid_host};

/// The sockets an instance opened, by handle.
#[derive(Default)]
pub struct Client {
    handles: HashMap<i64, Entry>,
    /// The timeout `set_timeout` gave each handle that has one
    /// (`std-net.timeout.set`).
    bounds: HashMap<i64, Duration>,
    next_handle: u64,
}

enum Entry {
    Stream(Arc<Stream>),
    Listener(Arc<tokio::net::TcpListener>),
    Udp(Arc<Udp>),
    /// Closed, so every later operation is `Disposed`. Its socket is gone.
    Closed,
}

/// What a `.start` does: complete at once, or wait for `Job`.
pub(crate) enum Start {
    Ready(Enc),
    /// `close` of socket `handle`, answered by `reply`: the caller then
    /// ends the operations pending on the handle (`ops::dispose`).
    Closed {
        reply: Enc,
        handle: i64,
    },
    Run(Job),
}

pub(crate) enum Value {
    None,
    Handle(u64),
    Addresses(Vec<String>),
    Bytes(Vec<u8>),
    Datagram(Vec<u8>, SocketAddr),
}

pub(crate) enum Outcome {
    Ok(Value),
    Error(NetError),
    Disposed,
    Unknown,
}

impl Client {
    /// Closes every socket: the instance has ended. A stream's both
    /// directions shut first, so its peer reads the end even while an
    /// aborted task still holds it.
    pub(crate) fn stop(&mut self) {
        for entry in self.handles.values() {
            if let Entry::Stream(stream) = entry {
                stream.shutdown(Shutdown::Both);
            }
        }
        self.handles.clear();
        self.bounds.clear();
    }

    /// The `.start` of `method`, or its `close`. Refusals, unknown and
    /// closed handles, and what the handle already holds complete at once;
    /// the rest wait as a `Job`.
    pub(crate) fn start(
        &mut self,
        method: &str,
        params: &[i64],
        bytes: &[u8],
        grants: &Grants,
    ) -> wasmtime::Result<Start> {
        let mut d = Dec::new(bytes);
        let scalar = |i: usize| {
            params
                .get(i)
                .copied()
                .ok_or_else(|| Stop::Contract(format!("argument {i} is not an integer")))
        };
        let resource = method.contains('.');
        let ready = |outcome| reply(outcome, resource).map(Start::Ready);
        let refused = |error| reply(Outcome::Error(error), resource).map(Start::Ready);
        // Every argument decodes before any check, as on Node.
        let mut bound = None;
        let (host, payload) = match method {
            "connect" | "connect_timeout" | "listen" | "bind_udp" => {
                (String::dec(&mut d)?, Vec::new())
            }
            "TcpStream.write" => (String::new(), Vec::<u8>::dec(&mut d)?),
            "UdpSocket.send_to" => (String::dec(&mut d)?, Vec::<u8>::dec(&mut d)?),
            "TcpStream.set_timeout" | "TcpListener.set_timeout" | "UdpSocket.set_timeout" => {
                bound = Option::<abi::Duration>::dec(&mut d)?.map(|t| millis(t.millis));
                (String::new(), Vec::new())
            }
            _ => (String::new(), Vec::new()),
        };
        // Closable Handles requires Disposed through every alias before any
        // destination validation or grant check.
        let entry = if resource {
            match self.handles.get(&scalar(0)?) {
                None => return ready(Outcome::Unknown),
                Some(Entry::Closed) => return ready(Outcome::Disposed),
                Some(entry) => Some(entry),
            }
        } else {
            None
        };
        let job = match (method, entry) {
            ("lookup", _) => {
                let host = String::from_utf8_lossy(bytes).into_owned();
                if !valid_host(&host) {
                    return refused(NetError::InvalidAddress { address: host });
                }
                if !covers_lookup(grants, &host) {
                    return refused(NetError::NotGranted { address: host });
                }
                // Node keeps a literal's spelling rather than formatting
                // the address through an IPv6 canonicalizer.
                if ip_literal(&host) {
                    return ready(Outcome::Ok(Value::Addresses(vec![host])));
                }
                Job::Lookup(host)
            }
            ("connect" | "connect_timeout" | "listen" | "bind_udp", _) => {
                let port = port(scalar(0)?)?;
                let timeout = if method == "connect_timeout" {
                    Some(millis(scalar(1)?))
                } else {
                    None
                };
                if let Some(error) = at(grants, &host, port) {
                    return refused(error);
                }
                // A zero bound ends the connect before it starts
                // (`std-net.timeout.negative`).
                if timeout.is_some_and(|t| t.is_zero()) {
                    return refused(NetError::Timeout);
                }
                if method.starts_with("connect") {
                    Job::Connect {
                        host,
                        port,
                        timeout,
                    }
                } else {
                    Job::Bind {
                        udp: method == "bind_udp",
                        host,
                        port,
                    }
                }
            }
            ("TcpStream.read", Some(Entry::Stream(stream))) => {
                let max = unsigned(scalar(1)?);
                if max == 0 {
                    return ready(Outcome::Ok(Value::Bytes(Vec::new())));
                }
                let mut state = stream.state();
                if !state.unread.is_empty() {
                    let n = state.unread.len().min(max);
                    let bytes = state.unread.drain(..n).collect();
                    return ready(Outcome::Ok(Value::Bytes(bytes)));
                }
                if let Some(end) = state.end.clone() {
                    return match ended(end) {
                        Ok(bytes) => ready(Outcome::Ok(Value::Bytes(bytes))),
                        Err(error) => refused(error),
                    };
                }
                Job::Read {
                    stream: Arc::clone(stream),
                    handle: scalar(0)?,
                    max,
                    bound: self.bound(scalar(0)?),
                }
            }
            ("TcpStream.write", Some(Entry::Stream(stream))) => {
                if matches!(stream.state().end, Some(End::Error(_))) {
                    return refused(NetError::Other {
                        message: "cannot write to a failed TCP stream".into(),
                    });
                }
                let bytes = payload;
                Job::Write {
                    stream: Arc::clone(stream),
                    handle: scalar(0)?,
                    bytes,
                }
            }
            ("TcpListener.accept", Some(Entry::Listener(listener))) => Job::Accept {
                listener: Arc::clone(listener),
                handle: scalar(0)?,
                bound: self.bound(scalar(0)?),
            },
            ("UdpSocket.send_to", Some(entry)) => {
                let bytes = payload;
                let port = port(scalar(1)?)?;
                // A live send always checks its own destination grant.
                if let Some(error) = at(grants, &host, port) {
                    return refused(error);
                }
                let Entry::Udp(socket) = entry else {
                    return refused(wrong_kind());
                };
                Job::Send {
                    socket: Arc::clone(socket),
                    handle: scalar(0)?,
                    host,
                    port,
                    bytes,
                }
            }
            ("UdpSocket.receive", Some(Entry::Udp(socket))) => {
                let max = unsigned(scalar(1)?);
                if let Some((mut bytes, peer)) = socket.unread().pop_front() {
                    bytes.truncate(max);
                    return ready(Outcome::Ok(Value::Datagram(bytes, peer)));
                }
                Job::Receive {
                    socket: Arc::clone(socket),
                    handle: scalar(0)?,
                    max,
                    bound: self.bound(scalar(0)?),
                }
            }
            (
                "TcpStream.set_timeout" | "TcpListener.set_timeout" | "UdpSocket.set_timeout",
                Some(_),
            ) => {
                let handle = scalar(0)?;
                match bound {
                    Some(bound) => self.bounds.insert(handle, bound),
                    None => self.bounds.remove(&handle),
                };
                return ready(Outcome::Ok(Value::None));
            }
            ("TcpStream.close" | "TcpListener.close" | "UdpSocket.close", Some(_)) => {
                let handle = scalar(0)?;
                self.close(handle);
                return Ok(Start::Closed {
                    reply: reply(Outcome::Ok(Value::None), true)?,
                    handle,
                });
            }
            (
                "TcpStream.read" | "TcpStream.write" | "TcpListener.accept" | "UdpSocket.receive",
                Some(_),
            ) => return refused(wrong_kind()),
            _ => return Err(Stop::Contract(format!("unknown Net method {method}")).into()),
        };
        Ok(Start::Run(job))
    }

    /// `close` shuts a stream's both directions and drops the handle's
    /// socket from the table; an operation still pending holds the last
    /// reference until the caller ends it, so the descriptor goes then
    /// (`std-net.close.release`).
    fn close(&mut self, handle: i64) {
        if let Some(Entry::Stream(stream)) = self.handles.get(&handle) {
            stream.shutdown(Shutdown::Both);
        }
        self.handles.insert(handle, Entry::Closed);
        self.bounds.remove(&handle);
    }

    /// The timeout of socket `handle`, if it has one.
    fn bound(&self, handle: i64) -> Option<Duration> {
        self.bounds.get(&handle).copied()
    }

    /// The `.finish` of a completed operation: a new socket gets its handle
    /// here, so handles follow the order the program takes results in.
    pub(crate) fn finish(&mut self, done: Done) -> wasmtime::Result<Enc> {
        let outcome = match done.result {
            Err(error) => Outcome::Error(error),
            Ok(Got::None) => Outcome::Ok(Value::None),
            Ok(Got::Disposed) => Outcome::Disposed,
            Ok(Got::Addresses(addresses)) => Outcome::Ok(Value::Addresses(addresses)),
            Ok(Got::Opened(opened)) => Outcome::Ok(Value::Handle(self.open(opened))),
            Ok(Got::Bytes { bytes, .. }) => Outcome::Ok(Value::Bytes(bytes)),
            Ok(Got::Datagram {
                mut bytes,
                max,
                peer,
                ..
            }) => {
                bytes.truncate(max);
                Outcome::Ok(Value::Datagram(bytes, peer))
            }
        };
        reply(outcome, done.resource)
    }

    /// Keeps what a completed `read!` or `receive!` took, when its
    /// suspension was cancelled before `.finish` returned it, for the next
    /// call on that handle (`std-net.cancel.read`).
    pub(crate) fn unread(&mut self, done: Done) {
        match done.result {
            Ok(Got::Bytes { handle, bytes }) => {
                if let Some(Entry::Stream(stream)) = self.handles.get(&handle) {
                    let mut state = stream.state();
                    for byte in bytes.into_iter().rev() {
                        state.unread.push_front(byte);
                    }
                }
            }
            Ok(Got::Datagram {
                handle,
                bytes,
                peer,
                ..
            }) => {
                if let Some(Entry::Udp(socket)) = self.handles.get(&handle) {
                    socket.unread().push_front((bytes, peer));
                }
            }
            _ => {}
        }
    }

    fn open(&mut self, opened: Opened) -> u64 {
        self.next_handle += 1;
        let entry = match opened {
            Opened::Stream(stream) => Entry::Stream(Arc::new(stream)),
            Opened::Listener(listener) => Entry::Listener(Arc::new(listener)),
            Opened::Udp(socket) => Entry::Udp(Arc::new(socket)),
        };
        self.handles
            .insert(i64::try_from(self.next_handle).unwrap_or(i64::MAX), entry);
        self.next_handle
    }
}

/// The result of an operation the runtime could not run.
pub(crate) fn not_run(error: &str, resource: bool) -> wasmtime::Result<Enc> {
    reply(
        Outcome::Error(NetError::Other {
            message: error.into(),
        }),
        resource,
    )
}

/// A `usize` count from the ABI's unsigned 32-bit value.
fn unsigned(value: i64) -> usize {
    u32::from_le_bytes(value.to_le_bytes()[..4].try_into().unwrap_or([0; 4])) as usize
}

/// A timeout of `value` milliseconds; a negative one is zero
/// (`std-net.timeout.negative`).
fn millis(value: i64) -> Duration {
    Duration::from_millis(u64::try_from(value).unwrap_or(0))
}

fn port(value: i64) -> wasmtime::Result<u16> {
    u16::try_from(value).map_err(|_| Stop::Contract("a Net port is outside u16".into()).into())
}

/// An operation's encoded result: `Result[T, NetError]` for a `Net`
/// method, `Result[T, ResourceError[NetError]]` for a handle's
/// (`resource`), where `T` is what its value holds.
fn reply(outcome: Outcome, resource: bool) -> wasmtime::Result<Enc> {
    fn ok<T: Wire>(value: T) -> Enc {
        encode(&Ok::<T, NetError>(value))
    }
    Ok(match outcome {
        Outcome::Unknown => {
            return Err(
                Stop::Contract("an operation on a handle the host never made".into()).into(),
            );
        }
        Outcome::Disposed => encode(&Err::<(), _>(ResourceError::<NetError>::Disposed)),
        Outcome::Error(error) if resource => {
            encode(&Err::<(), _>(ResourceError::Operation { error }))
        }
        Outcome::Error(error) => encode(&Err::<(), _>(error)),
        Outcome::Ok(Value::None) => ok(()),
        Outcome::Ok(Value::Handle(handle)) => ok(Handle(handle)),
        Outcome::Ok(Value::Addresses(addresses)) => ok(addresses),
        Outcome::Ok(Value::Bytes(bytes)) => ok(bytes),
        Outcome::Ok(Value::Datagram(bytes, peer)) => ok(Datagram {
            bytes,
            host: peer.ip().to_string(),
            port: peer.port(),
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::Entry;
    use crate::codec::{Dec, Enc};
    use crate::host::Stop;
    use crate::net::test_support::{Net, Started, host_args, result_bytes};
    use hd_run::{Grants, Limit};
    use socket2::Socket;
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpListener, TcpStream, UdpSocket};
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    fn pending(started: Started) -> i32 {
        match started {
            Started::Pending(h) => h,
            Started::Ready(e) => panic!("completed at once: {:?}", e.done()),
        }
    }

    fn ready(started: Started) -> Enc {
        match started {
            Started::Ready(e) => e,
            Started::Pending(h) => panic!("operation {h} waits"),
        }
    }

    fn read(net: &mut Net, handle: i64, max: i64) -> Started {
        net.start("TcpStream.read", &[handle, max], &[], &Grants::default())
            .unwrap()
    }

    #[test]
    fn refusals_complete_at_once_and_unknown_handles_precede_destination_checks() {
        let mut net = Net::new();
        let grants = Grants {
            limits: vec![("Net".into(), Limit::Deny)],
        };
        let mut expected = Enc::default();
        expected.leb(1).leb(0).str("127.0.0.1:9");
        let refused = net
            .start("connect", &[9, 0], host_args("127.0.0.1").done(), &grants)
            .unwrap();
        assert_eq!(ready(refused).done(), expected.done());
        for (mut bytes, message) in [
            (
                host_args("127.0.0.1"),
                "an operation on a handle the host never made",
            ),
            (
                host_args("bad host"),
                "an operation on a handle the host never made",
            ),
            // Every argument decodes before the handle is looked up.
            (Enc::default(), "an argument does not decode"),
        ] {
            if !bytes.done().is_empty() {
                bytes.bytes(b"payload");
            }
            let error = net
                .start("UdpSocket.send_to", &[123, 9, 0], bytes.done(), &grants)
                .err()
                .unwrap();
            assert_eq!(error.downcast_ref::<Stop>().unwrap().to_string(), message);
        }
        let error = net
            .start("TcpStream.close", &[123], &[], &Grants::default())
            .err()
            .unwrap();
        assert_eq!(
            error.downcast_ref::<Stop>().unwrap().to_string(),
            "an operation on a handle the host never made"
        );
        assert!(!net.reactor.waiting(), "nothing waits");
    }

    #[test]
    fn tcp_partial_reads_zero_read_eof_and_disposal_are_sticky() {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let writer = thread::spawn(move || {
            let (mut socket, _) = server.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            socket.write_all(b"abcdef").unwrap();
            socket.shutdown(Shutdown::Write).unwrap();
            let mut ack = [0; 1];
            socket.read_exact(&mut ack).unwrap();
            assert_eq!(ack, *b"x");
        });
        let mut net = Net::new();
        let handle = net.opened("connect", port);
        assert_eq!(net.read_n(handle, 2, 2), b"ab");
        assert!(result_bytes(&ready(read(&mut net, handle, 0))).is_empty());
        assert_eq!(net.read_n(handle, 4, i64::from(i32::MIN)), b"cdef");
        for _ in 0..2 {
            let eof = net
                .call("TcpStream.read", &[handle, 5], &[], &Grants::default())
                .unwrap();
            assert!(result_bytes(&eof).is_empty());
        }
        let actual = net
            .call(
                "TcpStream.write",
                &[handle, 0],
                Enc::default().bytes(b"x").done(),
                &Grants::default(),
            )
            .unwrap();
        assert_eq!(
            actual.done(),
            &[0],
            "read EOF does not close the write half"
        );
        assert_eq!(
            net.call("TcpStream.close", &[handle], &[], &Grants::default())
                .unwrap()
                .done(),
            &[0]
        );
        for (method, params) in [
            ("TcpStream.close", vec![handle]),
            ("TcpStream.read", vec![handle, 1]),
        ] {
            assert_eq!(
                net.call(method, &params, &[], &Grants::default())
                    .unwrap()
                    .done(),
                &[1, 1]
            );
        }
        writer.join().unwrap();
        net.client.stop();
    }

    #[test]
    fn udp_receives_one_datagram_and_zero_max_still_consumes_one() {
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = peer.local_addr().unwrap().port();
        peer.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let server = thread::spawn(move || {
            let mut bytes = [0; 16];
            let (n, from) = peer.recv_from(&mut bytes).unwrap();
            assert_eq!(&bytes[..n], b"ping");
            peer.send_to(b"hello", from).unwrap();
            peer.send_to(b"", from).unwrap();
            peer.send_to(b"wide", from).unwrap();
        });
        let mut net = Net::new();
        let handle = net.opened("bind_udp", 0);
        let mut args = host_args("127.0.0.1");
        args.bytes(b"ping");
        assert_eq!(
            net.call(
                "UdpSocket.send_to",
                &[handle, i64::from(port), 0],
                args.done(),
                &Grants::default()
            )
            .unwrap()
            .done(),
            &[0]
        );
        for (max, bytes) in [
            (2, &b"he"[..]),
            (0, &b""[..]),
            (i64::from(i32::MIN), &b"wide"[..]),
        ] {
            let actual = net
                .call("UdpSocket.receive", &[handle, max], &[], &Grants::default())
                .unwrap();
            let mut d = Dec::new(actual.done());
            assert_eq!(d.leb().unwrap(), 0);
            assert_eq!(d.bytes().unwrap(), bytes);
            assert_eq!(d.str().unwrap(), "127.0.0.1");
            assert_eq!(d.leb().unwrap(), u64::from(port));
        }
        server.join().unwrap();
        net.client.stop();
    }

    /// `std-net.cancel.read`: a read cancelled while it waits takes nothing,
    /// and one cancelled after it completed, before its `.finish`, gives
    /// its bytes back. Either way the next reads return the first bytes.
    #[test]
    fn a_cancelled_read_leaves_its_bytes_for_the_next_read() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut net = Net::new();
        let handle = net.opened("connect", listener.local_addr().unwrap().port());
        let (mut peer, _) = listener.accept().unwrap();
        let waiting = pending(read(&mut net, handle, 8));
        net.abort(waiting);
        peer.write_all(b"abc").unwrap();
        assert_eq!(net.read_n(handle, 3, 8), b"abc");
        let completed = pending(read(&mut net, handle, 8));
        peer.write_all(b"xyz").unwrap();
        net.wait_for(completed);
        net.abort(completed);
        assert_eq!(result_bytes(&ready(read(&mut net, handle, 2))), b"xy");
        assert_eq!(result_bytes(&ready(read(&mut net, handle, 8))), b"z");
        assert!(!net.reactor.waiting());
        net.client.stop();
    }

    /// As a read, a receive cancelled after it completed gives its whole
    /// datagram back for the next receive.
    #[test]
    fn a_cancelled_receive_leaves_its_datagram_for_the_next_receive() {
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut net = Net::new();
        let handle = net.opened("bind_udp", 0);
        let mut args = host_args("127.0.0.1");
        args.bytes(b"hi");
        net.call(
            "UdpSocket.send_to",
            &[handle, i64::from(peer.local_addr().unwrap().port()), 0],
            args.done(),
            &Grants::default(),
        )
        .unwrap();
        let mut bytes = [0; 8];
        let (_, address) = peer.recv_from(&mut bytes).unwrap();
        let receive = |net: &mut Net, max: i64| {
            net.start("UdpSocket.receive", &[handle, max], &[], &Grants::default())
                .unwrap()
        };
        let waiting = pending(receive(&mut net, 8));
        net.abort(waiting);
        let completed = pending(receive(&mut net, 1));
        peer.send_to(b"datagram", address).unwrap();
        net.wait_for(completed);
        net.abort(completed);
        let kept = ready(receive(&mut net, 8));
        let mut d = Dec::new(kept.done());
        assert_eq!(d.leb().unwrap(), 0);
        assert_eq!(d.bytes().unwrap(), b"datagram");
        net.client.stop();
    }

    /// `std-host.op.wait`: an accept and a connect to it wait together, so
    /// both complete.
    #[test]
    fn an_accept_and_a_connect_to_it_complete_together() {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut net = Net::new();
        let listener = net.opened("listen", port);
        let accept = pending(
            net.start("TcpListener.accept", &[listener], &[], &Grants::default())
                .unwrap(),
        );
        let connect = pending(
            net.start(
                "connect",
                &[i64::from(port), 0],
                host_args("127.0.0.1").done(),
                &Grants::default(),
            )
            .unwrap(),
        );
        net.wait_for(accept);
        net.wait_for(connect);
        assert_eq!(net.finish(accept).done(), &[0, 2]);
        assert_eq!(net.finish(connect).done(), &[0, 3]);
        let mut args = Enc::default();
        args.bytes(b"ok");
        net.call("TcpStream.write", &[3, 0], args.done(), &Grants::default())
            .unwrap();
        assert_eq!(net.read_n(2, 2, 8), b"ok");
        net.client.stop();
    }

    /// An abort ends a waiting accept, receive or backpressured write at
    /// once, and the socket goes on working.
    #[test]
    fn an_abort_ends_a_waiting_accept_receive_or_write_at_once() {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut net = Net::new();
        let listener = net.opened("listen", port);
        let udp = net.opened("bind_udp", 0);
        let accept = pending(
            net.start("TcpListener.accept", &[listener], &[], &Grants::default())
                .unwrap(),
        );
        let receive = pending(
            net.start("UdpSocket.receive", &[udp, 8], &[], &Grants::default())
                .unwrap(),
        );
        let start = Instant::now();
        net.abort(accept);
        net.abort(receive);
        assert!(start.elapsed() < Duration::from_secs(1));
        // The listener still accepts: the connection waited in the kernel.
        let client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        assert_eq!(
            net.call("TcpListener.accept", &[listener], &[], &Grants::default())
                .unwrap()
                .done(),
            &[0, 3]
        );
        let stream = net.opened("connect", server.local_addr().unwrap().port());
        let (peer, _) = server.accept().unwrap();
        socket2::SockRef::from(&peer)
            .set_recv_buffer_size(4096)
            .unwrap();
        let mut payload = Enc::default();
        payload.bytes(&vec![b'x'; 16 * 1024 * 1024]);
        let write = pending(
            net.start(
                "TcpStream.write",
                &[stream, 0],
                payload.done(),
                &Grants::default(),
            )
            .unwrap(),
        );
        let start = Instant::now();
        net.abort(write);
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(!net.reactor.waiting());
        drop(client);
        net.client.stop();
    }

    /// The instance's end closes a stream whose read still waits, so the
    /// peer reads the end at once.
    #[test]
    fn stop_closes_a_stream_with_a_waiting_read() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut net = Net::new();
        let handle = net.opened("connect", listener.local_addr().unwrap().port());
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let waiting = pending(read(&mut net, handle, 1));
        net.client.stop();
        assert_eq!(peer.read(&mut [0; 1]).unwrap(), 0);
        net.abort(waiting);
    }

    fn disposed() -> &'static [u8] {
        &[1, 1]
    }

    /// `std-net.close.release` and `std-net.close.pending`: `close` ends a
    /// waiting read with `Disposed`, drops the socket at once, and the peer
    /// reads the end at once.
    #[test]
    fn close_ends_a_waiting_read_and_releases_the_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut net = Net::new();
        let handle = net.opened("connect", listener.local_addr().unwrap().port());
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let socket = match net.client.handles.get(&handle) {
            Some(Entry::Stream(stream)) => Arc::downgrade(stream),
            _ => panic!("a stream handle"),
        };
        let waiting = pending(read(&mut net, handle, 1));
        let close = net
            .start("TcpStream.close", &[handle], &[], &Grants::default())
            .unwrap();
        assert_eq!(ready(close).done(), &[0]);
        assert!(socket.upgrade().is_none(), "the socket is dropped");
        assert_eq!(peer.read(&mut [0; 1]).unwrap(), 0);
        net.wait_for(waiting);
        assert_eq!(net.finish(waiting).done(), disposed());
        assert!(!net.reactor.waiting());
        for method in ["TcpStream.read", "TcpStream.close"] {
            assert_eq!(
                net.call(method, &[handle, 1], &[], &Grants::default())
                    .unwrap()
                    .done(),
                disposed()
            );
        }
    }

    /// A read and a write both pending, and a read that completed before
    /// its `.finish`, all end with `Disposed`.
    #[test]
    fn close_disposes_every_operation_not_yet_finished() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut net = Net::new();
        let handle = net.opened("connect", listener.local_addr().unwrap().port());
        let (mut peer, _) = listener.accept().unwrap();
        let completed = pending(read(&mut net, handle, 8));
        peer.write_all(b"abc").unwrap();
        net.wait_for(completed);
        let waiting = pending(read(&mut net, handle, 8));
        net.call("TcpStream.close", &[handle], &[], &Grants::default())
            .unwrap();
        for op in [completed, waiting] {
            net.wait_for(op);
            assert_eq!(net.finish(op).done(), disposed());
        }
        assert!(!net.reactor.waiting());
    }

    /// A closed listener ends its waiting accept with `Disposed` and frees
    /// its port, so a new `listen!` on it succeeds.
    #[test]
    fn close_ends_a_waiting_accept_and_frees_the_port() {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut net = Net::new();
        let listener = net.opened("listen", port);
        let accept = pending(
            net.start("TcpListener.accept", &[listener], &[], &Grants::default())
                .unwrap(),
        );
        net.call("TcpListener.close", &[listener], &[], &Grants::default())
            .unwrap();
        net.wait_for(accept);
        assert_eq!(net.finish(accept).done(), disposed());
        assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
        let again = net.opened("listen", port);
        assert_ne!(again, listener);
        net.client.stop();
    }

    #[test]
    fn close_ends_a_waiting_receive_and_frees_the_port() {
        let mut net = Net::new();
        let socket = net.opened("bind_udp", 0);
        let port = match net.client.handles.get(&socket) {
            Some(Entry::Udp(udp)) => udp.socket.local_addr().unwrap().port(),
            _ => panic!("a udp handle"),
        };
        let receive = pending(
            net.start("UdpSocket.receive", &[socket, 8], &[], &Grants::default())
                .unwrap(),
        );
        net.call("UdpSocket.close", &[socket], &[], &Grants::default())
            .unwrap();
        net.wait_for(receive);
        assert_eq!(net.finish(receive).done(), disposed());
        UdpSocket::bind(("127.0.0.1", port)).unwrap();
    }

    #[test]
    fn closed_udp_send_is_disposed_before_denied_or_invalid_destination() {
        let mut net = Net::new();
        let handle = net.opened("bind_udp", 0);
        assert_eq!(
            net.call("UdpSocket.close", &[handle], &[], &Grants::default())
                .unwrap()
                .done(),
            &[0]
        );
        let denied = Grants {
            limits: vec![("Net".into(), Limit::Deny)],
        };
        for host in ["127.0.0.1", "invalid/host"] {
            let mut args = host_args(host);
            args.bytes(b"x");
            assert_eq!(
                net.call("UdpSocket.send_to", &[handle, 9, 0], args.done(), &denied)
                    .unwrap()
                    .done(),
                &[1, 1]
            );
        }
    }

    #[test]
    fn tcp_reset_is_sticky_after_queued_bytes_are_delivered() {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let (ack, ready_to_reset) = mpsc::channel();
        let peer = thread::spawn(move || {
            let (mut stream, _) = server.accept().unwrap();
            stream.write_all(b"abc").unwrap();
            ready_to_reset
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            let socket = Socket::from(stream);
            socket.set_linger(Some(Duration::ZERO)).unwrap();
        });
        let mut net = Net::new();
        let handle = net.opened("connect", port);
        assert_eq!(net.read_n(handle, 3, 3), b"abc");
        ack.send(()).unwrap();
        peer.join().unwrap();
        let first = net
            .call("TcpStream.read", &[handle, 3], &[], &Grants::default())
            .unwrap();
        let mut decoded = Dec::new(first.done());
        assert_eq!(decoded.leb().unwrap(), 1);
        assert_eq!(decoded.leb().unwrap(), 0);
        assert_eq!(decoded.leb().unwrap(), 5);
        assert!(decoded.str().unwrap().starts_with("read: "));
        let again = net
            .call("TcpStream.read", &[handle, 3], &[], &Grants::default())
            .unwrap();
        assert_eq!(
            again.done(),
            first.done(),
            "the native read error remains sticky"
        );
        let mut args = Enc::default();
        args.bytes(b"x");
        let mut expected = Enc::default();
        expected
            .leb(1)
            .leb(0)
            .leb(5)
            .str("cannot write to a failed TCP stream");
        assert_eq!(
            net.call(
                "TcpStream.write",
                &[handle, 0],
                args.done(),
                &Grants::default()
            )
            .unwrap()
            .done(),
            expected.done()
        );
        net.client.stop();
    }
}
