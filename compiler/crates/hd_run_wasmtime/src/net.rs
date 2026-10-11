//! Per-instance synchronous Net client. One worker owns every socket,
//! buffers arrivals and preserves the first end/error, as net.mjs does.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{self, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use hd_run::{Grants, Limit};
use socket2::{Domain, Protocol, Socket, Type};

use crate::codec::{Dec, Enc};
use crate::host::Stop;

pub const METHODS: &[&str] = &[
    "lookup",
    "connect",
    "listen",
    "bind_udp",
    "TcpStream.read",
    "TcpStream.write",
    "TcpListener.accept",
    "UdpSocket.send_to",
    "UdpSocket.receive",
];
pub const CLOSE: &[&str] = &["TcpStream.close", "TcpListener.close", "UdpSocket.close"];
const TICK: Duration = Duration::from_millis(5);
const CHUNK: usize = 65536;
const CONNECT_ATTEMPT_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Default)]
pub struct Client {
    worker: Option<Worker>,
    known: HashSet<i64>,
    closed: HashSet<i64>,
}

struct Worker {
    requests: Sender<Request>,
    stopped: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

struct Request {
    op: Op,
    reply: Sender<Outcome>,
}

enum Op {
    Lookup(String),
    Open {
        method: &'static str,
        host: String,
        port: u16,
    },
    Read {
        handle: i64,
        max: u32,
    },
    Write {
        handle: i64,
        bytes: Vec<u8>,
    },
    Accept(i64),
    Send {
        handle: i64,
        host: String,
        port: u16,
        bytes: Vec<u8>,
    },
    Receive {
        handle: i64,
        max: u32,
    },
    Close(i64),
}

enum Value {
    None,
    Handle(u64),
    Addresses(Vec<String>),
    Bytes(Vec<u8>),
    Datagram(Vec<u8>, SocketAddr),
}

enum Outcome {
    Ok(Value),
    Error(NetError),
    Disposed,
    Unknown,
}

#[derive(Clone)]
struct NetError {
    variant: u64,
    text: String,
}

impl NetError {
    fn new(variant: u64, text: impl Into<String>) -> Self {
        Self {
            variant,
            text: text.into(),
        }
    }
}

impl Client {
    /// Stops and joins the per-instance socket owner, releasing every socket.
    pub fn stop(&mut self) {
        self.worker.take();
        self.known.clear();
        self.closed.clear();
    }

    pub fn call(
        &mut self,
        method: &str,
        params: &[i64],
        bytes: &[u8],
        grants: &Grants,
    ) -> wasmtime::Result<Enc> {
        let mut d = Dec::new(bytes);
        let scalar = |i: usize| {
            params
                .get(i)
                .copied()
                .ok_or_else(|| Stop::Contract(format!("argument {i} is not an integer")))
        };
        let resource = method.contains('.');
        if resource && !self.known.contains(&scalar(0)?) {
            return encode(Outcome::Unknown, true);
        }
        // Closable Handles requires Disposed through every alias before any
        // destination validation or grant check. Node currently checks a UDP
        // send's destination first; this is a documented spec correction.
        if resource && self.closed.contains(&scalar(0)?) {
            return encode(Outcome::Disposed, true);
        }
        let op = match method {
            "lookup" => {
                let host = String::from_utf8_lossy(bytes).into_owned();
                if !valid_host(&host) {
                    return encode(Outcome::Error(NetError::new(1, host)), false);
                }
                if !covers_lookup(grants, &host) {
                    return encode(Outcome::Error(NetError::new(0, host)), false);
                }
                Op::Lookup(host)
            }
            "connect" | "listen" | "bind_udp" => {
                let host = d.str()?;
                let port = port(scalar(0)?)?;
                if let Some(error) = at(grants, &host, port) {
                    return encode(Outcome::Error(error), false);
                }
                let method = match method {
                    "connect" => "connect",
                    "listen" => "listen",
                    _ => "bind_udp",
                };
                Op::Open { method, host, port }
            }
            "TcpStream.read" => Op::Read {
                handle: scalar(0)?,
                max: unsigned(scalar(1)?),
            },
            "TcpStream.write" => Op::Write {
                handle: scalar(0)?,
                bytes: d.bytes()?.to_vec(),
            },
            "TcpListener.accept" => Op::Accept(scalar(0)?),
            "UdpSocket.send_to" => {
                let host = d.str()?;
                let bytes = d.bytes()?.to_vec();
                let port = port(scalar(1)?)?;
                // Node checks the destination before checking the handle.
                if let Some(error) = at(grants, &host, port) {
                    return encode(Outcome::Error(error), true);
                }
                Op::Send {
                    handle: scalar(0)?,
                    host,
                    port,
                    bytes,
                }
            }
            "UdpSocket.receive" => Op::Receive {
                handle: scalar(0)?,
                max: unsigned(scalar(1)?),
            },
            "TcpStream.close" | "TcpListener.close" | "UdpSocket.close" => Op::Close(scalar(0)?),
            _ => return Err(Stop::Contract(format!("unknown Net method {method}")).into()),
        };
        let outcome = self.request(op)?;
        if let Outcome::Ok(Value::Handle(handle)) = &outcome {
            self.known
                .insert(i64::try_from(*handle).unwrap_or(i64::MAX));
        }
        if CLOSE.contains(&method) && matches!(outcome, Outcome::Ok(Value::None)) {
            self.closed.insert(scalar(0)?);
        }
        encode(outcome, resource)
    }

    fn request(&mut self, op: Op) -> wasmtime::Result<Outcome> {
        if self.worker.is_none() {
            self.worker = Some(Worker::new()?);
        }
        let worker = self.worker.as_ref().expect("initialized worker");
        let (reply, result) = mpsc::channel();
        worker
            .requests
            .send(Request { op, reply })
            .map_err(|_| Stop::Internal("the Net worker stopped".into()))?;
        result
            .recv()
            .map_err(|_| Stop::Internal("the Net worker stopped".into()).into())
    }
}

impl Worker {
    fn new() -> wasmtime::Result<Self> {
        let (requests, input) = mpsc::channel();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&stopped);
        let thread = thread::Builder::new()
            .name("hd-net".into())
            .spawn(move || State::default().serve(&input, &stop))
            .map_err(|error| Stop::Internal(format!("could not start the Net worker: {error}")))?;
        Ok(Self {
            requests,
            stopped,
            thread: Some(thread),
        })
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn unsigned(value: i64) -> u32 {
    u32::from_le_bytes(value.to_le_bytes()[..4].try_into().unwrap_or([0; 4]))
}

fn port(value: i64) -> wasmtime::Result<u16> {
    u16::try_from(value).map_err(|_| Stop::Contract("a Net port is outside u16".into()).into())
}

fn encode(outcome: Outcome, resource: bool) -> wasmtime::Result<Enc> {
    let mut e = Enc::default();
    match outcome {
        Outcome::Unknown => {
            return Err(
                Stop::Contract("an operation on a handle the host never made".into()).into(),
            );
        }
        Outcome::Disposed => {
            e.leb(1).leb(1);
        }
        Outcome::Error(error) => {
            e.leb(1);
            if resource {
                e.leb(0);
            }
            e.leb(error.variant).str(&error.text);
        }
        Outcome::Ok(value) => {
            e.leb(0);
            match value {
                Value::None => {}
                Value::Handle(handle) => {
                    e.leb(handle);
                }
                Value::Addresses(addresses) => {
                    e.list(&addresses, |e, address| {
                        e.str(address);
                    });
                }
                Value::Bytes(bytes) => {
                    e.bytes(&bytes);
                }
                Value::Datagram(bytes, peer) => {
                    e.bytes(&bytes)
                        .str(&peer.ip().to_string())
                        .leb(u64::from(peer.port()));
                }
            }
        }
    }
    Ok(e)
}

fn at(grants: &Grants, host: &str, port: u16) -> Option<NetError> {
    let address = format!("{host}:{port}");
    if !valid_host(host) {
        Some(NetError::new(1, address))
    } else if !grants.covers_host("Net", host, Some(port)) {
        Some(NetError::new(0, address))
    } else {
        None
    }
}

fn valid_host(host: &str) -> bool {
    if ip_literal(host) {
        return true;
    }
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    let host = host.strip_suffix('.').unwrap_or(host);
    host.split('.').all(|label| {
        !label.is_empty()
            && label.as_bytes()[0].is_ascii_alphanumeric()
            && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

fn ip_literal(host: &str) -> bool {
    if host.parse::<IpAddr>().is_ok() {
        return true;
    }
    if let Some((ip, zone)) = host.split_once('%')
        && ip.parse::<std::net::Ipv6Addr>().is_ok()
        && !zone.is_empty()
        && zone
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-.:".contains(&b))
    {
        return true;
    }
    false
}

fn covers_lookup(grants: &Grants, host: &str) -> bool {
    match grants.limit("Net") {
        None => true,
        Some(Limit::Deny) => false,
        Some(Limit::Entries(entries)) => entries.iter().any(|entry| {
            let name = if let Some(rest) = entry.strip_prefix('[') {
                rest.split_once(']')
                    .map_or(entry.as_str(), |(host, _)| host)
            } else if entry.bytes().filter(|b| *b == b':').count() == 1 {
                entry
                    .split_once(':')
                    .map_or(entry.as_str(), |(host, _)| host)
            } else {
                entry.as_str()
            };
            if let Some(suffix) = name.strip_prefix("*.") {
                host.strip_suffix(suffix)
                    .is_some_and(|head| head.ends_with('.') && head.len() > 1)
            } else {
                name.eq_ignore_ascii_case(host)
            }
        }),
    }
}

#[derive(Default)]
struct State {
    handles: HashMap<i64, Handle>,
    next_handle: u64,
}

struct Handle {
    closed: bool,
    socket: SocketKind,
}

enum End {
    Eof,
    Error(NetError),
}

enum SocketKind {
    Stream {
        socket: TcpStream,
        inbox: VecDeque<Vec<u8>>,
        rest: VecDeque<u8>,
        end: Option<End>,
    },
    Listener {
        socket: Option<TcpListener>,
        inbox: VecDeque<TcpStream>,
        end: Option<NetError>,
    },
    Udp {
        socket: Option<UdpSocket>,
        inbox: VecDeque<(Vec<u8>, SocketAddr)>,
        end: Option<NetError>,
        ipv6: bool,
    },
}

impl State {
    fn serve(&mut self, requests: &Receiver<Request>, stop: &AtomicBool) {
        while !stop.load(Ordering::Relaxed) {
            self.arrivals();
            match requests.recv_timeout(TICK) {
                Ok(request) => {
                    let outcome = self.run(request.op, stop);
                    let _ = request.reply.send(outcome);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        // No reader/accept/receive thread remains blocked on a duplicate
        // descriptor: all sockets belong to this worker and are nonblocking.
        for handle in self.handles.values() {
            if let SocketKind::Stream { socket, .. } = &handle.socket {
                let _ = socket.shutdown(Shutdown::Both);
            }
        }
        self.handles.clear();
    }

    fn open(&mut self, socket: SocketKind) -> Outcome {
        self.next_handle += 1;
        let handle = self.next_handle;
        self.handles.insert(
            i64::try_from(handle).unwrap_or(i64::MAX),
            Handle {
                closed: false,
                socket,
            },
        );
        Outcome::Ok(Value::Handle(handle))
    }

    fn run(&mut self, op: Op, stop: &AtomicBool) -> Outcome {
        match op {
            Op::Lookup(host) => {
                // Node preserves a literal's spelling rather than formatting
                // the address through an IPv6 canonicalizer.
                if ip_literal(&host) {
                    return Outcome::Ok(Value::Addresses(vec![host]));
                }
                match resolve(&host, 0) {
                    Ok(addresses) => Outcome::Ok(Value::Addresses(
                        addresses.iter().map(|a| a.ip().to_string()).collect(),
                    )),
                    Err(error) => Outcome::Error(error),
                }
            }
            Op::Open { method, host, port } => self.open_at(method, &host, port, stop),
            op => self.handle_op(&op, stop),
        }
    }

    fn open_at(&mut self, method: &str, host: &str, port: u16, stop: &AtomicBool) -> Outcome {
        let addresses = match resolve(host, port) {
            Ok(a) => a,
            Err(e) => return Outcome::Error(e),
        };
        if method == "connect" {
            return match self.connect_candidates(addresses, host, port, stop) {
                Ok(socket) => self.open(stream(socket)),
                Err(error) => Outcome::Error(error),
            };
        }
        let ipv6 = host
            .split('%')
            .next()
            .is_some_and(|ip| ip.parse::<std::net::Ipv6Addr>().is_ok());
        let Some(address) = addresses
            .into_iter()
            .find(|a| method != "bind_udp" || a.is_ipv6() == ipv6)
        else {
            return Outcome::Error(NetError::new(2, host));
        };
        let result = bind(method, address);
        match result {
            Ok(socket) => self.open(socket),
            Err(error) => Outcome::Error(socket_error(
                &error,
                if method == "listen" { "listen" } else { "bind" },
                Some(address),
                host,
                port,
            )),
        }
    }

    fn connect_candidates(
        &mut self,
        addresses: Vec<SocketAddr>,
        host: &str,
        port: u16,
        stop: &AtomicBool,
    ) -> Result<TcpStream, NetError> {
        let attempts = connect_attempts(addresses);
        let aggregate = attempts.len() > 1;
        let mut first_error = None;
        for attempt in attempts {
            match self.connect(attempt, stop) {
                Ok(socket) => return Ok(socket),
                Err(error) => {
                    first_error.get_or_insert_with(|| {
                        socket_error(&error, "connect", Some(attempt.address), host, port)
                    });
                }
            }
            if stop.load(Ordering::Relaxed) {
                break;
            }
        }
        let mut error = first_error.unwrap_or_else(|| NetError::new(2, host));
        // NodeAggregateError adopts the first failure's code but has an empty
        // message. Refused still names the original destination through Net's
        // error mapping; Other exposes that empty aggregate message.
        if aggregate && error.variant == 4 {
            error.text.clear();
        }
        Err(error)
    }

    fn connect(&mut self, attempt: ConnectAttempt, stop: &AtomicBool) -> io::Result<TcpStream> {
        let address = attempt.address;
        let socket = Socket::new(
            Domain::for_address(address),
            Type::STREAM,
            Some(Protocol::TCP),
        )?;
        socket.set_nonblocking(true)?;
        match socket.connect(&address.into()) {
            Ok(()) => return Ok(socket.into()),
            Err(error) if connecting(&error) => {}
            Err(error) => return Err(error),
        }
        let started = Instant::now();
        loop {
            if stop.load(Ordering::Relaxed) {
                return Err(io::ErrorKind::Interrupted.into());
            }
            if let Some(error) = socket.take_error()? {
                return Err(error);
            }
            if socket.peer_addr().is_ok() {
                return Ok(socket.into());
            }
            if attempt.expired(started.elapsed()) {
                // Dropping this candidate closes it before the next family is
                // attempted, exactly as Node's automatic-family timer does.
                return Err(io::ErrorKind::TimedOut.into());
            }
            self.arrivals();
            thread::sleep(TICK);
        }
    }

    fn handle_op(&mut self, op: &Op, stop: &AtomicBool) -> Outcome {
        let number = match op {
            Op::Read { handle, .. }
            | Op::Write { handle, .. }
            | Op::Send { handle, .. }
            | Op::Receive { handle, .. } => *handle,
            Op::Accept(handle) | Op::Close(handle) => *handle,
            _ => unreachable!("a handle operation"),
        };
        let Some(handle) = self.handles.get(&number) else {
            return Outcome::Unknown;
        };
        if handle.closed {
            return Outcome::Disposed;
        }
        if let Op::Close(_) = op {
            let handle = self.handles.get_mut(&number).expect("existing handle");
            match &mut handle.socket {
                SocketKind::Stream { socket, .. } => {
                    let _ = socket.shutdown(Shutdown::Write);
                }
                SocketKind::Listener { socket, .. } => {
                    socket.take();
                }
                SocketKind::Udp { socket, .. } => {
                    socket.take();
                }
            }
            handle.closed = true;
            return Outcome::Ok(Value::None);
        }
        if let Op::Send { host, port, .. } = op {
            if *port == 0 {
                return Outcome::Error(NetError::new(
                    4,
                    "Port should be > 0 and < 65536. Received type number (0).",
                ));
            }
            let addresses = match resolve(host, *port) {
                Ok(a) => a,
                Err(e) => return Outcome::Error(e),
            };
            let SocketKind::Udp {
                socket: Some(_),
                ipv6,
                ..
            } = &handle.socket
            else {
                return wrong_kind();
            };
            let address = if ip_literal(host) {
                addresses.into_iter().next()
            } else {
                addresses.into_iter().find(|a| a.is_ipv6() == *ipv6)
            };
            let Some(address) = address else {
                return Outcome::Error(NetError::new(2, host));
            };
            // Node's udp4/udp6 bindings parse the destination with the
            // socket's family, rejecting a literal of the other family
            // before send. std may instead expose a platform-specific
            // routing error; keep the provider's EINVAL spelling.
            if address.is_ipv6() != *ipv6 {
                return Outcome::Error(NetError::new(4, format!("send EINVAL {host}:{port}")));
            }
            let Op::Send { bytes, .. } = op else {
                unreachable!()
            };
            loop {
                if stop.load(Ordering::Relaxed) {
                    return Outcome::Error(NetError::new(4, "the socket stopped"));
                }
                let SocketKind::Udp {
                    socket: Some(socket),
                    ..
                } = &self.handles.get(&number).expect("existing handle").socket
                else {
                    unreachable!()
                };
                match socket.send_to(bytes, address) {
                    Ok(_) => return Outcome::Ok(Value::None),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        return Outcome::Error(socket_error(
                            &error,
                            "send",
                            Some(address),
                            host,
                            *port,
                        ));
                    }
                }
                self.arrivals();
                thread::sleep(TICK);
            }
        }
        let mut written = 0;
        loop {
            if stop.load(Ordering::Relaxed) {
                return Outcome::Error(NetError::new(4, "the socket stopped"));
            }
            self.arrivals();
            let handle = self.handles.get_mut(&number).expect("existing handle");
            match (op, &mut handle.socket) {
                (
                    Op::Read { max, .. },
                    SocketKind::Stream {
                        inbox, rest, end, ..
                    },
                ) => {
                    if *max == 0 {
                        return Outcome::Ok(Value::Bytes(Vec::new()));
                    }
                    if rest.is_empty()
                        && let Some(chunk) = inbox.pop_front()
                    {
                        rest.extend(chunk);
                    }
                    if !rest.is_empty() {
                        let n = rest.len().min(*max as usize);
                        return Outcome::Ok(Value::Bytes(rest.drain(..n).collect()));
                    }
                    if let Some(why) = end {
                        return match why {
                            End::Eof => Outcome::Ok(Value::Bytes(Vec::new())),
                            End::Error(error) => Outcome::Error(error.clone()),
                        };
                    }
                }
                (Op::Write { bytes, .. }, SocketKind::Stream { socket, end, .. }) => {
                    if matches!(end, Some(End::Error(_))) {
                        return Outcome::Error(NetError::new(
                            4,
                            "Cannot call write after a stream was destroyed",
                        ));
                    }
                    if matches!(end, Some(End::Eof)) {
                        return Outcome::Error(NetError::new(
                            4,
                            "This socket has been ended by the other party",
                        ));
                    }
                    if written == bytes.len() {
                        return Outcome::Ok(Value::None);
                    }
                    match socket.write(&bytes[written..]) {
                        Ok(0) => {
                            let error = NetError::new(4, "write EPIPE");
                            *end = Some(End::Error(error.clone()));
                            let _ = socket.shutdown(Shutdown::Both);
                            return Outcome::Error(error);
                        }
                        Ok(n) => {
                            written += n;
                            continue;
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                        Err(error) => {
                            let error = socket_error(&error, "write", None, "", 0);
                            *end = Some(End::Error(error.clone()));
                            let _ = socket.shutdown(Shutdown::Both);
                            return Outcome::Error(error);
                        }
                    }
                }
                (Op::Accept(_), SocketKind::Listener { inbox, end, .. }) => {
                    if let Some(socket) = inbox.pop_front() {
                        return self.open(stream(socket));
                    }
                    if let Some(error) = end {
                        return Outcome::Error(error.clone());
                    }
                }
                (Op::Receive { max, .. }, SocketKind::Udp { inbox, end, .. }) => {
                    if let Some((mut bytes, peer)) = inbox.pop_front() {
                        bytes.truncate(*max as usize);
                        return Outcome::Ok(Value::Datagram(bytes, peer));
                    }
                    if let Some(error) = end {
                        return Outcome::Error(error.clone());
                    }
                }
                _ => return wrong_kind(),
            }
            thread::sleep(TICK);
        }
    }

    fn arrivals(&mut self) {
        for handle in self.handles.values_mut() {
            match &mut handle.socket {
                SocketKind::Stream {
                    socket, inbox, end, ..
                } => {
                    if end.is_some() {
                        continue;
                    }
                    for _ in 0..64 {
                        let mut bytes = vec![0; CHUNK];
                        match socket.read(&mut bytes) {
                            Ok(0) => {
                                *end = Some(End::Eof);
                                // Node sockets default to allowHalfOpen=false.
                                let _ = socket.shutdown(Shutdown::Write);
                                break;
                            }
                            Ok(n) => {
                                bytes.truncate(n);
                                inbox.push_back(bytes);
                            }
                            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                            Err(error) => {
                                *end = Some(End::Error(socket_error(&error, "read", None, "", 0)));
                                break;
                            }
                        }
                    }
                }
                SocketKind::Listener {
                    socket: Some(socket),
                    inbox,
                    end,
                } => {
                    if end.is_some() {
                        continue;
                    }
                    for _ in 0..64 {
                        match socket.accept() {
                            Ok((socket, _)) => {
                                if let Err(error) = socket.set_nonblocking(true) {
                                    *end = Some(socket_error(&error, "accept", None, "", 0));
                                    break;
                                }
                                inbox.push_back(socket);
                            }
                            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                            Err(error) => {
                                *end = Some(socket_error(&error, "accept", None, "", 0));
                                break;
                            }
                        }
                    }
                }
                SocketKind::Udp {
                    socket: Some(socket),
                    inbox,
                    end,
                    ..
                } => {
                    if end.is_some() {
                        continue;
                    }
                    for _ in 0..64 {
                        let mut bytes = vec![0; CHUNK];
                        match socket.recv_from(&mut bytes) {
                            Ok((n, peer)) => {
                                bytes.truncate(n);
                                inbox.push_back((bytes, peer));
                            }
                            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                            Err(error) => {
                                *end = Some(socket_error(&error, "recvmsg", None, "", 0));
                                break;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn stream(socket: TcpStream) -> SocketKind {
    SocketKind::Stream {
        socket,
        inbox: VecDeque::new(),
        rest: VecDeque::new(),
        end: None,
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ConnectAttempt {
    pub(crate) address: SocketAddr,
    pub(crate) timeout: Option<Duration>,
}

impl ConnectAttempt {
    fn expired(self, elapsed: Duration) -> bool {
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

fn resolve(host: &str, port: u16) -> Result<Vec<SocketAddr>, NetError> {
    (host, port)
        .to_socket_addrs()
        .map(Iterator::collect)
        .map_err(|_| NetError::new(2, host))
}

fn bind(method: &str, address: SocketAddr) -> io::Result<SocketKind> {
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

fn connecting(error: &io::Error) -> bool {
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

fn wrong_kind() -> Outcome {
    Outcome::Error(NetError::new(4, "an operation on a handle of another kind"))
}

fn socket_error(
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
    let raw = network_errno(error);
    let (code, description) = if raw.0 == "UNKNOWN" {
        match error.kind() {
            io::ErrorKind::PermissionDenied => ("EACCES", "permission denied"),
            io::ErrorKind::ConnectionReset => ("ECONNRESET", "connection reset by peer"),
            io::ErrorKind::ConnectionAborted => {
                ("ECONNABORTED", "software caused connection abort")
            }
            io::ErrorKind::NotConnected => ("ENOTCONN", "socket is not connected"),
            io::ErrorKind::BrokenPipe => ("EPIPE", "broken pipe"),
            io::ErrorKind::AddrNotAvailable => ("EADDRNOTAVAIL", "address not available"),
            io::ErrorKind::NetworkUnreachable => ("ENETUNREACH", "network is unreachable"),
            io::ErrorKind::HostUnreachable => ("EHOSTUNREACH", "host is unreachable"),
            io::ErrorKind::TimedOut => ("ETIMEDOUT", "connection timed out"),
            io::ErrorKind::InvalidInput => ("EINVAL", "invalid argument"),
            io::ErrorKind::Unsupported => ("ENOTSUP", "operation not supported on socket"),
            io::ErrorKind::OutOfMemory => ("ENOMEM", "not enough memory"),
            _ => raw,
        }
    } else {
        raw
    };
    let text = if let Some(address) = actual {
        let suffix = format!("{}:{}", address.ip(), address.port());
        if matches!(syscall, "listen" | "bind") {
            format!("{syscall} {code}: {description} {suffix}")
        } else {
            format!("{syscall} {code} {suffix}")
        }
    } else {
        format!("{syscall} {code}")
    };
    NetError::new(4, text)
}

fn network_errno(error: &io::Error) -> (&'static str, &'static str) {
    #[cfg(unix)]
    match error.raw_os_error() {
        Some(libc::EMSGSIZE) => return ("EMSGSIZE", "message too long"),
        Some(libc::EAFNOSUPPORT) => return ("EAFNOSUPPORT", "address family not supported"),
        Some(libc::ENOBUFS) => return ("ENOBUFS", "no buffer space available"),
        Some(libc::ENOMEM) => return ("ENOMEM", "not enough memory"),
        Some(libc::EMFILE) => return ("EMFILE", "too many open files"),
        Some(libc::ENFILE) => return ("ENFILE", "file table overflow"),
        Some(libc::EBADF) => return ("EBADF", "bad file descriptor"),
        Some(libc::ENOTSOCK) => return ("ENOTSOCK", "socket operation on non-socket"),
        Some(libc::EPERM) => return ("EPERM", "operation not permitted"),
        _ => {}
    }
    #[cfg(windows)]
    match error.raw_os_error() {
        Some(10040) => return ("EMSGSIZE", "message too long"),
        Some(10047) => return ("EAFNOSUPPORT", "address family not supported"),
        Some(10055) => return ("ENOBUFS", "no buffer space available"),
        Some(10024) => return ("EMFILE", "too many open files"),
        Some(10038) => return ("ENOTSOCK", "socket operation on non-socket"),
        _ => {}
    }
    ("UNKNOWN", "unknown error")
}

#[cfg(test)]
mod tests {
    use super::{Client, Op, Outcome, Request, State, connect_attempts, covers_lookup, valid_host};
    use crate::codec::{Dec, Enc};
    use crate::host::Stop;
    use hd_run::{Grants, Limit};
    use socket2::Socket;
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpListener, UdpSocket};
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    fn host_args(host: &str) -> Enc {
        let mut e = Enc::default();
        e.str(host);
        e
    }

    fn opened(client: &mut Client, method: &str, port: u16) -> i64 {
        let result = client
            .call(
                method,
                &[i64::from(port), 0],
                host_args("127.0.0.1").done(),
                &Grants::default(),
            )
            .unwrap();
        let mut d = Dec::new(result.done());
        assert_eq!(d.leb().unwrap(), 0);
        i64::try_from(d.leb().unwrap()).unwrap()
    }

    fn result_bytes(e: &Enc) -> Vec<u8> {
        let mut d = Dec::new(e.done());
        assert_eq!(d.leb().unwrap(), 0);
        d.bytes().unwrap().to_vec()
    }

    #[test]
    fn names_literal_spelling_and_lookup_grants_match_node() {
        for valid in [
            "localhost",
            "a.",
            "a-b.example",
            "127.0.0.1",
            "::1",
            "fe80::1%lo0",
        ] {
            assert!(valid_host(valid), "{valid}");
        }
        for invalid in ["", "-a", "a-", "a..b", "a/b", "[::1]", "a\0b", "é.example"] {
            assert!(!valid_host(invalid), "{invalid}");
        }
        let grants = Grants {
            limits: vec![(
                "Net".into(),
                Limit::Entries(vec![
                    "127.0.0.1:80".into(),
                    "[::1]:9".into(),
                    "*.example:8".into(),
                ]),
            )],
        };
        assert!(covers_lookup(&grants, "127.0.0.1"));
        assert!(covers_lookup(&grants, "::1"));
        assert!(covers_lookup(&grants, "one.example"));
        assert!(!covers_lookup(&grants, "example"));
        assert!(!covers_lookup(&grants, "one.EXAMPLE"));
        let mut client = Client::default();
        for host in ["0:0:0:0:0:0:0:1", "fe80::1%lo0"] {
            let actual = client
                .call(
                    "lookup",
                    &[i64::try_from(host.len()).unwrap()],
                    host.as_bytes(),
                    &Grants::default(),
                )
                .unwrap();
            let mut expected = Enc::default();
            expected.leb(0).leb(1).str(host);
            assert_eq!(actual.done(), expected.done());
        }
    }

    #[test]
    fn refusal_is_lazy_and_unknown_handles_precede_destination_checks() {
        let mut client = Client::default();
        let grants = Grants {
            limits: vec![("Net".into(), Limit::Deny)],
        };
        let mut expected = Enc::default();
        expected.leb(1).leb(0).str("127.0.0.1:9");
        assert_eq!(
            client
                .call("connect", &[9, 0], host_args("127.0.0.1").done(), &grants)
                .unwrap()
                .done(),
            expected.done()
        );
        assert!(client.worker.is_none());
        for bytes in [
            host_args("127.0.0.1"),
            host_args("bad host"),
            Enc::default(),
        ] {
            let error = client
                .call("UdpSocket.send_to", &[123, 9, 0], bytes.done(), &grants)
                .err()
                .unwrap();
            assert_eq!(
                error.downcast_ref::<Stop>().unwrap().to_string(),
                "an operation on a handle the host never made"
            );
        }
        assert!(client.worker.is_none());
        let error = client
            .call("TcpStream.close", &[123], &[], &Grants::default())
            .err()
            .unwrap();
        assert_eq!(
            error.downcast_ref::<Stop>().unwrap().to_string(),
            "an operation on a handle the host never made"
        );
    }

    #[test]
    fn tcp_spillover_zero_read_eof_and_disposal_are_sticky() {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let writer = thread::spawn(move || {
            let (mut socket, _) = server.accept().unwrap();
            socket.write_all(b"abcdef").unwrap();
            socket.shutdown(Shutdown::Write).unwrap();
        });
        let mut client = Client::default();
        let handle = opened(&mut client, "connect", port);
        for (max, expected) in [
            (2, &b"ab"[..]),
            (0, &b""[..]),
            (i64::from(i32::MIN), &b"cdef"[..]),
            (5, &b""[..]),
            (5, &b""[..]),
        ] {
            let actual = client
                .call("TcpStream.read", &[handle, max], &[], &Grants::default())
                .unwrap();
            assert_eq!(result_bytes(&actual), expected);
        }
        let actual = client
            .call(
                "TcpStream.write",
                &[handle, 0],
                Enc::default().bytes(b"x").done(),
                &Grants::default(),
            )
            .unwrap();
        let mut expected = Enc::default();
        expected
            .leb(1)
            .leb(0)
            .leb(4)
            .str("This socket has been ended by the other party");
        assert_eq!(actual.done(), expected.done());
        assert_eq!(
            client
                .call("TcpStream.close", &[handle], &[], &Grants::default())
                .unwrap()
                .done(),
            &[0]
        );
        for (method, params) in [
            ("TcpStream.close", vec![handle]),
            ("TcpStream.read", vec![handle, 1]),
        ] {
            assert_eq!(
                client
                    .call(method, &params, &[], &Grants::default())
                    .unwrap()
                    .done(),
                &[1, 1]
            );
        }
        client.stop();
        writer.join().unwrap();
    }

    #[test]
    fn udp_receives_one_datagram_and_zero_max_still_consumes_one() {
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = peer.local_addr().unwrap().port();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let server = thread::spawn(move || {
            let mut bytes = [0; 16];
            let (n, from) = peer.recv_from(&mut bytes).unwrap();
            assert_eq!(&bytes[..n], b"ping");
            peer.send_to(b"hello", from).unwrap();
            peer.send_to(b"", from).unwrap();
            peer.send_to(b"wide", from).unwrap();
            from
        });
        let mut client = Client::default();
        let handle = opened(&mut client, "bind_udp", 0);
        let mut args = host_args("127.0.0.1");
        args.bytes(b"ping");
        assert_eq!(
            client
                .call(
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
            let actual = client
                .call("UdpSocket.receive", &[handle, max], &[], &Grants::default())
                .unwrap();
            let mut d = Dec::new(actual.done());
            assert_eq!(d.leb().unwrap(), 0);
            assert_eq!(d.bytes().unwrap(), bytes);
            assert_eq!(d.str().unwrap(), "127.0.0.1");
            assert_eq!(d.leb().unwrap(), u64::from(port));
        }
        let address = server.join().unwrap();
        client.stop();
        UdpSocket::bind(address).expect("stop releases the bound UDP address");
    }

    #[test]
    fn stop_wakes_a_pending_nonblocking_read_and_joins_the_owner() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = Client::default();
        let handle = opened(&mut client, "connect", port);
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let (reply, result) = mpsc::channel();
        client
            .worker
            .as_ref()
            .unwrap()
            .requests
            .send(Request {
                op: Op::Read { handle, max: 1 },
                reply,
            })
            .unwrap();
        client.stop();
        assert!(matches!(
            result.recv_timeout(Duration::from_secs(1)),
            Ok(Outcome::Error(_)) | Err(mpsc::RecvTimeoutError::Disconnected)
        ));
        assert_eq!(peer.read(&mut [0; 1]).unwrap(), 0);
    }

    #[test]
    fn closed_udp_send_is_disposed_before_denied_or_invalid_destination() {
        let mut client = Client::default();
        let handle = opened(&mut client, "bind_udp", 0);
        assert_eq!(
            client
                .call("UdpSocket.close", &[handle], &[], &Grants::default())
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
                client
                    .call("UdpSocket.send_to", &[handle, 9, 0], args.done(), &denied)
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
        let (ack, ready) = mpsc::channel();
        let peer = thread::spawn(move || {
            let (mut stream, _) = server.accept().unwrap();
            stream.write_all(b"abc").unwrap();
            ready.recv_timeout(Duration::from_secs(2)).unwrap();
            let socket = Socket::from(stream);
            socket.set_linger(Some(Duration::ZERO)).unwrap();
        });
        let mut client = Client::default();
        let handle = opened(&mut client, "connect", port);
        assert_eq!(
            result_bytes(
                &client
                    .call("TcpStream.read", &[handle, 3], &[], &Grants::default())
                    .unwrap()
            ),
            b"abc"
        );
        ack.send(()).unwrap();
        peer.join().unwrap();
        let mut expected = Enc::default();
        expected.leb(1).leb(0).leb(4).str("read ECONNRESET");
        for _ in 0..2 {
            assert_eq!(
                client
                    .call("TcpStream.read", &[handle, 3], &[], &Grants::default())
                    .unwrap()
                    .done(),
                expected.done()
            );
        }
        let mut args = Enc::default();
        args.bytes(b"x");
        let mut expected = Enc::default();
        expected
            .leb(1)
            .leb(0)
            .leb(4)
            .str("Cannot call write after a stream was destroyed");
        assert_eq!(
            client
                .call(
                    "TcpStream.write",
                    &[handle, 0],
                    args.done(),
                    &Grants::default()
                )
                .unwrap()
                .done(),
            expected.done()
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
    fn connection_candidates_advance_on_local_refusal_and_preserve_aggregate_code() {
        let refused = TcpListener::bind("127.0.0.1:0").unwrap();
        let refused_address = refused.local_addr().unwrap();
        drop(refused);
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = server.local_addr().unwrap();
        let stop = AtomicBool::new(false);
        let mut state = State::default();
        let socket = state
            .connect_candidates(
                vec![refused_address, address],
                "localhost",
                address.port(),
                &stop,
            )
            .unwrap_or_else(|e| panic!("connection failed: {}", e.text));
        assert_eq!(socket.peer_addr().unwrap(), address);
        let (_, _) = server.accept().unwrap();
        drop(socket);
        drop(server);
        let error = state
            .connect_candidates(
                vec![refused_address, address],
                "localhost",
                address.port(),
                &stop,
            )
            .unwrap_err();
        assert_eq!(error.variant, 3);
        assert_eq!(error.text, format!("localhost:{}", address.port()));
    }
}
