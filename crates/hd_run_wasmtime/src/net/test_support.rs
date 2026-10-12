use std::collections::HashSet;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::codec::{Dec, Enc};
use hd_run::Grants;

use super::client::{Client, Op, Outcome, Request, Value, Worker};
use super::state::{SocketKind, State};

pub(crate) fn host_args(host: &str) -> Enc {
    let mut e = Enc::default();
    e.str(host);
    e
}

pub(crate) fn reserved_refused_port() -> (SocketAddr, [TcpStream; 2]) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client =
        TcpStream::connect_timeout(&listener.local_addr().unwrap(), Duration::from_secs(1))
            .unwrap();
    let (peer, _) = listener.accept().unwrap();
    // An established client's source port stays reserved but has no
    // listener, so an unrelated connection tuple is refused. Merely
    // binding an unlistening socket would instead time out on Darwin.
    (client.local_addr().unwrap(), [client, peer])
}

pub(crate) fn opened(client: &mut Client, method: &str, port: u16) -> i64 {
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

pub(crate) fn result_bytes(e: &Enc) -> Vec<u8> {
    let mut d = Dec::new(e.done());
    assert_eq!(d.leb().unwrap(), 0);
    d.bytes().unwrap().to_vec()
}

// TCP returns at most max bytes, not an entire application message.
pub(crate) fn read_n(client: &mut Client, handle: i64, count: usize, max: i64) -> Vec<u8> {
    let until = Instant::now() + Duration::from_secs(2);
    let mut bytes = Vec::new();
    while bytes.len() < count {
        let encoded = client
            .call_until(
                "TcpStream.read",
                &[handle, max],
                &[],
                &Grants::default(),
                Some(until),
            )
            .unwrap();
        let chunk = result_bytes(&encoded);
        assert!(!chunk.is_empty(), "peer ended before the fixture message");
        bytes.extend(chunk);
    }
    assert_eq!(bytes.len(), count);
    bytes
}

pub(crate) fn owner_requests(client: &Client) -> mpsc::Sender<Request> {
    client.worker.as_ref().unwrap().requests.clone()
}

pub(crate) fn assert_owner_stopped(client: &Client, requests: &mpsc::Sender<Request>) {
    assert!(client.worker.is_none());
    assert!(client.known.is_empty());
    assert!(client.closed.is_empty());
    // The receiver is owned by the worker's closure. It disconnects only
    // after State::serve has returned and cleared every owned socket.
    // Bounded production teardown may detach a delayed owner; observe
    // its eventual acknowledgment rather than assuming an immediate join.
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        let (reply, _result) = mpsc::channel();
        if requests
            .send(Request {
                op: Op::Close(1),
                reply,
                deadline: None,
            })
            .is_err()
        {
            return;
        }
        assert!(
            Instant::now() < until,
            "stopped socket owner drops its receiver"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

pub(crate) fn client_with_bound_socket(socket: SocketKind) -> Client {
    let mut state = State::default();
    assert!(matches!(state.open(socket), Outcome::Ok(Value::Handle(1))));
    let (requests, input) = mpsc::channel();
    let stopped = Arc::new(AtomicBool::new(false));
    let stop = Arc::clone(&stopped);
    let thread = thread::spawn(move || state.serve(&input, &stop));
    Client {
        worker: Some(Worker {
            requests,
            stopped,
            thread: Some(thread),
        }),
        known: [1].into_iter().collect(),
        closed: HashSet::default(),
    }
}
