use std::collections::{HashMap, VecDeque};
use std::io::{self, Read};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Instant;

use socket2::{Domain, Protocol, Socket, Type};

use super::client::{NetError, Op, Outcome, Request, Value};
use super::socket::{
    ConnectAttempt, bind, cancelled, connect_attempts, connecting, resolve, socket_error,
};
use super::validate::ip_literal;
use super::{CHUNK, TICK};

#[derive(Default)]
pub(crate) struct State {
    pub(crate) handles: HashMap<i64, Handle>,
    pub(crate) next_handle: u64,
    pub(crate) deadline: Option<Instant>,
}

pub(crate) struct Handle {
    pub(crate) closed: bool,
    pub(crate) socket: SocketKind,
}

pub(crate) enum End {
    Eof,
    Error(NetError),
}

pub(crate) enum SocketKind {
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
    pub(crate) fn serve(&mut self, requests: &Receiver<Request>, stop: &AtomicBool) {
        while !stop.load(Ordering::Relaxed) {
            self.arrivals(stop);
            match requests.recv_timeout(TICK) {
                Ok(request) => {
                    self.deadline = request.deadline;
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

    pub(crate) fn open(&mut self, socket: SocketKind) -> Outcome {
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
        if cancelled(stop, self.deadline) {
            return Outcome::Error(NetError::new(4, "the socket stopped"));
        }
        match op {
            Op::Lookup(host) => {
                // Node preserves a literal's spelling rather than formatting
                // the address through an IPv6 canonicalizer.
                if ip_literal(&host) {
                    return Outcome::Ok(Value::Addresses(vec![host]));
                }
                match resolve(&host, 0, stop, self.deadline) {
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
        let addresses = match resolve(host, port, stop, self.deadline) {
            Ok(a) => a,
            Err(e) => return Outcome::Error(e),
        };
        if cancelled(stop, self.deadline) {
            return Outcome::Error(NetError::new(4, "the socket stopped"));
        }
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

    pub(crate) fn connect_candidates(
        &mut self,
        addresses: Vec<SocketAddr>,
        host: &str,
        port: u16,
        stop: &AtomicBool,
    ) -> Result<TcpStream, NetError> {
        let attempts = connect_attempts(addresses);

        let mut first_error = None;
        for attempt in attempts {
            if cancelled(stop, self.deadline) {
                break;
            }
            match self.connect(attempt, stop) {
                Ok(socket) => return Ok(socket),
                Err(error) => {
                    first_error.get_or_insert_with(|| {
                        socket_error(&error, "connect", Some(attempt.address), host, port)
                    });
                }
            }
            if cancelled(stop, self.deadline) {
                break;
            }
        }
        Err(first_error.unwrap_or_else(|| NetError::new(2, host)))
    }

    fn connect(&mut self, attempt: ConnectAttempt, stop: &AtomicBool) -> io::Result<TcpStream> {
        if cancelled(stop, self.deadline) {
            return Err(io::ErrorKind::Interrupted.into());
        }
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
            if cancelled(stop, self.deadline) {
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
            self.arrivals(stop);
            thread::sleep(TICK);
        }
    }

    pub(crate) fn arrivals(&mut self, stop: &AtomicBool) {
        for handle in self.handles.values_mut() {
            if cancelled(stop, self.deadline) {
                return;
            }
            match &mut handle.socket {
                SocketKind::Stream {
                    socket, inbox, end, ..
                } => {
                    if end.is_some() {
                        continue;
                    }
                    for _ in 0..64 {
                        if cancelled(stop, self.deadline) {
                            return;
                        }
                        let mut bytes = vec![0; CHUNK];
                        match socket.read(&mut bytes) {
                            Ok(0) => {
                                *end = Some(End::Eof);

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
                        if cancelled(stop, self.deadline) {
                            return;
                        }
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
                        if cancelled(stop, self.deadline) {
                            return;
                        }
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

pub(crate) fn stream(socket: TcpStream) -> SocketKind {
    SocketKind::Stream {
        socket,
        inbox: VecDeque::new(),
        rest: VecDeque::new(),
        end: None,
    }
}

#[cfg(test)]
mod tests {
    use super::State;
    use crate::net::test_support::reserved_refused_port;
    use std::net::TcpListener;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn connection_candidates_advance_on_local_refusal_and_preserve_aggregate_code() {
        // Retain both established pairs, keeping their source ports reserved
        // without listeners throughout all connection attempts.
        let (refused_address, _refused) = reserved_refused_port();
        let (also_refused_address, _also_refused) = reserved_refused_port();
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
        let error = state
            .connect_candidates(
                vec![refused_address, also_refused_address],
                "localhost",
                address.port(),
                &stop,
            )
            .unwrap_err();
        assert_eq!(error.variant, 3);
        assert_eq!(error.text, format!("localhost:{}", address.port()));
    }
}
