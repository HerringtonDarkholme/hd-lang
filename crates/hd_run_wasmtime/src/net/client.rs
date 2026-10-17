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
use super::socket::{self, End, Stream, Udp, wrong_kind};
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
    Address(SocketAddr),
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
            "TcpStream.set_timeout"
            | "TcpListener.set_timeout"
            | "UdpSocket.set_timeout"
            | "TcpStream.set_keepalive" => {
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
            ("TcpStream.set_nodelay", Some(Entry::Stream(stream))) => {
                return match socket::set_nodelay(stream, scalar(1)? != 0) {
                    Ok(()) => ready(Outcome::Ok(Value::None)),
                    Err(error) => refused(error),
                };
            }
            ("TcpStream.set_keepalive", Some(Entry::Stream(stream))) => {
                return match socket::set_keepalive(stream, bound) {
                    Ok(()) => ready(Outcome::Ok(Value::None)),
                    Err(error) => refused(error),
                };
            }
            ("TcpListener.local_address", Some(Entry::Listener(listener))) => {
                return match listener.local_addr() {
                    Ok(address) => ready(Outcome::Ok(Value::Address(address))),
                    Err(error) => refused(socket::local_address_error(&error)),
                };
            }
            ("UdpSocket.local_address", Some(Entry::Udp(socket))) => {
                return match socket.socket.local_addr() {
                    Ok(address) => ready(Outcome::Ok(Value::Address(address))),
                    Err(error) => refused(socket::local_address_error(&error)),
                };
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
                "TcpStream.read"
                | "TcpStream.write"
                | "TcpStream.set_nodelay"
                | "TcpStream.set_keepalive"
                | "TcpListener.accept"
                | "TcpListener.local_address"
                | "UdpSocket.receive"
                | "UdpSocket.local_address",
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
        Outcome::Ok(Value::Address(address)) => ok((address.ip().to_string(), address.port())),
        Outcome::Ok(Value::Datagram(bytes, peer)) => ok(Datagram {
            bytes,
            host: peer.ip().to_string(),
            port: peer.port(),
        }),
    })
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
