//! Socket ABI parity. Every peer binds loopback; DNS uses numeric addresses.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use crate::support::*;
use hd_run::{Engine, Grants, HostSetup, Instance, Limit, Limits, Outcome};
use hd_run_wasmtime::{EngineConfig, HostIo, WasmtimeEngine};
use wasm_encoder::{Instruction as I, ValType};

/// `NetError.Timeout`'s variant, which has no field.
const TIMEOUT: u64 = 4;

fn net_error(variant: u64, text: &str) -> Vec<u8> {
    Enc::default().leb(1).leb(variant).str(text).0
}
fn operation_error(variant: u64, text: &str) -> Vec<u8> {
    Enc::default().leb(1).leb(0).leb(variant).str(text).0
}
fn open(p: &mut Program, method: &str, host: &str, port: u16, result: Vec<u8>) {
    p.abi_call(
        "hd:Net",
        method,
        &[i32::from(port)],
        Some(&Enc::default().str(host).0),
        true,
        result,
    );
}
fn handle(p: &mut Program, method: &str, args: &[i32], payload: Option<&[u8]>, expected: Vec<u8>) {
    p.abi_call(
        "hd:Net",
        method,
        args,
        payload,
        !matches!(
            method,
            "TcpStream.close"
                | "TcpListener.close"
                | "UdpSocket.close"
                | "TcpStream.set_timeout"
                | "TcpListener.set_timeout"
                | "UdpSocket.set_timeout"
                | "TcpStream.set_nodelay"
                | "TcpStream.set_keepalive"
        ),
        expected,
    );
}

/// A `Duration?` of `millis` milliseconds, or `.None`.
fn bound(millis: Option<i64>) -> Vec<u8> {
    match millis {
        // `Duration.millis` is a zigzag LEB128 `i64`.
        Some(millis) => {
            Enc::default()
                .leb(1)
                .leb(((millis << 1) ^ (millis >> 63)).cast_unsigned())
                .0
        }
        None => Enc::default().leb(0).0,
    }
}
fn set_timeout(p: &mut Program, kind: &str, h: i32, millis: Option<i64>) {
    handle(
        p,
        &format!("{kind}.set_timeout"),
        &[h],
        Some(&bound(millis)),
        vec![0],
    );
}
fn connect_timeout(p: &mut Program, host: &str, port: u16, millis: i64, result: Vec<u8>) {
    p.abi_call_with(
        "hd:Net",
        "connect_timeout",
        &[Scalar::I32(i32::from(port)), Scalar::I64(millis)],
        Some(&Enc::default().str(host).0),
        true,
        result,
    );
}

#[test]
fn net_numeric_lookup_and_scope_host_match_ignores_port() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    p.call(
        "hd:Net",
        "lookup",
        b"127.0.0.1",
        Enc::default().leb(0).leb(1).str("127.0.0.1").0,
    );
    p.call(
        "hd:Net",
        "lookup",
        b"::1",
        Enc::default().leb(0).leb(1).str("::1").0,
    );
    p.call("hd:Net", "lookup", b"127.0.0.2", net_error(0, "127.0.0.2"));
    p.call("hd:Net", "lookup", b"bad host", net_error(1, "bad host"));
    assert_program(
        &scratch,
        p,
        vec![(
            "Net",
            Limit::Entries(vec!["127.0.0.1:99".into(), "[::1]:99".into()]),
        )],
        || {},
    );
}

#[test]
fn net_invalid_addresses_precede_grants_and_all_open_methods_obey_scopes() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    for method in ["connect", "listen", "bind_udp"] {
        open(&mut p, method, "bad host", 9, net_error(1, "bad host:9"));
        open(&mut p, method, "127.0.0.1", 9, net_error(0, "127.0.0.1:9"));
    }
    assert_program(&scratch, p, vec![("Net", Limit::Entries(vec![]))], || {});
}

#[test]
fn tcp_binary_write_read_zero_partial_reads_eof_and_disposal() {
    let scratch = Scratch::new();
    // All waits here are event-driven: the fixture blocks in read_exact until
    // the guest writes, and the guest blocks in each read until bytes or EOF
    // arrive. No wall-clock sleep is asserted. The shared TcpFixture's socket
    // timeouts are generous (10 s) so a slow guest under load (e.g. while
    // conformance runs) cannot trip them on Linux or elsewhere.
    let server = TcpFixture::new(|mut peer| {
        let mut received = [0; 4];
        peer.read_exact(&mut received).unwrap();
        assert_eq!(received, [0, 255, 195, 169]);
        peer.write_all(&[1, 255, 0, 42]).unwrap();
    });
    let mut p = Program::new();
    open(
        &mut p,
        "connect",
        "127.0.0.1",
        server.address.port(),
        vec![0, 1],
    );
    handle(&mut p, "TcpStream.read", &[1, 0], None, vec![0, 0]);
    handle(
        &mut p,
        "TcpStream.write",
        &[1],
        Some(&Enc::default().bytes(&[0, 255, 195, 169]).0),
        vec![0],
    );
    for byte in [1, 255, 0, 42] {
        handle(&mut p, "TcpStream.read", &[1, 1], None, vec![0, 1, byte]);
    }
    for _ in 0..2 {
        handle(&mut p, "TcpStream.read", &[1, 8], None, vec![0, 0]);
    }
    handle(&mut p, "TcpStream.close", &[1], None, vec![0]);
    handle(&mut p, "TcpStream.read", &[1, 0], None, vec![1, 1]);
    handle(
        &mut p,
        "TcpStream.write",
        &[1],
        Some(&Enc::default().bytes(b"x").0),
        vec![1, 1],
    );
    handle(&mut p, "TcpStream.close", &[1], None, vec![1, 1]);
    assert_program(
        &scratch,
        p,
        vec![(
            "Net",
            Limit::Entries(vec![format!("127.0.0.1:{}", server.address.port())]),
        )],
        || {},
    );
}

#[test]
fn tcp_listener_accept_write_close_and_disposed_accept() {
    let scratch = Scratch::new();
    // The ABI exposes no listener local_addr, so the peer needs a selected
    // port before the guest binds it. This setup still requires a lease
    // handoff; cleanup tests below never rebind a released guest port.
    let address = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let mut p = Program::new();
    open(&mut p, "listen", "127.0.0.1", address.port(), vec![0, 1]);
    handle(&mut p, "TcpListener.accept", &[1], None, vec![0, 2]);
    handle(&mut p, "TcpStream.read", &[2, 1], None, vec![0, 1, b'x']);
    handle(
        &mut p,
        "TcpStream.write",
        &[2],
        Some(&Enc::default().bytes(b"ack").0),
        vec![0],
    );
    handle(&mut p, "TcpStream.close", &[2], None, vec![0]);
    handle(&mut p, "TcpListener.close", &[1], None, vec![0]);
    handle(&mut p, "TcpListener.accept", &[1], None, vec![1, 1]);
    handle(&mut p, "TcpListener.close", &[1], None, vec![1, 1]);
    let clients = Mutex::new(Vec::new());
    assert_program(&scratch, p, vec![], || {
        clients.lock().unwrap().push(std::thread::spawn(move || {
            // Generous start bound: under load (e.g. conformance running
            // alongside) the guest engine can take many seconds to compile
            // and bind. The test proves accept/write/close framing, not fast
            // startup; the peer polls until the listener appears.
            //
            // The second engine's peer may first reach the first engine's
            // closed listener: an unrelated test's child can hold it
            // between its fork and its exec (see `CHILD_SPAWN`). That
            // listener never accepts, and resets the connection once the
            // child execs; the peer then connects again.
            let deadline = Instant::now() + Duration::from_secs(20);
            let bytes = loop {
                assert!(
                    Instant::now() < deadline,
                    "listener fixture starts before its deadline"
                );
                let Ok(mut peer) = TcpStream::connect_timeout(&address, Duration::from_millis(50))
                else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
                peer.set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut bytes = Vec::new();
                match peer
                    .write_all(b"x")
                    .and_then(|()| peer.read_to_end(&mut bytes))
                {
                    Ok(_) => break bytes,
                    Err(error)
                        if bytes.is_empty() && error.kind() != std::io::ErrorKind::WouldBlock => {}
                    Err(error) => panic!("the peer's exchange failed: {error}"),
                }
            };
            assert_eq!(bytes, b"ack");
        }));
    });
    for client in clients.into_inner().unwrap() {
        client.join().unwrap();
    }
}

#[test]
fn tcp_timed_out_read_keeps_the_stream_and_zero_bound_returns_only_buffered_bytes() {
    let scratch = Scratch::new();
    // The peer writes only after the guest's "go", so the first bounded
    // read has nothing to take. It then holds the stream open until the
    // guest closes it, so a zero bound finds no end, only buffered bytes.
    let server = TcpFixture::new(|mut peer| {
        let mut go = [0; 2];
        peer.read_exact(&mut go).unwrap();
        assert_eq!(&go, b"go");
        peer.write_all(b"abcdef").unwrap();
        let mut rest = Vec::new();
        peer.read_to_end(&mut rest).unwrap();
        assert!(rest.is_empty(), "the guest writes nothing more");
    });
    let timed_out = Enc::default().leb(1).leb(0).leb(TIMEOUT).0;
    let mut p = Program::new();
    open(
        &mut p,
        "connect",
        "127.0.0.1",
        server.address.port(),
        vec![0, 1],
    );
    set_timeout(&mut p, "TcpStream", 1, Some(50));
    handle(&mut p, "TcpStream.read", &[1, 8], None, timed_out.clone());
    handle(
        &mut p,
        "TcpStream.write",
        &[1],
        Some(&Enc::default().bytes(b"go").0),
        vec![0],
    );
    set_timeout(&mut p, "TcpStream", 1, Some(10_000));
    handle(
        &mut p,
        "TcpStream.read",
        &[1, 2],
        None,
        vec![0, 2, b'a', b'b'],
    );
    set_timeout(&mut p, "TcpStream", 1, Some(0));
    handle(
        &mut p,
        "TcpStream.read",
        &[1, 8],
        None,
        vec![0, 4, b'c', b'd', b'e', b'f'],
    );
    handle(&mut p, "TcpStream.read", &[1, 8], None, timed_out.clone());
    // A negative bound is a zero one.
    set_timeout(&mut p, "TcpStream", 1, Some(-5));
    handle(&mut p, "TcpStream.read", &[1, 8], None, timed_out);
    set_timeout(&mut p, "TcpStream", 1, None);
    handle(&mut p, "TcpStream.close", &[1], None, vec![0]);
    handle(
        &mut p,
        "TcpStream.set_timeout",
        &[1],
        Some(&bound(None)),
        vec![1, 1],
    );
    assert_program(
        &scratch,
        p,
        vec![(
            "Net",
            Limit::Entries(vec![format!("127.0.0.1:{}", server.address.port())]),
        )],
        || {},
    );
}

#[test]
fn tcp_nodelay_and_keepalive_set_and_a_closed_stream_is_disposed() {
    let scratch = Scratch::new();
    let server = TcpFixture::new(|mut peer| {
        let mut rest = Vec::new();
        peer.read_to_end(&mut rest).unwrap();
    });
    let mut p = Program::new();
    open(
        &mut p,
        "connect",
        "127.0.0.1",
        server.address.port(),
        vec![0, 1],
    );
    handle(&mut p, "TcpStream.set_nodelay", &[1, 1], None, vec![0]);
    handle(&mut p, "TcpStream.set_nodelay", &[1, 0], None, vec![0]);
    handle(
        &mut p,
        "TcpStream.set_keepalive",
        &[1],
        Some(&bound(Some(30_000))),
        vec![0],
    );
    // An idle shorter than a second rounds up, and none turns it off.
    handle(
        &mut p,
        "TcpStream.set_keepalive",
        &[1],
        Some(&bound(Some(1))),
        vec![0],
    );
    handle(
        &mut p,
        "TcpStream.set_keepalive",
        &[1],
        Some(&bound(None)),
        vec![0],
    );
    handle(&mut p, "TcpStream.close", &[1], None, vec![0]);
    handle(&mut p, "TcpStream.set_nodelay", &[1, 1], None, vec![1, 1]);
    handle(
        &mut p,
        "TcpStream.set_keepalive",
        &[1],
        Some(&bound(None)),
        vec![1, 1],
    );
    assert_program(
        &scratch,
        p,
        vec![(
            "Net",
            Limit::Entries(vec![format!("127.0.0.1:{}", server.address.port())]),
        )],
        || {},
    );
}

#[test]
fn connect_timeout_checks_the_grant_first_and_ends_a_zero_bound_at_once() {
    let scratch = Scratch::new();
    let server = TcpFixture::new(|mut peer| {
        let mut rest = Vec::new();
        peer.read_to_end(&mut rest).unwrap();
    });
    let port = server.address.port();
    let mut p = Program::new();
    connect_timeout(
        &mut p,
        "bad host",
        port,
        1000,
        net_error(1, &format!("bad host:{port}")),
    );
    connect_timeout(
        &mut p,
        "127.0.0.2",
        port,
        0,
        net_error(0, &format!("127.0.0.2:{port}")),
    );
    connect_timeout(&mut p, "127.0.0.1", port, 0, net_error_unit(TIMEOUT));
    connect_timeout(&mut p, "127.0.0.1", port, -1, net_error_unit(TIMEOUT));
    connect_timeout(&mut p, "127.0.0.1", port, 10_000, vec![0, 1]);
    handle(&mut p, "TcpStream.close", &[1], None, vec![0]);
    assert_program(
        &scratch,
        p,
        vec![("Net", Limit::Entries(vec![format!("127.0.0.1:{port}")]))],
        || {},
    );
}

fn net_error_unit(variant: u64) -> Vec<u8> {
    Enc::default().leb(1).leb(variant).0
}

/// A loopback listener whose accept queue is full, so a new connect waits
/// for an answer that never comes: the kernel drops its SYN. Windows
/// refuses it instead, so the test that uses it is Unix-only.
#[cfg(unix)]
struct FullListener {
    address: SocketAddr,
    _listener: socket2::Socket,
    _queued: Vec<TcpStream>,
}

#[cfg(unix)]
impl FullListener {
    fn new() -> Self {
        use socket2::{Domain, Socket, Type};
        let listener = Socket::new(Domain::IPV4, Type::STREAM, None).unwrap();
        listener
            .bind(&"127.0.0.1:0".parse::<SocketAddr>().unwrap().into())
            .unwrap();
        // A backlog of 1: Darwin reads 0 as its default of 128.
        listener.listen(1).unwrap();
        let address = listener.local_addr().unwrap().as_socket().unwrap();
        // Connect until one waits: the queue is then full.
        let mut queued = Vec::new();
        loop {
            assert!(queued.len() < 64, "the accept queue never fills");
            match TcpStream::connect_timeout(&address, Duration::from_millis(200)) {
                Ok(stream) => queued.push(stream),
                Err(error) if error.kind() == std::io::ErrorKind::TimedOut => break,
                Err(error) => panic!("filling the accept queue: {error}"),
            }
        }
        Self {
            address,
            _listener: listener,
            _queued: queued,
        }
    }
}

#[cfg(unix)]
#[test]
fn connect_timeout_ends_a_connect_that_gets_no_answer() {
    let scratch = Scratch::new();
    let full = FullListener::new();
    let port = full.address.port();
    let mut p = Program::new();
    connect_timeout(&mut p, "127.0.0.1", port, 100, net_error_unit(TIMEOUT));
    assert_program(
        &scratch,
        p,
        vec![("Net", Limit::Entries(vec![format!("127.0.0.1:{port}")]))],
        || {},
    );
}

#[test]
fn bounded_accept_and_receive_time_out_and_the_handles_stay_open() {
    let scratch = Scratch::new();
    let echo = UdpFixture::new();
    let echo_port = echo.address.port();
    let timed_out = Enc::default().leb(1).leb(0).leb(TIMEOUT).0;
    let mut p = Program::new();
    open(&mut p, "listen", "127.0.0.1", 0, vec![0, 1]);
    set_timeout(&mut p, "TcpListener", 1, Some(0));
    handle(&mut p, "TcpListener.accept", &[1], None, timed_out.clone());
    set_timeout(&mut p, "TcpListener", 1, Some(50));
    handle(&mut p, "TcpListener.accept", &[1], None, timed_out.clone());
    handle(&mut p, "TcpListener.close", &[1], None, vec![0]);
    open(&mut p, "bind_udp", "127.0.0.1", 0, vec![0, 2]);
    set_timeout(&mut p, "UdpSocket", 2, Some(50));
    handle(&mut p, "UdpSocket.receive", &[2, 8], None, timed_out);
    handle(
        &mut p,
        "UdpSocket.send_to",
        &[2, i32::from(echo_port)],
        Some(&Enc::default().str("127.0.0.1").bytes(b"ping").0),
        vec![0],
    );
    set_timeout(&mut p, "UdpSocket", 2, Some(10_000));
    handle(
        &mut p,
        "UdpSocket.receive",
        &[2, 8],
        None,
        datagram(b"ping", echo_port),
    );
    handle(&mut p, "UdpSocket.close", &[2], None, vec![0]);
    assert_program(
        &scratch,
        p,
        vec![(
            "Net",
            Limit::Entries(vec!["127.0.0.1:0".into(), format!("127.0.0.1:{echo_port}")]),
        )],
        || {},
    );
}

struct UdpFixture {
    address: SocketAddr,
    stopped: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    seen: Arc<Mutex<Vec<Vec<u8>>>>,
}
impl UdpFixture {
    fn new() -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = socket.local_addr().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let records = seen.clone();
        let thread = std::thread::spawn(move || {
            let mut bytes = [0; 4096];
            loop {
                match socket.recv_from(&mut bytes) {
                    Ok((n, from)) => {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        records.lock().unwrap().push(bytes[..n].to_vec());
                        socket.send_to(&bytes[..n], from).unwrap();
                    }
                    Err(err)
                        if matches!(
                            err.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                    }
                    Err(err) => panic!("local datagram fixture: {err}"),
                }
            }
        });
        Self {
            address,
            stopped,
            thread: Some(thread),
            seen,
        }
    }
}
impl Drop for UdpFixture {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        let wake = UdpSocket::bind("127.0.0.1:0").unwrap();
        let _ = wake.send_to(b"", self.address);
        if let Some(thread) = self.thread.take()
            && let Err(error) = thread.join()
            && !std::thread::panicking()
        {
            std::panic::resume_unwind(error);
        }
    }
}

fn send(p: &mut Program, host: &str, port: u16, bytes: &[u8], expected: Vec<u8>) {
    handle(
        p,
        "UdpSocket.send_to",
        &[1, i32::from(port)],
        Some(&Enc::default().str(host).bytes(bytes).0),
        expected,
    );
}
fn datagram(bytes: &[u8], port: u16) -> Vec<u8> {
    Enc::default()
        .leb(0)
        .bytes(bytes)
        .str("127.0.0.1")
        .leb(u64::from(port))
        .0
}

#[test]
fn udp_binary_truncation_zero_receive_consumption_and_each_send_grant() {
    let scratch = Scratch::new();
    let server = UdpFixture::new();
    let port = server.address.port();
    let mut p = Program::new();
    open(&mut p, "bind_udp", "127.0.0.1", 0, vec![0, 1]);
    for (bytes, max, received) in [
        (b"abcd".as_slice(), 2, b"ab".as_slice()),
        (b"discard", 0, b""),
        (b"next", 4, b"next"),
        (b"", 9, b""),
    ] {
        send(&mut p, "127.0.0.1", port, bytes, vec![0]);
        handle(
            &mut p,
            "UdpSocket.receive",
            &[1, max],
            None,
            datagram(received, port),
        );
    }
    send(
        &mut p,
        "127.0.0.2",
        port,
        b"denied",
        operation_error(0, &format!("127.0.0.2:{port}")),
    );
    send(
        &mut p,
        "bad host",
        port,
        b"invalid",
        operation_error(1, &format!("bad host:{port}")),
    );
    handle(&mut p, "UdpSocket.close", &[1], None, vec![0]);
    handle(&mut p, "UdpSocket.receive", &[1, 1], None, vec![1, 1]);
    send(&mut p, "127.0.0.1", port, b"closed", vec![1, 1]);
    handle(&mut p, "UdpSocket.close", &[1], None, vec![1, 1]);
    assert_program(
        &scratch,
        p,
        vec![(
            "Net",
            Limit::Entries(vec!["127.0.0.1:0".into(), format!("127.0.0.1:{port}")]),
        )],
        || {},
    );
    assert_eq!(server.seen.lock().unwrap().len(), 8);
}

#[test]
fn net_refused_connect_and_occupied_tcp_udp_bind_addresses() {
    let scratch = Scratch::new();
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    let tcp_port = occupied.local_addr().unwrap().port();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let udp_port = udp.local_addr().unwrap().port();
    let reservation = RefusedPort::new();
    let refused = reservation.address.port();
    let mut p = Program::new();
    open(
        &mut p,
        "connect",
        "127.0.0.1",
        refused,
        net_error(3, &format!("127.0.0.1:{refused}")),
    );
    open(
        &mut p,
        "listen",
        "127.0.0.1",
        tcp_port,
        net_error(3, &format!("127.0.0.1:{tcp_port}")),
    );
    open(
        &mut p,
        "bind_udp",
        "127.0.0.1",
        udp_port,
        net_error(3, &format!("127.0.0.1:{udp_port}")),
    );
    assert_program(&scratch, p, vec![], || {});
}

#[test]
fn net_unknown_handles_and_stale_finish_calls_are_contract_panics() {
    let scratch = Scratch::new();
    for (method, args, payload) in [
        ("TcpStream.read", vec![99, 0], None),
        (
            "TcpStream.write",
            vec![99],
            Some(Enc::default().bytes(b"x").0),
        ),
        ("TcpStream.close", vec![99], None),
        ("TcpListener.accept", vec![99], None),
        ("TcpListener.close", vec![99], None),
        ("UdpSocket.receive", vec![99, 0], None),
        (
            "UdpSocket.send_to",
            vec![99, 9],
            Some(Enc::default().str("127.0.0.1").bytes(b"x").0),
        ),
        ("UdpSocket.close", vec![99], None),
    ] {
        let mut p = Program::new();
        handle(&mut p, method, &args, payload.as_deref(), vec![]);
        let (wasm, _) = p.finish(7);
        let r = parity(&scratch, &wasm, vec![], || {});
        assert_eq!(r.status, 3);
        assert_eq!(
            r.stderr,
            b"panic: host-contract: an operation on a handle the host never made\n"
        );
    }
    for method in [
        "lookup",
        "connect",
        "listen",
        "bind_udp",
        "TcpStream.read",
        "TcpStream.write",
        "TcpListener.accept",
        "UdpSocket.send_to",
        "UdpSocket.receive",
    ] {
        let mut p = Program::new();
        let call = p.import(
            "hd:Net",
            &format!("{method}.finish"),
            &[ValType::I32],
            &[ValType::I32],
        );
        p.instructions(&[I::I32Const(1), I::Call(call), I::Drop]);
        let (wasm, _) = p.finish(7);
        let r = parity(&scratch, &wasm, vec![], || {});
        assert_eq!(r.status, 3);
        assert_eq!(
            r.stderr,
            b"panic: host-contract: a finish of an operation that is not pending\n"
        );
    }
}

#[test]
fn closed_udp_disposed_precedes_denied_or_invalid_destination() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    open(&mut p, "bind_udp", "127.0.0.1", 0, vec![0, 1]);
    handle(&mut p, "UdpSocket.close", &[1], None, vec![0]);
    send(&mut p, "127.0.0.2", 9, b"denied", vec![1, 1]);
    send(&mut p, "bad host", 9, b"invalid", vec![1, 1]);
    assert_program(
        &scratch,
        p,
        vec![("Net", Limit::Entries(vec!["127.0.0.1:0".into()]))],
        || {},
    );
}

#[test]
fn unknown_udp_handle_precedes_denied_or_invalid_destination() {
    let scratch = Scratch::new();
    for host in ["127.0.0.2", "bad host"] {
        let mut p = Program::new();
        handle(
            &mut p,
            "UdpSocket.send_to",
            &[99, 9],
            Some(&Enc::default().str(host).bytes(b"x").0),
            vec![],
        );
        let (wasm, _) = p.finish(7);
        let r = parity(
            &scratch,
            &wasm,
            vec![("Net", Limit::Entries(vec![]))],
            || {},
        );
        assert_eq!(r.status, 3);
        assert_eq!(
            r.stderr,
            b"panic: host-contract: an operation on a handle the host never made\n"
        );
    }
}

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);
impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn ended_instance_stops_socket_owner_while_embedding_retains_instance() {
    retained_instance_stops_socket_owner(false);
}

#[test]
fn trapped_instance_stops_socket_owner_while_embedding_retains_instance() {
    retained_instance_stops_socket_owner(true);
}

fn retained_instance_stops_socket_owner(trap: bool) {
    let _spawn = CHILD_SPAWN.lock().unwrap();
    let (closed, peer_closed) = std::sync::mpsc::channel();
    let peer = TcpFixture::new(move |mut peer| {
        assert_eq!(
            peer.read(&mut [0]).unwrap(),
            0,
            "ending the guest stops its socket owner and closes the peer stream"
        );
        closed.send(()).unwrap();
    });
    let mut p = Program::new();
    open(&mut p, "listen", "127.0.0.1", 0, vec![0, 1]);
    open(&mut p, "bind_udp", "127.0.0.1", 0, vec![0, 2]);
    open(
        &mut p,
        "connect",
        "127.0.0.1",
        peer.address.port(),
        vec![0, 3],
    );
    if trap {
        p.instructions(&[I::Unreachable]);
    }
    // No explicit handle close: ending the instance stops the worker that
    // exclusively owns all three socket kinds. Observe stream EOF while
    // retaining the instance, without racing another test to rebind its ports.
    // Unit tests separately prove this owner's request receiver is dropped
    // after State::serve clears every handle. hd:rt.stderr has already emitted
    // the proof of all successful opens,
    // so a subsequent raw trap does not append another panic diagnostic.
    let (wasm, expected) = p.finish(7);
    let setup = HostSetup {
        providers: vec![],
        grants: Grants::default(),
        limits: Limits::default(),
        args: vec![],
        program: String::new(),
    };
    let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
    let module = engine
        .load(&wasm, &hd_cache::MemoryStore::default())
        .unwrap();
    let output = Sink::default();
    let errors = Sink::default();
    let io = HostIo {
        out: Box::new(output.clone()),
        err: Box::new(errors.clone()),
        input: None,
    };
    let mut instance: Box<dyn Instance> =
        Box::new(engine.instantiate_with(&module, &setup, io).unwrap());
    assert_eq!(instance.init(), Outcome::Exit(if trap { 3 } else { 0 }));
    assert_eq!(
        hd_run::drive(instance.as_mut(), 5),
        Outcome::Exit(if trap { 3 } else { 7 })
    );
    assert_eq!(*errors.0.lock().unwrap(), expected.as_bytes());
    assert_eq!(*output.0.lock().unwrap(), b"core-parity\n");
    peer_closed
        .recv_timeout(Duration::from_secs(3))
        .expect("guest socket owner stops while its ended instance remains alive");
    drop(instance);
}

#[cfg(unix)]
#[test]
fn udp_ipv4_socket_to_numeric_ipv6_reports_the_native_error() {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let native = socket.send_to(b"x", "[::1]:9").unwrap_err();
    let mut p = Program::new();
    open(&mut p, "bind_udp", "127.0.0.1", 0, vec![0, 1]);
    // A UDP4 socket cannot reach an IPv6 destination; this fails locally
    // before any datagram can leave the machine.
    send(
        &mut p,
        "::1",
        9,
        b"x",
        operation_error(5, &format!("send [::1]:9: {native}")),
    );
    handle(&mut p, "UdpSocket.close", &[1], None, vec![0]);
    let (wasm, expected) = p.finish(7);
    let result = hd_run_wasmtime::run_captured(
        EngineConfig::default(),
        &wasm,
        &HostSetup {
            providers: vec![],
            grants: Grants::default(),
            limits: Limits::default(),
            args: vec![],
            program: "native-udp-error".into(),
        },
    )
    .unwrap();
    assert_eq!(result.status, 7);
    assert_eq!(result.stdout, b"core-parity\n");
    assert_eq!(result.stderr, expected.as_bytes());
}
