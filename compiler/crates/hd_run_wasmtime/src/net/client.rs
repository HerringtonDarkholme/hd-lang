use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use hd_run::Grants;

use crate::codec::{Dec, Enc};
use crate::host::Stop;

use super::state::State;
use super::validate::{at, covers_lookup, valid_host};
use super::{CLEANUP_TIMEOUT, CLOSE, TICK};

#[derive(Default)]
pub struct Client {
    pub(crate) worker: Option<Worker>,
    pub(crate) known: HashSet<i64>,
    pub(crate) closed: HashSet<i64>,
}

pub(crate) struct Worker {
    pub(crate) requests: Sender<Request>,
    pub(crate) stopped: Arc<AtomicBool>,
    pub(crate) thread: Option<JoinHandle<()>>,
}

pub(crate) struct Request {
    pub(crate) op: Op,
    pub(crate) reply: Sender<Outcome>,
    pub(crate) deadline: Option<Instant>,
}

pub(crate) enum Op {
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

#[derive(Clone)]
pub(crate) struct NetError {
    pub(crate) variant: u64,
    pub(crate) text: String,
}

impl NetError {
    pub(crate) fn new(variant: u64, text: impl Into<String>) -> Self {
        Self {
            variant,
            text: text.into(),
        }
    }
}

impl Client {
    /// Requests owner shutdown and waits up to the bounded cleanup budget.
    /// A delayed owner finishes releasing sockets after this method returns.
    pub(crate) fn stop(&mut self) {
        self.worker.take();
        self.known.clear();
        self.closed.clear();
    }

    #[cfg(test)]
    pub(crate) fn call(
        &mut self,
        method: &str,
        params: &[i64],
        bytes: &[u8],
        grants: &Grants,
    ) -> wasmtime::Result<Enc> {
        self.call_until(method, params, bytes, grants, None)
    }

    /// Synchronous guest calls remain bounded by the program's deadline.
    pub(crate) fn call_until(
        &mut self,
        method: &str,
        params: &[i64],
        bytes: &[u8],
        grants: &Grants,
        deadline: Option<Instant>,
    ) -> wasmtime::Result<Enc> {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(Stop::TimeLimit.into());
        }
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
        // destination validation or grant check. Node's metadata-only worker
        // preflight enforces the same precedence for UDP sends.
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
                // A live send always validates its own destination grant.
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
        let outcome = self.request(op, deadline)?;
        if let Outcome::Ok(Value::Handle(handle)) = &outcome {
            self.known
                .insert(i64::try_from(*handle).unwrap_or(i64::MAX));
        }
        if CLOSE.contains(&method) && matches!(outcome, Outcome::Ok(Value::None)) {
            self.closed.insert(scalar(0)?);
        }
        encode(outcome, resource)
    }

    fn request(&mut self, op: Op, deadline: Option<Instant>) -> wasmtime::Result<Outcome> {
        if self.worker.is_none() {
            self.worker = Some(Worker::new()?);
        }
        let worker = self.worker.as_ref().expect("initialized worker");
        let (reply, result) = mpsc::channel();
        worker
            .requests
            .send(Request {
                op,
                reply,
                deadline,
            })
            .map_err(|_| Stop::Internal("the Net worker stopped".into()))?;
        let outcome = if let Some(deadline) = deadline {
            match result.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() >= deadline => {
                    self.stop();
                    return Err(Stop::TimeLimit.into());
                }
                Ok(outcome) => outcome,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(Stop::Internal("the Net worker stopped".into()).into());
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    self.stop();
                    return Err(Stop::TimeLimit.into());
                }
            }
        } else {
            result
                .recv()
                .map_err(|_| Stop::Internal("the Net worker stopped".into()))?
        };
        Ok(outcome)
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
            let until = Instant::now() + CLEANUP_TIMEOUT;
            while !thread.is_finished() && Instant::now() < until {
                thread::sleep(TICK);
            }
            if thread.is_finished() {
                let _ = thread.join();
            }
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

#[cfg(test)]
mod tests {
    use super::Client;
    use crate::codec::Enc;
    use crate::host::Stop;
    use crate::net::state::SocketKind;
    use crate::net::test_support::{
        assert_owner_stopped, client_with_bound_socket, host_args, opened, owner_requests,
    };
    use hd_run::{Grants, Limit};
    use std::collections::VecDeque;
    use std::io::Read;
    use std::net::{TcpListener, UdpSocket};
    use std::time::{Duration, Instant};

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
    fn expired_program_deadline_traps_without_starting_net_worker() {
        let mut client = Client::default();
        let error = client
            .call_until(
                "lookup",
                &[],
                b"127.0.0.1",
                &Grants::default(),
                Some(Instant::now()),
            )
            .err()
            .expect("program deadline traps");
        assert!(matches!(
            error.downcast_ref::<Stop>(),
            Some(Stop::TimeLimit)
        ));
        assert!(client.worker.is_none());
    }
    #[test]
    fn program_deadline_interrupts_read_and_backpressured_write_and_releases_stream() {
        for writing in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let mut client = Client::default();
            let handle = opened(
                &mut client,
                "connect",
                listener.local_addr().unwrap().port(),
            );
            let (mut peer, _) = listener.accept().unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
            let mut payload = Enc::default();
            if writing {
                socket2::SockRef::from(&peer)
                    .set_recv_buffer_size(4096)
                    .unwrap();
                payload.bytes(&vec![b'x'; 16 * 1024 * 1024]);
            }
            let method = if writing {
                "TcpStream.write"
            } else {
                "TcpStream.read"
            };
            let start = Instant::now();
            let error = client
                .call_until(
                    method,
                    &[handle, 1],
                    payload.done(),
                    &Grants::default(),
                    Some(start + Duration::from_millis(50)),
                )
                .err()
                .expect("program deadline traps");
            assert!(matches!(
                error.downcast_ref::<Stop>(),
                Some(Stop::TimeLimit)
            ));
            assert!(
                client.worker.is_none(),
                "timeout stops and releases guest's socket owner"
            );
            assert!(start.elapsed() < Duration::from_millis(500));
            if !writing {
                assert_eq!(
                    peer.read(&mut [0; 1]).unwrap(),
                    0,
                    "read cancellation closes peer stream"
                );
            }
        }
    }
    #[test]
    fn program_deadline_interrupts_accept_and_releases_listener_before_returning() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut client = client_with_bound_socket(SocketKind::Listener {
            socket: Some(listener),
            inbox: VecDeque::default(),
            end: None,
        });
        let requests = owner_requests(&client);
        let start = Instant::now();
        let error = client
            .call_until(
                "TcpListener.accept",
                &[1],
                &[],
                &Grants::default(),
                Some(start + Duration::from_millis(50)),
            )
            .err()
            .expect("program deadline traps");
        assert!(matches!(
            error.downcast_ref::<Stop>(),
            Some(Stop::TimeLimit)
        ));
        assert!(start.elapsed() < Duration::from_millis(500));
        assert_owner_stopped(&client, &requests);
    }
    #[test]
    fn program_deadline_interrupts_receive_and_releases_udp_before_returning() {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.set_nonblocking(true).unwrap();
        let mut client = client_with_bound_socket(SocketKind::Udp {
            socket: Some(socket),
            inbox: VecDeque::default(),
            end: None,
            ipv6: false,
        });
        let requests = owner_requests(&client);
        let start = Instant::now();
        let error = client
            .call_until(
                "UdpSocket.receive",
                &[1, 1],
                &[],
                &Grants::default(),
                Some(start + Duration::from_millis(50)),
            )
            .err()
            .expect("program deadline traps");
        assert!(matches!(
            error.downcast_ref::<Stop>(),
            Some(Stop::TimeLimit)
        ));
        assert!(start.elapsed() < Duration::from_millis(500));
        assert_owner_stopped(&client, &requests);
    }
}
